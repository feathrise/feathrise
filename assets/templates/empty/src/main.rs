//! 编辑器提供的空工程运行入口；工程只依赖公共runtime，不依赖编辑器作者工具。
mod controller;
use feathrise_runtime::{GameView, ProjectContext};
fn run() -> Result<(), String> {
    let project = ProjectContext::load(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")),
        env!("CARGO_PKG_NAME"),
    )?;
    let view = GameView::load(&project)?;
    feathrise_runtime::run(
        project,
        controller::ProjectScenePlugin { view },
    )
}
fn main() {
    if let Err(error) = run() {
        eprintln!("工程启动失败：{error}");
        std::process::exit(1);
    }
}
