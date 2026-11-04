//! 用原鼠标来源和实际GPU像素核对按钮反馈；禁用态及拖出释放均不得触发业务。
use super::*;

fn control(app: &mut App, wanted: ManagerControl) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &ManagerControl)>()
        .iter(world)
        .find_map(|(entity, value)| {
            (std::mem::discriminant(value) == std::mem::discriminant(&wanted)).then_some(entity)
        })
        .expect("生产管理器必须存在固定操作按钮")
}

fn pixel(pixels: &[u8], rect: Rect) -> [u8; 4] {
    // 按钮左侧中点避开文字、边框和圆角，直接比较填充在GPU中的实际颜色。
    let x = (rect.min.x + 4.).floor() as usize;
    let y = rect.center().y.floor() as usize;
    let offset = (y * 1280 + x) * 4;
    pixels[offset..offset + 4].try_into().unwrap()
}

fn one_button(
    app: &mut App,
    target: &Handle<Image>,
    output: &Path,
    prefix: &str,
    wanted: ManagerControl,
    disabled: bool,
) -> Value {
    let button = control(app, wanted);
    assert_eq!(
        app.world()
            .get::<bevy::ui::InteractionDisabled>(button)
            .is_some(),
        disabled
    );
    let before = app.world().resource::<PluginManagerState>().sequence;
    let rect = bounds(app, button);
    app.world_mut().resource_mut::<CursorFixture>().position = None;
    settle(app);
    let normal = pixel(
        &screenshot(app, target, &output.join(format!("{prefix}-normal.png"))),
        rect,
    );
    app.world_mut().resource_mut::<CursorFixture>().position = Some(rect.center());
    settle(app);
    let hovered = pixel(
        &screenshot(app, target, &output.join(format!("{prefix}-hover.png"))),
        rect,
    );
    let window = app.world().resource::<CursorFixture>().window;
    app.world_mut().write_message(MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Pressed,
        window,
    });
    settle(app);
    let pressed = pixel(
        &screenshot(app, target, &output.join(format!("{prefix}-press.png"))),
        rect,
    );
    if disabled {
        assert_eq!(normal, hovered, "禁用按钮不能仍显示悬停增亮：{prefix}");
        assert_eq!(normal, pressed, "禁用按钮不能仍显示按压反馈：{prefix}");
    } else {
        assert!(
            hovered[..3]
                .iter()
                .zip(&normal[..3])
                .any(|(a, b)| a.abs_diff(*b) >= 5),
            "可用按钮必须有实际悬停像素反馈：{prefix}/{normal:?}/{hovered:?}"
        );
        assert!(
            pressed[..3]
                .iter()
                .zip(&hovered[..3])
                .any(|(a, b)| a.abs_diff(*b) >= 5),
            "可用按钮必须有独立按压像素反馈：{prefix}/{hovered:?}/{pressed:?}"
        );
        // 原按钮捕获可以保留Pressed，但移出后视觉必须恢复，并取消松开动作。
        app.world_mut().resource_mut::<CursorFixture>().position = None;
        settle(app);
        let outside = pixel(
            &screenshot(app, target, &output.join(format!("{prefix}-drag-out.png"))),
            rect,
        );
        assert_eq!(outside, normal, "移出按钮不能保留按压高亮");
    }
    app.world_mut().write_message(MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Released,
        window,
    });
    frame(app);
    app.world_mut().resource_mut::<CursorFixture>().position = None;
    wait_for_idle(app);
    assert_eq!(
        app.world().resource::<PluginManagerState>().sequence,
        before,
        "禁用或拖出释放不能提交后台操作"
    );
    assert!(
        app.world()
            .resource::<PluginManagerState>()
            .confirm_remove
            .is_none(),
        "取消释放不能打开移除确认"
    );
    assert!(
        !app.world()
            .contains_non_send::<super::super::resources::PluginPackagePicker>(),
        "拖出更新按钮不能打开系统选择器"
    );
    json!({"button":prefix,"disabled":disabled,"normal":normal,"hovered":hovered,"pressed":pressed,
        "source":"official MouseButtonInput and actual RenderApp texture readback","cancelled_release_preserved_sequence":true})
}

pub(super) fn operation_buttons(
    app: &mut App,
    target: &Handle<Image>,
    output: &Path,
    phase: &str,
    pending: bool,
) -> Vec<Value> {
    [
        (ManagerControl::Toggle, "toggle"),
        (ManagerControl::Update, "update"),
        (ManagerControl::Remove, "remove"),
        (ManagerControl::CancelPending, "cancel-pending"),
    ]
    .into_iter()
    .map(|(control, name)| {
        one_button(
            app,
            target,
            output,
            &format!("feedback-{phase}-{name}"),
            control,
            if name == "cancel-pending" {
                !pending
            } else {
                pending
            },
        )
    })
    .collect()
}

pub(super) fn legacy_controls(app: &mut App) -> Value {
    for (wanted, disabled) in [
        (ManagerControl::Toggle, true),
        (ManagerControl::Update, true),
        (ManagerControl::Remove, false),
        (ManagerControl::CancelPending, true),
    ] {
        let entity = control(app, wanted);
        assert_eq!(
            app.world()
                .get::<bevy::ui::InteractionDisabled>(entity)
                .is_some(),
            disabled
        );
    }
    json!({"legacy_remove_enabled":true,"legacy_toggle_and_update_disabled":true,"cancel_without_pending_disabled":true})
}

pub(super) fn directory(app: &mut App, scope: PluginScope, expected: &Path) -> Value {
    assert_eq!(app.world().resource::<PluginManagerState>().scope, scope);
    let expected = crate::project::display_path(expected);
    let world = app.world_mut();
    let text = world
        .query::<(&Text, &ComputedNode, &InheritedVisibility)>()
        .iter(world)
        .find_map(|(text, node, visible)| {
            (text.0.contains(&expected) && !node.is_empty() && visible.get())
                .then_some(text.0.clone())
        })
        .expect("当前范围必须显示真实、可见的插件目录");
    let button = control(app, ManagerControl::OpenDirectory);
    assert!(
        app.world()
            .get::<bevy::ui::InteractionDisabled>(button)
            .is_none()
    );
    #[cfg(windows)]
    {
        // 只打开本轮owner夹具的两个目录；外部观察器核对Shell实际位置，不触及用户插件根。
        click(app, |action| matches!(action, ManagerAction::OpenDirectory));
        assert!(
            app.world()
                .resource::<PluginManagerState>()
                .status
                .contains("已打开插件目录"),
            "真实后台启动目录后必须发布完成回执"
        );
    }
    json!({"scope":format!("{scope:?}"),"actual_path":expected,"visible_text":text,"directory_button_enabled":true,
        "production_open_action_completed":cfg!(windows),"os_explorer_location_verified_by_external_observer":false})
}

pub(super) fn assert_global_committed(app: &App, global: &Path, id: &str) {
    let manager = app.world().resource::<PluginManagerState>();
    assert!(!manager.has_project());
    let row = manager.rows.iter().find(|row| row.id == id).unwrap();
    assert!(
        row.pending.is_none() && row.path.is_file(),
        "全局成功回执必须对应已发布文件且没有待应用计划"
    );
    let journal: Value =
        serde_json::from_slice(&fs::read(global.join(".management/pending.json")).unwrap())
            .unwrap();
    assert!(
        journal["changes"].get(id).is_none(),
        "必须真正完成日志回执，而非仅隐藏待生效文字"
    );
    let index: Value =
        serde_json::from_slice(&fs::read(global.join("index.json")).unwrap()).unwrap();
    let digest = packages::binary_digest(&row.path).unwrap();
    assert_eq!(
        index["entries"][id]["digest"].as_str(),
        Some(digest.as_str())
    );
    assert!(
        fs::read_dir(global.join(".management/receipts"))
            .unwrap()
            .next()
            .is_some(),
        "必须有真实事务回执"
    );
    assert!(manager.load_states.is_empty(), "无工程不能伪造DLL已加载");
}
