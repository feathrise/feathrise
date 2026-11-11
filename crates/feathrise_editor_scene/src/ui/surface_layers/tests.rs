//! 地表列表真实ECS交互回归：分页、过期目标与未引用预设不得误删。

use feathrise_editor_ui::UiInteraction;
use bevy::prelude::*; // 直接运行正式输入与回显系统以验证查询互斥。
use feathrise_scene_content::{SCENE_SURFACE_SCHEMA_VERSION, SceneSurfaceAsset, SceneSurfaceLayer};
use std::sync::Arc; // 检查显式新建只修改层定义，不复制或填充整图像素。 // 使用正式场景地表协议。

use crate::authoring::{
    EditorAuthoringSession, SurfaceBrushStroke, SurfaceLayerEditTask, test_heightfield_draft,
}; // 复用完整草稿夹具而不写用户场景。
use crate::resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState}; // 使用正式鼠标门与作者选择状态。

use super::components::*; // 构建最小固定行池。
use super::constants::{LAYOUT, ROW_COUNT}; // 测试UI容量不限制场景层数。
use super::input::handle_input; // 运行真实选择与删除请求入口。
use super::systems::{sync_interactions, sync_rows}; // 运行真实ECS查询与增量回显。
use super::types::SurfaceLayerAction; // 模拟用户分页和删除按钮。

/// 构造七层场景，让第二页只有一行并暴露尾页收缩行为。
fn fixture() -> App {
    let mut draft = test_heightfield_draft(vec![0; 121].into()); // 无需窗口或外部文件的正式地形草稿。
    draft.surface = Some(SceneSurfaceAsset {
        schema_version: SCENE_SURFACE_SCHEMA_VERSION, // 使用当前正式格式。
        scene_id: draft.scene_id.clone(),               // 地表与草稿身份一致。
        weight_source_path: "scenes/EDGE-TEST/surface/weights.rgba8".to_owned(), // 仅测试内存，不实际写入。
        weight_texture_path: "scenes/EDGE-TEST/surface/weights.ktx2".to_owned(), // 保留完整协议字段。
        index_source_path: "scenes/EDGE-TEST/surface/indices.rgba32".to_owned(), // 场景局部索引作者源。
        index_texture_path: "scenes/EDGE-TEST/surface/indices.ktx2".to_owned(),  // 正式派生路径。
        resolution: [1, 1], // 选择测试不需要整图像素。
        layers: (0..7)
            .map(|index| SceneSurfaceLayer {
                id: format!("layer-{index}"),         // 每个场景引用层具有稳定ID。
                label: format!("材质{index}"),        // 行显示真实作者名称。
                preset_id: format!("preset-{index}"), // 不同材质用于验证笔刷同步。
            })
            .collect(), // 层数大于固定行池容量。
    }); // 不依赖全局预设资源加载。
    let mut app = App::new(); // 仅运行必要ECS系统。
    app.insert_resource(SceneEditorState {
        open: true,                                  // 地表列表接受真实输入。
        selected_scene_id: Some(draft.scene_id.clone()), // 编辑器与当前作者草稿处于同一场景。
        ..default()                                  // 其他工具使用默认无事务状态。
    }); // 编辑器处于可交互状态。
    app.insert_resource(EditorAuthoringSession {
        draft: Some(draft),
        surface_revision: 3,
        ..default()
    }); // 初始修订必须传播到行目标。
    app.init_resource::<SceneEditorNumericScrubMouseGuard>(); // 默认鼠标未被数值工具占用。
    app.init_resource::<SurfaceBrushStroke>(); // 没有活动地表笔划。
    app.init_resource::<SurfaceLayerEditTask>(); // 没有在途删除或共享物化。
    app.add_systems(Update, (handle_input, sync_rows, sync_interactions).chain()); // 与产品先输入后回显一致。
    app.world_mut().spawn((SurfaceLayerList, Node::default())); // 单个固定列表容器。
    app.world_mut()
        .spawn((SurfaceLayerActions, Node::default())); // 单个局部操作行。
    app.world_mut().spawn((
        SurfaceLayerListTitle,
        Text::new(""),
        TextColor(Color::WHITE),
    )); // 真实标题查询。
    for action in [
        SurfaceLayerAction::Create, // 空表也必须存在可点击的新建按钮。
        SurfaceLayerAction::PreviousPage,
        SurfaceLayerAction::NextPage,
        SurfaceLayerAction::DeleteSelected,
    ] {
        app.world_mut().spawn((
            SurfaceLayerButton(action),
            Node::default(),
            UiInteraction::None,
            feathrise_editor_ui::EditorButtonClick::default(),
            BackgroundColor(Color::BLACK),
        )); // 四个静态操作按钮。
    } // 不复用全局预设删除按钮。
    for slot in 0..ROW_COUNT {
        let text = app
            .world_mut()
            .spawn((SurfaceLayerRowText, Text::new(""), TextColor(Color::WHITE)))
            .id(); // 每行仅一个文本。
        app.world_mut()
            .spawn((
                SurfaceLayerRow { slot, target: None },
                Node::default(),
                UiInteraction::None,
                feathrise_editor_ui::EditorButtonClick::default(),
                BackgroundColor(Color::BLACK),
            ))
            .add_child(text); // 固定关系模拟真实列表子树。
    } // 无论地表总层数多少始终只有六行。
    app.update(); // 首帧完成真实行目标绑定，并验证Query不会发生B0001冲突。
    app // 调用方继续模拟操作。
} // 测试不会启动IO或后台整图删除。

/// 无窗口领域夹具显式传入已确认释放的动作，不以按下替代点击协议。
fn press_button(app: &mut App, action: SurfaceLayerAction) {
    let world = app.world_mut(); // 只改测试世界。
    let mut query = world.query::<(&SurfaceLayerButton, &mut UiInteraction, &mut feathrise_editor_ui::EditorButtonClick)>(); // 固定三按钮集合。
    for (button, mut interaction, mut click) in query.iter_mut(world) {
        *click = if button.0 == action { feathrise_editor_ui::EditorButtonClick::triggered() }
            else { feathrise_editor_ui::EditorButtonClick::default() };
        *interaction = if button.0 == action {
            UiInteraction::Pressed
        } else {
            UiInteraction::None
        }; // 模拟单个按钮真实按下。
    } // 其他按钮恢复未按下。
    app.update(); // 由正式输入系统消费动作。
} // 不直接调用分页实现。

#[test] // 验证真实UI可访问第七层且实体数保持恒定。
fn surface_list_paginates_selects_brush_and_shrinks_last_page() {
    let mut app = fixture(); // 初始第一页显示六层。
    let entity_count = app.world().entities().len(); // 记录固定UI实体数量。
    press_button(&mut app, SurfaceLayerAction::NextPage); // 作者明确选择后一页首项。
    let editor = app.world().resource::<SceneEditorState>(); // 检查笔刷绑定而不是只看按钮颜色。
    assert_eq!(editor.selected_surface_layer, 6); // 第七层真实可选。
    assert_eq!(
        editor.surface_selected_preset_id.as_deref(),
        Some("preset-6")
    ); // 后续笔刷不会继续涂第一层。
    assert_eq!(app.world().entities().len(), entity_count); // 翻页不增删UI实体。
    let world = app.world_mut(); // 读取真实可见行与容器尺寸。
    let mut rows = world.query::<(&SurfaceLayerRow, &Node)>(); // 只检查固定行池。
    assert_eq!(
        rows.iter(world)
            .filter(|(_, node)| node.display == Display::Flex)
            .count(),
        1
    ); // 尾页仅一行占布局。
    let mut lists = world.query_filtered::<&Node, With<SurfaceLayerList>>(); // 检查实际容器而非纯函数替身。
    assert_eq!(lists.single(world).unwrap().height, px(LAYOUT.height(1))); // 不预留五行空白。
    press_button(&mut app, SurfaceLayerAction::PreviousPage); // 能返回原始第一页。
    assert_eq!(
        app.world()
            .resource::<SceneEditorState>()
            .selected_surface_layer,
        0
    ); // 上页首项选择语义明确。
} // 回显系统没有ECS可变查询冲突。

#[test] // 尚未用于场景的管理预设不能继承旧索引作为删除目标。
fn unused_preset_selection_cannot_delete_previously_selected_layer() {
    let mut app = fixture(); // 第一层当前有效且已显示。
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .surface_selected_preset_id = Some("new-unused-preset".to_owned()); // 模拟选择目录中新预设。
    press_button(&mut app, SurfaceLayerAction::DeleteSelected); // 用户尚未显式选择场景层。
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .status
            .contains("请先在地表作者层")
    ); // UI说明缺少有效删除选择。
    assert!(!app.world().resource::<SurfaceLayerEditTask>().is_pending()); // 不启动任何整图任务。
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    ); // 无假撤销记录或作者修改。
} // 与全局预设的新增管理选择完全隔离。

#[test] // 删除或热更之后，旧UI行不能按复用下标选中另一层。
fn stale_surface_row_is_rejected_after_structure_revision() {
    let mut app = fixture(); // 初始行绑定地表修订3。
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>(); // 模拟后台采用新的引用层表。
        session
            .draft
            .as_mut()
            .unwrap()
            .surface
            .as_mut()
            .unwrap()
            .layers
            .remove(0); // 下标0现在指向原第二层。
        session.surface_revision += 1; // 正式结构变化推进地表身份。
    } // UI仍显示上一帧目标。
    let world = app.world_mut(); // 在下一帧Presentation之前模拟旧行点击。
    let mut rows = world.query::<(&SurfaceLayerRow, &mut UiInteraction, &mut feathrise_editor_ui::EditorButtonClick)>(); // 固定行不会因结构改变自动换身份。
    for (row, mut interaction, mut click) in rows.iter_mut(world) {
        if row.slot == 1 {
            *interaction = UiInteraction::Pressed;
            *click = feathrise_editor_ui::EditorButtonClick::triggered();
        } // 旧第二行当前下标已经代表另一层。
    } // 原选择仍为第0层。
    app.update(); // 正式输入先复验，再由回显刷新所有行。
    let editor = app.world().resource::<SceneEditorState>(); // 检查用户选择没有被串改。
    assert_eq!(editor.selected_surface_layer, 0); // 不把旧行下标1当作新层选择。
    assert!(editor.status.contains("已更新")); // 提示重新点击刷新后的真实行。
} // 同一身份门也保护切换场景和撤销后的行。

#[test] // 空表和重复预设都通过真实按钮创建独立作者层。
fn surface_create_button_restores_empty_scene_and_repeats_preset_without_painting() {
    use feathrise_scene_content::{
        // 构建正式解析目录，避免绕过输入系统的预设校验。
        ResolvedSurfacePresetCatalog,
        SURFACE_PRESET_SCHEMA_VERSION,
        SurfacePresetCatalogAsset,
        SurfacePresetDescriptor,
        SurfacePresetRegistry,
        SurfaceProjection,
    }; // 注册表只包含一个有效预设。
    let catalog = SurfacePresetCatalogAsset {
        // 测试不加载任何实际纹理文件。
        schema_version: SURFACE_PRESET_SCHEMA_VERSION, // 使用正式目录版本。
        texture_resolution: [4, 4],                    // 保留有效纹理规格。
        base_color_array_path: "config/surface/base.ktx2".to_owned(), // 有效目录路径。
        normal_array_path: "config/surface/normal.ktx2".to_owned(), // 有效法线路径。
        arm_height_array_path: "config/surface/arm.ktx2".to_owned(), // 有效材质路径。
        presets: vec![SurfacePresetDescriptor {
            // 同一预设可以创建两条独立作者记录。
            id: "soil".to_owned(),                  // 稳定全局预设身份。
            aliases: Vec::new(),                    // 无需旧名称映射。
            label: "草土".to_owned(),               // 行名称应保留真实中文预设名。
            base_color_srgba: [1.0; 4],             // 有效纯色预设。
            base_color_texture_path: None,          // 无外部纹理依赖。
            normal_texture_path: None,              // 无外部法线依赖。
            arm_height_texture_path: None,          // 无外部材质依赖。
            tile_meters: 4.0,                       // 合法平铺尺度。
            normal_strength: 1.0,                   // 标准法线强度。
            normal_y_flip: false,                   // 默认法线方向。
            height_blend_strength: 0.0,             // 不使用高度混合。
            perceptual_roughness: 1.0,              // 合法粗糙度。
            metallic: 0.0,                          // 普通非金属地表。
            crater_response: 1.0,                   // 未设置时浅坑不缩减。
            projection: SurfaceProjection::WorldXz, // 正式世界坐标投影。
        }], // 只有这一个预设，重复创建不能依赖切换预设。
    }; // 目录构造完成。
    let mut app = fixture(); // 复用正式UI系统与固定按钮实体。
    app.insert_resource(SurfacePresetRegistry {
        // 输入入口实际读取该资源。
        catalog: Handle::default(), // 纯ECS测试无需AssetServer。
        revision: 1,                // 触发目录显示修订。
        resolved: ResolvedSurfacePresetCatalog::build(&catalog).expect("测试目录有效"), // 使用生产解析器。
        last_error: None, // 目录处于就绪状态。
    }); // 不修改全局磁盘目录。
    let weights = Arc::new(vec![0; 4]); // 空地表保持透明权重。
    let indices = Arc::new(vec![0; 16]); // 对应一个像素的四个u32索引。
    {
        // 清空场景层，模拟用户删除最后一层后的状态。
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>(); // 借用当前真实草稿。
        let draft = session.draft.as_mut().unwrap(); // 夹具保证草稿存在。
        draft.surface.as_mut().unwrap().layers.clear(); // 只清空作者定义。
        draft.surface_weights = Some(weights.clone()); // 共享来源用于验证无整图写入。
        draft.surface_indices = Some(indices.clone()); // 索引和权重成对共享。
        session.surface_revision += 1; // 通知真实回显系统刷新空表。
    } // 释放资源借用后模拟预设选择。
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .surface_selected_preset_id = Some("soil".to_owned()); // 选择不应隐式创建作者层。
    app.update(); // 刷新空表按钮可见性。
    {
        // 从真实Node读取新建入口，而非只检查纯函数。
        let world = app.world_mut(); // 查询固定UI节点。
        let mut actions = world.query_filtered::<&Node, With<SurfaceLayerActions>>(); // 检查父操作行。
        assert_eq!(actions.single(world).unwrap().display, Display::Flex); // 空表仍保留操作行。
        let mut buttons = world.query::<(&SurfaceLayerButton, &Node)>(); // 检查真实按钮显示状态。
        for (button, node) in buttons.iter(world) {
            // 四按钮无需动态增删。
            if button.0 == SurfaceLayerAction::Create {
                // 新建不能随空列表隐藏。
                assert_eq!(node.display, Display::Flex); // 用户确实可以点击新建。
            } else {
                // 空表不显示删除或翻页。
                assert_eq!(node.display, Display::None); // 避免无目标操作占位。
            } // 每个按钮语义独立验证。
        } // 空表显示验证完成。
    } // 结束查询借用。
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    ); // 单独选择预设没有创建历史。
    press_button(&mut app, SurfaceLayerAction::Create); // 第一次点击显式创建零层场景的首层。
    assert_eq!(
        app.world()
            .resource::<SceneEditorState>()
            .selected_surface_layer,
        0
    ); // 新层立即选中。
    press_button(&mut app, SurfaceLayerAction::Create); // 同一预设再次新建，必须产生独立作者层。
    assert_eq!(
        app.world()
            .resource::<SceneEditorState>()
            .selected_surface_layer,
        1
    ); // 第二次新建立即切到新层。
    let session = app.world().resource::<EditorAuthoringSession>(); // 检查真实作者事务结果。
    let draft = session.draft.as_ref().unwrap(); // 草稿仍然存在。
    let layers = &draft.surface.as_ref().unwrap().layers; // 读取当前场景层表。
    assert_eq!(layers.len(), 2); // 同一预设也允许两层。
    assert_eq!(layers[0].preset_id, "soil"); // 第一层引用所选预设。
    assert_eq!(layers[1].preset_id, "soil"); // 第二层同样引用该预设。
    assert_ne!(layers[0].id, layers[1].id); // 独立层稳定身份不能冲突。
    assert_ne!(layers[0].label, layers[1].label); // 列表名称足以辨认两个独立作者层。
    assert_eq!(session.undo.len(), 2); // 两次新建可以分别撤销。
    assert!(Arc::ptr_eq(
        draft.surface_weights.as_ref().unwrap(),
        &weights
    )); // 新建没有复制或涂满权重图。
    assert!(Arc::ptr_eq(
        draft.surface_indices.as_ref().unwrap(),
        &indices
    )); // 新建没有复制或改写索引图。
} // 测试同时覆盖清空后的恢复编辑和同预设多层需求。
