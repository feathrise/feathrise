//! 固定框选轮廓与多选计数；每个普通实例用原点十字标记，不伪造模型包围盒。

use super::PlacementSelectionSet;
use crate::components::SceneEditorViewport;
use crate::resources::{SceneEditorState, SceneEditorViewportState};
use crate::viewport::{
    SceneEditorBrushGizmos,
    placement_move::{MoveCamera, PlacementMoveState},
};
use bevy::{picking::Pickable, prelude::*};

#[derive(Component)]
pub(crate) struct MultiSelectionRectangle;
#[derive(Component)]
pub(crate) struct MultiSelectionBadge;

pub(crate) fn setup_multi_selection_ui(
    mut commands: Commands,
    assets: Res<AssetServer>,
    viewports: Query<Entity, Added<SceneEditorViewport>>,
) {
    for entity in &viewports {
        commands.entity(entity).with_children(|parent| {
            parent.spawn((
                MultiSelectionRectangle,
                Pickable::IGNORE,
                Node {
                    display: Display::None,
                    position_type: PositionType::Absolute,
                    border: UiRect::all(px(1)),
                    ..default()
                },
                BorderColor::all(crate::ui::theme::COPPER_BORDER),
                BackgroundColor(Color::srgba(0.745, 0.529, 0.243, 0.08)),
                ZIndex(30),
            ));
            parent.spawn((
                MultiSelectionBadge,
                Pickable::IGNORE,
                Text::new(""),
                TextFont {
                    font: feathrise_editor_core::editor_font(&assets, crate::ui::scene::FONT_PATH).into(),
                    font_size: 11.0.into(),
                    ..default()
                },
                TextColor(crate::ui::theme::TEXT_PRIMARY),
                Node {
                    display: Display::None,
                    position_type: PositionType::Absolute,
                    left: px(8),
                    bottom: px(8),
                    max_width: percent(90),
                    ..default()
                },
                ZIndex(31),
            ));
        });
    }
}

pub(crate) fn sync_multi_selection_ui(
    editor: Res<SceneEditorState>,
    selection: Res<PlacementSelectionSet>,
    mut rectangles: Query<&mut Node, (With<MultiSelectionRectangle>, Without<MultiSelectionBadge>)>,
    mut badges: Query<(&mut Node, &mut Text), With<MultiSelectionBadge>>,
) {
    for mut node in &mut rectangles {
        let gesture = selection
            .gesture
            .as_ref()
            .filter(|gesture| editor.open && gesture.dragging);
        node.display = if gesture.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if let Some(gesture) = gesture {
            let min = gesture.start.min(gesture.current);
            let max = gesture.start.max(gesture.current);
            node.left = percent((min.x + 0.5) * 100.0);
            node.top = percent((min.y + 0.5) * 100.0);
            node.width = percent((max.x - min.x) * 100.0);
            node.height = percent((max.y - min.y) * 100.0);
        }
    }
    for (mut node, mut text) in &mut badges {
        let shown = editor.open && selection.members.len() > 1;
        node.display = if shown { Display::Flex } else { Display::None };
        if shown && selection.is_changed() {
            **text = format!(
                "已选 {} 个普通资产 · W/E/R 整组变换 · Ctrl+C/V 跨场景复制粘贴 · Ctrl+D 复制 · Delete 删除",
                selection.members.len()
            );
        }
    }
}

pub(crate) fn draw_multi_selection(
    editor: Res<SceneEditorState>,
    selection: Res<PlacementSelectionSet>,
    movement: Res<PlacementMoveState>,
    viewport: Option<Res<SceneEditorViewportState>>,
    nodes: Query<&ComputedNode, With<SceneEditorViewport>>,
    mut gizmos: Gizmos<SceneEditorBrushGizmos>,
) {
    if !editor.open || selection.members.len() < 2 || editor.selected_scene_id != selection.scene_id {
        return;
    }
    let (Some(viewport), Ok(node)) = (viewport, nodes.single()) else {
        return;
    };
    let Some(camera) = MoveCamera::new(&editor, &viewport, node.inverse_scale_factor()) else {
        return;
    };
    let color = Color::srgb(0.92, 0.72, 0.40);
    // 固定最多256个原点，各两条线；不查实体树、Mesh或重新构造选择范围。
    for member in selection.members.iter() {
        let position = movement.group_position(member.position);
        let Some(length) = camera.handle_length(position).map(|length| length * 0.06) else {
            continue;
        };
        gizmos.line(
            position - Vec3::X * length,
            position + Vec3::X * length,
            color,
        );
        gizmos.line(
            position - Vec3::Z * length,
            position + Vec3::Z * length,
            color,
        );
    }
}
