//! 中央长表单资产选择可见性：发起、等待、选定和离开页面的实际ECS滚动回归。

use super::super::scroll::sync_picker_scroll; // 生产一次性滚动交接系统。
use super::*; // 正式草稿及冻结字段身份夹具。
use crate::resources::MapEditorState; // F12可见性门。

fn app() -> (App, Entity) {
    let mut state = state(false); // 从普通草LOD网格字段发起。
    state.asset_reference = Some(request(&state, mesh_reference_field(false))); // 正在检查资源，尚未显示完整候选。
    let mut editor = MapEditorState::default(); // 不涉及地图作者修改。
    editor.open = true; // 中央编辑正文当前可见。
    let mut app = App::new(); // 测试不打开窗口。
    app.insert_resource(state)
        .insert_resource(editor)
        .add_systems(Update, sync_picker_scroll); // 使用真实Query与Local缓存。
    let page = app
        .world_mut()
        .spawn((
            PresetWorkspaceEditorPage,
            ScrollPosition(Vec2::new(0.0, 700.0)),
        ))
        .id(); // 模拟原字段位于长表单靠下位置。
    (app, page)
}

#[test] // 发起和候选就绪露出选择区，成功后即使路径已变也应回到原字段。
fn picker_reveals_once_then_restores_the_original_field_position() {
    let (mut app, page) = app(); // 原滚动位置700逻辑像素。
    app.update(); // 发起请求后显示选择区域。
    assert_eq!(app.world().get::<ScrollPosition>(page).unwrap().y, 0.0); // 不能在屏幕外显示唯一取消按钮。
    app.world_mut().get_mut::<ScrollPosition>(page).unwrap().y = 90.0; // 作者选择期间仍可手动滚动。
    app.update(); // 同阶段没有抢回行为。
    assert_eq!(app.world().get::<ScrollPosition>(page).unwrap().y, 90.0); // 不逐帧拉回顶部。
    app.world_mut()
        .resource_mut::<VegetationPresetEditorState>()
        .asset_reference
        .as_mut()
        .unwrap()
        .phase = ReferencePhase::Choosing; // 模拟完整候选到达。
    app.update(); // 新阶段再次露出候选。
    assert_eq!(app.world().get::<ScrollPosition>(page).unwrap().y, 0.0); // 长等待期间滚远也能看到结果。
    {
        let mut state = app
            .world_mut()
            .resource_mut::<VegetationPresetEditorState>(); // 模拟生产已经验证通过后的唯一落值。
        let target = state.asset_reference.as_ref().unwrap().target.clone(); // 原请求身份。
        apply_value(&mut state, &target, "models/new.glb").unwrap(); // 原值真的发生变化，不能依赖原路径判断UI回位。
        state.asset_reference = None; // 完成后移除选择区。
    }
    app.update(); // 表单与滚动一起恢复。
    assert_eq!(app.world().get::<ScrollPosition>(page).unwrap().y, 700.0); // 回到刚刚修改的字段。
}

#[test] // 离开原分区导致取消时，不把新分区卷到旧字段位置。
fn picker_does_not_restore_old_scroll_after_section_navigation() {
    let (mut app, page) = app(); // 原字段位于700逻辑像素。
    app.update(); // 请求开始并记录原位置。
    {
        let mut state = app
            .world_mut()
            .resource_mut::<VegetationPresetEditorState>(); // 模拟用户点击新的分区。
        state.active_draft_mut().unwrap().section = PresetEditorSection::BladeLod; // 新页不属于原LOD字段。
        state.asset_reference = None; // 轮询根据导航变化取消旧请求。
    }
    app.world_mut().get_mut::<ScrollPosition>(page).unwrap().y = 30.0; // 用户新分区的位置。
    app.update(); // 旧请求结束时禁止跨分区恢复。
    assert_eq!(app.world().get::<ScrollPosition>(page).unwrap().y, 30.0); // 新导航优先。
}
