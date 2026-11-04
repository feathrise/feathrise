//! 在官方提取和源码着色之后映射所属绘制项；逻辑组件保持原色，避免稳定帧反复写回。
use super::{
    button_feedback::{ButtonFeedback, interaction_for_cursor},
    components::*,
    resources::*,
};
use bevy::{
    prelude::*,
    render::Extract,
    ui::{InteractionDisabled, RelativeCursorPosition},
    ui_render::{ExtractedUiItem, ExtractedUiNodes, NodeType},
};
use feathrise_ui_widgets::{
    ButtonColors, NumericInputDisabled, NumericInputFieldButton, NumericInputStepButton,
};

const CACHE_ENTITIES: usize = 16_384; // 最多约512 KiB；旧UI销毁后不无限保留身份。
const MAX_DEPTH: usize = 128;

pub(super) fn parent_inserted(_: On<Insert, ChildOf>, mut epoch: ResMut<ThemeTopology>) {
    epoch.0 = epoch.0.wrapping_add(1); // 非Node包装祖先也可能改变所属根，事件时只递增一个小修订。
}
pub(super) fn parent_removed(_: On<Remove, ChildOf>, mut epoch: ResMut<ThemeTopology>) {
    epoch.0 = epoch.0.wrapping_add(1);
}
pub(super) fn root_inserted(_: On<Insert, EditorThemeRoot>, mut epoch: ResMut<ThemeTopology>) {
    epoch.0 = epoch.0.wrapping_add(1);
}
pub(super) fn root_removed(_: On<Remove, EditorThemeRoot>, mut epoch: ResMut<ThemeTopology>) {
    epoch.0 = epoch.0.wrapping_add(1);
}

pub(super) fn owned(
    entity: Entity,
    state: &mut ThemeExtraction,
    parent: impl Fn(Entity) -> Option<Entity>,
    root: impl Fn(Entity) -> bool,
) -> bool {
    if let Some(&owned) = state.scopes.get(&entity) {
        return owned;
    }
    let mut trail = [Entity::PLACEHOLDER; MAX_DEPTH];
    let mut used = 0;
    let mut cursor = entity;
    let result = loop {
        if let Some(&cached) = state.scopes.get(&cursor) {
            break cached;
        }
        if used == MAX_DEPTH {
            break false;
        } // 非法深层或循环失败关闭，不碰游戏UI。
        trail[used] = cursor;
        used += 1;
        if root(cursor) {
            break true;
        }
        let Some(next) = parent(cursor) else {
            break false;
        };
        cursor = next;
    };
    if state.scopes.len() + used > CACHE_ENTITIES {
        state.scopes.clear();
    }
    for entity in &trail[..used] {
        state.scopes.insert(*entity, result);
    }
    result
}

/// 官方文本批次保持排版行顺序；以保守边距定位可见行，不遍历长文档的隐藏字形。
pub(super) fn visible_glyphs(
    nodes: &ExtractedUiNodes,
    index: usize,
    range: std::ops::Range<usize>,
) -> std::ops::Range<usize> {
    let node = &nodes.uinodes[index];
    let Some(clip) = node.clip else {
        return range;
    };
    let inverse = node.transform.inverse();
    // 单轴overflow的另一轴是±∞；直接变换四角会因0×∞得到NaN，漏掉整条标题。
    // 分别变换轴区间，零系数直接贡献0；旋转遇到无界轴时保守纳入整个字形批次。
    let bounds = |scale: f32, min: f32, max: f32| {
        if scale == 0.0 {
            (0.0, 0.0)
        } else if scale > 0.0 {
            (scale * min, scale * max)
        } else {
            (scale * max, scale * min)
        }
    };
    let x = bounds(inverse.matrix2.x_axis.y, clip.min.x, clip.max.x);
    let y = bounds(inverse.matrix2.y_axis.y, clip.min.y, clip.max.y);
    let min = inverse.translation.y + x.0 + y.0 - 512.0;
    let max = inverse.translation.y + x.1 + y.1 + 512.0;
    let glyphs = &nodes.glyphs[range.clone()];
    let start = glyphs.partition_point(|glyph| glyph.translation.y < min);
    let end = glyphs
        .partition_point(|glyph| glyph.translation.y <= max)
        .max(start);
    range.start + start..range.start + end
}

#[allow(clippy::type_complexity)]
pub(super) fn colors(
    mut nodes: ResMut<ExtractedUiNodes>,
    palette: Extract<Res<EditorPalette>>,
    topology: Extract<Res<ThemeTopology>>,
    parents: Extract<Query<&ChildOf>>,
    roots: Extract<Query<(), With<EditorThemeRoot>>>,
    code: Extract<Query<(), With<EditorCodeSurface>>>,
    literal: Extract<Query<(), With<EditorLiteralColor>>>,
    buttons: Extract<
        Query<
            (
                &Interaction,
                &BackgroundColor,
                Option<&ButtonColors>,
                Has<InteractionDisabled>,
                Option<&NumericInputFieldButton>,
                Option<&NumericInputStepButton>,
                Option<&RelativeCursorPosition>,
            ),
            // 标准遮罩即使是关闭Button，也只映射主题色，不参与普通按钮的悬停/按压增亮。
            (With<Button>, Without<crate::EditorDialogBackdrop>),
        >,
    >,
    disabled_numeric: Extract<Query<(), With<NumericInputDisabled>>>,
    mut state: ResMut<ThemeExtraction>,
) {
    if state.topology != topology.0 {
        state.scopes.clear();
        state.topology = topology.0; // UI重新挂载必须先撤销旧所有权缓存。
    }
    apply(
        &mut nodes,
        &palette,
        &mut state,
        |e| parents.get(e).ok().map(ChildOf::parent),
        |e| roots.contains(e),
        |e| code.contains(e),
        |e| literal.contains(e),
        |e| {
            let (interaction, background, colors, disabled, field, step, cursor) =
                buttons.get(e).ok()?;
            let numeric_root = field
                .map(|field| field.root)
                .or_else(|| step.map(|step| step.root));
            Some(ButtonFeedback {
                interaction: interaction_for_cursor(*interaction, cursor),
                disabled: disabled
                    || disabled_numeric.contains(e)
                    || numeric_root.is_some_and(|root| disabled_numeric.contains(root)),
                background: background.0.to_linear(),
                colors: colors.copied(),
            })
        },
    );
}

pub(super) fn apply(
    nodes: &mut ExtractedUiNodes,
    palette: &EditorPalette,
    state: &mut ThemeExtraction,
    parent: impl Fn(Entity) -> Option<Entity>,
    root: impl Fn(Entity) -> bool,
    code: impl Fn(Entity) -> bool,
    literal: impl Fn(Entity) -> bool,
    button: impl Fn(Entity) -> Option<ButtonFeedback>,
) {
    // 原始色板恒等映射，但保留共同的按钮反馈；不用微调颜色绕过语义身份。
    let original = EditorPalette::warm_copper();
    for i in 0..nodes.uinodes.len() {
        let entity = nodes.uinodes[i].main_entity.id();
        if !owned(entity, state, &parent, &root) {
            continue;
        }
        let palette = if literal(entity) { &original } else { palette };
        match &nodes.uinodes[i].item {
            ExtractedUiItem::Node { .. } => {
                // 背景绘制使用整数层级和默认白图；排除同实体的边框、图片、外部填充及文字装饰。
                let is_background = nodes.uinodes[i].z_order.fract() == 0.0
                    && nodes.uinodes[i].image == AssetId::<Image>::default();
                if let ExtractedUiItem::Node {
                    color, node_type, ..
                } = &mut nodes.uinodes[i].item
                {
                    *color = if is_background
                        && *node_type == NodeType::Rect
                        && let Some(feedback) = button(entity)
                        && feedback.background == *color
                    {
                        feedback.resolve(palette)
                    } else if palette.name() != "WarmCopper"
                        && code(entity)
                        && key(*color) == key(crate::colors::INPUT_BG.to_linear())
                    {
                        palette.resolve(crate::colors::OVERLAY_BG).to_linear()
                    } else {
                        palette.resolve_linear(*color)
                    };
                }
            }
            ExtractedUiItem::Glyphs { range } => {
                let range = visible_glyphs(&nodes, i, range.clone());
                for glyph in &mut nodes.glyphs[range] {
                    glyph.color = palette.resolve_linear(glyph.color);
                }
            }
        }
    }
}
