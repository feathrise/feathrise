//! 只读取固定作者控件的已布局物理边界；为原生子视口留孔，不移动控件或改变相机投影。

use crate::{
    components::*,
    viewport::{modes::*, multi_select::*, shading::ViewShadingRoot},
};
use bevy::prelude::*;

const MAX_RECTS: usize = 32;

pub(super) type UiRoots = Or<(
    With<ViewportModeToolsRoot>,
    With<MeshEditToolbarRoot>,
    With<ViewportControlsRoot>,
    With<ViewShadingRoot>,
    With<MapEditorSelectedWaterBadge>,
    With<MapEditorSelectedPortalBadge>,
    With<MapEditorSelectedPlacementBadge>,
    With<MultiSelectionBadge>,
    // 非模态浮层只让出实际绘制的区域；其余中央像素继续由同一个游戏child呈现。
    With<crate::ui::tooltips::TooltipPopup>,
    With<crate::ui::status_bar::StatusPopup>,
    With<crate::ui::context_help::ContextHelpPopup>,
    With<crate::workbench::WorkbenchFloatingPanel>,
)>;
pub(super) type SelectionBorders = Or<(
    With<MultiSelectionRectangle>,
    With<crate::ui::courtyard::CourtyardSelectionRegion>,
)>;
type Layout<'a> = (
    &'a Node,
    &'a ComputedNode,
    &'a UiGlobalTransform,
    &'a InheritedVisibility,
);

pub(super) fn capture<'a>(
    viewport: Rect,
    roots: impl Iterator<Item = Layout<'a>>,
    borders: impl Iterator<Item = Layout<'a>>,
) -> Result<Vec<Rect>, String> {
    let nodes = roots
        .map(|layout| (layout, false))
        .chain(borders.map(|layout| (layout, true)))
        .map(|((node, computed, transform, visibility), outline)| {
            let border = computed.border;
            (
                node.display != Display::None && visibility.get() && !computed.is_empty(),
                Rect::from_center_size(transform.translation, computed.size()),
                outline.then_some([
                    border.min_inset.x,
                    border.min_inset.y,
                    border.max_inset.x,
                    border.max_inset.y,
                ]),
            )
        });
    collect(viewport, nodes)
}

fn collect(
    viewport: Rect,
    nodes: impl IntoIterator<Item = (bool, Rect, Option<[f32; 4]>)>,
) -> Result<Vec<Rect>, String> {
    let mut rects = Vec::new();
    for (index, (visible, rect, outline)) in nodes.into_iter().enumerate() {
        // 同类根节点数量本应固定；重复异常也不能把检查变成无界实体遍历。
        if index == MAX_RECTS {
            return Err("作者浮动控件超过32个固定窗口区域，已隐藏工程视口以保护原控件".into());
        }
        if !visible {
            continue;
        }
        if !rect.min.is_finite() || !rect.max.is_finite() {
            return Err("作者浮动控件布局包含非法坐标，已隐藏工程视口".into());
        }
        if let Some([left, top, right, bottom]) = outline {
            if [left, top, right, bottom]
                .iter()
                .any(|width| !width.is_finite() || *width < 0.)
            {
                return Err("作者框选边界包含非法宽度，已隐藏工程视口".into());
            }
            // 框选只露出四条边，不能把整个框内部换回旧作者场景。
            for edge in [
                Rect::from_corners(rect.min, Vec2::new(rect.min.x + left, rect.max.y)),
                Rect::from_corners(rect.min, Vec2::new(rect.max.x, rect.min.y + top)),
                Rect::from_corners(Vec2::new(rect.max.x - right, rect.min.y), rect.max),
                Rect::from_corners(Vec2::new(rect.min.x, rect.max.y - bottom), rect.max),
            ] {
                push(&mut rects, edge, viewport)?;
            }
        } else {
            push(&mut rects, rect, viewport)?;
        }
    }
    // 固定小集合排序消除ECS迭代顺序变化，稳定布局不会每帧重建原生区域。
    rects.sort_by(|a, b| {
        a.min
            .x
            .total_cmp(&b.min.x)
            .then(a.min.y.total_cmp(&b.min.y))
            .then(a.max.x.total_cmp(&b.max.x))
            .then(a.max.y.total_cmp(&b.max.y))
    });
    rects.dedup();
    Ok(rects)
}

fn push(rects: &mut Vec<Rect>, rect: Rect, viewport: Rect) -> Result<(), String> {
    let minimum = rect.min.max(viewport.min);
    let maximum = rect.max.min(viewport.max);
    if !minimum.cmplt(maximum).all() {
        return Ok(());
    }
    if rects.len() == MAX_RECTS {
        return Err("作者浮动控件超过32个固定窗口区域，已隐藏工程视口以保护原控件".into());
    }
    rects.push(Rect::from_corners(minimum, maximum));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_controls_clip_to_viewport_and_selection_only_reserves_four_edges() {
        let viewport = Rect::new(100., 50., 500., 350.);
        let rects = collect(
            viewport,
            [
                (true, Rect::new(90., 40., 130., 75.), None),
                (false, viewport, None),
                (true, Rect::new(600., 50., 700., 60.), None),
                (true, Rect::new(200., 100., 400., 300.), Some([2.; 4])),
            ],
        )
        .unwrap();
        assert_eq!(rects.len(), 5);
        assert!(rects.contains(&Rect::new(100., 50., 130., 75.)));
        assert!(
            rects
                .iter()
                .all(|rect| !rect.contains(Vec2::new(300., 200.)))
        );
    }

    #[test]
    fn overflow_is_a_whole_projection_error_instead_of_silent_truncation() {
        let viewport = Rect::new(0., 0., 100., 100.);
        assert!(collect(viewport, std::iter::repeat_n((true, viewport, None), 32)).is_ok());
        assert!(collect(viewport, std::iter::repeat_n((true, viewport, None), 33)).is_err());
        assert!(
            collect(
                viewport,
                std::iter::repeat_n((true, viewport, Some([1.; 4])), 9)
            )
            .is_err()
        );
    }

    #[test]
    fn transparent_xyz_root_never_reserves_its_full_rectangle() {
        let mut world = World::new();
        let root = world.spawn(MapEditorOrientationAxisRoot).id();
        let toolbar = world.spawn(ViewportControlsRoot).id();
        let mut query = world.query_filtered::<Entity, UiRoots>();
        assert_eq!(query.iter(&world).collect::<Vec<_>>(), vec![toolbar]);
        assert!(query.get(&world, root).is_err(), "82像素根只捕获原输入，不让出整块游戏区域");
    }
}
