//! 实际Bevy GLTF加载器端到端回归；测试专用模型，不依赖作者磁盘资源或GPU。

use super::super::interaction::handle_reference_buttons; // 生产输入系统。
use super::super::loading::{invalidate_changed_assets, poll_reference}; // 完整生产加载/候选/落值链。
use super::interaction::press; // 通过真正带页拥有者的Interaction操作。
use super::*; // 使用正式随包预设作为目标草稿。
use crate::resources::{EditorAssetCatalog, MapEditorNumericScrubMouseGuard, MapEditorState}; // 正式会话与鼠标门。
use crate::ui::sidebars::MapEditorSidebarState;
use bevy::gltf::GltfPlugin; // 安装项目实际版本的官方加载器。
use std::time::{Duration, Instant}; // 轮询限时仅为测试失败快速退出。 // 保持资产页资源契约。

/// 只安装资产插件，模型不进入RenderApp，也不打开游戏窗口。
fn app() -> App {
    let mut app = App::new(); // 独立无GPU应用。
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin {
            file_path: concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/asset_reference"
            )
            .to_owned(), // 仅能读取本回归的固定测试目录。
            ..default() // 沿用正式AssetServer异步IO与缓存。
        },
        GltfPlugin::default(),
    )) // 与生产同一GLTF解析器和子资产标签规则。
    .init_asset::<WorldAsset>() // Bevy0.19的GLTF Scene标签承载WorldAsset，Scene是BSN构造trait。
    .init_asset::<Mesh>()
    .init_asset::<Image>() // 场景、网格和图片依赖类型。
    .init_asset::<StandardMaterial>() // 网格用例不需要PBR渲染插件，但最终验证资源必须存在。
    .init_asset::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>() // GLTF蒙皮类型登记，不实际创建蒙皮。
    .insert_resource(state(true)) // 一个包含其他LOD、材质和变换的真实Mesh草稿。
    .init_resource::<crate::ui::text_input::EditorTextInputGuard>() // 官方文本候选保护是生产输入系统的资源契约。
    .init_resource::<EditorAssetCatalog>()
    .init_resource::<MapEditorNumericScrubMouseGuard>() // 输入所有权契约。
    .init_resource::<MapEditorSidebarState>() // 目录定位不在本回归启动。
    .add_systems(
        Update,
        (
            handle_reference_buttons,
            invalidate_changed_assets,
            poll_reference,
        )
            .chain(),
    ); // 和生产相同先输入再失效/采用。
    let mut editor = MapEditorState::default(); // 不涉及地图写入。
    editor.open = true; // F12编辑会话启用。
    editor.selected_asset = Some("reference.gltf".to_owned()); // 两个真实网格候选。
    app.insert_resource(editor); // 生产输入从此冻结右侧选择。
    app.finish(); // GltfPlugin在finish阶段登记实际加载器。
    app.cleanup(); // 不省略插件生命周期。
    app
}

/// 以真实帧推进资产IO；截止时间避免损坏回归挂住测试进程。
fn until(app: &mut App, done: impl Fn(&VegetationPresetEditorState) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10); // 测试超时，不改变生产等待策略。
    loop {
        app.update(); // AssetServer真实轮询和生产字段采用。
        let state = app.world().resource::<VegetationPresetEditorState>(); // 检查当前完整阶段。
        if done(state) {
            return;
        } // 期望状态已经到达。
        assert!(
            Instant::now() < deadline,
            "实际GLTF加载未完成：{}",
            state.status
        ); // 失败携带诊断而不是永久等候。
        std::thread::sleep(Duration::from_millis(1)); // 仅测试让出CPU，不在生产帧线程等待资源。
    }
}

#[test] // 多网格文件必须等待显式选择，然后只替换指定网格字段。
fn real_gltf_waits_for_a_choice_then_applies_the_chosen_mesh_only() {
    let mut app = app(); // 实际测试GLTF有Mesh0和Mesh1。
    let (owner, original) = {
        let state = app.world().resource::<VegetationPresetEditorState>(); // 当前活动Mesh草稿。
        (
            state.active_preset_tab_id().unwrap(),
            state.active_draft().unwrap().descriptor.clone(),
        ) // 保存全部字段供精确比较。
    };
    let field = PresetField::MeshPart {
        lod: 0,
        part: 0,
        field: MeshPartField::MeshPath,
    }; // 只允许修改这条引用。
    press(&mut app, owner, AssetReferenceAction::UseSelected(field)); // 真正开始异步根加载。
    until(&mut app, |state| {
        state
            .asset_reference
            .as_ref()
            .is_some_and(|request| matches!(request.phase, ReferencePhase::Choosing))
    }); // 不是虚构Loaded状态。
    let ticket = {
        let state = app.world().resource::<VegetationPresetEditorState>(); // 全部候选已完整采用到临时面板。
        assert_eq!(state.active_draft().unwrap().descriptor, original); // 作者选择前原值完全不动。
        let request = state.asset_reference.as_ref().unwrap(); // 两个真实primitive。
        assert_eq!(
            request.choices,
            vec![
                SubAssetChoice::Mesh {
                    mesh: 0,
                    primitive: 0
                },
                SubAssetChoice::Mesh {
                    mesh: 1,
                    primitive: 0
                }
            ]
        ); // 路径来源于实际加载器。
        request.ticket // 选择必须携带当前请求票据。
    };
    press(
        &mut app,
        owner,
        AssetReferenceAction::Choose { ticket, index: 1 },
    ); // 明确选择第二网格，不能偷偷回退第一项。
    until(&mut app, |state| state.asset_reference.is_none()); // 经过最终Mesh类型验证才结束。
    let mut expected = original; // 所有其他参数保留原样。
    let VegetationPresetRuntime::Mesh { lods, .. } = &mut expected.runtime else {
        panic!()
    }; // 真实强类型后端。
    lods[0].parts[0].mesh_path = "reference.gltf#Mesh1/Primitive0".to_owned(); // 唯一授权修改。
    assert_eq!(
        app.world()
            .resource::<VegetationPresetEditorState>()
            .active_draft()
            .unwrap()
            .descriptor,
        expected
    ); // 材质、半径、变换、距离、作者来源及其他LOD完整保留。
}

#[test] // 备注中的文字绝不能被GLTF资源操作覆盖。
fn gltf_actions_cannot_assign_to_notes() {
    let mut app = app(); // 仍使用包含两个网格的模型。
    let (owner, expected) = {
        let state = app.world().resource::<VegetationPresetEditorState>(); // 当前正式草稿。
        (
            state.active_preset_tab_id().unwrap(),
            state.active_draft().unwrap().descriptor.clone(),
        ) // 只允许source字段变化。
    };
    press(
        &mut app,
        owner,
        AssetReferenceAction::UseSelected(PresetField::Notes), // 故意向非资产字段发送使用选中。
    ); // 正式输入系统必须拒绝，不启动GLTF枚举。
    until(&mut app, |state| state.asset_reference.is_none()); // 真实根与所有依赖加载完成。
    assert_eq!(
        app.world()
            .resource::<VegetationPresetEditorState>()
            .active_draft()
            .unwrap()
            .descriptor,
        expected
    ); // 不覆盖任何运行时字段。
}

#[test] // 后缀合法但磁盘缺失的文件会给出加载错误，不能把坏路径写入草稿。
fn missing_real_gltf_keeps_the_original_field() {
    let mut app = app(); // 官方GLTF加载器负责报告IO失败。
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .selected_asset = Some("missing.gltf".to_owned()); // 模拟浏览后文件被外部删除。
    let (owner, original) = {
        let state = app.world().resource::<VegetationPresetEditorState>(); // 保存输入前完整值。
        (
            state.active_preset_tab_id().unwrap(),
            state.active_draft().unwrap().descriptor.clone(),
        ) // 不是仅检查某个字符串。
    };
    press(
        &mut app,
        owner,
        AssetReferenceAction::UseSelected(mesh_reference_field(true)),
    ); // 初步后缀检查通过，真正IO应失败。
    until(&mut app, |state| state.asset_reference.is_none()); // 不依赖超时来掩盖LoadState::Failed。
    let state = app.world().resource::<VegetationPresetEditorState>(); // 最终只显示诊断。
    assert_eq!(state.active_draft().unwrap().descriptor, original); // 全部参数保留。
    assert!(state.status.contains("失败")); // 明确告知失败而不是声称已设置。
}
