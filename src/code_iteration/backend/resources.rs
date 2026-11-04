//! 两个固定后台线程的所有权与原子代次门；UI 不获取后台互斥锁，也不等待进程或文件扫描。

use super::types::{BuildIdentity, BuildRequest, Event, WatchStart};
use crate::{build_process::BuildControl, project::Project, project_log::ProjectLogChannel};
use crate::code_iteration::hotpatch::{PatchContext, PatchTarget};
use feathrise_editor_core::ProjectBuildProfile;
use std::{
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
};

pub(super) struct Shared {
    pub control: BuildControl,
    pub generation: AtomicU64,
    pub profile: AtomicU8,
    pub stopped: AtomicBool,
    /// 只有监听与构建线程访问此一槽；运行中的构建占另一个固定槽，不因过期而提前归还。
    pub latest: Mutex<Option<BuildRequest>>,
    pub wake: Condvar,
    pub output_scope: Arc<super::output_scope::OutputScope>,
}

impl Shared {
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
    pub fn profile(&self) -> ProjectBuildProfile {
        decode_profile(self.profile.load(Ordering::Acquire))
    }
    pub fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire) || self.control.is_cancelled()
    }
    pub fn advance(&self) -> u64 {
        match self.generation.fetch_update(Ordering::AcqRel, Ordering::Acquire, |generation| generation.checked_add(1)) {
            Ok(previous) => previous + 1,
            Err(generation) => {
                // 不能回绕复活旧按钮或旧产物；极端代次耗尽同样撤销所有采用权限。
                self.stopped.store(true, Ordering::Release);
                self.control.cancel();
                self.wake.notify_all();
                generation
            }
        }
    }
    pub fn current(&self, generation: u64, profile: ProjectBuildProfile) -> bool {
        !self.stopped()
            && self.generation() == generation
            && self.profile() == profile
            && self.generation() == generation
    }
}

pub(crate) struct Backend {
    project_id: String,
    root: PathBuf,
    shared: Arc<Shared>,
    receiver: mpsc::Receiver<Event>,
    patch_sender: mpsc::SyncSender<Option<PatchContext>>,
    /// 仅记录已经成功投递的目标；满槽时不能提前记成已清除，否则None永远无法撤销旧上下文。
    last_patch_target: Option<PatchTarget>,
    source_thread: Option<JoinHandle<()>>,
    build_thread: Option<JoinHandle<()>>,
}

impl Backend {
    /// 生命周期回归使用真实线程句柄和同一Shared；两个独立门不代表任何编译或运行成功。
    #[cfg(test)]
    pub(crate) fn controlled_for_test(
        project: &Project,
        profile: ProjectBuildProfile,
        control: BuildControl,
        source_release: mpsc::Receiver<()>,
        build_release: mpsc::Receiver<()>,
    ) -> (Self, mpsc::Receiver<()>, mpsc::Receiver<()>) {
        let shared = Arc::new(Shared { control, generation: AtomicU64::new(1),
            profile: AtomicU8::new(encode_profile(profile)), stopped: AtomicBool::new(false),
            latest: Mutex::new(None), wake: Condvar::new(), output_scope: Arc::default() });
        let (_events, receiver) = mpsc::sync_channel(16);
        let (patch_sender, _patch_context) = mpsc::sync_channel(1);
        let (source_ended, source_end) = mpsc::sync_channel(1);
        let (build_ended, build_end) = mpsc::sync_channel(1);
        let source_thread = thread::spawn(move || {
            let _ = source_release.recv_timeout(std::time::Duration::from_secs(5));
            let _ = source_ended.try_send(());
        });
        let build_thread = thread::spawn(move || {
            let _ = build_release.recv_timeout(std::time::Duration::from_secs(5));
            let _ = build_ended.try_send(());
        });
        (Self { project_id: project.id.clone(), root: project.root.clone(), shared, receiver,
            patch_sender, last_patch_target: None,
            source_thread: Some(source_thread), build_thread: Some(build_thread) }, source_end, build_end)
    }

    pub(crate) fn spawn(
        project: Project,
        target: PathBuf,
        profile: ProjectBuildProfile,
        logs: Option<ProjectLogChannel>,
    ) -> Self {
        Self::spawn_started(project, target, profile, logs, WatchStart::BaselineOnly { generation: 1 })
    }

    /// 调用方必须先确认旧实例 is_finished 并成功 finish；传入其停止后的最终代次。
    pub(crate) fn spawn_resumed(
        project: Project,
        target: PathBuf,
        profile: ProjectBuildProfile,
        logs: Option<ProjectLogChannel>,
        previous_generation: u64,
    ) -> Result<Self, String> {
        let start = resumed_start(previous_generation)?;
        Ok(Self::spawn_started(project, target, profile, logs, start))
    }

    fn spawn_started(
        project: Project,
        target: PathBuf,
        profile: ProjectBuildProfile,
        logs: Option<ProjectLogChannel>,
        start: WatchStart,
    ) -> Self {
        let control = logs
            .map(|logs| {
                BuildControl::with_log(logs.begin_task(crate::project_log::LogTaskKind::CodeBuild))
            })
            .unwrap_or_default();
        let shared = Arc::new(Shared {
            control,
            generation: AtomicU64::new(start.generation()),
            profile: AtomicU8::new(encode_profile(profile)),
            stopped: AtomicBool::new(false),
            latest: Mutex::new(None),
            wake: Condvar::new(),
            output_scope: Arc::default(),
        });
        let (events, receiver) = mpsc::sync_channel(16);
        let (patch_sender, patch_context) = mpsc::sync_channel(1);
        let source_shared = shared.clone();
        let source_events = events.clone();
        let source_project = project.clone();
        let source_target = target.clone();
        let source_thread = thread::spawn(move || {
            super::worker::watch(source_project, source_target, source_shared, source_events, start)
        });
        let build_shared = shared.clone();
        let build_project = project.clone();
        let build_thread = thread::spawn(move || {
            super::worker::build(build_project, target, build_shared, events, patch_context)
        });
        Self {
            project_id: project.id,
            root: project.root,
            shared,
            receiver,
            patch_sender,
            last_patch_target: None,
            source_thread: Some(source_thread),
            build_thread: Some(build_thread),
        }
    }

    pub(crate) fn set_profile(&self, profile: ProjectBuildProfile) {
        if self
            .shared
            .profile
            .swap(encode_profile(profile), Ordering::AcqRel)
            != encode_profile(profile)
        {
            self.shared.advance();
            self.shared.wake.notify_all();
        }
    }

    /// 作者帧只投递一个浅共享上下文；不持后台锁。调用方每帧重试，后台换代最终仍由真实回执门核对。
    pub(crate) fn set_patch_context(&mut self, context: Option<PatchContext>) {
        let target = context.as_ref().map(|context| context.target.clone());
        if target == self.last_patch_target { return; }
        if self.patch_sender.try_send(context).is_ok() {
            self.last_patch_target = target;
            self.shared.wake.notify_one();
        }
    }

    pub(crate) fn current_generation(&self) -> u64 {
        self.shared.generation()
    }
    pub(crate) fn generation_is_current(&self, generation: u64, profile: ProjectBuildProfile) -> bool {
        self.shared.current(generation, profile)
    }
    /// 导出复用现有监听代次；即使源码改后还原，也永久撤销这一次导出的发布权。
    pub(crate) fn export_input_guard(&self) -> crate::build_process::InputGuard {
        let shared = self.shared.clone();
        let generation = shared.generation();
        crate::build_process::InputGuard(Arc::new(move || !shared.stopped() && shared.generation() == generation))
    }
    pub(crate) fn export_output_registrar(&self) -> crate::build_process::OutputRegistrar {
        let shared = self.shared.clone();
        let project_id = self.project_id.clone();
        crate::build_process::OutputRegistrar(Arc::new(move |path| {
            if shared.stopped() { return Err("代码监听已停止，不能登记导出目标".into()); }
            let invalidated = shared.clone();
            shared.output_scope.register(path, &project_id, move || {
                invalidated.advance();
                invalidated.wake.notify_all();
            })
        }))
    }
    pub(crate) fn current_profile(&self) -> ProjectBuildProfile {
        self.shared.profile()
    }
    pub(crate) fn cleanup_failed(&self) -> bool { self.shared.control.cleanup_failed() }
    pub(crate) fn is_current(&self, identity: &BuildIdentity) -> bool {
        self.project_id == identity.project_id
            && self.root == identity.root
            && self.shared.current(identity.generation, identity.profile)
    }
    pub(crate) fn try_recv(&self) -> Result<Event, mpsc::TryRecvError> {
        self.receiver.try_recv()
    }
    pub(crate) fn cancel(&self) {
        self.shared.stopped.store(true, Ordering::Release);
        self.shared.control.cancel();
        self.shared.wake.notify_all();
    }
    pub(crate) fn is_finished(&self) -> bool {
        self.source_thread
            .as_ref()
            .is_none_or(JoinHandle::is_finished)
            && self
                .build_thread
                .as_ref()
                .is_none_or(JoinHandle::is_finished)
    }
    pub(crate) fn finish(mut self) -> Result<(), String> {
        if !self.is_finished() {
            return Err("代码迭代后台尚未收口，不能等待或丢弃作者关闭事务".into());
        }
        let source_ok = self
            .source_thread
            .take()
            .is_none_or(|thread| thread.join().is_ok());
        let build_ok = self
            .build_thread
            .take()
            .is_none_or(|thread| thread.join().is_ok());
        if !source_ok || !build_ok {
            return Err("代码迭代后台线程异常退出".into());
        }
        if self.shared.control.cleanup_failed() {
            return Err("代码迭代构建进程树未确认收口，作者现场保留".into());
        }
        Ok(())
    }

    /// 只供生命周期回归持有真实收尾线程；不创建监听、Cargo或伪造构建完成值。
    #[cfg(test)]
    pub(crate) fn held_for_test(
        project: &Project,
        profile: ProjectBuildProfile,
        generation: u64,
        cleanup_failed: bool,
    ) -> (Self, mpsc::SyncSender<()>) {
        let control = BuildControl::default();
        if cleanup_failed { control.mark_cleanup_failed(); }
        let shared = Arc::new(Shared {
            control, generation: AtomicU64::new(generation),
            profile: AtomicU8::new(encode_profile(profile)), stopped: AtomicBool::new(false),
            latest: Mutex::new(None), wake: Condvar::new(),
            output_scope: Arc::default(),
        });
        let (events, receiver) = mpsc::sync_channel(16);
        let (patch_sender, _patch_context) = mpsc::sync_channel(1);
        let (release, waiting) = mpsc::sync_channel(1);
        let source_thread = thread::spawn(move || {
            let _ = waiting.recv();
            drop(events); // 持有期间完成邮箱仍然连接，取消只撤销身份，不冒称线程已退役。
        });
        (Self {
            project_id: project.id.clone(), root: project.root.clone(), shared, receiver,
            patch_sender, last_patch_target: None,
            source_thread: Some(source_thread), build_thread: None,
        }, release)
    }
}

fn resumed_start(previous_generation: u64) -> Result<WatchStart, String> {
    let generation = previous_generation.checked_add(1)
        .ok_or("源码代次已耗尽，不能恢复自动编译；请重新打开工程")?;
    Ok(WatchStart::RebuildCurrentSource { generation })
}

impl Drop for Backend {
    fn drop(&mut self) {
        self.cancel();
    }
}

impl Drop for Shared {
    fn drop(&mut self) {
        self.control.cancel();
    }
}

fn encode_profile(profile: ProjectBuildProfile) -> u8 {
    match profile {
        ProjectBuildProfile::Debug => 0,
        ProjectBuildProfile::Release => 1,
    }
}
fn decode_profile(profile: u8) -> ProjectBuildProfile {
    if profile == 1 {
        ProjectBuildProfile::Release
    } else {
        ProjectBuildProfile::Debug
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_generation_rejects_aba_profile_and_cancelled_versions() {
        let shared = Shared {
            control: BuildControl::default(),
            generation: AtomicU64::new(5),
            profile: AtomicU8::new(0),
            stopped: AtomicBool::new(false),
            latest: Mutex::new(None),
            wake: Condvar::new(),
            output_scope: Arc::default(),
        };
        assert!(shared.current(5, ProjectBuildProfile::Debug));
        shared.advance();
        shared.advance(); // 字节可回到原值，事件身份仍然前进两代。
        assert!(!shared.current(5, ProjectBuildProfile::Debug));
        assert!(shared.current(7, ProjectBuildProfile::Debug));
        shared.profile.store(1, Ordering::Release);
        assert!(!shared.current(7, ProjectBuildProfile::Debug));
        shared.control.cancel();
        assert!(!shared.current(7, ProjectBuildProfile::Release));
    }

    #[test]
    fn resumed_start_advances_the_final_generation_and_never_reuses_a_baseline() {
        let start = resumed_start(73).unwrap();
        assert_eq!(start.generation(), 74);
        assert!(start.rebuild_current_source());
        assert!(!WatchStart::BaselineOnly { generation: 74 }.rebuild_current_source());
        assert!(resumed_start(u64::MAX).is_err(), "耗尽时不能回绕为首次打开的代次1");
    }

    #[test]
    fn resumed_backend_rejects_the_old_identity_even_with_equal_source_and_profile() {
        let start = resumed_start(73).unwrap();
        let root = PathBuf::from("C:/owned-resumed-code-fixture");
        let shared = Arc::new(Shared {
            control: BuildControl::default(), generation: AtomicU64::new(start.generation()),
            profile: AtomicU8::new(0), stopped: AtomicBool::new(false),
            latest: Mutex::new(None), wake: Condvar::new(),
            output_scope: Arc::default(),
        });
        let (_events, receiver) = mpsc::sync_channel(16);
        let (patch_sender, _patch_context) = mpsc::sync_channel(1);
        let backend = Backend {
            project_id: "owned-resumed-code".into(), root: root.clone(), shared, receiver,
            patch_sender, last_patch_target: None,
            source_thread: None, build_thread: None,
        };
        let mut identity = BuildIdentity {
            project_id: "owned-resumed-code".into(), root, profile: ProjectBuildProfile::Debug,
            generation: 73, source_sha256: "same-source".into(), context_sha256: "same-context".into(),
        };
        assert!(!backend.is_current(&identity));
        identity.generation = 74;
        assert!(backend.is_current(&identity));
        assert!(backend.generation_is_current(74, ProjectBuildProfile::Debug));
        identity.profile = ProjectBuildProfile::Release;
        assert!(!backend.is_current(&identity));
        identity.profile = ProjectBuildProfile::Debug;
        identity.project_id = "another-project".into();
        assert!(!backend.is_current(&identity));
        backend.cancel();
        identity.project_id = "owned-resumed-code".into();
        assert!(!backend.is_current(&identity));
        assert!(!backend.generation_is_current(74, ProjectBuildProfile::Debug), "停止状态同样撤销发送补丁前的原子代次门");
        backend.finish().unwrap();
    }

    #[test]
    fn generation_exhaustion_cancels_instead_of_reviving_old_tokens() {
        let shared = Shared {
            control: BuildControl::default(), generation: AtomicU64::new(u64::MAX),
            profile: AtomicU8::new(0), stopped: AtomicBool::new(false),
            latest: Mutex::new(None), wake: Condvar::new(),
            output_scope: Arc::default(),
        };
        assert_eq!(shared.advance(), u64::MAX);
        assert!(shared.stopped());
        assert!(shared.control.is_cancelled());
        assert!(!shared.current(u64::MAX, ProjectBuildProfile::Debug));
        assert!(!shared.current(1, ProjectBuildProfile::Debug));
    }

    #[test]
    fn full_patch_mailbox_retries_none_invalidation_instead_of_marking_it_delivered() {
        let shared = Arc::new(Shared {
            control: BuildControl::default(), generation: AtomicU64::new(1),
            profile: AtomicU8::new(0), stopped: AtomicBool::new(false),
            latest: Mutex::new(None), wake: Condvar::new(),
            output_scope: Arc::default(),
        });
        let (_events, receiver) = mpsc::sync_channel(16);
        let (patch_sender, patches) = mpsc::sync_channel(1);
        let previous = PatchTarget { project_id: "owned-patch-mailbox".into(), session: 7, pid: 9,
            world_id: 11, aslr_reference: 13, base_build: "fat-base".into() };
        patch_sender.try_send(None).unwrap(); // 故意占满唯一槽，模拟后台尚在真正构建。
        let mut backend = Backend {
            project_id: previous.project_id.clone(), root: PathBuf::from("C:/owned-patch-mailbox"),
            shared, receiver, patch_sender, last_patch_target: Some(previous.clone()),
            source_thread: None, build_thread: None,
        };
        backend.set_patch_context(None);
        assert_eq!(backend.last_patch_target, Some(previous));
        assert!(patches.try_recv().unwrap().is_none());
        backend.set_patch_context(None);
        assert!(backend.last_patch_target.is_none());
        assert!(patches.try_recv().unwrap().is_none());
        backend.set_patch_context(None);
        assert!(matches!(patches.try_recv(), Err(mpsc::TryRecvError::Empty)));
    }
}
