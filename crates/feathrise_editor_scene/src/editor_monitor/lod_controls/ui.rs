//! F4 六个植被页的四档 LOD 控件；布局、字号、颜色和交互逐项复刻 F3。
//! 【功能设计】同一组固定按钮按当前页面绑定共享状态，叶片与Mesh显示各自真实档位名称。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::EditorButtonClick;
use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use feathrise_fps_display::{ALL_LODS_ENABLED, LOD_LEVEL_COUNT, LodControlState}; // 直接读写F3/F4唯一共享状态，避免面板之间互相恢复已关闭档位。

use super::state::content_for_editor_page; // 当前六个植被页映射到共享内容身份。
use crate::monitor_resources::EditorSceneMonitorState; // 当前页决定共享内容和控件显隐。

#[derive(Component)]
pub(in crate::editor_monitor) struct EditorVegetationLodControls;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditorVegetationLodAction {
    Toggle(usize),
    Solo(usize),
    EnableAll,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::editor_monitor) struct EditorVegetationLodButton {
    action: EditorVegetationLodAction,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::editor_monitor) struct EditorVegetationLodButtonLabel {
    action: EditorVegetationLodAction,
}

/// 在二级页签与正文之间创建 F3 同款两行 LOD 控件。
pub(in crate::editor_monitor) fn spawn_vegetation_lod_controls(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
) {
    parent
        .spawn((
            EditorVegetationLodControls,
            Node {
                width: Val::Percent(100.0),
                min_width: Val::Px(0.0),
                padding: UiRect::all(Val::Px(5.0)),
                border: UiRect::all(Val::Px(1.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                display: Display::None, // F4默认打开总览；只有六个植被页占用LOD控件高度。
                ..default()
            },
            BackgroundColor(Color::srgba(0.044, 0.036, 0.030, 0.72)),
            BorderColor::all(Color::srgba(0.520, 0.380, 0.190, 0.44)),
            FocusPolicy::Pass,
        ))
        .with_children(|controls| {
            spawn_control_row(
                controls,
                font,
                "档位开关",
                true,
                EditorVegetationLodAction::Toggle,
            );
            spawn_control_row(
                controls,
                font,
                "单独查看",
                false,
                EditorVegetationLodAction::Solo,
            );
        });
}

fn spawn_control_row(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    title: &'static str,
    include_enable_all: bool,
    action_for_level: fn(usize) -> EditorVegetationLodAction,
) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(5.0),
                ..default()
            },
            FocusPolicy::Pass,
        ))
        .with_children(|row| {
            row.spawn((
                Text::new(title),
                TextFont {
                    font: font.clone().into(),
                    font_size: 11.0.into(),
                    ..default()
                },
                TextColor(Color::srgba(0.83, 0.74, 0.58, 0.94)),
                Node {
                    width: Val::Px(66.0),
                    ..default()
                },
                FocusPolicy::Pass,
            ));
            for level in 0..LOD_LEVEL_COUNT {
                spawn_action_button(row, font, action_for_level(level));
            }
            if include_enable_all {
                spawn_action_button(row, font, EditorVegetationLodAction::EnableAll);
            }
        });
}

fn spawn_action_button(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    action: EditorVegetationLodAction,
) {
    let button_width = if action == EditorVegetationLodAction::EnableAll {
        104.0
    } else {
        112.0
    };
    parent
        .spawn((
            InteractionButton,
            EditorVegetationLodButton { action },
            Node {
                width: Val::Px(button_width),
                height: Val::Px(24.0),
                padding: UiRect::horizontal(Val::Px(7.0)),
                border: UiRect::all(Val::Px(1.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.055, 0.044, 0.035, 0.90)),
            BorderColor::all(Color::srgba(0.52, 0.38, 0.19, 0.56)),
            FocusPolicy::Block,
        ))
        .with_child((
            EditorVegetationLodButtonLabel { action },
            Text::new("等待同步"),
            TextFont {
                font: font.clone().into(),
                font_size: 11.0.into(),
                ..default()
            },
            TextColor(Color::srgba(0.94, 0.88, 0.73, 0.96)),
            FocusPolicy::Pass,
        ));
}

/// 响应当前植被页的独立开关、单档隔离和全部恢复。
pub(in crate::editor_monitor) fn handle_vegetation_lod_control_interactions(
    mut monitor: ResMut<EditorSceneMonitorState>,
    bindings: Option<Res<feathrise_fps_display::LodControlPresetBindings>>,
    mut lod_state: ResMut<LodControlState>,
    buttons: Query<
        (&EditorVegetationLodButton, &EditorButtonClick),
        (With<InteractionButton>, Changed<EditorButtonClick>),
    >,
) {
    let Some(content) = content_for_editor_page(monitor.page) else {
        return; // 非植被页没有可操作的四档内容。
    }; // 固定本次点击所属共享内容，避免切页后串改。
    if !monitor.visible || !monitor.page.available(bindings.as_deref()) {
        return;
    }
    let mut changed = false;
    for (button, interaction) in &buttons {
        if !interaction.is_clicked() {
            continue;
        }
        match button.action {
            EditorVegetationLodAction::Toggle(level) => lod_state.toggle(content, level),
            EditorVegetationLodAction::Solo(level) => lod_state.solo(content, level),
            EditorVegetationLodAction::EnableAll => lod_state.enable_all(content),
        }
        changed = true;
    }
    if changed {
        monitor.force_refresh = true;
    }
}

/// 同步六个植被页控件显隐、标签和 F3 同款水墨铜木状态色。
pub(in crate::editor_monitor) fn update_vegetation_lod_control_ui(
    monitor: Res<EditorSceneMonitorState>,
    bindings: Option<Res<feathrise_fps_display::LodControlPresetBindings>>,
    lod_state: Res<LodControlState>,
    mut roots: Query<&mut Node, With<EditorVegetationLodControls>>,
    mut buttons: Query<
        (
            &EditorVegetationLodButton,
            &UiInteraction,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        With<InteractionButton>,
    >,
    mut labels: Query<(&EditorVegetationLodButtonLabel, &mut Text)>,
) {
    let content = content_for_editor_page(monitor.page); // 六个植被页都映射到唯一共享内容。
    let supported = content.is_some() && monitor.page.available(bindings.as_deref());
    for mut node in &mut roots {
        node.display = if supported {
            Display::Flex
        } else {
            Display::None
        };
    }
    if !supported {
        return;
    }
    let content = content.expect("显示F4逐档控件的页面必须有共享内容身份"); // 上方已经排除非植被页。
    let mask = lod_state.mask(content); // 当前页只读取自身四位掩码。
    for (button, interaction, mut background, mut border) in &mut buttons {
        let selected = match button.action {
            EditorVegetationLodAction::Toggle(level) => {
                mask & (1_u8 << level.min(LOD_LEVEL_COUNT - 1)) != 0
            }
            EditorVegetationLodAction::Solo(level) => {
                mask == 1_u8 << level.min(LOD_LEVEL_COUNT - 1)
            }
            EditorVegetationLodAction::EnableAll => mask == ALL_LODS_ENABLED,
        };
        *background = BackgroundColor(if selected {
            Color::srgba(0.38, 0.24, 0.10, 0.94)
        } else if *interaction == UiInteraction::Hovered {
            Color::srgba(0.22, 0.16, 0.09, 0.90)
        } else {
            Color::srgba(0.055, 0.044, 0.035, 0.90)
        });
        *border = BorderColor::all(if selected {
            Color::srgba(0.88, 0.66, 0.31, 0.92)
        } else {
            Color::srgba(0.52, 0.38, 0.19, 0.56)
        });
    }
    for (label, mut text) in &mut labels {
        let level_label = |level| {
            if bindings.as_deref().and_then(|bindings| bindings.profile(content)).is_none() {
                format!("LOD{level}") // 通用预设不保证末档为树木公告板，显示真实档号。
            } else { content.level_label(level).to_owned() }
        };
        let next = match label.action {
            EditorVegetationLodAction::Toggle(level) => format!(
                "{}：{}",
                level_label(level),
                if lod_state.is_enabled(content, level) {
                    "开"
                } else {
                    "关"
                }
            ),
            EditorVegetationLodAction::Solo(level) => {
                format!("只看{}", level_label(level))
            }
            EditorVegetationLodAction::EnableAll => "全部开启".to_owned(),
        };
        if text.as_str() != next {
            **text = next;
        }
    }
}

#[cfg(test)] // 最小UI世界验证六页显隐，不启动窗口或渲染器。
mod tests {
    use super::*; // 直接运行生产UI同步系统。
    use bevy::ecs::system::RunSystemOnce; // 使用真实SystemParam和资源变更语义。

    use crate::monitor_resources::EditorSceneMonitorPage; // 遍历F4全部互斥页面。

    #[test] // 六个植被页都显示逐档控件，其他十一页必须隐藏。
    fn lod_controls_are_visible_on_all_six_vegetation_pages_only() {
        let mut world = World::new(); // 创建不含完整面板的最小UI世界。
        world.init_resource::<EditorSceneMonitorState>(); // 默认总览页提供当前页面资源。
        world.init_resource::<LodControlState>(); // 共享状态默认九类四档全开。
        let root = world
            .spawn((EditorVegetationLodControls, Node::default()))
            .id(); // 唯一根足以验证Display同步，按钮查询允许为空。
        for page in EditorSceneMonitorPage::ALL {
            world.resource_mut::<EditorSceneMonitorState>().page = page; // 逐页模拟F4切换。
            world
                .run_system_once(update_vegetation_lod_control_ui)
                .expect("F4六类逐档UI同步应成功运行"); // 应用生产显隐逻辑。
            let expected = if content_for_editor_page(page).is_some() && page.available(None) {
                Display::Flex // 六个正式植被输出显示同款两行控件。
            } else {
                Display::None // 其余对象不显示无效LOD入口。
            }; // 完成当前页期望。
            assert_eq!(
                world.get::<Node>(root).map(|node| node.display),
                Some(expected)
            ); // 锁定页面与布局显隐。
        } // 完成十七页遍历。
    } // 完成F4逐档控件覆盖回归。
} // 结束F4逐档UI测试。
