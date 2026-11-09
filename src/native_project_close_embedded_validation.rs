//! 仅自有常驻验收使用的原关闭入口与只读呈现证据；不直接设置关闭阶段或选择结果。
use feathrise_editor_ui::UiInteraction;
use super::{
    components::{CloseAction, CloseOverlay},
    resources::{CloseStage, NativeProjectCloseState},
};
use bevy::{
    prelude::*,
    ui::InteractionDisabled,
    window::{PrimaryWindow, WindowCloseRequested},
};
use serde_json::{Value, json};

pub(crate) fn request(world: &mut World) -> Result<(), String> {
    let window = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
        .map_err(|error| error.to_string())?;
    world.write_message(WindowCloseRequested { window });
    Ok(())
}

pub(crate) fn visible_prompt(world: &mut World) -> Option<Value> {
    let state = world.get_resource::<NativeProjectCloseState>()?;
    if state.stage != CloseStage::Confirm || state.documents.is_empty() {
        return None;
    }
    let documents = state.documents.clone();
    let overlays: Vec<_> = world
        .query_filtered::<(&Node, &InheritedVisibility, &ComputedNode), With<CloseOverlay>>()
        .iter(world)
        .map(|(node, visibility, computed)| {
            json!({"display":format!("{:?}", node.display),
            "inherited_visible":visibility.get(),"size":computed.size().to_array()})
        })
        .collect();
    if overlays.is_empty()
        || overlays.iter().any(|value| {
            value["display"] == "None"
                || value["inherited_visible"] != true
                || value["size"][0].as_f64().unwrap_or(0.) < 100.
                || value["size"][1].as_f64().unwrap_or(0.) < 100.
        })
    {
        return None;
    }
    Some(
        json!({"source":"original CloseStage + Node + InheritedVisibility + ComputedNode", "overlays":overlays,"documents":documents}),
    )
}

pub(crate) fn cancel(world: &mut World) -> Result<Entity, String> {
    let button = world
        .query::<(Entity, &CloseAction, Has<InteractionDisabled>)>()
        .iter(world)
        .find_map(|(entity, action, disabled)| {
            (*action == CloseAction::Cancel && !disabled).then_some(entity)
        })
        .ok_or("原关闭取消按钮不可用")?;
    world.entity_mut(button).insert((
        UiInteraction::Pressed,
        feathrise_editor_ui::EditorButtonClick::triggered(),
    ));
    Ok(button)
}

/// 收尾中的真实原确认层；仅观察阶段、布局和取消按钮，不向资源写入关闭授权。
pub(crate) fn draining_prompt(world: &mut World) -> Option<Value> {
    let state = world.get_resource::<NativeProjectCloseState>()?;
    if state.stage != CloseStage::Draining { return None; }
    let documents = state.documents.clone();
    let status = state.status.clone();
    let visible_overlay = world
        .query_filtered::<(&Node, &InheritedVisibility, &ComputedNode), With<CloseOverlay>>()
        .iter(world).any(|(node, visible, computed)| node.display != Display::None
            && visible.get() && computed.size().min_element() > 100.);
    let cancel = world.query::<(Entity, &CloseAction, &InheritedVisibility, &ComputedNode, Has<InteractionDisabled>)>()
        .iter(world).find_map(|(entity, action, visible, computed, disabled)| {
            (*action == CloseAction::Cancel && visible.get() && !computed.is_empty() && !disabled)
                .then_some(entity.to_bits())
        })?;
    visible_overlay.then(|| json!({"source":"original CloseStage::Draining + visible original overlay + enabled Cancel",
        "stage":"Draining","documents":documents,"status":status,"cancel_button":cancel}))
}

/// 专项完成后的显式夹具放弃；仍点击原按钮，让生产关闭事务确认全部Job收口。
pub(crate) fn discard(world: &mut World) -> Result<Option<Entity>, String> {
    if visible_prompt(world).is_none() { return Ok(None); }
    let button = world.query::<(Entity, &CloseAction, Has<InteractionDisabled>)>()
        .iter(world).find_map(|(entity, action, disabled)| {
            (*action == CloseAction::Discard && !disabled).then_some(entity)
        }).ok_or("本轮夹具原放弃并关闭按钮不可用")?;
    world.entity_mut(button).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    Ok(Some(button))
}

/// 仅已核验归属的专项夹具结束时使用：有草稿／保存失败点击原放弃，干净确认点击原关闭。
/// 与visible_prompt的“必须有脏文档”取证合同分开，不写choice/intent/stage，也不替生产事务确认收口。
pub(crate) fn finish_fixture(world: &mut World) -> Result<Option<Entity>, String> {
    let Some(button) = fixture_finish_target(world)? else { return Ok(None); };
    world.entity_mut(button).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    Ok(Some(button))
}

/// 只读提供同一个原确认按钮；三层窗口专项用官方鼠标输入，不插入点击成功组件。
pub(crate) fn fixture_finish_target(world: &mut World) -> Result<Option<Entity>, String> {
    let Some(state) = world.get_resource::<NativeProjectCloseState>() else { return Ok(None); };
    if state.stage != CloseStage::Confirm { return Ok(None); }
    let action = if state.needs_save_decision() { CloseAction::Discard } else { CloseAction::Close };
    if !state.action_enabled(action) { return Ok(None); }
    let (present, visible) = world
        .query_filtered::<(&Node, &InheritedVisibility, &ComputedNode), With<CloseOverlay>>()
        .iter(world).fold((false, true), |(_, previous), (node, inherited, computed)| {
            (true, previous && node.display != Display::None && inherited.get()
                && computed.size().is_finite() && computed.size().min_element() > 100.)
        });
    if !present || !visible { return Ok(None); }
    // UI布局和InteractionDisabled可能比Confirm阶段晚一帧发布；只等待实际可见、可用的对应原按钮。
    let mut button = None;
    for (entity, current, node, inherited, computed, disabled) in world
        .query::<(Entity, &CloseAction, &Node, &InheritedVisibility, &ComputedNode, Has<InteractionDisabled>)>()
        .iter(world)
    {
        if *current != action || node.display == Display::None || !inherited.get()
            || computed.is_empty() || !computed.size().is_finite() || disabled { continue; }
        if button.replace(entity).is_some() { return Err("夹具关闭确认出现重复的可用原按钮，拒绝选择".into()); }
    }
    let Some(button) = button else { return Ok(None); };
    Ok(Some(button))
}

pub(crate) fn idle(world: &World) -> bool {
    world
        .get_resource::<NativeProjectCloseState>()
        .is_some_and(|state| state.stage == CloseStage::Idle && !state.captures_input())
}

pub(crate) fn hidden_prompt(world: &mut World) -> bool {
    world
        .query_filtered::<&Node, With<CloseOverlay>>()
        .iter(world)
        .all(|node| node.display == Display::None)
}
