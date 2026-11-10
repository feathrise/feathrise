//! 输入链回归：使用真实Interaction、祖先归属与AssetServer，不模拟旁路分派。

use feathrise_editor_ui::UiInteraction;
use super::super::interaction::{handle_reference_buttons, selection_action}; // 生产按钮系统和票据检查。
use super::*; // 正式目录草稿及字段身份夹具。
use crate::resources::{EditorAssetCatalog, SceneEditorNumericScrubMouseGuard, SceneEditorState}; // 真实编辑会话资源。
use crate::ui::sidebars::{EditorRightSidebarTab, SceneEditorSidebarState}; // 验证右栏导航。

/// 提供输入和后续轮询共用的最小资产资源，不初始化窗口与渲染。
pub(super) fn app(mesh: bool) -> App {
    let mut app = App::new(); // 只安装当前功能需要的系统。
    app.add_plugins((MinimalPlugins, AssetPlugin::default())) // 真实异步句柄管理与任务池。
        .init_asset::<Gltf>()
        .init_asset::<GltfMesh>()
        .init_asset::<bevy::gltf::GltfNode>() // 根与元数据的AssetEvent。
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .init_asset::<Image>() // 最后落值的真实类型容器。
        .insert_resource(state(mesh)) // 一个实际随包预设草稿。
        .init_resource::<crate::ui::text_input::EditorTextInputGuard>() // 官方候选门默认允许事务。
        .init_resource::<EditorAssetCatalog>()
        .init_resource::<SceneEditorSidebarState>() // 定位调用不直接开始IO扫描。
        .init_resource::<SceneEditorNumericScrubMouseGuard>(); // 默认没有拖动捕获。
    let mut editor = SceneEditorState::default(); // 启用F12后才响应按钮。
    editor.open = true; // 不创建真实窗口。
    app.insert_resource(editor); // Query<Window>允许空集合。
    app
}

/// 将动作放到真实所属页下面，确保测试覆盖ChildOf祖先查找。
pub(super) fn press(app: &mut App, owner: u64, action: AssetReferenceAction) {
    let root = app.world_mut().spawn(PresetReferenceOwner(owner)).id(); // 显式标签拥有者。
    let row = app.world_mut().spawn(ChildOf(root)).id(); // 和生产同行控件相同的中间父节点。
    app.world_mut().spawn((
        ChildOf(row),
        feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed,
        PresetEditorButton(PresetEditorAction::AssetReference(action)),
    )); // 生产系统消费已确认点击。
    app.update(); // 一帧只分派一次动作。
}

#[test] // 定位当前可见字符但不把正在输入的值提交到预设。
fn locate_preserves_the_edit_buffer_and_entire_draft() {
    let mut app = app(false); // Blade LOD网格路径。
    app.add_systems(Update, handle_reference_buttons); // 真正输入系统。
    let (id, original) = {
        let mut state = app
            .world_mut()
            .resource_mut::<VegetationPresetEditorState>(); // 模拟用户正编辑路径。
        state.active_field = Some(mesh_reference_field(false)); // 同行输入框有焦点。
        state.input_buffer = "models/typed.glb#Mesh2/Primitive1".to_owned(); // 尚未按Enter的字符。
        (
            state.active_preset_tab_id().unwrap(),
            state.active_draft().unwrap().descriptor.clone(),
        ) // 冻结整份正式草稿比较。
    };
    press(
        &mut app,
        id,
        AssetReferenceAction::Locate(mesh_reference_field(false)),
    ); // 使用生产专用动作。
    let state = app.world().resource::<VegetationPresetEditorState>(); // 定位完成登记后读取会话。
    assert_eq!(state.active_draft().unwrap().descriptor, original); // 场景和预设都没有因导航变脏。
    assert_eq!(state.input_buffer, "models/typed.glb#Mesh2/Primitive1"); // 原字符缓冲仍可继续编辑。
    assert_eq!(state.active_field, Some(mesh_reference_field(false))); // 不偷走输入焦点。
    assert_eq!(
        app.world().resource::<SceneEditorSidebarState>().navigator,
        crate::ui::sidebars::EditorNavigatorTab::Assets
    ); // 立即打开左侧浏览器，验证完成前不替换右侧检查来源。
    assert_eq!(app.world().resource::<SceneEditorSidebarState>().right, EditorRightSidebarTab::Scene);
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .status
            .contains("models/typed.glb#Mesh2/Primitive1")
    ); // 导航使用可见字符而不是旧草稿路径。
}

#[test] // 图片无法写进GLB字段，也不能顺带提交另一个正在输入的名称。
fn wrong_type_and_old_page_buttons_cannot_commit_other_inputs() {
    for stale_owner in [false, true] {
        let mut app = app(false); // 两种拒绝场景分别独立。
        app.add_systems(Update, handle_reference_buttons); // 使用真实Query初始化。
        let id = {
            let mut state = app
                .world_mut()
                .resource_mut::<VegetationPresetEditorState>(); // 保持未提交名称输入。
            state.active_field = Some(PresetField::Label); // 不同字段正在输入。
            state.input_buffer = "尚未提交的名字".to_owned(); // 必须在失败后完整保留。
            state.active_preset_tab_id().unwrap() // 活动页真实ID。
        };
        let original = app
            .world()
            .resource::<VegetationPresetEditorState>()
            .active_draft()
            .unwrap()
            .descriptor
            .clone(); // 完整草稿基线。
        app.world_mut()
            .resource_mut::<SceneEditorState>()
            .selected_asset = Some(if stale_owner { "tree.glb" } else { "grass.png" }.to_owned()); // 旧拥有者用合法类型以验证归属门。
        press(
            &mut app,
            if stale_owner { id + 99 } else { id },
            AssetReferenceAction::UseSelected(mesh_reference_field(false)),
        ); // 不合法类型或迟到旧页动作。
        let state = app.world().resource::<VegetationPresetEditorState>(); // 两条拒绝路径都检查所有副作用。
        assert_eq!(state.active_draft().unwrap().descriptor, original); // 不能错误提交名称。
        assert_eq!(state.input_buffer, "尚未提交的名字"); // 不能清空其他字段字符。
        assert!(state.asset_reference.is_none()); // 不发起错误请求。
    }
}

#[test] // 点击网格行的使用选中，目标始终是网格行而不是此前材质焦点。
fn assignment_is_bound_to_the_clicked_field_and_accepts_empty_input() {
    let mut app = app(true); // 真实Mesh部件。
    app.add_systems(Update, handle_reference_buttons); // 输入系统创建异步请求。
    let field = PresetField::MeshPart {
        lod: 0,
        part: 0,
        field: MeshPartField::MeshPath,
    }; // 要填的具体网格行。
    let (id, original) = {
        let mut state = app
            .world_mut()
            .resource_mut::<VegetationPresetEditorState>(); // 模拟先把当前输入清空再选文件。
        state.active_field = Some(field); // 明确本行焦点。
        state.input_buffer.clear(); // 空缓冲不应阻碍通过浏览器设置资产。
        (
            state.active_preset_tab_id().unwrap(),
            state.active_draft().unwrap().descriptor.clone(),
        ) // 保留真实原值。
    };
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_asset = Some("models/new.glb".to_owned()); // 合法类型，先异步检查。
    press(&mut app, id, AssetReferenceAction::UseSelected(field)); // 不执行文本提交。
    let state = app.world().resource::<VegetationPresetEditorState>(); // 加载完成前不落值。
    assert_eq!(state.asset_reference.as_ref().unwrap().target.field, field); // 冻结明确按钮目标。
    assert_eq!(state.active_draft().unwrap().descriptor, original); // 加载开始不代表设置成功。
    assert!(state.active_field.is_none()); // 请求已经接管本行，IME不继续修改底层缓冲。
}

#[test] // 旧面板的取消/选择和不同标签都不能影响后来的请求。
fn old_picker_tickets_cannot_cancel_or_choose_a_new_request() {
    let mut app = app(true); // 仅需要真实AssetServer资源。
    let server = app.world().resource::<AssetServer>().clone(); // 克隆共享服务器，不新增后台服务。
    let mut state = app
        .world_mut()
        .resource_mut::<VegetationPresetEditorState>(); // 当前活动草稿。
    let field = PresetField::MeshPart {
        lod: 0,
        part: 0,
        field: MeshPartField::MeshPath,
    }; // 精确单字段。
    let mut pending = request(&state, field); // 已完成候选枚举。
    pending.ticket = 7; // 新的请求票据。
    pending.phase = ReferencePhase::Choosing; // 作者正在明确选择。
    pending.choices = vec![SubAssetChoice::Mesh {
        mesh: 1,
        primitive: 0,
    }]; // 真实候选索引夹具。
    let owner = pending.target.tab_id; // 所属页与票据要同时匹配。
    state.asset_reference = Some(pending); // 生产资源唯一槽。
    assert!(
        selection_action(
            &mut state,
            owner,
            AssetReferenceAction::Cancel { ticket: 6 },
            &server
        )
        .is_err()
    ); // 旧票据不能取消新请求。
    assert!(
        selection_action(
            &mut state,
            owner + 1,
            AssetReferenceAction::Choose {
                ticket: 7,
                index: 0
            },
            &server
        )
        .is_err()
    ); // 错误所属页不能采用。
    assert!(
        selection_action(
            &mut state,
            owner,
            AssetReferenceAction::Choose {
                ticket: 7,
                index: 2
            },
            &server
        )
        .is_err()
    ); // 越界项也不猜默认值。
    assert_eq!(state.asset_reference.as_ref().unwrap().ticket, 7); // 拒绝不移除真正的新请求。
    selection_action(
        &mut state,
        owner,
        AssetReferenceAction::Cancel { ticket: 7 },
        &server,
    )
    .unwrap(); // 正确的取消按钮才生效。
    assert!(state.asset_reference.is_none()); // 结束但不写任何字段。
}
