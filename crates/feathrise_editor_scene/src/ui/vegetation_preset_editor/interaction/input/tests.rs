//! 预设字符输入的字段分流回归；只检查内存缓冲，不启动窗口或系统剪贴板。

use super::*; // 子模块直接验证生产输入辅助函数，不复制另一套字符规则。

#[test] // 数值输入只保留既有Rust数字字符，不接受路径或中文。
fn numeric_input_keeps_the_existing_ascii_grammar() {
    // 备注功能不得改变任何数值字段。
    let mut value = String::new(); // 模拟一个尚未提交的普通数值缓冲。
    push_number(&mut value, "-1.25e+2米abc"); // 混合输入只取合法数字语法字符。
    assert_eq!(value, "-1.25e+2"); // 原有指数、符号和小数支持完整保留。
} // 数值分流回归完成。

#[test] // 原生焦点本帧已经切走时仍必须提交旧实体的最后一次官方编辑。
fn switching_fields_reads_the_previous_official_value_before_commit() {
    // 不启动窗口或读取剪贴板。
    use super::super::super::types::{PresetDraft, PresetDraftMode}; // 真实业务草稿。
    use bevy::input_focus::{FocusCause, InputFocus}; // 与生产相同的焦点切换。
    let catalog: feathrise_scene_content::VegetationPresetCatalogAsset =
        serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/vegetation_presets.vegetation_presets.json"
        )))
        .unwrap(); // 完整正式预设夹具。
    let mut state = VegetationPresetEditorState::default(); // 单次独立编辑会话。
    state.open_draft(PresetDraft::new(
        PresetDraftMode::Create,
        catalog.presets.into_iter().next().unwrap(),
    )); // 进入真实活动标签。
    state.active_field = Some(PresetField::Label); // 原先正在编辑名称。
    state.input_buffer = "上帧旧名称".to_owned(); // 模拟业务镜像还没收到本帧末字。
    let mut app = App::new(); // 仅运行真实业务输入系统。
    app.insert_resource(state)
        .insert_resource(SceneEditorState {
            open: true,
            ..default()
        })
        .init_resource::<InputFocus>()
        .init_resource::<EditorTextInputGuard>()
        .add_message::<KeyboardInput>()
        .add_systems(Update, handle_text_input); // Query<Window>允许无窗口。
    app.world_mut().spawn((
        PresetEditorFieldText(PresetField::Label),
        EditableText::new("同帧最后中文"),
    )); // 官方系统已经更新的旧名称实体。
    let notes = app
        .world_mut()
        .spawn((
            PresetEditorFieldText(PresetField::Notes),
            EditableText::new("备注"),
        ))
        .id(); // 用户本帧刚点中的新字段。
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(notes, FocusCause::Pressed); // 焦点已离开旧名称。
    app.update(); // 执行正式切字段提交链。
    let state = app.world().resource::<VegetationPresetEditorState>(); // 检查真实业务结果。
    assert_eq!(
        state.active_draft().unwrap().descriptor.label,
        "同帧最后中文"
    ); // 末字不得丢失。
    assert_eq!(state.active_field, Some(PresetField::Notes)); // 仍正常进入新字段。
    assert_eq!(state.input_buffer, "备注"); // 新字段镜像不复用旧名称。
} // 同帧失焦读回回归结束。
