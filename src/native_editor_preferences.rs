//! 作者偏好始终保存在用户配置旁的独立文件；不读取工程settings.json，也不干扰主窗口最近工程原文身份。
//! 帧内只收敛固定设置快照和非阻塞轮询，最多一个IO任务加一份最新待存值；关闭等待原槽排空。

use crate::user_preferences::{Preferences, migrate_file_if_missing, publish_checked, read_optional};
use async_channel::{Receiver, TryRecvError};
use bevy::{prelude::*, tasks::IoTaskPool};
use feathrise_editor_core::{EditorSettings, EditorSettingsData};
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, path::PathBuf, rc::Rc};

const FORMAT: u32 = 1;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    format: u32,
    editor: EditorSettingsData,
}

pub(crate) struct Store {
    file: PathBuf,
    original: Option<Vec<u8>>,
    load_failed: bool,
    pub(crate) editor: EditorSettingsData,
}

impl Store {
    fn load(file: PathBuf) -> Result<Self, String> {
        let original = read_optional(&file)?;
        let editor = match original.as_deref() {
            Some(bytes) => {
                let stored: Stored = serde_json::from_slice(bytes)
                    .map_err(|error| format!("作者个人设置损坏，已保留原文件：{error}"))?;
                if stored.format != FORMAT {
                    return Err(format!(
                        "作者个人设置版本{}不受支持，已保留原文件",
                        stored.format
                    ));
                }
                stored.editor
            }
            None => EditorSettingsData::default(),
        };
        Ok(Self {
            file,
            original,
            load_failed: false,
            editor,
        })
    }

    fn fallback(file: PathBuf) -> Self {
        Self {
            file,
            original: None,
            load_failed: true,
            editor: EditorSettingsData::default(),
        }
    }

    fn save(&mut self) -> Result<(), String> {
        if self.load_failed {
            return Err("作者个人设置未成功读取，已拒绝覆盖；请修复原文件并重新打开编辑器".into());
        }
        let mut bytes = serde_json::to_vec_pretty(&Stored {
            format: FORMAT,
            editor: self.editor.clone(),
        })
        .map_err(|error| format!("无法编码作者个人设置：{error}"))?;
        bytes.push(b'\n');
        publish_checked(&self.file, &self.original, &bytes)?;
        self.original = Some(bytes);
        Ok(())
    }
}

/// 显式跟随主窗口的用户配置位置，验收覆盖同样隔离；不把任何工程路径作为设置根。
pub(crate) fn load(preferences: &Preferences) -> (Store, Option<String>) {
    let file = preferences.file().with_extension("author.json");
    let loaded = (|| {
        // 仅标准用户配置带有迁移来源；显式隔离的验收配置不接触用户目录。
        if let Some(legacy) = preferences.legacy_file() {
            migrate_file_if_missing(&legacy.with_extension("author.json"), &file)?;
        }
        Store::load(file.clone())
    })();
    match loaded {
        Ok(store) => (store, None),
        Err(error) => (Store::fallback(file), Some(error)),
    }
}

struct Written {
    store: Store,
    result: Result<(), String>,
    revision: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Authority {
    Home,
    Author,
    Reloading,
}

struct Reloaded {
    generation: u64,
    store: Store,
    error: Option<String>,
}

struct State {
    store: Option<Store>,
    current: EditorSettingsData,
    committed: EditorSettingsData,
    revision: u64,
    job: Option<Receiver<Written>>,
    failure: Option<(u64, String)>,
    authority: Authority,
    generation: u64,
    reload: Option<Receiver<Reloaded>>,
}

impl State {
    fn capture(&mut self, current: EditorSettingsData) {
        if self.current != current {
            self.current = current;
            self.revision = self.revision.wrapping_add(1);
            self.failure = None; // 只在实际新输入后重试，稳定失败帧不会每帧重新写盘。
        }
    }

    fn dirty(&self) -> bool {
        self.current != self.committed
    }

    fn error(&self) -> Option<&str> {
        self.failure
            .as_ref()
            .filter(|(revision, _)| *revision == self.revision)
            .map(|(_, error)| error.as_str())
    }

    fn adopt(&mut self, written: Written) {
        match written.result {
            Ok(()) => {
                self.committed = written.store.editor.clone();
                self.failure = None;
            }
            Err(error) => {
                warn!("作者偏好未保存：{error}");
                self.failure = Some((written.revision, error));
            }
        }
        self.store = Some(written.store); // 只有真实写入成功的结果才带回新的原文身份。
        self.job = None;
    }

    fn advance(&mut self) {
        let received = self.job.as_ref().map(|job| job.try_recv());
        match received {
            Some(Ok(written)) => self.adopt(written),
            Some(Err(TryRecvError::Closed)) => {
                self.job = None;
                self.failure = Some((
                    self.revision,
                    "作者偏好后台写入意外结束，原文件保留，请重新打开编辑器".into(),
                ));
            }
            Some(Err(TryRecvError::Empty)) | None => (),
        }
        if self.authority != Authority::Home
            || self.job.is_some()
            || !self.dirty()
            || self.error().is_some()
        {
            return;
        }
        let Some(mut store) = self.store.take() else {
            self.failure = Some((
                self.revision,
                "作者偏好写入槽没有归还原文身份，请重新打开编辑器".into(),
            ));
            return;
        };
        store.editor = self.current.clone();
        let revision = self.revision;
        let (sender, receiver) = async_channel::bounded(1);
        self.job = Some(receiver);
        IoTaskPool::get()
            .spawn(async move {
                let result = store.save();
                let _ = sender.try_send(Written {
                    store,
                    result,
                    revision,
                });
            })
            .detach(); // 单飞槽只在收到完整结果后释放，不因新输入丢掉在途写入或原文身份。
    }

    fn begin_reload(&mut self) {
        let Some(file) = self.store.as_ref().map(|store| store.file.clone()) else {
            return; // 写槽尚未归还时不能把旧身份当成可重新读取的配置；正常交接先排空写者。
        };
        self.authority = Authority::Reloading;
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        let (sender, receiver) = async_channel::bounded(1);
        self.reload = Some(receiver);
        IoTaskPool::get()
            .spawn(async move {
                let (store, error) = match Store::load(file.clone()) {
                    Ok(store) => (store, None),
                    Err(error) => (Store::fallback(file), Some(error)),
                };
                let _ = sender.try_send(Reloaded {
                    generation,
                    store,
                    error,
                });
            })
            .detach();
    }

    fn adopt_reload(&mut self, loaded: Reloaded) -> Option<(EditorSettings, Option<String>)> {
        if self.authority != Authority::Reloading
            || loaded.generation != self.generation
            || self.store.as_ref().map(|store| &store.file) != Some(&loaded.store.file)
        {
            return None; // 旧读回仍占原槽直到真实回执；换作者/配置身份后不能迟到覆盖当前设置。
        }
        let settings = loaded.store.editor.clone().into_runtime();
        let baseline = EditorSettingsData::from(&settings);
        self.store = Some(loaded.store);
        self.current = baseline.clone();
        self.committed = baseline;
        self.revision = self.revision.wrapping_add(1);
        self.failure = None;
        self.authority = Authority::Home;
        Some((settings, loaded.error))
    }
}

struct Runtime(Rc<RefCell<State>>);

/// 主窗口可能退役尚未呈现的候选实例；App退出后仍拥有槽，避免任务随World销毁丢失最后值。
pub(crate) struct Completion(Rc<RefCell<State>>);

pub(super) fn install(app: &mut App, store: Store) -> Completion {
    let completion = install_runtime(app, store);
    app.add_systems(Last, capture_and_poll);
    completion
}

/// 首页和作者复用同一受身份保护写者；宿主通过生命周期显式驱动，避免Last中的无序捕获越过交接门。
pub(crate) fn install_host(app: &mut App, store: Store) -> Completion {
    app.insert_resource(store.editor.clone().into_runtime());
    install_runtime(app, store)
}

fn install_runtime(app: &mut App, store: Store) -> Completion {
    // 先按运行时规范化值建立基线；仅启动读取不会把旧原文或缺省配置自动覆盖一遍。
    let baseline = EditorSettingsData::from(app.world().resource::<EditorSettings>());
    let shared = Rc::new(RefCell::new(State {
        store: Some(store),
        current: baseline.clone(),
        committed: baseline,
        revision: 0,
        job: None,
        failure: None,
        authority: Authority::Home,
        generation: 0,
        reload: None,
    }));
    app.insert_non_send(Runtime(Rc::clone(&shared)));
    Completion(shared)
}

fn capture_and_poll(settings: Res<EditorSettings>, runtime: NonSend<Runtime>) {
    let mut state = runtime.0.borrow_mut();
    if settings.is_changed() {
        state.capture(EditorSettingsData::from(&*settings));
    }
    state.advance();
}

/// 不接触磁盘的输入门；候选准备、作者活动和返首页读回期间都没有首页设置写权限。
pub(crate) fn home_ready(world: &World) -> bool {
    world
        .get_non_send_resource::<Runtime>()
        .is_none_or(|runtime| runtime.0.borrow().authority == Authority::Home)
}

/// 只在真实保存末值后交出写权限；调用方必须在同一主帧封住设置输入，再创建会读取偏好的候选。
pub(crate) fn yield_to_author(world: &mut World) -> Result<bool, String> {
    let Some(runtime) = world.get_non_send_resource::<Runtime>() else {
        return Ok(true);
    };
    match runtime.0.borrow().authority {
        Authority::Author => return Ok(true),
        Authority::Reloading => return Ok(false),
        Authority::Home => (),
    }
    if !ready_to_exit(world, false)? {
        return Ok(false);
    }
    let runtime = world.non_send_resource::<Runtime>();
    let mut state = runtime.0.borrow_mut();
    state.authority = Authority::Author;
    state.generation = state.generation.wrapping_add(1);
    Ok(true)
}

/// 宿主每帧只非阻塞轮询固定大小的设置。所有作者都退出后才异步重读，绝不用root旧快照争写。
pub(crate) fn poll_host(world: &mut World, authors_present: bool) -> Option<String> {
    let runtime = world.get_non_send_resource::<Runtime>()?;
    let shared = Rc::clone(&runtime.0);
    let mut state = shared.borrow_mut();
    let previous_error = state.error().map(str::to_owned);
    if authors_present && state.authority != Authority::Author {
        state.authority = Authority::Author;
        state.generation = state.generation.wrapping_add(1);
    }
    let mut replacement = None;
    let mut error = None;
    let received = state.reload.as_ref().map(|job| job.try_recv());
    match received {
        Some(Ok(loaded)) => {
            state.reload = None;
            if let Some((settings, failure)) = state.adopt_reload(loaded) {
                replacement = Some(settings);
                error = failure;
            }
        }
        Some(Err(TryRecvError::Closed)) => {
            state.reload = None;
            if state.authority == Authority::Reloading {
                // 不把丢失回执当成成功读回；沿坏原文只读托底，后续修改继续拒绝覆盖。
                let file = state.store.as_ref()?.file.clone();
                let generation = state.generation;
                let loaded = Reloaded {
                    generation,
                    store: Store::fallback(file),
                    error: Some("作者偏好后台读回意外结束，已保留原文件；请重新打开编辑器".into()),
                };
                if let Some((settings, failure)) = state.adopt_reload(loaded) {
                    replacement = Some(settings);
                    error = failure;
                }
            }
        }
        Some(Err(TryRecvError::Empty)) | None => (),
    }
    if !authors_present && state.authority == Authority::Author && state.reload.is_none() {
        state.begin_reload();
    }
    if state.authority == Authority::Home {
        if replacement.is_none() {
            state.capture(EditorSettingsData::from(world.resource::<EditorSettings>()));
        }
        state.advance();
        if let Some(failure) = state.error() {
            if previous_error.as_deref() != Some(failure) {
                error = Some(failure.to_owned());
            }
        }
    }
    drop(state);
    if let Some(settings) = replacement {
        world.insert_resource(settings);
    }
    error
}

/// 排空当前持权者的最后快照，供原关闭授权及候选交接复用；false时继续事件循环，不等待后台IO。
pub(crate) fn ready_to_exit(world: &mut World, retry: bool) -> Result<bool, String> {
    let Some(runtime) = world.get_non_send_resource::<Runtime>() else {
        return Ok(true);
    };
    let latest = EditorSettingsData::from(world.resource::<EditorSettings>());
    let mut state = runtime.0.borrow_mut();
    if state.authority != Authority::Home {
        return Ok(false); // 主宿主不能用作者期间的旧设置关闭写槽；先等退役和完整读回。
    }
    state.capture(latest);
    if retry {
        state.failure = None;
    } // 新一次关闭请求只重试一笔；等待中的普通帧不能形成磁盘忙循环。
    state.advance();
    if state.dirty() {
        if let Some(error) = state.error() {
            return Err(format!(
                "作者个人设置未保存：{error}；工程原文和作者现场保留，请取消关闭后检查个人配置"
            ));
        }
    }
    Ok(!state.dirty() && state.job.is_none())
}

impl Completion {
    /// 只在App::run归还后调用；普通关闭已异步排空，这里兜底候选退役等非标准AppExit。
    pub(crate) fn finish(self) -> Result<(), String> {
        let mut state = self.0.borrow_mut();
        if let Some(job) = state.reload.take() {
            let loaded = job
                .recv_blocking()
                .map_err(|_| "作者偏好后台读回未返回完整结果".to_owned())?;
            let _ = state.adopt_reload(loaded);
        }
        if state.authority != Authority::Home {
            // 作者仍拥有写权限时，即便主App非正常退役也绝不能把root旧值写回用户配置。
            return if state.job.is_none() && !state.dirty() {
                Ok(())
            } else {
                Err("作者偏好交接未收口，已保留原文件和作者写权限".into())
            };
        }
        if let Some(job) = state.job.take() {
            let written = job
                .recv_blocking()
                .map_err(|_| "作者偏好后台任务未返回完整结果".to_owned())?;
            state.adopt(written);
        }
        if !state.dirty() {
            return Ok(());
        }
        if let Some(error) = state.error() {
            return Err(error.to_owned());
        }
        let latest = state.current.clone();
        let store = state
            .store
            .as_mut()
            .ok_or("作者偏好写入槽没有归还原文身份")?;
        store.editor = latest;
        store.save()?; // 退出后的最终快照同步发布不占用UI帧，且继续使用完全相同的身份保护。
        state.committed = state.current.clone();
        Ok(())
    }
}

#[cfg(test)]
#[path = "native_editor_preferences_tests.rs"]
mod tests;
