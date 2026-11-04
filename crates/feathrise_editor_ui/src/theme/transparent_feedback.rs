//! 透明按钮只在悬停/按下时补背景绘制；稀疏候选来自变化事件，不改命中或普通态透明度。

use super::{
    button_feedback::{ButtonFeedback, interaction_for_cursor},
    components::EditorThemeRoot,
    extract::owned,
    resources::*,
};
use bevy::{
    prelude::*,
    render::{Extract, sync_world::TemporaryRenderEntity},
    ui::{ComputedStackIndex, InteractionDisabled, RelativeCursorPosition},
    ui_render::{ExtractedUiItem, ExtractedUiNode, ExtractedUiNodes, NodeType, UiCameraMap},
};
use feathrise_ui_widgets::{NumericInputDisabled, NumericInputFieldButton, NumericInputStepButton};

const MAX_CANDIDATES: usize = 16_384; // 与主题所有权缓存同阶有界，异常状态不随历史实体累积。

#[allow(clippy::type_complexity)]
pub(super) fn extract(
    mut commands: Commands,
    mut nodes: ResMut<ExtractedUiNodes>,
    palette: Extract<Res<EditorPalette>>,
    parents: Extract<Query<&ChildOf>>,
    roots: Extract<Query<(), With<EditorThemeRoot>>>,
    changed: Extract<
        Query<
            (Entity, &Interaction, &BackgroundColor),
            (
                With<Button>,
                // 遮罩绘制固定；不能因关闭Button悬停生成另一层普通按钮背景。
                Without<crate::EditorDialogBackdrop>,
                Or<(Changed<Interaction>, Changed<BackgroundColor>)>,
            ),
        >,
    >,
    buttons: Extract<
        Query<
            (
                &ComputedNode,
                &ComputedStackIndex,
                &UiGlobalTransform,
                &InheritedVisibility,
                Option<&CalculatedClip>,
                &ComputedUiTargetCamera,
                &Interaction,
                &BackgroundColor,
                Has<InteractionDisabled>,
                Option<&NumericInputFieldButton>,
                Option<&NumericInputStepButton>,
                Option<&RelativeCursorPosition>,
            ),
            (With<Button>, Without<crate::EditorDialogBackdrop>),
        >,
    >,
    disabled_numeric: Extract<Query<(), With<NumericInputDisabled>>>,
    camera_map: Extract<UiCameraMap>,
    mut state: ResMut<ThemeExtraction>,
) {
    for (entity, interaction, background) in &changed {
        if *interaction != Interaction::None && background.is_fully_transparent() {
            if state.transparent_buttons.len() == MAX_CANDIDATES
                && !state.transparent_buttons.contains(&entity)
            {
                state.transparent_buttons.clear();
            }
            state.transparent_buttons.insert(entity);
        } else {
            state.transparent_buttons.remove(&entity);
        }
    }
    let mut candidates = std::mem::take(&mut state.transparent_buttons);
    let mut cameras = camera_map.get_mapper();
    candidates.retain(|&entity| {
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
            return false;
        };
        if *interaction == Interaction::None || !background.is_fully_transparent() {
            return false;
        }
        let numeric_root = field
            .map(|field| field.root)
            .or_else(|| step.map(|step| step.root));
        let interaction = interaction_for_cursor(*interaction, cursor);
        if disabled
            || disabled_numeric.contains(entity)
            || numeric_root.is_some_and(|root| disabled_numeric.contains(root))
            || interaction == Interaction::None
            || !visible.get()
            || computed.is_empty()
            || !owned(
                entity,
                &mut state,
                |e| parents.get(e).ok().map(ChildOf::parent),
                |e| roots.contains(e),
            )
        {
            return true;
        }
        let Some(extracted_camera_entity) = cameras.map(camera) else {
            return true;
        };
        // 与官方背景提取相同的几何、层级和裁切；不添加主世界节点，因此不会遮挡输入。
        nodes.uinodes.push(ExtractedUiNode {
            z_order: stack.0 as f32,
            image: default(),
            clip: clip.map(|clip| clip.clip),
            extracted_camera_entity,
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
            main_entity: entity.into(),
            render_entity: commands.spawn(TemporaryRenderEntity).id(),
        });
        true
    });
    state.transparent_buttons = candidates;
}
