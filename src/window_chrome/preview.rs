//! 独立原生预览不改变作者顶栏；始终发布原控件的真实布局，保留旧专项的只读取证格式。
use super::{
    components::ChromeButton,
    resources::{ChromeParts, PreviewChromeLayout},
    types::{ChromeGeometry, PixelRect, WindowAction},
};
use bevy::prelude::*;

fn rect(world: &World, entity: Entity) -> Option<PixelRect> {
    let node = world.get::<ComputedNode>(entity)?;
    let transform = world.get::<UiGlobalTransform>(entity)?;
    if node.is_empty() || !world.get::<InheritedVisibility>(entity)?.get() {
        return None;
    }
    Some(PixelRect::from_node(node, transform))
}

pub(super) fn measure(world: &mut World) {
    let mut geometry = ChromeGeometry::default();
    if let Some(parts) = world.resource::<ChromeParts>().0.as_ref()
        && rect(world, parts.header).is_some()
    {
        geometry.visible = true;
        if let Some(spacer) = rect(world, parts.spacer) {
            geometry.drag = spacer;
        }
        for child in world
            .get::<Children>(parts.captions)
            .into_iter()
            .flat_map(|children| children.iter())
        {
            if let Some(ChromeButton::Window(action)) = world.get::<ChromeButton>(child)
                && let Some(bounds) = rect(world, child)
            {
                match action {
                    WindowAction::Minimize => geometry.minimize = bounds,
                    WindowAction::Maximize => geometry.maximize = bounds,
                    WindowAction::Close => geometry.close = bounds,
                }
            }
        }
    }
    // 不创建预览UI根或第二个相机，不移动原三键；窗口模式只属于独立工程窗口。
    world
        .resource_mut::<PreviewChromeLayout>()
        .set_if_neq(PreviewChromeLayout::default());
    world.resource_mut::<ChromeGeometry>().set_if_neq(geometry);
}

#[cfg(feature = "validation")]
pub(crate) fn probe(world: &World) -> Option<serde_json::Value> {
    use serde_json::json;
    let regular = world.get_resource::<ChromeParts>()?.0.as_ref()?;
    let controls: Vec<_> = world.get::<Children>(regular.captions)?.iter().filter_map(|entity| {
        let ChromeButton::Window(action) = world.get::<ChromeButton>(entity)? else { return None; };
        let action = match action { WindowAction::Minimize => "minimize", WindowAction::Maximize => "maximize", WindowAction::Close => "close" };
        let bounds = rect(world, entity).map(|bounds| [bounds.left, bounds.top, bounds.right, bounds.bottom]);
        Some(json!({"action":action,"entity":entity.to_bits(),"rect":bounds,"visible":bounds.is_some()}))
    }).collect();
    Some(json!({"layout":{"visible":false,"bottom":0},"preview_header":null,"camera":null,
        "regular_header":regular.header.to_bits(),"active_header":regular.header.to_bits(),
        "spacer":regular.spacer.to_bits(),"captions":controls}))
}

#[cfg(test)]
#[path = "preview_tests.rs"]
mod tests;
