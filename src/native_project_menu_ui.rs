//! 工程下拉只附加在原28px顶栏，不改原三栏、标签、工具条或作者控件。

use super::{components::*, resources::ProjectMenuSettings};
use bevy::{
    prelude::*,
    ui::{FocusPolicy, RelativeCursorPosition},
};
use feathrise_editor_map::workbench::WorkbenchHeader;
use feathrise_editor_ui::colors as c;

pub(super) fn mount(
    mut commands: Commands,
    headers: Query<Entity, Added<WorkbenchHeader>>,
    settings: Res<ProjectMenuSettings>,
    assets: Res<AssetServer>,
) {
    if crate::workspace_window::EmbedTarget::from_env().ok().flatten().is_some() {
        return; // 同窗口工程页提供唯一工程菜单；保留本模块原保存状态、反馈与退出等待。
    }
    let font = feathrise_editor_core::editor_font(&assets, feathrise_editor_core::FONT_PATH);
    for header in &headers {
        let toggle = commands
            .spawn((
                Button,
                ProjectAction::Toggle,
                crate::window_chrome::ChromeSlot::Project,
                button_node(58.),
                button_colors(),
                BackgroundColor(c::BUTTON_DEV_SMALL_NORMAL),
                BorderColor::all(c::COPPER_BORDER_WEAK),
            ))
            .with_child(caption(&font, "工程", 13.))
            .id();
        commands.entity(header).insert_children(1, &[toggle]);
        commands.entity(header).with_children(|header| {
            // 全窗口透明遮挡负责原生UI命中；原鼠标门另负责3D射线与关闭帧，不靠渲染遮挡代替输入隔离。
            header
                .spawn((
                    ProjectMenuOverlay,
                    GlobalZIndex(4200),
                    FocusPolicy::Block,
                    Interaction::None,
                    RelativeCursorPosition::default(),
                    Node {
                        display: Display::None,
                        position_type: PositionType::Absolute,
                        left: px(0),
                        top: px(0),
                        width: Val::Vw(100.),
                        height: Val::Vh(100.),
                        ..default()
                    },
                ))
                .with_children(|overlay| {
                    overlay
                        .spawn((
                            ProjectMenuPanel,
                            GlobalZIndex(4201),
                            FocusPolicy::Block,
                            Interaction::None,
                            RelativeCursorPosition::default(),
                            Node {
                                position_type: PositionType::Absolute,
                                left: px(54),
                                top: px(if cfg!(target_os="windows") {36}else{28}),
                                width: px(440),
                                max_width: percent(94),
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::all(px(12)),
                                row_gap: px(8),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(4)),
                                ..default()
                            },
                            BackgroundColor(c::PANEL_BG_STRONG),
                            BorderColor::all(c::COPPER_BORDER),
                        ))
                        .with_children(|panel| {
                            panel.spawn(caption(&font, &settings.project.name, 14.));
                            panel.spawn((
                                caption(&font, &crate::project::display_path(&settings.project.root), 12.),
                                TextLayout {
                                    linebreak: bevy::text::LineBreak::AnyCharacter,
                                    ..default()
                                },
                                Node {
                                    width: percent(100),
                                    max_height: px(64),
                                    overflow: Overflow::clip_y(),
                                    ..default()
                                },
                            ));
                            button(
                                panel,
                                &font,
                                ProjectAction::SetStartup,
                                "当前地图设为启动地图",
                            );
                            panel.spawn(caption(
                                &font,
                                "此操作只保存启动设置；地图草稿请使用原保存按钮。",
                                12.,
                            ));
                            button(panel, &font, ProjectAction::Export, "导出工程…");
                            panel.spawn((
                                ProjectMenuStatus,
                                caption(&font, "", 12.),
                                Node {
                                    width: percent(100),
                                    max_height: px(84),
                                    overflow: Overflow::clip_y(),
                                    ..default()
                                },
                            ));
                            button(
                                panel,
                                &font,
                                ProjectAction::Plugins,
                                "插件管理…",
                            );
                            button(
                                panel,
                                &font,
                                ProjectAction::BackToManager,
                                if settings.managed {
                                    "返回工程管理"
                                } else {
                                    "退出编辑器"
                                },
                            );
                        });
                });
            // 原底栏的全局详情保留结果；菜单关闭或切地图也不会吃掉设置保存失败。
            header.spawn((
                ProjectMenuStatus,
                feathrise_editor_ui::PanelStatusText("工程启动设置"),
                feathrise_editor_ui::feedback::GlobalPanelStatus,
                Text::new(""),
                Node {
                    display: Display::None,
                    ..default()
                },
            ));
        });
    }
}

fn caption(font: &Handle<Font>, label: &str, size: f32) -> impl Bundle {
    (
        feathrise_editor_ui::localization::editor_label(label),
        TextFont {
            font: font.clone().into(),
            font_size: size.into(),
            ..default()
        },
        TextColor(c::TEXT_PRIMARY),
        Pickable::IGNORE,
    )
}

fn button_node(width: f32) -> Node {
    Node {
        width: px(width),
        height: px(26),
        flex_shrink: 0.,
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        padding: UiRect::horizontal(px(6)),
        border: UiRect::all(px(1)),
        border_radius: BorderRadius::all(px(3)),
        ..default()
    }
}

fn button_colors() -> feathrise_editor_ui::ButtonColors {
    feathrise_editor_ui::ButtonColors::new(
        c::BUTTON_DEV_SMALL_NORMAL,
        c::BUTTON_DEV_SMALL_HOVERED,
        c::BUTTON_DEV_SMALL_PRESSED,
    )
}

fn button(
    panel: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    action: ProjectAction,
    label: &str,
) {
    let mut node = button_node(0.);
    node.width = percent(100);
    panel
        .spawn((
            Button,
            action,
            node,
            button_colors(),
            BackgroundColor(c::BUTTON_DEV_SMALL_NORMAL),
            BorderColor::all(c::COPPER_BORDER_WEAK),
        ))
        .with_child(caption(font, label, 13.));
}
