//! 原生事件去抖与单飞完整构建；监听不等待 Cargo，连续保存只覆盖一个最新等待槽。

use super::{
    artifacts,
    resources::Shared,
    source,
    types::{BuildIdentity, BuildRequest, Event, WatchStart},
};
use crate::project::Project;
use crate::code_iteration::hotpatch::PatchContext;
use notify::{EventKind, RecursiveMode, Watcher};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}, mpsc},
    thread,
    time::{Duration, Instant},
};

const POLL: Duration = Duration::from_millis(30);
const DEBOUNCE: Duration = Duration::from_millis(350);
const WATCH_LIMIT: usize = 4096;

pub(super) fn watch(
    project: Project,
    target: PathBuf,
    shared: Arc<Shared>,
    events: mpsc::SyncSender<Event>,
    start: WatchStart,
) {
    let errors = Arc::new(Mutex::new(None::<String>));
    let callback_shared = shared.clone();
    let callback_errors = errors.clone();
    let refresh_registration = Arc::new(AtomicBool::new(false));
    let callback_refresh = refresh_registration.clone();
    let exclusions = source::Exclusions::new(&project, &target);
    let scope = Arc::new(Mutex::new(EventScope::initial(&project.root)));
    let callback_scope = scope.clone();
    let watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
        if callback_shared.stopped() {
            return;
        }
        match result {
            Ok(event) if !matches!(event.kind, EventKind::Access(_)) => {
                let Ok(scope) = callback_scope.lock() else {
                    if let Ok(mut errors) = callback_errors.lock() { *errors = Some("源码监听作用域异常，未继续宣称已同步".into()); }
                    callback_shared.advance();
                    return;
                };
                if (event.need_rescan() && event.paths.is_empty())
                    || event.paths.iter().any(|path| {
                        let ancestor_changed = scope.compiled_ancestor_change(&event.kind, path);
                        let relevant = event.need_rescan() || relevant_change(&event.kind, path)
                            || scope.compiled_input_change(&event.kind, path);
                        let accepted = (!exclusions.contains(path)
                            || (scope.captured_file(path) || ancestor_changed) && exclusions.allows_compiled_file(path))
                            && scope.contains(path)
                            && !callback_shared.output_scope.ignores(path, relevant) && relevant;
                        if accepted && ancestor_changed { callback_refresh.store(true, Ordering::Release); }
                        accepted
                    })
                {
                    callback_shared.advance();
                    callback_shared.wake.notify_all();
                }
            }
            Ok(_) => (),
            Err(error) => {
                if let Ok(mut errors) = callback_errors.lock() {
                    *errors = Some(format!("源码原生监听失败，未继续宣称已同步：{error}"));
                }
                callback_shared.advance();
            }
        }
    });
    let mut watcher = match watcher {
        Ok(watcher) => watcher,
        Err(error) => {
            send(
                &shared,
                &events,
                Event::Failed(format!("无法建立源码监听：{error}")),
            );
            stop(&shared);
            return;
        }
    };
    // 先覆盖工程根以保护初始扫描窗口；其余本地 SDK 根在解析后登记，再复扫验证。
    if let Err(error) = watcher.watch(&project.root, RecursiveMode::Recursive) {
        send(
            &shared,
            &events,
            Event::Failed(format!("无法监听工程源码：{error}")),
        );
        stop(&shared);
        return;
    }
    let project_watch = (project.root.clone(), true);
    let mut installed = BTreeSet::from([project_watch.clone()]);
    let mut observed = shared.generation();
    let mut last_change = Instant::now() - DEBOUNCE;
    let mut progress = WatchProgress::new(start);
    let mut registration_stale = false;
    while !shared.stopped() {
        registration_stale |= refresh_registration.swap(false, Ordering::AcqRel);
        if shared.output_scope.refresh() {
            shared.advance();
            shared.wake.notify_all();
        }
        if let Some(error) = errors.lock().ok().and_then(|mut error| error.take()) {
            send(&shared, &events, Event::Failed(error));
            stop(&shared);
            return;
        }
        let generation = shared.generation();
        if generation != observed {
            observed = generation;
            last_change = Instant::now();
        }
        if generation == progress.captured_generation || last_change.elapsed() < DEBOUNCE {
            thread::sleep(POLL);
            continue;
        }
        let snapshot = match source::capture(&project, &target, &shared.control) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                if !shared.stopped() && generation == shared.generation() {
                    send(&shared, &events, Event::Failed(error));
                    progress.captured_generation = generation;
                }
                thread::sleep(POLL);
                continue;
            }
        };
        let desired = normalized_watches(&snapshot.watches, &project_watch.0);
        if desired.len() > WATCH_LIMIT {
            send(
                &shared,
                &events,
                Event::Failed("源码监听超过 4096 个登记路径，未截断成功".into()),
            );
            stop(&shared);
            return;
        }
        // 祖先只为继承配置而监听，不能将共享父目录里别的工程创建/删除当成本工程源码。
        // 作用域在后台准备、先发布后登记监听；回调只读有界路径表，不做文件IO。
        let desired_scope = EventScope::captured(&snapshot, &project_watch.0);
        let scope_changed = match scope.lock() {
            Ok(mut scope) => {
                let changed = *scope != desired_scope;
                *scope = desired_scope.clone();
                changed
            }
            Err(_) => {
                send(&shared, &events, Event::Failed("源码监听作用域更新失败".into()));
                stop(&shared);
                return;
            }
        };
        let mut watch_failed = false;
        if registration_stale {
            // 目录删除后notify可自行退订；内存路径未变不代表OS监听仍在，恢复时必须重装再复扫。
            for (path, _) in &installed {
                if shared.stopped() { return; }
                let _ = watcher.unwatch(path);
            }
            installed.clear();
            registration_stale = false;
        }
        for (path, recursive) in desired.difference(&installed) {
            if shared.stopped() { return; }
            let mode = if *recursive {
                RecursiveMode::Recursive
            } else {
                RecursiveMode::NonRecursive
            };
            if let Err(error) = watcher.watch(path, mode) {
                send(
                    &shared,
                    &events,
                    Event::Failed(format!("源码监听路径不可用：{}：{error}", path.display())),
                );
                watch_failed = true;
                break;
            }
        }
        if watch_failed {
            stop(&shared);
            return;
        }
        for (path, _) in installed.difference(&desired) {
            if !desired.iter().any(|(desired_path, _)| desired_path == path) {
                let _ = watcher.unwatch(path);
            }
        }
        let topology_changed = desired != installed || scope_changed;
        installed = desired;
        // 路径及源码职责都先发布/登记再复扫；复扫若又发现依赖，下一轮继续收敛而不建立盲区基线。
        let snapshot = if topology_changed {
            match source::capture(&project, &target, &shared.control) {
                Ok(current) => current,
                Err(error) => {
                    send(&shared, &events, Event::Failed(error));
                    continue;
                }
            }
        } else {
            snapshot
        };
        if let Err(error) = progress.adopt(&project.root, &installed, &desired_scope,
            generation, snapshot, &shared, &events) {
            send(&shared, &events, Event::Failed(error));
            return;
        }
    }
    // Drop watcher 会收口其原生监听；没有 UI 帧等待或全目录轮询。
}

/// 只在真实监听拓扑覆盖复扫结果后提交基线/等待槽；拒收不能吞掉本代的下一次扫描。
struct WatchProgress {
    start: WatchStart,
    baseline: bool,
    captured_generation: u64,
}

impl WatchProgress {
    fn new(start: WatchStart) -> Self {
        Self { start, baseline: false, captured_generation: 0 }
    }

    fn adopt(&mut self, project_root: &Path, installed: &BTreeSet<(PathBuf, bool)>,
        published: &EventScope, generation: u64, snapshot: source::SourceSnapshot,
        shared: &Shared, events: &mpsc::SyncSender<Event>) -> Result<bool, String> {
        if shared.stopped() || generation != shared.generation()
            || normalized_watches(&snapshot.watches, project_root) != *installed
            || EventScope::captured(&snapshot, project_root) != *published
        {
            return Ok(false);
        }
        // 作用域计算期间同样可能收到新保存或取消；沿用原子代次门，不能靠摘要相同接受 ABA。
        if shared.stopped() || generation != shared.generation() { return Ok(false); }
        self.captured_generation = generation;
        if !self.baseline && !self.start.rebuild_current_source() && generation == self.start.generation() {
            self.baseline = true;
            return Ok(true); // 打开工程只建立基线，首个原生预览仍走既有启动流程。
        }
        self.baseline = true;
        let request = BuildRequest { generation, profile: shared.profile(), source: snapshot };
        *shared.latest.lock().map_err(|_| "最新源码等待槽异常")? = Some(request);
        let _ = events.try_send(Event::Waiting { generation });
        shared.wake.notify_one();
        Ok(true)
    }
}

fn normalized_watches(
    watches: &BTreeSet<(PathBuf, bool)>,
    project_root: &std::path::Path,
) -> BTreeSet<(PathBuf, bool)> {
    let mut paths = BTreeMap::new();
    for (path, recursive) in watches {
        *paths.entry(path.clone()).or_insert(false) |= *recursive;
    }
    paths.insert(project_root.to_owned(), true);
    paths.into_iter().collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct EventScope {
    recursive: BTreeSet<PathBuf>,
    direct_source: BTreeSet<PathBuf>,
    configuration: BTreeSet<PathBuf>,
    captured_files: BTreeSet<PathBuf>,
    compiled_recursive: BTreeSet<PathBuf>,
}

impl EventScope {
    fn initial(project_root: &Path) -> Self {
        Self { recursive: BTreeSet::from([source::comparison_path(project_root)]), direct_source: BTreeSet::new(), configuration: BTreeSet::new(),
            captured_files: BTreeSet::new(), compiled_recursive: BTreeSet::new() }
    }

    fn captured(snapshot: &source::SourceSnapshot, project_root: &Path) -> Self {
        let watches = normalized_watches(&snapshot.watches, project_root);
        let mut scope = Self::from_watches(&watches);
        let directories: BTreeSet<_> = watches.iter().map(|(path, _)| source::comparison_path(path)).collect();
        // 显式包外入口/构建脚本可能没有同父Cargo清单，须采用扫描时确认的源码职责，不能降成祖先配置。
        for (path, recursive) in &snapshot.source_watches {
            let path = source::comparison_path(path);
            if *recursive {
                scope.recursive.insert(path.clone());
                scope.compiled_recursive.insert(path);
            } else if directories.contains(&path) {
                scope.direct_source.insert(path);
            } else {
                scope.captured_files.insert(path);
            }
        }
        scope
    }

    fn from_watches(watches: &BTreeSet<(PathBuf, bool)>) -> Self {
        // 清单存在性只在监听线程核对；显式Rust入口的父目录也归其实际包/工作区源码域。
        let packages: BTreeSet<_> = watches.iter().filter_map(|(path, _)| path.join("Cargo.toml").is_file().then_some(path)).collect();
        Self {
            recursive: watches.iter().filter_map(|(path, recursive)| recursive.then(|| source::comparison_path(path))).collect(),
            direct_source: watches.iter().filter_map(|(path, recursive)|
                (!recursive && packages.iter().any(|package| path.starts_with(package))).then(|| source::comparison_path(path))).collect(),
            configuration: watches.iter().filter_map(|(path, recursive)| (!recursive).then(|| source::comparison_path(path))).collect(),
            captured_files: BTreeSet::new(),
            compiled_recursive: BTreeSet::new(),
        }
    }

    fn contains(&self, path: &Path) -> bool {
        let path = source::comparison_path(path);
        // 根或其祖先的删除/重建也会改变真实来源；兄弟工程不与本源码根形成前缀关系。
        if self.recursive.iter().any(|root| path.starts_with(root) || root.starts_with(&path))
            || self.direct_source.contains(&path) || self.captured_files.contains(&path) { return true; }
        let Some(parent) = path.parent() else { return false; };
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else { return false; };
        let name_lower = name.to_ascii_lowercase();
        let name = name_lower.as_str();
        if self.direct_source.contains(parent)
            && (source::source_file_path(&path) || matches!(name, "src" | "tests" | "examples" | "benches")) { return true; }
        self.configuration.contains(parent)
            && (matches!(name, "cargo.toml" | "cargo.lock" | "rust-toolchain" | "rust-toolchain.toml" | ".cargo")
                || parent.file_name().is_some_and(|name| name == ".cargo") && matches!(name, "config" | "config.toml"))
            || matches!(name, "config" | "config.toml") && parent.file_name().is_some_and(|name| name == ".cargo")
                && parent.parent().is_some_and(|ancestor| self.configuration.contains(ancestor))
    }

    /// 已捕获文件不限扩展名；新文件只因已扫描源码树内的结构事件进入，未知目录的普通Modify不扩围。
    fn compiled_input_change(&self, kind: &EventKind, path: &Path) -> bool {
        // 精确声明的编译资产已通过包内/缓存边界检查；先认真实文件项，不能拿目录标记放开assets。
        if self.captured_file(path) || self.compiled_ancestor_change(kind, path) { return true; }
        if source::ignored_path(path) { return false; }
        let path = source::comparison_path(path);
        structural_change(kind) && self.compiled_recursive.iter().any(|root| path.starts_with(root))
    }

    fn captured_file(&self, path: &Path) -> bool {
        self.captured_files.contains(&source::comparison_path(path))
    }

    fn compiled_ancestor_change(&self, kind: &EventKind, path: &Path) -> bool {
        if !structural_change(kind) { return false; }
        let path = source::comparison_path(path);
        // 有序前缀查询不遍历完整文件集；只接受真实文件的严格祖先，旁支和普通Modify没有此例外。
        self.captured_files.range(path.clone()..).next()
            .is_some_and(|file| file != &path && file.starts_with(&path))
    }
}

fn relevant_change(kind: &EventKind, path: &Path) -> bool {
    if !source::relevant_path(path) {
        return false;
    }
    if source::source_file_path(path) {
        return true;
    }
    // Windows 在子目录写入时也会通知父 src 的普通 Modify。目录只因结构变化触发，
    // 否则排除缓存/作者根中的生成文件仍会通过父通知自触发下一次构建。
    structural_change(kind)
}

fn structural_change(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_)
            | EventKind::Remove(_)
            | EventKind::Modify(notify::event::ModifyKind::Name(_))
    )
}

fn stop(shared: &Shared) {
    shared.stopped.store(true, Ordering::Release);
    shared.control.cancel();
    shared.wake.notify_all();
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;

pub(super) fn build(
    project: Project,
    target: PathBuf,
    shared: Arc<Shared>,
    events: mpsc::SyncSender<Event>,
    patch_receiver: mpsc::Receiver<Option<PatchContext>>,
) {
    let mut patch_context = None;
    while !shared.stopped() {
        drain_patch_context(&patch_receiver, &mut patch_context);
        let request = {
            let Ok(mut latest) = shared.latest.lock() else {
                send(&shared, &events, Event::Failed("源码等待槽异常".into()));
                return;
            };
            while latest.is_none() && !shared.stopped() {
                let Ok((next, _)) = shared.wake.wait_timeout(latest, POLL) else {
                    send(&shared, &events, Event::Failed("源码等待条件异常".into()));
                    return;
                };
                latest = next;
                drain_patch_context(&patch_receiver, &mut patch_context);
            }
            latest.take()
        };
        let Some(request) = request else {
            continue;
        };
        if !shared.current(request.generation, request.profile) {
            continue;
        }
        let mut identity = BuildIdentity {
            project_id: project.id.clone(),
            root: project.root.clone(),
            profile: request.profile,
            generation: request.generation,
            source_sha256: request.source.sha256.clone(),
            context_sha256: request.source.environment_sha256.clone(),
        };
        send(
            &shared,
            &events,
            Event::Building {
                identity: identity.clone(),
            },
        );
        match artifacts::context(&project, &request, &shared) {
            Ok(context) => identity.context_sha256 = context,
            Err(error) => {
                send(
                    &shared,
                    &events,
                    Event::Finished {
                        identity,
                        result: Err(error),
                        diagnostics: Vec::new(),
                    },
                );
                continue;
            }
        }
        let (result, diagnostics) =
            {
                drain_patch_context(&patch_receiver, &mut patch_context);
                artifacts::prepare(&project, &target, &request, &identity, &shared, patch_context.as_ref())
            };
        send(
            &shared,
            &events,
            Event::Finished {
                identity,
                result,
                diagnostics,
            },
        );
        crate::build_artifacts::reclaim_unused();
        if shared.control.cleanup_failed() {
            shared.stopped.store(true, Ordering::Release);
            shared.wake.notify_all();
        }
    }
    crate::build_artifacts::reclaim_unused();
}

fn drain_patch_context(receiver: &mpsc::Receiver<Option<PatchContext>>, context: &mut Option<PatchContext>) {
    while let Ok(current) = receiver.try_recv() { *context = current; }
}

/// 最终构建回执不能因 UI 暂时忙而丢失；等待只在后台，取消仍可立即撤销投递并收口。
fn send(shared: &Shared, sender: &mpsc::SyncSender<Event>, mut event: Event) {
    loop {
        if shared.stopped() {
            return;
        }
        match sender.try_send(event) {
            Ok(()) | Err(mpsc::TrySendError::Disconnected(_)) => return,
            Err(mpsc::TrySendError::Full(returned)) => {
                event = returned;
                thread::sleep(POLL);
            }
        }
    }
}
