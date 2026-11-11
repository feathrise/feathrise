//! 自有夹具的数值控件原始输入驱动；只发窗口消息，不改控件值、作者值或提交消息。

use bevy::{
    camera::NormalizedRenderTarget,
    ecs::{entity::ContainsEntity, message::MessageUpdateSystems},
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
        mouse::MouseButtonInput,
    },
    picking::{
        PickingSystems,
        pointer::{PointerAction, PointerButton, PointerId, PointerInput},
    },
    prelude::*,
    window::{CursorMoved, PrimaryWindow, WindowEvent},
};
use feathrise_editor_ui::{
    ActiveNumericInputScrub, NumericInputScrubConfig, NumericInputScrubMapping, NumericInputValue,
};
use serde_json::{Value, json};
use std::collections::VecDeque;

#[path = "embedded_validation_numeric_ready.rs"]
mod ready;

#[derive(Clone, Copy)]
enum Action {
    Move(Vec2),
    Press,
    Release,
    Escape,
    ReleaseAfterEscape,
}

#[derive(Clone, Copy)]
struct Gesture {
    root: Entity,
    window: Entity,
    anchor: Vec2,
    start: f32,
    scale: f32,
    config: NumericInputScrubConfig,
}

#[derive(Resource, Default)]
struct Driver {
    gesture: Option<Gesture>,
    last: Option<Gesture>,
    actions: VecDeque<Action>,
    error: Option<String>,
    serial: u64,
    frame: u64,
    pointer_press: u32,
    pointer_move: u32,
    pointer_release: u32,
    escape_press: u32,
    escape_release: u32,
    trace: Vec<Value>,
    field_diagnostic: Option<Value>,
    ready: ready::State,
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<Driver>().add_systems(
        First,
        (
            inject
                .after(MessageUpdateSystems)
                .before(PickingSystems::Input),
            observe.after(PickingSystems::Input),
        ),
    );
}

pub(super) fn select_author(world: &mut World, id: &str) {
    ready::select(world, id);
}
pub(super) fn author_ready(world: &mut World, root: Entity) -> (bool, Value) {
    ready::poll(world, root)
}

/// UI 全局坐标是物理像素；系统消息及 PointerInput 的客户区坐标必须除以真实窗口倍率。
pub(super) fn field_ready(world: &World, root: Entity) -> bool {
    visible_center(world, root).is_ok()
}

pub(super) fn field_diagnostic(world: &World, root: Entity) -> Value {
    let result = visible_center(world, root);
    json!({"entity":root.to_bits(),"ready":result.is_ok(),"reason":result.err(),
        "size":world.get::<ComputedNode>(root).map(|node|node.size().to_array()),
        "center_physical":world.get::<UiGlobalTransform>(root).map(|value|value.translation.to_array())})
}

pub(super) fn record_field_diagnostic(world: &mut World, value: Value) {
    world.resource_mut::<Driver>().field_diagnostic = Some(value);
}

fn visible_center(world: &World, root: Entity) -> Result<Vec2, String> {
    if world.get::<Node>(root).is_none() {
        return Err(format!("字段 {root:?} 没有原 UI Node"));
    }
    let size = world
        .get::<ComputedNode>(root)
        .ok_or("字段尚无 ComputedNode")?
        .size();
    let center = world
        .get::<UiGlobalTransform>(root)
        .ok_or("字段尚无 UiGlobalTransform")?
        .translation;
    if !size.is_finite() || size.min_element() <= 0.0 || !center.is_finite() {
        return Err(format!(
            "字段布局尚未可见：size={size:?}，center={center:?}"
        ));
    }
    let mut entity = root;
    let mut clipped = true;
    for _ in 0..128 {
        // 与原 UI 一致：逻辑祖先没有 Node 不等于隐藏；真正的 Display/Visibility 仍须逐级验证。
        if world
            .get::<Node>(entity)
            .is_some_and(|node| node.display == Display::None)
            || world
                .get::<Visibility>(entity)
                .is_some_and(|v| *v == Visibility::Hidden)
            || world
                .get::<InheritedVisibility>(entity)
                .is_some_and(|v| !v.get())
        {
            return Err(format!("字段祖先 {entity:?} 显式隐藏或继承不可见"));
        }
        if world.get::<bevy::ui::OverrideClip>(entity).is_some() {
            clipped = false;
        }
        let Some(parent) = world.get::<ChildOf>(entity) else {
            return Ok(center);
        };
        entity = parent.parent();
        // 用各祖先的局部裁切矩形复核实际字段中心，确认字段还在可点击区域。
        if clipped
            && let (Some(node), Some(computed), Some(transform)) = (
                world.get::<Node>(entity),
                world.get::<ComputedNode>(entity),
                world.get::<UiGlobalTransform>(entity),
            )
            && !node.overflow.is_visible()
            && transform.try_inverse().is_none_or(|inverse| {
                !computed
                    .resolve_clip_rect(node.overflow, node.overflow_clip_margin)
                    .contains(inverse.transform_point2(center))
            })
        {
            return Err(format!("字段中心被祖先 {entity:?} 的原 UI 裁剪区域遮挡"));
        }
    }
    Err("字段祖先超过 128 层预算".into())
}

fn pixels_to_value(
    start: f32,
    requested: f32,
    config: NumericInputScrubConfig,
) -> Result<f32, String> {
    if !start.is_finite()
        || !requested.is_finite()
        || !config.sensitivity.is_finite()
        || config.sensitivity <= 0.0
        || !config.drag_step.is_finite()
        || config.drag_step <= 0.0
        || requested < config.minimum
        || requested > config.maximum
    {
        return Err("数值输入验收的原字段映射或目标范围无效".into());
    }
    let delta = match config.mapping {
        NumericInputScrubMapping::Linear { units_per_pixel } => {
            (requested - start) / (units_per_pixel * config.sensitivity)
        }
        NumericInputScrubMapping::Exponential { exponent_per_pixel }
            if start > 0.0 && requested > 0.0 =>
        {
            (requested / start).ln() / (exponent_per_pixel * config.sensitivity)
        }
        _ => return Err("原字段无法用有限鼠标位移达到验收目标".into()),
    };
    if !delta.is_finite() || delta.abs() < config.drag_threshold_pixels.max(0.0) {
        return Err("数值验收目标未越过原控件拖动阈值".into());
    }
    // Win32 SetCursorPos 只接受整数物理像素；不能用不可实现的半像素目标冒充真实鼠标输入。
    let integer = delta.round();
    if (delta - integer).abs() > 0.0001 {
        return Err("当前原字段映射无法由整物理像素精确达到验收目标".into());
    }
    Ok(integer)
}

pub(super) fn change(world: &mut World, root: Entity, requested: f32) -> Result<(), String> {
    if let Some(error) = world.resource::<Driver>().error.as_ref() {
        return Err(error.clone());
    }
    if !world.resource::<Driver>().actions.is_empty() {
        return Err("上一批数值原始输入尚未消费".into());
    }
    let existing = world.resource::<Driver>().gesture;
    if existing.is_none() {
        ready::check(world, root)?;
    }
    let gesture = if let Some(gesture) = existing {
        if gesture.root != root {
            return Err("数值验收不能跨字段接管已有按住手势".into());
        }
        gesture
    } else {
        if world
            .resource::<ButtonInput<MouseButton>>()
            .pressed(MouseButton::Left)
            || world.resource::<ActiveNumericInputScrub>().entity.is_some()
        {
            return Err("数值验收开始前原指针仍未释放".into());
        }
        let physical = visible_center(world, root)?.round();
        if !world
            .get::<ComputedNode>(root)
            .unwrap()
            .contains_point(*world.get::<UiGlobalTransform>(root).unwrap(), physical)
        {
            return Err("原数值字段内没有可用的整物理像素起点".into());
        }
        let (window, value) = world
            .query_filtered::<(Entity, &Window), With<PrimaryWindow>>()
            .single(world)
            .map_err(|_| "数值验收缺少唯一作者主窗口")?;
        let scale = value.scale_factor();
        let anchor = physical / scale;
        if !scale.is_finite()
            || scale <= 0.0
            || anchor.min_element() < 0.0
            || anchor.x >= value.width()
            || anchor.y >= value.height()
        {
            return Err("数值字段不在当前作者窗口客户区内".into());
        }
        Gesture {
            root,
            window,
            anchor,
            scale,
            start: world
                .get::<NumericInputValue>(root)
                .and_then(|value| value.0.as_f32())
                .ok_or("原字段不是浮点数值")?,
            config: *world
                .get::<NumericInputScrubConfig>(root)
                .ok_or("原字段没有拖动映射")?,
        }
    };
    let point = ((gesture.anchor * gesture.scale).round()
        + Vec2::X * pixels_to_value(gesture.start, requested, gesture.config)?)
        / gesture.scale;
    let window = world
        .get::<Window>(gesture.window)
        .ok_or("数值验收窗口已销毁")?;
    if point.x < 0.0 || point.x >= window.width() || point.y < 0.0 || point.y >= window.height() {
        return Err("原配置要求的数值拖动超出当前客户区，不能伪造目标值".into());
    }
    let mut driver = world.resource_mut::<Driver>();
    if existing.is_none() {
        driver.serial += 1;
        driver.gesture = Some(gesture);
        driver.last = Some(gesture);
        // 每帧最多一个原始动作，悬停先完成真实 UI 命中，按下完成公共捕获后才移动。
        driver
            .actions
            .extend([Action::Move(gesture.anchor), Action::Press]);
    }
    driver.actions.push_back(Action::Move(point));
    Ok(())
}

pub(super) fn finish(world: &mut World, root: Entity, cancel: bool) -> Result<(), String> {
    let mut driver = world.resource_mut::<Driver>();
    if let Some(error) = driver.error.as_ref() {
        return Err(error.clone());
    }
    if !driver.actions.is_empty() || driver.gesture.is_none_or(|gesture| gesture.root != root) {
        return Err("数值结束请求与当前完整鼠标手势不符".into());
    }
    if cancel {
        // Escape 按下必须独占一帧，让原控件真正取消；下一帧才释放键和鼠标，避免变成普通提交。
        driver
            .actions
            .extend([Action::Escape, Action::ReleaseAfterEscape]);
    } else {
        driver.actions.push_back(Action::Release);
    }
    Ok(())
}

pub(super) fn idle(world: &World) -> bool {
    let driver = world.resource::<Driver>();
    driver.gesture.is_none()
        && driver.actions.is_empty()
        && world.resource::<ActiveNumericInputScrub>().entity.is_none()
        && !world
            .resource::<ButtonInput<MouseButton>>()
            .pressed(MouseButton::Left)
        && !world
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::Escape)
}

pub(super) fn probe(world: &World) -> Result<Value, String> {
    let driver = world.resource::<Driver>();
    if let Some(error) = driver.error.as_ref() {
        return Err(error.clone());
    }
    let gesture = driver.gesture.or(driver.last);
    Ok(
        json!({"source":"WindowEvent and independent Bevy input messages", "physical_input":false,
        "gesture":driver.serial,"queued_actions":driver.actions.len(),"idle":idle(world),
        "field":gesture.map(|value|value.root.to_bits()),"window":gesture.map(|value|value.window.to_bits()),
        "anchor_logical":gesture.map(|value|value.anchor.to_array()),"scale_factor":gesture.map(|value|value.scale),
        "start_value":gesture.map(|value|value.start),
        "widget_value":gesture.and_then(|value|world.get::<NumericInputValue>(value.root)).and_then(|value|value.0.as_f32()),
        "captured":gesture.is_some_and(|value|world.resource::<ActiveNumericInputScrub>().entity==Some(value.root)),
        "mouse_down":world.resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left),
        "escape_down":world.resource::<ButtonInput<KeyCode>>().pressed(KeyCode::Escape),
        "pointer_press":driver.pointer_press,"pointer_move":driver.pointer_move,"pointer_release":driver.pointer_release,
        "escape_press":driver.escape_press,"escape_release":driver.escape_release,"events":driver.trace,
        "field_readiness":driver.field_diagnostic}),
    )
}

fn inject(world: &mut World) {
    let (action, gesture) = {
        let mut driver = world.resource_mut::<Driver>();
        driver.frame += 1;
        if driver.error.is_some() {
            return;
        }
        let Some(action) = driver.actions.pop_front() else {
            return;
        };
        let Some(gesture) = driver.gesture else {
            driver.error = Some("原始输入队列丢失手势身份".into());
            return;
        };
        (action, gesture)
    };
    if world.get::<Window>(gesture.window).is_none()
        || world.get::<NumericInputValue>(gesture.root).is_none()
    {
        world.resource_mut::<Driver>().error = Some("数值验收输入目标在手势中换代".into());
        return;
    }
    if matches!(action, Action::Press)
        && let Err(error) = ready::check(world, gesture.root)
    {
        world.resource_mut::<Driver>().error = Some(error);
        return;
    }
    let name = match action {
        Action::Move(point) => {
            let mut window = world.get_mut::<Window>(gesture.window).unwrap();
            // 与 Winit/Win32 回声共用同一个整数物理位置，避免 logical f32 往返把 N+0.5 变成 N。
            let physical = (point * gesture.scale).round();
            let point = physical / gesture.scale;
            let delta = window.cursor_position().map(|before| point - before);
            window.set_physical_cursor_position(Some(physical.as_dvec2()));
            let event = CursorMoved {
                window: gesture.window,
                position: point,
                delta,
            };
            world.write_message(event.clone());
            world.write_message(WindowEvent::CursorMoved(event));
            "cursor_moved"
        }
        Action::Press => {
            mouse(world, gesture.window, ButtonState::Pressed);
            "left_pressed"
        }
        Action::Release => {
            mouse(world, gesture.window, ButtonState::Released);
            "left_released"
        }
        Action::Escape => {
            escape(world, gesture.window, ButtonState::Pressed);
            "escape_pressed"
        }
        Action::ReleaseAfterEscape => {
            escape(world, gesture.window, ButtonState::Released);
            mouse(world, gesture.window, ButtonState::Released);
            "escape_and_left_released"
        }
    };
    let point = world
        .get::<Window>(gesture.window)
        .and_then(Window::cursor_position);
    let mut driver = world.resource_mut::<Driver>();
    let (frame, serial) = (driver.frame, driver.serial);
    // 两笔固定用例共 10 个动作；硬上限防止诊断本身无界增长。
    if driver.trace.len() < 24 {
        driver.trace.push(json!({"gesture":serial,"frame":frame,"event":name,"position_logical":point.map(|p|p.to_array())}));
    }
    if matches!(action, Action::Release | Action::ReleaseAfterEscape) {
        driver.gesture = None;
    }
}

fn mouse(world: &mut World, window: Entity, state: ButtonState) {
    let event = MouseButtonInput {
        window,
        state,
        button: MouseButton::Left,
    };
    world.write_message(event);
    world.write_message(WindowEvent::MouseButtonInput(event));
}

fn escape(world: &mut World, window: Entity, state: ButtonState) {
    let event = KeyboardInput {
        window,
        state,
        key_code: KeyCode::Escape,
        logical_key: Key::Escape,
        text: None,
        repeat: false,
    };
    world.write_message(event.clone());
    world.write_message(WindowEvent::KeyboardInput(event));
}

/// 计数来自原 PointerInputPlugin 转换后的消息，不能用“已排队输入”冒充公共指针链已运行。
fn observe(
    mut inputs: MessageReader<PointerInput>,
    mut keys: MessageReader<KeyboardInput>,
    mut driver: ResMut<Driver>,
) {
    for input in inputs.read() {
        let Some(gesture) = driver.gesture.or(driver.last) else {
            continue;
        };
        if input.pointer_id != PointerId::Mouse
            || !matches!(&input.location.target,NormalizedRenderTarget::Window(window) if window.entity()==gesture.window)
        {
            continue;
        }
        match input.action {
            PointerAction::Press(PointerButton::Primary) => driver.pointer_press += 1,
            PointerAction::Move { .. } => driver.pointer_move += 1,
            PointerAction::Release(PointerButton::Primary) => driver.pointer_release += 1,
            _ => {}
        }
    }
    for input in keys.read() {
        if input.key_code != KeyCode::Escape
            || input.repeat
            || driver
                .last
                .is_none_or(|gesture| gesture.window != input.window)
        {
            continue;
        }
        match input.state {
            ButtonState::Pressed => driver.escape_press += 1,
            ButtonState::Released => driver.escape_release += 1,
        }
    }
}

#[cfg(test)]
#[path = "embedded_validation_numeric_tests.rs"]
mod tests;
