//! 绘制范围面板的固定控件及两套连续滚动行池；对象数量不会扩大UI实体批次。
use feathrise_editor_ui::metrics as ui_metrics;
use super::components::*;
use crate::ui::{
    list_layout::{SCENE_OBJECT_LIST_ROWS, SceneObjectListScroll},
    theme as colors,
};
use bevy::{prelude::*, ui::RelativeCursorPosition};

pub(crate) fn panel() -> impl Scene {
    bsn! { PaintPanel Node {width:percent(100),min_width:px(0),flex_direction:FlexDirection::Column,row_gap:px(3),flex_shrink:0.0} }
}
fn text(label: &str, font: &Handle<Font>) -> impl Bundle {
    (
        feathrise_editor_ui::localization::editor_label(label),
        TextFont {
            font: font.clone().into(),
            font_size: px(ui_metrics::FONT_CONTROL).into(),
            ..default()
        },
        TextColor(colors::TEXT_VALUE),
        Node {
            width: percent(100),
            min_width: px(0),
            ..default()
        },
    )
}
fn button(
    parent: &mut ChildSpawnerCommands,
    action: PaintAction,
    label: &str,
    font: &Handle<Font>,
) {
    parent
        .spawn((
            Button,
            action,
            Node {
                width: percent(100),
                height: px(25),
                min_width: px(0),
                padding: UiRect::horizontal(px(4)),
                border: UiRect::all(px(1)),
                align_items: AlignItems::Center,
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(colors::SLOT_BG),
            BorderColor::all(colors::COPPER_BORDER_WEAK),
        ))
        .with_children(|p| {
            p.spawn(text(label, font));
        });
}
pub(super) fn setup(
    mut commands: Commands,
    roots: Query<Entity, Added<PaintPanel>>,
    assets: Res<AssetServer>,
) {
    let font = feathrise_editor_core::editor_font(&assets, "fonts/open_source/noto_sans/cjk/NotoSansCJKsc-VF.otf");
    for root in &roots {
        commands.entity(root).with_children(|p| {
            p.spawn(Node {
                width: percent(100), min_width: px(0), height: px(24),
                padding: UiRect::right(px(22)), flex_shrink: 0.0,
                ..default()
            }).with_children(|header| {
                header.spawn(text("绘制范围", &font));
                // 原教程移至同一标题问号，绘制目标、行池与勾选输入保持原位。
                feathrise_editor_ui::context_help::spawn_help_button(header, &font, "植被绘制范围",
                    "隐藏/锁定网格跳过；移动旋转时草随网格。一笔统一撤销，Shift擦除。");
            });
            button(p, PaintAction::Current, "当前网格 · 当前作者层", &font);
            button(p, PaintAction::Cross, "跨网格 · 绘制组", &font);
            button(p, PaintAction::New, "用所选预设新建绘制组", &font);
            for kind in [ListKind::Groups, ListKind::Targets] {
                if kind == ListKind::Targets {
                    button(p, PaintAction::Bind, "绑定当前作者层到绘制组", &font);
                    button(p, PaintAction::Delete, "解除绘制组（保留已画草地）", &font);
                    p.spawn((
                        text("勾选目标 · 与检查器选择独立", &font),
                        TextLayout::linebreak(LineBreak::AnyCharacter),
                    ));
                }
                p.spawn((
                    PaintList(kind),
                    SceneObjectListScroll,
                    RelativeCursorPosition::default(),
                    ScrollPosition::default(),
                    Node {
                        display: Display::None,
                        width: percent(100),
                        height: px(0),
                        min_width: px(0),
                        overflow: Overflow::scroll_y(),
                        flex_shrink: 0.0,
                        ..default()
                    },
                ))
                .with_children(|viewport| {
                    viewport
                        .spawn((
                            PaintListContent(kind),
                            Node {
                                width: percent(100),
                                min_width: px(0),
                                position_type: PositionType::Relative,
                                flex_shrink: 0.0,
                                ..default()
                            },
                        ))
                        .with_children(|content| {
                            for slot in 0..SCENE_OBJECT_LIST_ROWS {
                                let action = if kind == ListKind::Groups {
                                    PaintAction::Group(String::new())
                                } else {
                                    PaintAction::Target(feathrise_map_content::TerrainId::legacy())
                                };
                                content
                                    .spawn((
                                        Button,
                                        PaintRow { kind, slot },
                                        action,
                                        Node {
                                            display: Display::None,
                                            position_type: PositionType::Absolute,
                                            width: percent(100),
                                            height: px(24),
                                            min_width: px(0),
                                            align_items: AlignItems::Center,
                                            padding: UiRect::horizontal(px(4)),
                                            border: UiRect::all(px(1)),
                                            overflow: Overflow::clip_x(),
                                            ..default()
                                        },
                                        BackgroundColor(colors::SLOT_BG),
                                        BorderColor::all(colors::COPPER_BORDER_WEAK),
                                    ))
                                    .with_children(|row| {
                                        row.spawn((
                                            PaintRowText { kind, slot },
                                            text("", &font),
                                            TextLayout::no_wrap(),
                                            Pickable::IGNORE,
                                        ));
                                    });
                            }
                        });
                });
            }
        });
    }
}
