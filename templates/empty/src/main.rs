//! 编辑器提供的空工程运行入口；工程只依赖公共runtime，不依赖编辑器作者工具。
mod controller;
use feathrise_runtime::{GameView, ProjectContext};
fn run() -> Result<(), String> {
    // 编辑器与 cargo run 都从工程目录启动；导出包由 SDK 按 EXE 同级标记定位。
    let directory = std::env::current_dir().map_err(|error| format!("工程启动目录不可读：{error}"))?;
    let project = ProjectContext::load(
        &directory,
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
