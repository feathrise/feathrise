//! 子资产引用纯会话回归；不读取或修改测试机器剪贴板。

use feathrise_editor_ui::UiInteraction;
use super::*; // 直接验证UI与复制共用的生产引用生成规则。
use crate::resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState}; // 行为测试确认不写场景dirty状态。
use crate::ui::sidebars::{EditorRightSidebarTab, SceneEditorSidebarState}; // 只有真实资产页接受按钮。
use bevy::asset::AssetPlugin; // 提供生产AssetServer但不启动窗口或渲染器。
use bevy::world_serialization::WorldAsset; // 构造最小Scene0内容句柄。

/// 创建一个默认Scene0模型预览会话。
fn state() -> AssetPreviewState {
    AssetPreviewState {
        selected_path: Some("models/tree.glb".to_owned()), // 当前文件身份。
        content: AssetPreviewContent::Model {
            handle: Handle::<WorldAsset>::default(), // 测试不执行实际加载。
            root: Entity::PLACEHOLDER,               // 测试不访问ECS实体。
            phase: AssetPreviewPhase::Ready,         // 引用生成不依赖渲染。
        },
        model: Some(AssetPreviewModelSession::new(1, Handle::default())), // 默认目标固定为Scene0。
        ..default() // 相机、灯光和边界与文本引用无关。
    }
}

#[test] // 默认完整预览必须明确复制Scene0而不是含糊文件路径。
fn scene_and_primitive_references_are_complete() {
    let mut state = state(); // 初始完整Scene0。
    assert_eq!(
        primary_reference(&state).as_deref(),
        Some("models/tree.glb#Scene0")
    ); // 多Scene文件也只声明当前第零Scene。
    assert!(selected_material_reference(&state).is_none()); // Scene0没有唯一材质可复制。
    let choice = AssetPreviewPrimitiveChoice {
        id: AssetPreviewPrimitiveId {
            mesh: 2,      // 真实Mesh编号。
            primitive: 1, // 真实primitive编号。
        },
        material: Some(3),        // 此primitive原关联Material3。
        requires_skinning: false, // 静态引用仍可成为当前单体。
    };
    let model = state.model.as_mut().unwrap(); // 当前唯一模型会话。
    model.target = AssetPreviewModelTarget::Primitive(choice.id); // 模拟作者明确选择。
    model.selected_primitive = Some(choice); // 冻结材质身份。
    assert_eq!(
        primary_reference(&state).as_deref(),
        Some("models/tree.glb#Mesh2/Primitive1")
    ); // 主引用精确到单体。
    assert_eq!(
        selected_material_reference(&state).as_deref(),
        Some("models/tree.glb#Material3/std")
    ); // 材质引用使用Bevy 0.19 StandardMaterial标签。
}

#[test] // 省略材质的primitive必须显示真实默认材质标签。
fn missing_explicit_material_uses_default_material_reference() {
    let mut state = state(); // 同一GLB文件。
    let choice = AssetPreviewPrimitiveChoice {
        id: AssetPreviewPrimitiveId {
            mesh: 0,      // 第一网格。
            primitive: 0, // 第一primitive。
        },
        material: None,           // GLTF确实省略材质。
        requires_skinning: false, // 默认材质与是否蒙皮相互独立。
    };
    let model = state.model.as_mut().unwrap(); // 当前唯一模型会话。
    model.target = AssetPreviewModelTarget::Primitive(choice.id); // 切成单体。
    model.selected_primitive = Some(choice); // 保留默认材质事实。
    assert_eq!(
        selected_material_reference(&state).as_deref(),
        Some("models/tree.glb#DefaultMaterial/std")
    ); // 不猜Material0。
}

#[test] // primitive选择和切回Scene0只改变预览会话，并保持唯一可见根。
fn primitive_selection_is_preview_only_and_scene0_is_restorable() {
    let mut app = App::new(); // 无GPU测试应用。
    app.add_plugins((MinimalPlugins, AssetPlugin::default())) // 使用真实异步句柄登记入口。
        .init_asset::<Mesh>() // show_primitive登记精确Mesh句柄。
        .init_asset::<StandardMaterial>() // show_primitive登记/std材质句柄。
        .init_resource::<SceneEditorNumericScrubMouseGuard>() // 默认没有其他鼠标所有者。
        .init_resource::<AssetPreviewNavigation>() // 切换目标可验证重置。
        .add_systems(Update, handle_asset_preview_actions); // 只安装本任务生产输入系统。
    let scene_root = app
        .world_mut()
        .spawn((AssetPreviewModelRoot, Visibility::Inherited))
        .id(); // 当前完整Scene0根。
    let mut preview = state(); // 默认Scene0会话。
    if let AssetPreviewContent::Model { root, .. } = &mut preview.content {
        *root = scene_root; // 绑定真实测试实体。
    }
    let choice = AssetPreviewPrimitiveChoice {
        id: AssetPreviewPrimitiveId {
            mesh: 2,      // 当前页唯一真实网格编号。
            primitive: 1, // 当前页唯一真实primitive编号。
        },
        material: Some(3),        // 原材质必须随请求冻结。
        requires_skinning: false, // 本用例继续覆盖正常静态单体往返。
    };
    let model = preview.model.as_mut().unwrap(); // 当前唯一模型会话。
    model.catalog.phase = AssetPreviewCatalogPhase::Ready; // 完整候选已经原子发布。
    model.catalog.choices = vec![choice]; // 固定行零号绑定这一个候选。
    model.scene_ready = true; // Scene0可在切回时立即恢复。
    model.scene_bounds = Some(AssetPreviewModelBounds {
        minimum: Vec3::splat(-2.0), // 已缓存完整Scene0下界。
        maximum: Vec3::splat(2.0),  // 已缓存完整Scene0上界。
    });
    model.scene_mesh_count = 4; // 完整Scene0状态栏计数。
    app.insert_resource(preview); // 生产系统读取当前预览。
    let mut editor = SceneEditorState::default(); // 场景作者基线。
    editor.open = true; // F12已打开。
    editor.selected_asset = Some("models/tree.glb".to_owned()); // 与预览文件身份匹配。
    app.insert_resource(editor); // 发布作者状态。
    let mut sidebar = SceneEditorSidebarState::default(); // 默认页签状态。
    sidebar.right = EditorRightSidebarTab::Assets; // 显示真实资产页。
    app.insert_resource(sidebar); // 输入资格完整。
    app.world_mut().spawn((
        AssetPreviewAction::ShowPrimitive(0), // 点击当前页第一固定槽。
        feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed,                 // 本帧按下边沿。
    ));
    app.finish(); // 完成AssetPlugin加载器注册。
    app.cleanup(); // 与正式App插件生命周期一致。
    app.update(); // 执行真实选择系统并应用Commands。
    {
        let preview = app.world().resource::<AssetPreviewState>(); // 检查单体请求而非磁盘加载结果。
        assert_eq!(
            preview.model.as_ref().unwrap().target,
            AssetPreviewModelTarget::Primitive(choice.id)
        ); // 当前目标精确到Mesh2/Primitive1。
        assert_eq!(
            preview.model.as_ref().unwrap().selected_primitive,
            Some(choice)
        ); // 原材质身份同时冻结。
        assert!(preview.model.as_ref().unwrap().primitive_load.is_some()); // 同时最多一个在途组合。
        assert!(preview.model_bounds.is_none()); // 不能暂用Scene0范围冒充单体居中。
        assert!(matches!(
            preview.content,
            AssetPreviewContent::Model {
                phase: AssetPreviewPhase::Loading,
                ..
            }
        )); // 网格、材质和范围完成前不显示。
    }
    assert_eq!(
        app.world().entity(scene_root).get::<Visibility>(),
        Some(&Visibility::Hidden)
    ); // 单体模式同帧隐藏完整Scene0。
    assert!(!app.world().resource::<SceneEditorState>().dirty); // 选择没有进入场景撤销/保存语义。
    let primitive_root = app.world_mut().spawn(AssetPreviewPrimitiveRoot).id(); // 模拟完整单体已经采用。
    {
        let mut preview = app.world_mut().resource_mut::<AssetPreviewState>(); // 建立切回前稳定状态。
        let model = preview.model.as_mut().unwrap(); // 当前模型会话。
        model.primitive_root = Some(primitive_root); // 当前唯一单体实体。
        model.primitive_load = None; // 已不再加载。
        if let AssetPreviewContent::Model { phase, .. } = &mut preview.content {
            *phase = AssetPreviewPhase::Ready; // 模拟单体显示完成。
        }
    }
    app.world_mut().spawn((
        AssetPreviewAction::ShowScene0, // 作者明确切回完整场景。
        feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed,           // 新按钮实体提供独立Changed边沿。
    ));
    app.update(); // 执行Scene0恢复并回收单体。
    let preview = app.world().resource::<AssetPreviewState>(); // 最终完整场景状态。
    assert_eq!(
        preview.model.as_ref().unwrap().target,
        AssetPreviewModelTarget::Scene0
    ); // 默认目标已恢复。
    assert!(preview.model.as_ref().unwrap().primitive_root.is_none()); // 会话不再保留旧单体。
    assert!(preview.model_bounds.is_some()); // 同代Scene0直接复用完整缓存。
    assert!(matches!(
        preview.content,
        AssetPreviewContent::Model {
            phase: AssetPreviewPhase::Ready,
            ..
        }
    )); // 不重复扫描稳定Scene0。
    assert_eq!(
        app.world().entity(scene_root).get::<Visibility>(),
        Some(&Visibility::Inherited)
    ); // 完整根重新成为唯一可见目标。
    assert!(app.world().get_entity(primitive_root).is_err()); // 旧单体实体已精确回收。
    assert!(!app.world().resource::<SceneEditorState>().dirty); // 往返选择仍不修改场景。
}

#[test] // 点击蒙皮primitive必须保留当前完整场景并给出明确提示，不能进入裸Mesh3d加载。
fn skinned_primitive_selection_is_rejected_before_scene0_is_hidden() {
    // 覆盖截图中的真实崩溃入口顺序。
    let mut app = App::new(); // 无GPU应用足以证明生成命令从未登记。
    app.add_plugins((MinimalPlugins, AssetPlugin::default())) // 提供正式AssetServer与输入系统运行基础。
        .init_asset::<Mesh>() // 系统签名要求正式Mesh资产资源。
        .init_asset::<StandardMaterial>() // 系统签名要求正式材质资产资源。
        .init_resource::<SceneEditorNumericScrubMouseGuard>() // 默认没有其他鼠标所有者。
        .init_resource::<AssetPreviewNavigation>() // 拒绝动作不得修改当前观察状态。
        .add_systems(Update, handle_asset_preview_actions); // 直接执行生产点击入口。
    let scene_root = app // 建立当前仍可见的完整Scene0根。
        .world_mut() // 测试主世界负责实体生命周期。
        .spawn((AssetPreviewModelRoot, Visibility::Inherited)) // 模拟已经完整显示的角色场景。
        .id(); // 冻结后续可见性断言身份。
    let mut preview = state(); // 从默认Scene0模型会话开始。
    if let AssetPreviewContent::Model { root, .. } = &mut preview.content {
        // 把测试会话绑定到真实根。
        *root = scene_root; // 拒绝点击后必须继续引用并显示此根。
    } // 不改动默认Ready阶段。
    let choice = AssetPreviewPrimitiveChoice {
        // 模拟目录从JOINTS与WEIGHTS识别出的身体网格。
        id: AssetPreviewPrimitiveId {
            // 使用截图对应的首个子资产编号。
            mesh: 0,      // 蒙皮身体为Mesh0。
            primitive: 0, // 身体只有Primitive0。
        }, // 完成数字标签身份。
        material: Some(0),       // 原材质存在但不能代替Skin绑定。
        requires_skinning: true, // 关键分类阻止直接生成Mesh3d。
    }; // 候选仍保留在UI目录供作者理解GLB结构。
    let model = preview.model.as_mut().expect("测试模型会话存在"); // 取得当前唯一目录。
    model.catalog.phase = AssetPreviewCatalogPhase::Ready; // 只有完整发布的目录行才能接收点击。
    model.catalog.choices = vec![choice]; // 第一固定槽绑定蒙皮身体。
    model.scene_ready = true; // 完整Scene0已经可以继续显示。
    app.insert_resource(preview); // 发布生产输入系统读取的预览状态。
    let mut editor = SceneEditorState::default(); // 建立F12作者状态。
    editor.open = true; // 输入只在编辑器打开时有效。
    editor.selected_asset = Some("models/tree.glb".to_owned()); // 与测试预览路径保持完全一致。
    app.insert_resource(editor); // 发布当前作者身份。
    let mut sidebar = SceneEditorSidebarState::default(); // 建立右栏会话。
    sidebar.right = EditorRightSidebarTab::Assets; // 只有资产页接受子资产点击。
    app.insert_resource(sidebar); // 发布输入资格。
    app.world_mut().spawn((
        // 生成一次真实Pressed边沿。
        AssetPreviewAction::ShowPrimitive(0), // 点击当前页第一行蒙皮primitive。
        feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed,                 // 本帧触发生产动作处理。
    )); // UI实体本身不携带任何资产数据。
    app.finish(); // 完成AssetPlugin加载器注册。
    app.cleanup(); // 与正式应用保持相同插件生命周期。
    app.update(); // 执行拒绝门并应用本帧命令。

    let preview = app.world().resource::<AssetPreviewState>(); // 核对动作没有切换模型目标。
    let model = preview.model.as_ref().expect("模型会话必须保留"); // Scene0恢复入口仍然存在。
    assert_eq!(model.target, AssetPreviewModelTarget::Scene0); // 蒙皮行不能成为当前单体。
    assert!(model.selected_primitive.is_none()); // 不冻结可被摆放入口持久化的危险选择。
    assert!(model.primitive_load.is_none()); // 不登记任何Mesh或材质单体加载请求。
    assert!(preview.status.contains("蒙皮网格")); // 画布状态明确说明拒绝原因。
    assert!(preview.status.contains("完整场景Scene0")); // 作者可直接知道应选择的恢复路径。
    assert_eq!(
        // 完整角色场景必须保持可见而不是变为空白画布。
        app.world().entity(scene_root).get::<Visibility>(), // 读取同一Scene0根显隐组件。
        Some(&Visibility::Inherited),                       // 拒绝发生在隐藏操作之前。
    ); // GPU渲染仍只接收官方GLTF加载器生成的完整蒙皮层级。
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .status
            .contains("蒙皮网格")
    ); // F12底栏同步同一提示。
} // 测试未创建AssetPreviewPrimitiveRoot，也未修改场景dirty状态。
