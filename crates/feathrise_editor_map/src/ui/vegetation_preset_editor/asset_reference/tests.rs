//! 引用编辑回归：目标身份、单字段写入、真实子资产编号、类型检查和预算。

use super::super::fields::field_value; // 测试不用复制字段映射规则。
use super::super::types::*;
use super::enumeration::{WORK_PER_FRAME, collect_choices}; // 真实有界枚举函数。
use super::helpers::{apply_value, choice_path, freeze_target, reference_kind, target_is_current}; // 唯一生产赋值边界。
use super::interaction::prepare_source; // 真实输入前类型门。
use bevy::gltf::{Gltf, GltfMesh, GltfPrimitive}; // 构造真实Bevy元数据而非自定义GLB替身。
use bevy::prelude::*; // 测试仅创建内存Assets，不启动窗口/GPU。
use feathrise_map_content::{VegetationPresetCatalogAsset, VegetationPresetRuntime}; // 使用实际随包预设数据作为基线。 // 内部会话和精确字段身份。

/// 来源字段移除后，引用回归必须使用实际可编辑的LOD网格，而不是隐藏记录。
pub(super) const fn mesh_reference_field(mesh: bool) -> PresetField {
    // 两种后端保留各自精确字段身份。
    if mesh {
        PresetField::MeshPart {
            lod: 0,
            part: 0,
            field: MeshPartField::MeshPath,
        }
    } else {
        PresetField::BladeLod {
            lod: 0,
            field: BladeLodField::MeshPath,
        }
    } // 测试当前第零档而不依赖已移除来源动作。
}

/// 从正式随包目录创建一个内存草稿，测试不写作者文件。
pub(super) fn state(mesh: bool) -> VegetationPresetEditorState {
    let catalog: VegetationPresetCatalogAsset = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/vegetation_presets.vegetation_presets.json"
    )))
    .unwrap(); // 真实目录协议。
    let descriptor = catalog
        .presets
        .into_iter()
        .find(|preset| matches!(preset.runtime, VegetationPresetRuntime::Mesh { .. }) == mesh)
        .unwrap(); // 分别覆盖Blade与Mesh后端。
    let mut state = VegetationPresetEditorState::default(); // 独立会话，不访问运行时注册表。
    state.open_draft(PresetDraft::new(PresetDraftMode::Create, descriptor)); // 和生产打开标签使用同一身份生成。
    state
}

/// 为枚举夹具冻结一个合法字段请求，不要求AssetServer伪造加载状态。
pub(super) fn request(
    state: &VegetationPresetEditorState,
    field: PresetField,
) -> AssetReferenceRequest {
    AssetReferenceRequest {
        ticket: 1, // 固定测试请求身份。
        target: freeze_target(state, state.active_preset_tab_id().unwrap(), field).unwrap(), // 使用生产目标冻结规则。
        kind: reference_kind(field).unwrap(), // 由真实字段决定类型。
        source_path: "models/test.glb".to_owned(), // 仅生成路径，不读取这个文件。
        gltf: None,                           // 枚举直接读取下方内存根。
        image: None,                          // 这些夹具验证GLTF候选。
        phase: ReferencePhase::Collecting,    // 不绕过最终赋值的类型检查。
        cursor: ReferenceCursor::default(),   // 从第一项开始计算预算。
        choices: Vec::new(),                  // 不预填候选。
        page: 0,                              // 初始可见组。
        started: std::time::Instant::now(),   // 保持生产请求字段完整。
    }
}

/// 真实Gltf根的最小空数据，只有本用例需要的网格/材质会单独填入。
fn gltf() -> Gltf {
    Gltf {
        scenes: Vec::new(),          // 不实例化场景。
        named_scenes: default(),     // 无命名场景。
        meshes: Vec::new(),          // 测试显式添加网格句柄。
        named_meshes: default(),     // 不靠名称猜测编号。
        materials: Vec::new(),       // 测试显式添加材质句柄。
        named_materials: default(),  // 材质名称不影响路径。
        nodes: Vec::new(),           // 纯网格夹具没有节点变换。
        named_nodes: default(),      // 无名称查找。
        skins: Vec::new(),           // 测试不涉及蒙皮。
        named_skins: default(),      // 无蒙皮索引。
        default_scene: None,         // 不需要Scene0。
        animations: Vec::new(),      // 没有动画依赖。
        named_animations: default(), // 无动画名称表。
        source: None,                // 不克隆解析器整份JSON。
    }
}

#[test] // 选择只作用于明确字段，保留完整LOD和作者来源。
fn mesh_reference_changes_only_the_selected_field() {
    let mut state = state(true); // 包含多LOD与多部件的正式模型预设。
    let field = PresetField::MeshPart {
        lod: 0,
        part: 0,
        field: MeshPartField::MeshPath,
    }; // 单个部件网格。
    let target = freeze_target(&state, state.active_preset_tab_id().unwrap(), field).unwrap(); // 精确冻结目标。
    let original = state.active_draft().unwrap().descriptor.clone(); // 只在测试中保存比较快照。
    apply_value(&mut state, &target, "models/changed.glb#Mesh7/Primitive3").unwrap(); // 生产单字段采用。
    let mut expected = original; // 期望只改变一条路径。
    let VegetationPresetRuntime::Mesh { lods, .. } = &mut expected.runtime else {
        panic!()
    }; // 使用真实数据后端。
    lods[0].parts[0].mesh_path = "models/changed.glb#Mesh7/Primitive3".to_owned(); // 唯一被授权修改的值。
    assert_eq!(state.active_draft().unwrap().descriptor, expected); // 半径、距离、材质、变换、来源及其他LOD均保持原样。
}

#[test] // 自由备注不是资产，不能被定位或使用选中误解释为路径。
fn notes_are_not_asset_references() {
    // 来源只在内部保留，不再提供资产编辑动作。
    let state = state(false); // 真实Blade四档基线。
    assert!(reference_kind(PresetField::Notes).is_none()); // 备注不会创建同行资产按钮。
    assert!(
        freeze_target(
            &state,
            state.active_preset_tab_id().unwrap(),
            PresetField::Notes
        )
        .is_err()
    ); // 注入旧式动作同样失败关闭。
    assert!(prepare_source(PresetField::Notes, Some("model.glb")).is_err()); // 使用选中也不能把备注当模型引用。
}

#[test] // 加载期间切标签、换LOD、改部件或改原值都不能让旧请求串写。
fn stale_target_rejects_tab_lod_part_and_value_changes() {
    let mut state = state(true); // 当前第一个模型部件。
    let field = PresetField::MeshPart {
        lod: 0,
        part: 0,
        field: MeshPartField::MaterialPath,
    }; // 具体材质字段。
    let target = freeze_target(&state, state.active_preset_tab_id().unwrap(), field).unwrap(); // 原始快照身份。
    state.active_tab = PresetWorkspaceTab::Map; // 切换到地图。
    assert!(!target_is_current(&state, &target)); // 不允许向旧预设写入。
    state.active_tab = PresetWorkspaceTab::Vegetation(target.tab_id); // 回到原标签。
    state.active_draft_mut().unwrap().selected_lod = 1; // 切换LOD。
    assert!(!target_is_current(&state, &target)); // 不能把第0档选择交给第1档。
    state.active_draft_mut().unwrap().selected_lod = 0; // 重新选原LOD。
    let VegetationPresetRuntime::Mesh { lods, .. } =
        &mut state.active_draft_mut().unwrap().descriptor.runtime
    else {
        panic!()
    }; // 改变下标对应的稳定部件身份。
    lods[0].parts[0].part_id += 100; // 模拟外部结构换代。
    assert!(!target_is_current(&state, &target)); // 同一下标也不能冒充原部件。
    let target = freeze_target(&state, target.tab_id, field).unwrap(); // 重新冻结新身份。
    let VegetationPresetRuntime::Mesh { lods, .. } =
        &mut state.active_draft_mut().unwrap().descriptor.runtime
    else {
        panic!()
    }; // 模拟晚到前已有新路径。
    lods[0].parts[0].material_path = "newer.glb#Material2/std".to_owned(); // 后来的作者值优先。
    assert!(apply_value(&mut state, &target, "old.glb#Material0/std").is_err()); // 迟到结果必须零写入。
    assert_eq!(
        field_value(&state.active_draft().unwrap().descriptor, field).unwrap(),
        "newer.glb#Material2/std"
    ); // 保留后来值。
}

#[test] // 图片和模型不互相赋值；没有选择也不能清空原字段。
fn reference_input_validates_selection_type_and_safe_path() {
    let mesh = PresetField::BladeLod {
        lod: 0,
        field: BladeLodField::MeshPath,
    }; // 明确网格字段。
    assert!(prepare_source(mesh, Some("textures/grass.png")).is_err()); // 图片不进入Mesh。
    assert!(prepare_source(PresetField::BladeBaseColorPath, Some("models/tree.glb")).is_err()); // GLB不进入二维纹理。
    assert!(prepare_source(mesh, None).is_err()); // 不存在“默认第一个文件”。
    assert!(prepare_source(mesh, Some("../tree.glb")).is_err()); // 不能越出assets根。
    assert_eq!(
        prepare_source(mesh, Some("models\\tree.glb#Mesh4/Primitive2"))
            .unwrap()
            .1,
        "models/tree.glb"
    ); // 子资产正确映射所属文件。
    assert!(reference_kind(PresetField::Label).is_none()); // 名称依然是普通输入框。
}

#[test] // 单个GLB多网格候选使用真实编号，预算耗尽时不发布半套列表。
fn gltf_enumeration_is_bounded_and_preserves_real_primitive_indices() {
    let state = state(true); // 普通Mesh字段不导入节点变换。
    let mut request = request(
        &state,
        PresetField::MeshPart {
            lod: 0,
            part: 0,
            field: MeshPartField::MeshPath,
        },
    ); // 精确字段目标。
    let mut meshes = Assets::<GltfMesh>::default(); // 内存元数据容器。
    let count = WORK_PER_FRAME + 3; // 确保至少跨两帧。
    let primitives = (0..count)
        .map(|index| GltfPrimitive {
            index: index + 2,                  // 刻意不从0开始，暴露猜测编号错误。
            parent_mesh_index: 7,              // 数组位置0不等于实际GLTF网格编号。
            name: format!("Primitive{index}"), // 名称仅作为元数据。
            mesh: Handle::default(),           // 枚举不接触实际顶点数据。
            material: None,                    // 当前只测网格候选。
            extras: None,                      // 无扩展。
            material_extras: None,             // 无材质扩展。
        })
        .collect(); // 测试夹具可以同步构建固定数量数据。
    let handle = meshes.add(GltfMesh {
        index: 7,
        name: "TestMesh".to_owned(),
        primitives,
        extras: None,
    }); // 真实Bevy GltfMesh。
    let mut gltf = gltf(); // 最小根。
    gltf.meshes.push(handle); // 只有一个网格，但包含超过一帧预算的primitive。
    assert!(!collect_choices(&mut request, &gltf, &meshes, &Assets::default(), false).unwrap()); // 首帧不能假装完整完成。
    assert_eq!(request.choices.len(), WORK_PER_FRAME); // 一帧严格只枚举64项。
    assert!(collect_choices(&mut request, &gltf, &meshes, &Assets::default(), false).unwrap()); // 下一帧完整收尾。
    assert_eq!(request.choices.len(), count); // 不因预算丢弃候选。
    assert_eq!(
        choice_path("test.glb", request.choices[0]),
        "test.glb#Mesh7/Primitive2"
    ); // 不默认Mesh0/Primitive0。
}

#[test] // 材质候选包括未被场景使用的声明项，不把GLTF包装写进StandardMaterial字段。
fn material_choices_use_std_labels_and_real_default_presence() {
    let state = state(true); // 当前模型材质字段。
    let mut request = request(
        &state,
        PresetField::MeshPart {
            lod: 0,
            part: 0,
            field: MeshPartField::MaterialPath,
        },
    ); // 真实目标身份。
    let mut gltf = gltf(); // 不创建Scene，也不依赖网格使用材质。
    gltf.materials = vec![Handle::default(), Handle::default()]; // 两个声明材质。
    assert!(
        collect_choices(
            &mut request,
            &gltf,
            &Assets::default(),
            &Assets::default(),
            false
        )
        .unwrap()
    ); // 普通材质一次完成。
    assert_eq!(
        request.choices,
        vec![SubAssetChoice::Material(0), SubAssetChoice::Material(1)]
    ); // 不伪造默认材质。
    assert_eq!(
        choice_path("test.glb", request.choices[1]),
        "test.glb#Material1/std"
    ); // 使用Bevy0.19真实标准材质标签。
    request.cursor = ReferenceCursor::default(); // 独立模拟真实默认材质存在的加载结果。
    request.choices.clear(); // 测试数据重置。
    let mut meshes = Assets::<GltfMesh>::default(); // 默认材质必须同时被真实primitive使用。
    gltf.meshes.push(meshes.add(GltfMesh {
        index: 0,                      // 根中的真实第一网格。
        name: "NoMaterial".to_owned(), // 名称不影响默认材质语义。
        primitives: vec![GltfPrimitive {
            index: 0,                      // 该网格第一primitive。
            parent_mesh_index: 0,          // 真实所属Mesh。
            name: "Primitive0".to_owned(), // 普通元数据名称。
            mesh: Handle::default(),       // 此测试只枚举元数据。
            material: None,                // 显式省略材质才允许默认项出现。
            extras: None,                  // 无扩展。
            material_extras: None,         // 无额外材质数据。
        }],
        extras: None, // 无网格扩展。
    }));
    collect_choices(&mut request, &gltf, &meshes, &Assets::default(), true).unwrap(); // 显式存在默认材质。
    assert_eq!(
        request.choices.last(),
        Some(&SubAssetChoice::DefaultMaterial)
    ); // 默认候选只追加一次。
}

#[test] // 请求存在时冻结草稿编辑，取消以后解除，不能在选择过程中删除目标LOD。
fn pending_reference_participates_in_the_existing_busy_guard() {
    let mut state = state(false); // 正常编辑状态。
    assert!(!state.busy()); // 初始可修改。
    state.asset_reference = Some(request(&state, mesh_reference_field(false))); // 登记当前Blade网格请求，不再使用已移除来源字段。
    assert!(state.busy()); // 结构、保存和输入复用同一个保护门。
    state.asset_reference = None; // 取消仅丢弃临时POD候选。
    assert!(!state.busy()); // 原编辑能力恢复。
}

mod gltf_loading;
mod interaction; // 真正按钮Query、定位不提交和旧票据保护。
mod loading; // 关闭、超时、热更和Image维度失败都保持原值。 // 实际GLTF加载器端到端验证网格候选、最终采用和失败保留。
mod scroll; // 长表单中的选择可见性及结束回位。
