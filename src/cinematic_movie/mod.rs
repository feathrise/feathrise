//! 演出文档发出导出意图，宿主使用本工程自己的运行程序摄影。游戏保存和作者预览不共用此会话。
mod ui;
mod worker;
use crate::{build_process::BuildControl, project::Project};
use bevy::prelude::*;
use feathrise_cinematic::movie::{MovieChoicePrompt, MovieSettings};
use feathrise_editor_documents_model::cinematic_export::OpenCinematicExport;
use std::{
    path::PathBuf,
    sync::{Mutex, mpsc},
};

struct Options {
    asset: PathBuf,
    root: PathBuf,
    output: PathBuf,
    scene: Option<String>,
    settings: MovieSettings,
    ffmpeg: Option<PathBuf>,
    sound: Option<PathBuf>,
    replace: bool,
}
struct Running {
    control: BuildControl,
    task: std::thread::JoinHandle<Result<PathBuf, String>>,
    progress: Mutex<mpsc::Receiver<Feedback>>,
}
#[derive(Debug)]
enum Feedback {
    Status(String),
    Choice(MovieChoicePrompt),
}
#[derive(Resource)]
struct MoviePanel {
    choice: Option<MovieChoicePrompt>,
    project: Project,
    target: PathBuf,
    document: Option<OpenCinematicExport>,
    status: String,
    advanced: bool,
    replace: bool,
    running: Option<Running>,
}
pub(crate) struct CinematicMoviePlugin(pub Project, pub PathBuf);
impl Plugin for CinematicMoviePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<OpenCinematicExport>()
            .insert_resource(MoviePanel {
                choice: None,
                project: self.0.clone(),
                target: self.1.clone(),
                document: None,
                status: String::new(),
                advanced: false,
                replace: false,
                running: None,
            })
            .add_systems(
                Update,
                (
                    ui::open,
                    ui::actions,
                    ui::scroll,
                    ui::select_choice,
                    poll,
                    ui::choice_buttons,
                    ui::refresh,
                )
                    .chain()
                    .after(bevy::text::apply_text_edits)
                    .after(crate::project_export::ExportInput)
                    .before(crate::window_chrome::ChromeInput)
                    .before(crate::workspace_ui::WorkspaceUiSet::Input)
                    .before(crate::viewport_mode::ViewportModeInput)
                    .before(feathrise_editor_ui::EditorUiSystemSet::NumericInput)
                    .before(feathrise_editor_core::SceneEditorSystemSet::Input),
            );
    }
}
fn launch(state: &mut MoviePanel, options: Options) {
    state.choice = None;
    let project = state.project.clone();
    let target = state.target.clone();
    let control = BuildControl::default();
    let worker_control = control.clone();
    let (tx, rx) = mpsc::channel();
    // 构建和编码要阻塞等子进程，单个导出工作线程不占用编辑器用于读取地图与资产的 Bevy IO 池。
    let task = match std::thread::Builder::new()
        .name("cinematic-movie".into())
        .spawn(move || worker::export(project, target, options, &worker_control, tx))
    {
        Ok(task) => task,
        Err(error) => {
            state.status = format!("无法启动影片导出：{error}");
            return;
        }
    };
    state.running = Some(Running {
        task,
        control,
        progress: Mutex::new(rx),
    });
    state.status = "正在检查演出、输出文件及编码器".into();
}
fn poll(mut state: ResMut<MoviePanel>) {
    let Some(job) = &mut state.bypass_change_detection().running else {
        return;
    };
    let message = job
        .progress
        .get_mut()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .try_iter()
        .last();
    let finished = job.task.is_finished();
    if let Some(message) = message {
        match message {
            Feedback::Status(message) => state.status = message,
            Feedback::Choice(choice) => state.choice = Some(choice),
        }
    }
    if finished {
        let job = state.running.take().unwrap();
        let result = job
            .task
            .join()
            .unwrap_or_else(|_| Err("影片导出线程意外退出".into()));
        state.status = match result {
            Ok(path) => format!("已导出 {}", path.display()),
            Err(error) => error,
        };
    }
}
pub(crate) fn ready_to_exit(world: &mut World) -> bool {
    if let Some(state) = world.get_resource::<MoviePanel>()
        && let Some(job) = &state.running
    {
        job.control.cancel();
        return false;
    }
    true
}
