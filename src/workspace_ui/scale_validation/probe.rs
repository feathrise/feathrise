//! 只读原设置文字、按钮祖先及布局；不把共享crate私有控件扩大为公开API。
use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use super::types::*;
use bevy::{prelude::*, text::TextLayoutInfo, ui::InteractionDisabled, window::PrimaryWindow};
use feathrise_editor_core::{EditorSettings, effective_editor_ui_scale};
use feathrise_editor_map::workbench::{WorkbenchHostPageState, WorkbenchSettingsBody};
use serde_json::{Value, json};

fn below(world: &World, mut entity: Entity, root: Entity) -> bool {
    for _ in 0..24 {
        if entity == root { return true; }
        let Some(parent) = world.get::<ChildOf>(entity) else { return false; };
        entity = parent.parent();
    }
    false
}

fn button_ancestor(world: &World, mut entity: Entity) -> Option<Entity> {
    for _ in 0..24 {
        if world.get::<InteractionButton>(entity).is_some() { return Some(entity); }
        entity = world.get::<ChildOf>(entity)?.parent();
    }
    None
}

pub(super) fn body(world: &mut World) -> Option<Entity> {
    world.query_filtered::<Entity, With<WorkbenchSettingsBody>>().iter(world).next()
}

pub(super) fn bounds(world: &World, entity: Entity) -> Option<Rect> {
    let node = world.get::<ComputedNode>(entity)?;
    let at = world.get::<UiGlobalTransform>(entity)?;
    (!node.is_empty()).then(|| Rect::from_center_size(at.translation, node.size()))
}

pub(super) fn button(world: &mut World, target: Target) -> Option<Entity> {
    find_button(world, target, true)
}

fn find_button(world: &mut World, target: Target, require_clip: bool) -> Option<Entity> {
    let body = body(world)?;
    let frame = world.get::<ChildOf>(body)?.parent();
    let label = target.label();
    let texts: Vec<_> = world.query::<(Entity, &Text)>().iter(world)
        .filter_map(|(entity, text)| (text.0 == label).then_some(entity)).collect();
    texts.into_iter().find_map(|text| {
        let button = button_ancestor(world, text)?;
        if !world.get::<InheritedVisibility>(button)?.get() || world.get::<InteractionDisabled>(button).is_some() { return None; }
        let rect = bounds(world, button)?;
        let correct = match target {
            Target::Open => !below(world, button, frame),
            Target::Close => below(world, button, frame),
            _ => below(world, button, body) && (!require_clip || bounds(world, body)?.contains(rect.center())),
        };
        correct.then_some(button)
    })
}

/// 只返回官方滚轮所需的物理位置/增量；不写ScrollPosition或任何原交互状态。
pub(super) fn scroll_request(world: &mut World) -> Result<Option<(Entity, Vec2, f32)>, String> {
    let Some(body) = body(world) else { return Ok(None); };
    let Some(clip) = bounds(world, body) else { return Ok(None); };
    let factor = world.get::<ComputedNode>(body).ok_or("原设置正文缺少布局")?.inverse_scale_factor().recip();
    let mut group: Option<Rect> = None;
    for target in [Target::Minus, Target::Plus, Target::Scale(75), Target::Scale(100), Target::Scale(125), Target::Scale(150), Target::Auto] {
        let Some(button) = find_button(world, target, false) else { return Ok(None); };
        let Some(rect) = bounds(world, button) else { return Ok(None); };
        group = Some(group.map_or(rect, |previous| Rect { min: previous.min.min(rect.min), max: previous.max.max(rect.max) }));
    }
    let Some(group) = group else { return Ok(None); };
    let margin = (6. * factor).max(3.);
    if group.height() + 2. * margin > clip.height() { return Err("原缩放组高于本轮设置正文视口，不能靠滚动伪造全部可见".into()); }
    let delta = if group.max.y > clip.max.y - margin + 1. { clip.max.y - margin - group.max.y }
        else if group.min.y < clip.min.y + margin - 1. { clip.min.y + margin - group.min.y } else { return Ok(None); };
    // 右侧正文padding避免命中子按钮/数值框；原滚动器仍必须真正观察到cursor_over才消费。
    let pointer = Vec2::new(clip.max.x - margin, clip.center().y);
    Ok(Some((body, pointer, delta.clamp(-120., 120.))))
}

fn node_diagnostic(world: &World, entity: Entity) -> Value {
    json!({"entity":format!("{entity:?}"),"rect":bounds(world,entity).map(|rect|[rect.min.x,rect.min.y,rect.max.x,rect.max.y]),
        "visible":world.get::<InheritedVisibility>(entity).map(|value|value.get()),
        "interaction":world.get::<UiInteraction>(entity).map(|value|format!("{value:?}")),
        "disabled":world.get::<InteractionDisabled>(entity).is_some(),
        "node":world.get::<Node>(entity).map(|node|format!("width={:?},height={:?},display={:?}",node.width,node.height,node.display)),
        "inverse_scale_factor":world.get::<ComputedNode>(entity).map(|node|node.inverse_scale_factor()),
        "scroll_position":world.get::<ScrollPosition>(entity).map(|scroll|[scroll.x,scroll.y]),
        "cursor_over":world.get::<bevy::ui::RelativeCursorPosition>(entity).map(|cursor|cursor.cursor_over())})
}

pub(super) fn diagnostics(world: &mut World) -> Value {
    let body = body(world);
    let body_info = body.map(|entity|node_diagnostic(world,entity));
    let mut buttons = Vec::new();
    for target in [Target::Open,Target::Minus,Target::Plus,Target::Scale(75),Target::Scale(100),Target::Scale(125),Target::Scale(150),Target::Auto,Target::Close] {
        let candidate = find_button(world,target,false);
        let info = candidate.map(|entity|node_diagnostic(world,entity));
        let inside = candidate.zip(body).and_then(|(entity,body)|Some(bounds(world,body)?.contains(bounds(world,entity)?.center())));
        buttons.push(json!({"label":target.label(),"candidate":info,"center_inside_body":inside}));
    }
    let texts: Vec<_> = world.query::<(Entity,&Text)>().iter(world)
        .filter_map(|(entity,text)| (text.0.ends_with('%')).then_some((entity,text.0.clone()))).collect();
    let readouts: Vec<_> = texts.into_iter().filter(|(entity,_)| body.is_some_and(|body|below(world,*entity,body)) && button_ancestor(world,*entity).is_none())
        .map(|(entity,text)|json!({"text":text,"layout":node_diagnostic(world,entity),
            "glyphs":world.get::<TextLayoutInfo>(entity).map(|layout|layout.glyphs.len()),
            "font":world.get::<TextFont>(entity).map(|font|format!("{:?}",font.font_size))})).collect();
    json!({"settings_open":world.resource::<WorkbenchHostPageState>().settings_open,"settings":values(world),
        "ui_scale":world.get_resource::<UiScale>().map(|scale|scale.0),"body":body_info,"buttons":buttons,"readouts":readouts})
}

pub(super) fn values(world: &World) -> Values {
    let settings = world.resource::<EditorSettings>();
    Values { manual: settings.editor_ui_scale, automatic: settings.editor_ui_scale_auto, offset: settings.editor_ui_scale_auto_offset }
}

pub(super) fn view(world: &mut World, step: Step) -> Result<Option<Value>, String> {
    let actual = values(world);
    if !step.values.same(actual) || world.resource::<WorkbenchHostPageState>().settings_open != step.settings_open { return Ok(None); }
    let (logical, physical, dpi, scale) = {
        let Ok(window) = world.query_filtered::<&Window, With<PrimaryWindow>>().single(world) else { return Ok(None); };
        ([window.width(), window.height()], [window.physical_width(), window.physical_height()], window.scale_factor(), world.resource::<UiScale>().0)
    };
    if let Some((width, height)) = step.resize && ((logical[0] - width).abs() > 1. || (logical[1] - height).abs() > 1.) { return Ok(None); }
    let expected = effective_editor_ui_scale(actual.manual, actual.automatic, actual.offset, logical[0], logical[1]);
    if (scale - expected).abs() > 0.001 { return Ok(None); }
    let physical_factor = dpi * scale;
    let target = if step.settings_open { Target::Scale(75) } else { Target::Open };
    let Some(button) = button(world, target) else { return Ok(None); };
    let Some(rect) = bounds(world, button) else { return Ok(None); };
    let (width, height, inverse_scale_factor) = {
        let node = world.get::<Node>(button).ok_or("原固定按钮缺少Node")?;
        let computed = world.get::<ComputedNode>(button).ok_or("原固定按钮缺少ComputedNode")?;
        let Val::Px(width) = node.width else { return Err("原固定按钮宽度不再为逻辑像素".into()); };
        let Val::Px(height) = node.height else { return Err("原固定按钮高度不再为逻辑像素".into()); };
        (width, height, computed.inverse_scale_factor())
    };
    if (rect.width() - width * physical_factor).abs() > 2. || (rect.height() - height * physical_factor).abs() > 2.
        || (inverse_scale_factor * physical_factor - 1.).abs() > 0.02 { return Ok(None); }
    let mut readout = Value::Null;
    if step.settings_open {
        let body = body(world).ok_or("原设置正文不存在")?;
        let Some(clip) = bounds(world, body) else { return Ok(None); };
        for target in [Target::Minus,Target::Plus,Target::Scale(75)] {
            let Some(entity) = find_button(world,target,false) else { return Ok(None); };
            let Some(rect) = bounds(world,entity) else { return Ok(None); };
            if rect.min.y < clip.min.y - 1. || rect.max.y > clip.max.y + 1. { return Ok(None); }
        }
        let text = if actual.automatic { format!("自动 {:.0}%", expected * 100.) } else { format!("{:.0}%", expected * 100.) };
        let candidates: Vec<_> = world.query::<(Entity, &Text, &InheritedVisibility)>().iter(world)
            .filter_map(|(entity, value, visible)| (value.0 == text && visible.get()).then_some(entity)).collect();
        let readout_entity = candidates.into_iter().find(|entity| below(world, *entity, body) && button_ancestor(world, *entity).is_none());
        let Some(entity) = readout_entity else { return Ok(None); };
        let layout = world.get::<TextLayoutInfo>(entity).ok_or("原百分比读数没有真实文字布局")?;
        let Some(bounds) = bounds(world, entity) else { return Ok(None); };
        if layout.glyphs.is_empty() || bounds.min.y < clip.min.y - 1. || bounds.max.y > clip.max.y + 1. { return Ok(None); }
        readout = json!({"entity":format!("{entity:?}"),"text":text,"glyphs":layout.glyphs.len(),
                          "rect":[bounds.min.x,bounds.min.y,bounds.max.x,bounds.max.y]});
    }
    Ok(Some(json!({"settings":actual,"ui_scale":scale,"expected_effective":expected,"logical_window":logical,
        "physical_window":physical,"os_dpi_scale":dpi,"settings_open":step.settings_open,"readout":readout,
        "fixed_original_button":{"entity":format!("{button:?}"),"label":target.label(),"logical_size":[width,height],
            "physical_rect":[rect.min.x,rect.min.y,rect.max.x,rect.max.y],"inverse_scale_factor":inverse_scale_factor}})))
}
