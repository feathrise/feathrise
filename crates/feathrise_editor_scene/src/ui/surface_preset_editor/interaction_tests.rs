//! 地表预设选择只设置新建目标，不隐式追加作者层的回归。

use feathrise_editor_ui::UiInteraction;
use super::*;
use bevy::{prelude::*, text::EditableText, input_focus::{InputFocus, FocusCause}, input::keyboard::KeyboardInput};
use crate::{resources::SceneEditorState, authoring::EditorAuthoringSession, ui::{vegetation_preset_editor::VegetationPresetEditorState, text_input::EditorTextInputGuard}};
use feathrise_editor_surface_model::{SurfacePresetEditorState, types::*, components::*};
use feathrise_scene_content::SurfacePresetRegistry;
// 通过真实 Create 动作构造夹具，不复制预设工厂，也不公开实现库私有函数。
fn create_preset(state: &mut SurfacePresetEditorState, workspace: &mut VegetationPresetEditorState, registry: Option<&SurfacePresetRegistry>) -> Result<(), String> {
    let mut app = App::new();
    app.add_plugins(feathrise_editor_surface::SurfaceEditorPlugin);
    app.insert_resource(std::mem::take(state)).insert_resource(std::mem::take(workspace))
        .insert_resource(SceneEditorState { open: true, ..default() })
        .init_resource::<EditorAuthoringSession>()
        .init_resource::<crate::authoring::SurfaceBrushStroke>()
        .init_resource::<crate::authoring::SurfaceLayerEditTask>()
        .init_resource::<InputFocus>().init_resource::<EditorTextInputGuard>()
        .add_systems(Update, handle_buttons);
    let registry = registry.unwrap();
    app.insert_resource(SurfacePresetRegistry { catalog: registry.catalog.clone(), revision: registry.revision, resolved: registry.resolved.clone(), last_error: None });
    app.world_mut().spawn((SurfacePresetButton(SurfacePresetAction::Create), feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed));
    app.update();
    *state = app.world_mut().remove_resource::<SurfacePresetEditorState>().unwrap();
    *workspace = app.world_mut().remove_resource::<VegetationPresetEditorState>().unwrap();
    assert!(!state.tabs.is_empty());
    Ok(())
} // 调用生产选择与新建预设夹具生成逻辑。
use feathrise_scene_content::{
    SCENE_SURFACE_SCHEMA_VERSION, SceneSurfaceAsset, ResolvedSurfacePresetCatalog,
    SURFACE_PRESET_SCHEMA_VERSION, SurfacePresetCatalogAsset,
}; // 正式协议构造测试目录和空作者表。

#[test] // 选预设只设目标，必须由新建层按钮产生作者记录。
fn surface_preset_selection_never_creates_author_layers() {
    let mut registry = SurfacePresetRegistry {
        catalog: default(),
        revision: 1,
        resolved: default(),
        last_error: None,
    }; // 空目录供生产预设工厂生成合法描述。
    let mut state = SurfacePresetEditorState::default(); // 默认没有编辑标签。
    let mut workspace = VegetationPresetEditorState::default(); // 生产工厂所需工作区状态。
    create_preset(&mut state, &mut workspace, Some(&registry)).unwrap(); // 复用正式有效预设字段，避免另造参数模板。
    let descriptor = state.tabs[0].draft.descriptor.clone(); // 取工厂生成的完整预设。
    let catalog = SurfacePresetCatalogAsset {
        // 有效纹理路径仅作协议验证。
        schema_version: SURFACE_PRESET_SCHEMA_VERSION, // 当前版本。
        texture_resolution: [4, 4],                    // 合法小尺寸。
        base_color_array_path: "config/surface/base.ktx2".into(), // 合法相对路径。
        normal_array_path: "config/surface/normal.ktx2".into(), // 合法相对路径。
        arm_height_array_path: "config/surface/arm.ktx2".into(), // 合法相对路径。
        presets: vec![descriptor.clone()],             // 单预设足以验证选择语义。
    }; // 不读取磁盘。
    registry.resolved = ResolvedSurfacePresetCatalog::build(&catalog).unwrap(); // 使用正式解析规则。
    let mut draft = crate::authoring::test_heightfield_draft(vec![0; 121].into()); // 当前编辑场景上下文。
    draft.surface = Some(SceneSurfaceAsset {
        // 模拟删除最后一层后的定义。
        schema_version: SCENE_SURFACE_SCHEMA_VERSION,
        scene_id: draft.scene_id.clone(), // 格式与场景身份一致。
        weight_source_path: "scenes/test/weights.rgba8".into(),
        weight_texture_path: "scenes/test/weights.ktx2".into(), // 权重路径保持可继续保存。
        index_source_path: "scenes/test/indices.rgba32".into(),
        index_texture_path: "scenes/test/indices.ktx2".into(), // 索引路径与权重配对。
        resolution: [1, 1],
        layers: vec![], // 空表本身是合法作者状态。
    }); // 纯选择无需访问任何像素。
    let mut editor = SceneEditorState {
        selected_scene_id: Some(draft.scene_id.clone()),
        ..default()
    }; // 保持同图身份。
    let session = EditorAuthoringSession {
        draft: Some(draft),
        ..default()
    }; // 初始没有历史。
    state.status = super::super::selection::select_preset(&mut editor, &session, registry.presets().first().unwrap(), &registry).unwrap(); // 用户真实预设点击入口。
    assert!(
        session
            .draft
            .as_ref()
            .unwrap()
            .surface
            .as_ref()
            .unwrap()
            .layers
            .is_empty()
    ); // 点击后仍等待显式新建。
    assert!(session.undo.is_empty() && !editor.dirty); // 选择不制造作者历史或保存责任。
    assert_eq!(
        editor.surface_selected_preset_id.as_deref(),
        Some(descriptor.id.as_str())
    ); // 预设正确成为新建目标。
    assert!(state.status.contains("新建层")); // 提示明确指向实际按钮。
} // 锁定显式建层交互。

#[test] // 中文已确认值同步不得重建工作区或提前改写草稿。
fn official_name_edit_keeps_entity_and_draft_until_submit() {
    // 覆盖官方输入与业务缓冲之间的边界。
    let registry = SurfacePresetRegistry {
        catalog: default(),
        revision: 1,
        resolved: default(),
        last_error: None,
    }; // 创建工厂所需空目录。
    let mut state = SurfacePresetEditorState::default(); // 独立地表会话。
    let mut workspace = VegetationPresetEditorState::default(); // 独立共享工作区。
    create_preset(&mut state, &mut workspace, Some(&registry)).unwrap(); // 使用生产创建逻辑生成合法描述符。
    state.active_field = Some(SurfacePresetField::Label); // 模拟名称已经聚焦。
    let revision = state.ui_revision; // 捕获不允许被打字改变的界面修订。
    let original = state.tabs[0].draft.descriptor.label.clone(); // 记录尚未提交的原草稿值。
    let mut app = App::new(); // 只运行业务系统，无窗口或渲染依赖。
    app.add_plugins(feathrise_editor_surface::SurfaceEditorPlugin);
    app.insert_resource(state)
        .insert_resource(workspace)
        .insert_resource(SceneEditorState {
            open: true,
            ..default()
        }); // 准备有效编辑会话。
    app.init_resource::<InputFocus>()
        .init_resource::<EditorTextInputGuard>()
        .add_message::<KeyboardInput>(); // 提供官方焦点与业务消息。
    app.add_systems(Update, handle_text_input); // 调用正式业务同步入口。
    let entity = app
        .world_mut()
        .spawn((
            SurfacePresetButton(SurfacePresetAction::Focus(SurfacePresetField::Label)),
            EditableText::new("黄河湿沙"),
        ))
        .id(); // 官方组件持有已确认中文。
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(entity, FocusCause::Navigated); // 与实际名称点击采用相同焦点。
    app.update(); // 同步一帧已确认文字。
    let state = app.world().resource::<SurfacePresetEditorState>(); // 检查真实生产状态。
    assert_eq!(state.input_buffer, "黄河湿沙"); // 中文完整进入业务缓冲。
    assert_eq!(state.ui_revision, revision); // 打字不会销毁承载光标的实体。
    assert_eq!(state.tabs[0].draft.descriptor.label, original); // 保存前不改写正式草稿。
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(entity)); // 同一官方输入实体持续持有焦点。
    app.world_mut()
        .get_mut::<EditableText>(entity)
        .unwrap()
        .editor
        .set_text("最后确认字"); // 模拟官方本帧已应用的最后一次编辑。
    let other = app.world_mut().spawn(EditableText::new("其他输入框")).id(); // 用户鼠标已切换到另一个官方输入域。
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(other, FocusCause::Pressed); // 切换发生在业务同步之前。
    app.world_mut()
        .resource_mut::<Messages<KeyboardInput>>()
        .write(KeyboardInput {
            // 新输入域的确认键不能提交底层草稿。
            key_code: KeyCode::Enter, // 使用真实确认键。
            logical_key: bevy::input::keyboard::Key::Enter, // 与系统生产分支一致。
            state: bevy::input::ButtonState::Pressed, // 本帧按下边沿。
            text: None,               // 控制键不含文字。
            repeat: false,            // 单次按下。
            window: other,            // 最小测试只需要合法实体身份。
        }); // 事件进入真实业务Reader。
    app.update(); // 执行失焦情况下的业务同步。
    let state = app.world().resource::<SurfacePresetEditorState>(); // 检查旧会话保留的最新文字。
    assert_eq!(state.input_buffer, "最后确认字"); // 最新字必须在失焦返回前读回，而非沿用上一帧缓冲。
    assert_eq!(state.active_field, Some(SurfacePresetField::Label)); // 其他输入域的Enter不能提交底层名称。
    assert_eq!(state.ui_revision, revision); // 失焦同步不重建活动输入树。
} // 中文输入稳定性回归结束。
