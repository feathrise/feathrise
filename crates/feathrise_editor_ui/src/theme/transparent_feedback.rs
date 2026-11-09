//! 透明按钮只在悬停/按下时补背景绘制，和官方绘制项共用失效生命周期。

use crate::UiInteraction;
use super::{
    button_feedback::{ButtonFeedback, interaction_for_cursor},
    components::EditorThemeRoot,
    extract::owned,
    resources::*,
};
use crate::InteractionButton;
use bevy::{
    prelude::*,
    render::Extract,
    ui::{ComputedStackIndex, InteractionDisabled, RelativeCursorPosition},
    ui_render::{ExtractedUiItem, ExtractedUiNode, ExtractedUiNodes, NodeType, UiCameraMap},
};
use feathrise_ui_widgets::{NumericInputDisabled, NumericInputFieldButton, NumericInputStepButton};

#[allow(clippy::type_complexity)]
pub(super) fn extract(
    mut commands: Commands,
    nodes: ResMut<ExtractedUiNodes>,
    palette: Extract<Res<EditorPalette>>,
    parents: Extract<Query<&ChildOf>>,
    roots: Extract<Query<(), With<EditorThemeRoot>>>,
    buttons: Extract<
        Query<
            (
                &ComputedNode,
                &ComputedStackIndex,
                &UiGlobalTransform,
                &InheritedVisibility,
                Option<&CalculatedClip>,
                &ComputedUiTargetCamera,
                &UiInteraction,
                &BackgroundColor,
                Has<InteractionDisabled>,
                Option<&NumericInputFieldButton>,
                Option<&NumericInputStepButton>,
                Option<&RelativeCursorPosition>,
            ),
            // 遮罩绘制固定；不能因关闭按钮悬停生成另一层普通按钮背景。
            (With<InteractionButton>, Without<crate::EditorDialogBackdrop>),
        >,
    >,
    disabled_numeric: Extract<Query<(), With<NumericInputDisabled>>>,
    camera_map: Extract<UiCameraMap>,
    mut state: ResMut<ThemeExtraction>,
) {
    let nodes = nodes.into_inner();
    let mut cameras = camera_map.get_mapper();
    for &main in &nodes.changed {
        let entity = main.entity();
        let Ok((
            computed,
            stack,
            transform,
            visible,
            clip,
            camera,
            interaction,
            background,
            disabled,
            field,
            step,
            cursor,
        )) = buttons.get(entity)
        else {
            continue;
        };
        if *interaction == UiInteraction::None || !background.is_fully_transparent() {
            continue;
        }
        let numeric_root = field
            .map(|field| field.root)
            .or_else(|| step.map(|step| step.root));
        let interaction = interaction_for_cursor(*interaction, cursor);
        if disabled
            || disabled_numeric.contains(entity)
            || numeric_root.is_some_and(|root| disabled_numeric.contains(root))
            || interaction == UiInteraction::None
            || !visible.get()
            || computed.is_empty()
            || !owned(
                entity,
                &mut state,
                |e| parents.get(e).ok().map(ChildOf::parent),
                |e| roots.contains(e),
            )
        {
            continue;
        }
        let Some(extracted_camera_entity) = cameras.map(camera) else {
            continue;
        };
        // 与官方背景提取相同的几何、层级和裁切；不添加主世界节点，因此不会遮挡输入。
        let (_, draws) = nodes.uinodes.entry(main).or_insert_with(|| (extracted_camera_entity, default()));
        draws.insert(
            commands.spawn_empty().id(),
            ExtractedUiNode {
                z_order: stack.0 as f32,
                image: default(),
                clip: clip.cloned(),
                transform: transform.into(),
                item: ExtractedUiItem::Node {
                    color: ButtonFeedback {
                        interaction,
                        disabled: false,
                        background: crate::colors::BUTTON_DEV_SMALL_NORMAL.to_linear(),
                        colors: None,
                    }
                    .resolve(&palette),
                    rect: Rect::from_corners(Vec2::ZERO, computed.size()),
                    atlas_scaling: None,
                    flip_x: false,
                    flip_y: false,
                    border: computed.border(),
                    border_radius: computed.border_radius(),
                    node_type: NodeType::Rect,
                },
            },
        );
        state.themed_entities.insert(entity);
    }
}
