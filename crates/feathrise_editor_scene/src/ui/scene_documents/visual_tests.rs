//! 可选真实GPU标签排版验收；独立窗口只渲染生产标签控件，不载入或修改作者场景。

use super::*;
use bevy::{
    diagnostic::FrameCount,
    render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk},
    winit::WinitPlugin,
};

#[test]
#[ignore = "需要本机窗口和GPU；RPG_SCENE_TABS_VISUAL_PATH指定截图输出，单测试运行"]
fn render_scene_document_tabs_with_chinese_font() {
    let output =
        std::path::PathBuf::from(std::env::var("RPG_SCENE_TABS_VISUAL_PATH").expect("指定截图路径"));
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WinitPlugin {
                run_on_any_thread: true,
                ..default()
            })
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "场景标签排版验收".into(),
                    resolution: (800, 180).into(),
                    ..default()
                }),
                ..default()
            }),
    );
    app.insert_resource(ClearColor(crate::ui::theme::PANEL_BG_STRONG));
    let font = Font::from_bytes(
        feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES.to_vec(),
    );
    let font = app.world_mut().resource_mut::<Assets<Font>>().add(font);
    app.add_systems(Startup, move |mut commands: Commands| {
        commands.spawn(Camera2d);
        let mut docs = SceneDocuments::default();
        let first = SceneId::parse("MAP-visual-a").unwrap();
        docs.upsert(first.clone(), "开封城区·城门与街巷".into(), true);
        docs.upsert(
            SceneId::parse("MAP-visual-b").unwrap(),
            "黄河故道·渡口".into(),
            false,
        );
        docs.upsert(
            SceneId::parse("MAP-visual-c").unwrap(),
            "练功场".into(),
            false,
        );
        commands
            .spawn(Node {
                width: percent(100),
                height: percent(100),
                padding: UiRect::all(px(16)),
                ..default()
            })
            .with_children(|parent| {
                parent
                    .spawn((
                        Node {
                            height: px(30),
                            max_width: percent(100),
                            overflow: Overflow::scroll_x(),
                            ..default()
                        },
                        ScrollPosition::default(),
                    ))
                    .with_children(|tabs| {
                        spawn_scene_tabs(tabs, &font, &docs, Some(&first), PresetWorkspaceTab::Scene)
                    });
            });
    });
    let path = output.clone();
    app.add_systems(
        Update,
        move |mut commands: Commands, frames: Res<FrameCount>, mut exit: MessageWriter<AppExit>| {
            if frames.0 == 60 {
                commands
                    .spawn(Screenshot::primary_window())
                    .observe(save_to_disk(path.clone()))
                    .observe(
                        |_: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                            exit.write(AppExit::Success);
                        },
                    );
            }
            if frames.0 > 900 {
                exit.write(AppExit::error());
            } // GPU回执丢失时有界退出，不留下后台窗口。
        },
    );
    app.run();
    assert!(output.is_file(), "没有收到真实GPU截图");
}
