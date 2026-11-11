//! 一次建立八行空气墙列表和紧凑操作区，字体显式使用项目中文字体。
use feathrise_editor_ui::UiInteraction;
use super::components::*;
use crate::ui::theme as colors;
use bevy::prelude::*;
use bevy::text::LineHeight;
fn list_row() -> Node {
    Node {
        width: percent(100),
        min_width: px(0),
        min_height: px(34),
        padding: UiRect::all(px(4)),
        border: UiRect::all(px(1)),
        border_radius: BorderRadius::all(px(3)),
        align_items: AlignItems::Center,
        flex_shrink: 0.0,
        ..default()
    }
}

fn action_button() -> Node {
    // Action和WallRow必需Button以接收释放点击；作者面板在表现阶段保留选择与动作底色。
    Node {
        width: percent(100),
        min_width: px(0),
        height: px(26),
        padding: UiRect::horizontal(px(3)),
        border: UiRect::all(px(1)),
        border_radius: BorderRadius::all(px(3)),
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        flex_shrink: 0.0,
        ..default()
    }
}
pub(super) fn setup(
    mut commands: Commands,
    assets: Res<AssetServer>,
    slots: Query<Entity, Added<WallPanelSlot>>,
) {
    // 只处理新增面板。
    let font = feathrise_editor_core::editor_font(&assets, crate::ui::scene::FONT_PATH); // 所有标签支持中文。
    for slot in &slots {
        // 编辑器通常只有一个挂载槽。
        commands.entity(slot).with_children(|parent| {
            parent.spawn((
                WallTitle,
                feathrise_editor_ui::localization::editor_label("空气墙"),
                TextFont {
                    font: font.clone().into(), // 显式中文字体，不依赖继承。
                    font_size: px(10).into(),  // 标题稍高于正文。
                    ..default()
                },
                TextColor(colors::TEXT_MUTED),
                TextLayout {
                    linebreak: LineBreak::AnyCharacter,
                    ..default()
                },
                Node {
                    width: percent(100),
                    min_width: px(0),
                    flex_shrink: 0.0,
                    ..default()
                },
            )); // 动态标题。
            for index in 0..8 {
                // 固定行池，不为每面墙创建实体。
                let mut row = parent.spawn((
                    WallRow(index),
                    UiInteraction::None,
                    list_row(),
                    BackgroundColor(colors::SLOT_BG),
                    BorderColor::all(colors::COPPER_BORDER_WEAK),
                )); // 独立交互样式。
                let entity = row.id(); // 子文字绑定当前行。
                row.with_children(|row| {
                    row.spawn((
                        WallLabel(entity),
                        Text::new(""),
                        TextFont {
                            font: font.clone().into(), // 显式中文字体，不依赖继承。
                            font_size: px(9.5).into(), // 紧凑正文大小。
                            ..default()
                        },
                        TextColor(colors::TEXT_PRIMARY),
                        TextLayout {
                            linebreak: LineBreak::AnyCharacter,
                            ..default()
                        },
                        Node {
                            width: percent(100),
                            min_width: px(0),
                            ..default()
                        },
                    ));
                }); // 显式中文字体。
            }
            for (controls, pair) in [
                (
                    Some(WallControls::Pagination),
                    [("上一页", Action::Previous), ("下一页", Action::Next)],
                ),
                (
                    None,
                    [
                        ("新建内部墙", Action::CreateInternal),
                        ("新建外框", Action::CreateOuter),
                    ],
                ),
                (
                    Some(WallControls::Selection),
                    [("复制所选", Action::Copy), ("删除所选", Action::Delete)],
                ),
                (
                    Some(WallControls::Selection),
                    [
                        ("切换碰撞", Action::Enabled),
                        ("切换辅助线", Action::Visible),
                    ],
                ),
            ] {
                // 两列布局减少滚动。
                let mut pair_row = parent.spawn(Node {
                    width: percent(100), // 占满所在栏宽度。
                    min_width: px(0),
                    column_gap: px(3), // 双列操作留出铜框间距。
                    flex_shrink: 0.0,
                    ..default()
                });
                if let Some(controls) = controls {
                    pair_row.insert(controls);
                }
                pair_row.with_children(|row| {
                    for (label, action) in pair {
                        // 同行等宽。
                        row.spawn((
                            action,
                            UiInteraction::None,
                            Node {
                                flex_basis: px(0),
                                flex_grow: 1.0,
                                min_width: px(0), // 扣除间距后严格等宽，避免两列超过容器。
                                ..action_button()
                            },
                            BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL),
                            BorderColor::all(colors::COPPER_BORDER_WEAK),
                        ))
                        .with_children(|button| {
                            button.spawn((
                                Text::new(label),
                                TextFont {
                                    font: font.clone().into(), // 显式中文字体，不依赖继承。
                                    font_size: px(11).into(),
                                    ..default()
                                },
                                TextColor(colors::TEXT_PRIMARY),
                            )); // 不依赖继承文字渲染。
                        });
                    }
                });
            }
            parent.spawn((
                WallControls::Selection,
                feathrise_editor_ui::localization::editor_label("所选空气墙名称"),
                TextFont {
                    font: font.clone().into(),
                    font_size: px(9.5).into(),
                    ..default()
                },
                TextColor(colors::TEXT_MUTED),
                Node {
                    width: percent(100),
                    min_width: px(0),
                    flex_shrink: 0.0,
                    ..default()
                },
            ));
            parent.spawn((
                WallControls::Selection,
                WallName,
                UiInteraction::None,
                crate::ui::text_input::single_line_input("", 64),
                crate::ui::text_input::EditorTextInput,
                TextFont {
                    font: font.clone().into(), // 显式中文字体，不依赖继承。
                    font_size: px(11).into(),
                    ..default()
                },
                TextColor(colors::TEXT_PRIMARY),
                TextLayout::no_wrap(),
                LineHeight::Px(14.0),
                Node {
                    // EditableText直接位于根上；固定行高和对称内边距才能真正居中字形与光标。
                    padding: UiRect::all(px(5)),
                    overflow: Overflow::clip_x(),
                    ..action_button()
                },
                BackgroundColor(colors::INPUT_BG),
                BorderColor::all(colors::INPUT_BORDER),
            )); // 官方中文输入与焦点。
            parent
                .spawn((
                    Action::Rename,
                    WallControls::Selection,
                    UiInteraction::None,
                    action_button(),
                    BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL),
                    BorderColor::all(colors::COPPER_BORDER_WEAK),
                ))
                .with_children(|button| {
                    button.spawn((
                        feathrise_editor_ui::localization::editor_label("应用名称"),
                        TextFont {
                            font: font.clone().into(), // 显式中文字体，不依赖继承。
                            font_size: px(11).into(),
                            ..default()
                        },
                        TextColor(colors::TEXT_PRIMARY),
                    )); // 显式提交名称，不逐字制造历史。
                });
        });
    }
}
