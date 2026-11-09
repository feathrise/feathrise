//! 视口、构建方式和Debug/Release按工程保存在作者数据目录，旧全局侧文件只用于首次继承模式。
//! 最多一个后台写入和一份最新选择；只有成功回执才能更新磁盘身份，失败保留最后有效文件。

use super::{ViewportMode, ViewportModeState};
use crate::user_preferences::{Preferences, publish_checked, read_optional};
use crate::{native_play::ProjectBuildBackend, project::{Project, ProjectContext}};
use feathrise_editor_core::{ProjectBuildProfile, ProjectLaunchSettings};
use async_channel::{Receiver, TryRecvError};
use bevy::{prelude::*, tasks::IoTaskPool};
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, path::PathBuf, rc::Rc};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    format: u32,
    #[serde(default)]
    mode: ViewportMode,
    #[serde(default)]
    profile: ProjectBuildProfile,
    #[serde(default)]
    backend: ProjectBuildBackend,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Selection {
    mode: ViewportMode,
    profile: ProjectBuildProfile,
    backend: ProjectBuildBackend,
}

struct Store {
    path: PathBuf,
    original: Option<Vec<u8>>,
    mode: ViewportMode,
    profile: ProjectBuildProfile,
    backend: ProjectBuildBackend,
    context: Option<ProjectContext>,
}

impl Store {
    fn load(path: PathBuf) -> Result<Self, String> {
        let original = read_optional(&path)?;
        let selection = if let Some(bytes) = original.as_deref() {
            let stored: Stored = serde_json::from_slice(bytes)
                .map_err(|error| format!("视口偏好损坏，原文保留：{error}"))?;
            if stored.format != 1 {
                return Err(format!("不支持视口偏好版本{}，原文保留", stored.format));
            }
            Selection { mode: stored.mode, profile: stored.profile, backend: stored.backend }
        } else {
            Selection::default()
        };
        Ok(Self {
            path,
            original,
            mode: selection.mode,
            profile: selection.profile,
            backend: selection.backend,
            context: None,
        })
    }

    fn for_project(project: &Project, legacy: PathBuf) -> Result<Self, String> {
        let mut store = Self::load(project.context.checked_data_directory()?.join("launch.json"))?;
        if store.original.is_none() {
            // 只继承过去已保存的视口模式；旧全局配置不再拥有本工程后续选择的写权。
            store.mode = Self::load(legacy)?.mode;
        }
        store.context = Some(project.context.clone());
        Ok(store)
    }

    fn selection(&self) -> Selection {
        Selection { mode: self.mode, profile: self.profile, backend: self.backend }
    }

    fn set_selection(&mut self, selection: Selection) {
        self.mode = selection.mode;
        self.profile = selection.profile;
        self.backend = selection.backend;
    }

    fn save(&mut self) -> Result<(), String> {
        if let Some(context) = &self.context {
            // 每次后台写入重新核对工程边界，防止打开后替换目录将配置写到其它工程。
            if context.ensure_data_directory()?.join("launch.json") != self.path {
                return Err("工程启动配置路径已变化，原文保留".into());
            }
        }
        let mut bytes = serde_json::to_vec_pretty(&Stored {
            format: 1,
            mode: self.mode,
            profile: self.profile,
            backend: self.backend,
        })
        .map_err(|error| error.to_string())?;
        bytes.push(b'\n');
        publish_checked(&self.path, &self.original, &bytes)?;
        self.original = Some(bytes);
        Ok(())
    }
}

struct Written {
    store: Store,
    revision: u64,
    result: Result<(), String>,
}

struct State {
    store: Option<Store>,
    current: Selection,
    committed: Selection,
    revision: u64,
    job: Option<Receiver<Written>>,
    failure: Option<(u64, String)>,
}

impl State {
    fn capture(&mut self, selection: Selection) {
        if self.current != selection {
            self.current = selection;
            self.revision = self.revision.wrapping_add(1).max(1);
            self.failure = None;
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
                self.committed = written.store.selection();
                self.failure = None;
            }
            Err(error) => {
                self.failure = Some((written.revision, error));
            }
        }
        self.store = Some(written.store);
        self.job = None;
    }

    fn advance(&mut self) {
        match self.job.as_ref().map(|job| job.try_recv()) {
            Some(Ok(written)) => self.adopt(written),
            Some(Err(TryRecvError::Closed)) => {
                self.job = None;
                self.failure = Some((self.revision, "视口偏好写入任务异常结束，原文保留".into()));
            }
            Some(Err(TryRecvError::Empty)) | None => (),
        }
        if self.job.is_some() || !self.dirty() || self.error().is_some() {
            return;
        }
        let Some(mut store) = self.store.take() else {
            self.failure = Some((
                self.revision,
                "视口偏好写入槽未归还原文身份，请重新打开编辑器".into(),
            ));
            return;
        };
        store.set_selection(self.current);
        let revision = self.revision;
        let (sender, receiver) = async_channel::bounded(1);
        self.job = Some(receiver);
        IoTaskPool::get()
            .spawn(async move {
                let result = store.save();
                let _ = sender.try_send(Written {
                    store,
                    revision,
                    result,
                });
            })
            .detach();
    }
}

struct Runtime(Rc<RefCell<State>>);

pub(crate) struct Completion(Rc<RefCell<State>>);

pub(super) fn install(app: &mut App, preferences: &Preferences, project: Option<&Project>) -> Completion {
    let legacy = preferences.file().with_extension("viewport.json");
    let path = project.map_or_else(|| legacy.clone(), |project| project.context.editor_data.join("launch.json"));
    let loaded = match project {
        Some(project) => Store::for_project(project, legacy),
        None => Store::load(path.clone()),
    };
    let (store, error) = match loaded {
        Ok(store) => (store, None),
        Err(error) => (
            Store {
                path,
                original: None,
                mode: ViewportMode::default(),
                profile: ProjectBuildProfile::default(),
                backend: ProjectBuildBackend::default(),
                context: project.map(|project| project.context.clone()),
            },
            Some(error),
        ),
    };
    let selection = store.selection();
    let state = Rc::new(RefCell::new(State {
        store: Some(store),
        current: selection,
        committed: selection,
        revision: 1,
        job: None,
        failure: None,
    }));
    app.insert_resource(ViewportModeState {
        selected: selection.mode,
        revision: 1,
        status: error.unwrap_or_default(),
    })
    .insert_resource(selection.backend)
    .init_resource::<ProjectLaunchSettings>()
    .insert_non_send(Runtime(Rc::clone(&state)))
    .add_systems(Last, capture_and_poll);
    // 先于代码监听和首次工程预览恢复，避免按默认DX开始一次无用构建。
    app.world_mut().resource_mut::<ProjectLaunchSettings>().profile = selection.profile;
    Completion(state)
}

fn capture_and_poll(mut mode: ResMut<ViewportModeState>, launch: Res<ProjectLaunchSettings>, backend: Res<ProjectBuildBackend>, runtime: NonSend<Runtime>) {
    let mut state = runtime.0.borrow_mut();
    state.capture(Selection { mode: mode.selected, profile: launch.profile, backend: *backend });
    state.advance();
    if let Some(error) = state.error() {
        let message = format!("工程启动配置尚未保存：{error}");
        if mode.status != message {
            mode.status = message;
        }
    }
}

/// 作者退出已经获准后再排空；帧中只检查真实回执，不等待线程或磁盘。
pub(crate) fn ready_to_exit(world: &mut World, retry: bool) -> Result<bool, String> {
    let Some(runtime) = world.get_non_send::<Runtime>() else {
        return Ok(true);
    };
    let mut state = runtime.0.borrow_mut();
    state.capture(Selection {
        mode: world.resource::<ViewportModeState>().selected,
        profile: world.resource::<ProjectLaunchSettings>().profile,
        backend: *world.resource::<ProjectBuildBackend>(),
    });
    if retry {
        state.failure = None;
    }
    state.advance();
    if state.dirty()
        && let Some(error) = state.error()
    {
        return Err(format!("工程启动配置未保存：{error}"));
    }
    Ok(!state.dirty() && state.job.is_none())
}

impl Completion {
    /// 仅 App::run 返回后调用；候选作者被退役时也保留最后用户选择，正常关闭应已异步排空。
    pub(crate) fn finish(self) -> Result<(), String> {
        let mut state = self.0.borrow_mut();
        if let Some(job) = state.job.take() {
            let written = job
                .recv_blocking()
                .map_err(|_| "视口偏好后台任务没有返回结果".to_owned())?;
            state.adopt(written);
        }
        if !state.dirty() {
            return Ok(());
        }
        if let Some(error) = state.error() {
            return Err(error.to_owned());
        }
        let selection = state.current;
        let store = state.store.as_mut().ok_or("视口偏好写入槽未归还")?;
        store.set_selection(selection);
        store.save()?;
        state.committed = selection;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "feathrise-viewport-preferences-{}-{}-{}.json",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            assert!(!path.exists());
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[test]
    fn missing_selection_keeps_original_mode_and_choice_survives_reopen() {
        let fixture = Fixture::new();
        let mut store = Store::load(fixture.0.clone()).unwrap();
        assert_eq!(store.mode, ViewportMode::SavedWorld);
        assert!(!fixture.0.exists());
        store.mode = ViewportMode::EmbeddedWorld;
        store.save().unwrap();
        assert_eq!(
            Store::load(fixture.0.clone()).unwrap().mode,
            ViewportMode::EmbeddedWorld
        );
        store.mode = ViewportMode::SavedWorld;
        store.save().unwrap();
        assert_eq!(
            Store::load(fixture.0.clone()).unwrap().mode,
            ViewportMode::SavedWorld
        );
    }

    #[test]
    fn corrupt_and_external_preferences_are_not_overwritten() {
        let fixture = Fixture::new();
        let mut store = Store::load(fixture.0.clone()).unwrap();
        fs::write(&fixture.0, b"broken").unwrap();
        assert!(Store::load(fixture.0.clone()).is_err());
        assert!(store.save().is_err());
        assert_eq!(fs::read(&fixture.0).unwrap(), b"broken");
        fs::remove_file(&fixture.0).unwrap();
        store.save().unwrap();
        let mut stale = Store::load(fixture.0.clone()).unwrap();
        store.mode = ViewportMode::EmbeddedWorld;
        store.save().unwrap();
        assert!(stale.save().is_err());
        assert_eq!(
            Store::load(fixture.0.clone()).unwrap().mode,
            ViewportMode::EmbeddedWorld
        );
    }

    struct ProjectFixture {
        root: PathBuf,
        project: Project,
    }

    impl ProjectFixture {
        fn new() -> Self {
            let root = Fixture::new().0.with_extension("project");
            fs::create_dir(&root).unwrap();
            fs::create_dir(root.join("assets")).unwrap();
            fs::write(root.join("Cargo.toml"), "[package]\nname='launch_fixture'\nversion='0.1.0'\n").unwrap();
            fs::write(root.join("editor-project.json"), br#"{"format":2,"id":"8f6700ee-7cd3-45d0-b89b-e95ed950b806","name":"launch fixture","cargo_manifest":"Cargo.toml","package":"launch_fixture","asset_roots":["assets"],"editor_data":"author-state"}"#).unwrap();
            let project = crate::project::open(&root).unwrap();
            Self { root, project }
        }
    }

    impl Drop for ProjectFixture {
        fn drop(&mut self) {
            // 只清理本测试独占创建、位于系统临时根且不含Cargo缓存的夹具目录。
            let temporary = std::env::temp_dir().canonicalize().unwrap();
            if self.root.canonicalize().ok().is_some_and(|root| root.parent() == Some(temporary.as_path()))
                && self.root.file_name().is_some_and(|name| name.to_string_lossy().starts_with("feathrise-viewport-preferences-")) {
                let _ = fs::remove_dir_all(&self.root);
            }
        }
    }

    #[test]
    fn project_choices_survive_reopen_and_remain_separate_from_other_projects_and_legacy() {
        let legacy = Fixture::new();
        // 旧文件缺少新增字段时保持Debug/DX，且不在读取时改写原文。
        fs::write(&legacy.0, br#"{"format":1,"mode":"embedded_world"}"#).unwrap();
        let original = fs::read(&legacy.0).unwrap();
        let first = ProjectFixture::new();
        let second = ProjectFixture::new();
        let descriptor = fs::read(first.root.join("editor-project.json")).unwrap();
        let mut store = Store::for_project(&first.project, legacy.0.clone()).unwrap();
        assert_eq!(store.mode, ViewportMode::EmbeddedWorld);
        assert_eq!(store.profile, ProjectBuildProfile::Debug);
        assert_eq!(store.backend, ProjectBuildBackend::Dx);
        assert!(!store.path.exists());
        store.backend = ProjectBuildBackend::Native;
        store.profile = ProjectBuildProfile::Release;
        store.save().unwrap();
        assert!(store.path.starts_with(first.root.join("author-state").canonicalize().unwrap()));
        let reopened = Store::for_project(&first.project, legacy.0.clone()).unwrap();
        assert_eq!(reopened.backend, ProjectBuildBackend::Native);
        assert_eq!(reopened.profile, ProjectBuildProfile::Release);
        assert_eq!(Store::for_project(&second.project, legacy.0.clone()).unwrap().backend, ProjectBuildBackend::Dx);
        assert_eq!(fs::read(&legacy.0).unwrap(), original);
        assert_eq!(fs::read(first.root.join("editor-project.json")).unwrap(), descriptor);
        let stored: serde_json::Value = serde_json::from_slice(&fs::read(store.path).unwrap()).unwrap();
        assert_eq!(stored["backend"], "cargo");
        assert_eq!(stored["profile"], "release");
    }

    #[test]
    fn last_frame_autosaves_latest_choices_and_restores_before_next_build_setup() {
        let fixture = ProjectFixture::new();
        let preferences = Preferences::load_at(&fixture.root.join("user/editor.json"), &fixture.root).unwrap();
        let mut app = App::new();
        app.add_plugins(TaskPoolPlugin::default());
        let completion = install(&mut app, &preferences, Some(&fixture.project));
        app.insert_resource(ProjectBuildBackend::Native);
        app.update();
        // 在第一份写入尚可能在途时再变配置，收口必须落盘末次完整选择。
        app.world_mut().resource_mut::<ProjectLaunchSettings>().profile = ProjectBuildProfile::Release;
        app.world_mut().resource_mut::<ViewportModeState>().selected = ViewportMode::EmbeddedWorld;
        app.update();
        completion.finish().unwrap();
        let mut reopened = App::new();
        let completion = install(&mut reopened, &preferences, Some(&fixture.project));
        assert_eq!(ProjectBuildBackend::selected(reopened.world()), ProjectBuildBackend::Native);
        assert_eq!(reopened.world().resource::<ProjectLaunchSettings>().profile, ProjectBuildProfile::Release);
        assert_eq!(reopened.world().resource::<ViewportModeState>().selected, ViewportMode::EmbeddedWorld);
        completion.finish().unwrap();
    }
}
