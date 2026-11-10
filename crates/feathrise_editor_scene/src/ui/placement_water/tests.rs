//! Blender整网格赋水作者动作的无窗口回归测试。

use super::*; // 直接验证父模块纯修改函数，不启动AssetServer或RenderApp。
use feathrise_scene::DynamicWaterMeshOverrideStatus; // 极值Presentation夹具显式构造运行时状态。

fn selection() -> EditorGltfMeshSelection {
    EditorGltfMeshSelection {
        placement_id: "placement-water".to_owned(),
        node_path: vec!["河流对象".to_owned()],
        mesh_name: "河流水面".to_owned(),
        mesh_index: Some(3),
        valid: true,
        orphaned_water_binding: false,
    }
}

fn record() -> ScenePlacementRecord {
    ScenePlacementRecord {
        mesh_lod: None,
        terrain_binding: None,
        editor: Default::default(),
        terrain_patch: None,
        id: "placement-water".to_owned(),
        asset_path: "gltf/river.glb".to_owned(),
        asset_target: Default::default(), // 网格水属性只面向完整Scene0。
        building_preset_id: None,         // 网格水属性夹具不引用建筑预设。
        courtyard: None,                  // 独立摆放记录不属于任何院落副本。
        translation: [0.0, 0.0, 0.0],
        height_mode: feathrise_scene_content::ScenePlacementHeightMode::FixedHeight, // 网格水UI测试不访问地形。
        terrain_offset: 0.0, // 固定高度不携带离地偏移。
        yaw_degrees: 0.0,
        scale: [1.0, 1.0, 1.0],
        water_surfaces: Vec::new(),
    }
}

fn scene_id() -> feathrise_scene_content::SceneId {
    feathrise_scene_content::SceneId::parse("MAP-placement-water-ui").expect("测试场景ID必须合法") // 全部预览根共享当前作者会话场景身份。
}

#[test]
fn whole_mesh_water_actions_keep_one_stable_binding_and_clear_it() {
    let selection = selection();
    let mut record = record();
    assert!(
        apply_action_to_record(&mut record, &selection, PlacementWaterAction::SetSloped,)
            .expect("显式命名网格可赋水")
    );
    assert_eq!(record.water_surfaces.len(), 1); // 一个Blender对象网格只有一位水体语义所有者。
    assert_eq!(record.water_surfaces[0].kind, ScenePlacementWaterKind::Sloped);
    assert_eq!(
        record.water_surfaces[0].flow_speed,
        DEFAULT_MESH_WATER_FLOW_SPEED
    );

    assert!(
        apply_action_to_record(&mut record, &selection, PlacementWaterAction::Clear,)
            .expect("取消赋水合法")
    );
    assert!(record.water_surfaces.is_empty()); // 清除后运行时可完整恢复原GLB材质。
}

#[test]
fn generated_or_pathless_mesh_identity_is_rejected() {
    let mut invalid = selection();
    invalid.valid = false; // 浏览器仍显示该诊断行，但作者动作必须失败关闭。
    let mut record = record();
    assert!(
        apply_action_to_record(&mut record, &invalid, PlacementWaterAction::SetHorizontal,)
            .is_err()
    );
    assert!(record.water_surfaces.is_empty()); // 非稳定引用不能污染正式摆放JSON。

    let valid = selection();
    assert!(
        apply_action_to_record(&mut record, &valid, PlacementWaterAction::SetHorizontal)
            .expect("先建立一条合法绑定")
    );
    invalid.node_path = valid.node_path;
    invalid.mesh_name = valid.mesh_name;
    invalid.mesh_index = None; // GLB加载诊断索引暂不可恢复时仍应允许移除已有绑定。
    invalid.orphaned_water_binding = true; // 当前Ready GLB目标消失时属性区仍必须保留清除通道。
    assert!(
        apply_action_to_record(&mut record, &invalid, PlacementWaterAction::Clear)
            .expect("清除只依赖已经保存的稳定身份")
    );
    assert!(record.water_surfaces.is_empty());
}

#[test] // 第257项必须在按钮动作当帧失败并恢复，而不是等保存零写入门才告诉作者。
fn whole_scene_binding_assignment_accepts_the_old_budget_plus_one() {
    let mut placements = ScenePlacementsAsset {
        schema_version: feathrise_scene_content::SCENE_SCHEMA_VERSION,
        scene_id: feathrise_scene_content::SceneId::parse("MAP-water-budget").expect("测试场景ID必须合法"),
        placements: (0..feathrise_scene_content::MAX_SCENE_PLACEMENT_WATER_SURFACES_PER_SCENE)
            .map(|index| {
                let mut item = record();
                item.id = format!("water-{index}");
                item.water_surfaces.push(ScenePlacementWaterSurface {
                    mesh_index: index as u32,
                    node_path: vec![format!("水面对象_{index}")],
                    mesh_name: format!("水面网格_{index}"),
                    kind: ScenePlacementWaterKind::Horizontal,
                    flow_speed: 1.0,
                    height_offset: 0.0,
                });
                item
            })
            .chain(std::iter::once(record())) // 末项是本次作者将尝试赋水的空普通实例。
            .collect(),
    };
    assert!(placements.validate_water_surfaces().is_ok()); // 精确256项是合法基线。
    apply_action_to_placements(
        &mut placements, feathrise_scene_content::MAX_SCENE_PLACEMENT_WATER_SURFACES_PER_SCENE,
        feathrise_scene_content::MAX_SCENE_PLACEMENT_WATER_SURFACES_PER_SCENE,
        &selection(), PlacementWaterAction::SetHorizontal,
    ).expect("第257项合法绑定应立即进入作者事务");
    assert_eq!(placements.placements.last().unwrap().water_surfaces.len(), 1);
    placements.validate_water_surfaces().unwrap();
} // 完成全图绑定预算UI回归。

#[derive(Resource)]
struct PreviewRecord(ScenePlacementRecord); // ECS回归中向预览系统提供一份小型权威覆盖。

#[derive(Resource)]
struct PreviewSceneId(SceneId); // 定向ECS夹具显式模拟生产入口已从当前草稿取得的场景信用。

fn apply_preview_once(
    mut commands: Commands,
    session: Res<EditorAuthoringSession>,
    root_index: Res<PlacementInstanceIndex>,
    roots: Query<(Entity, &ScenePlacementInstance)>,
    current_scene_id: Res<PreviewSceneId>,
    record: Res<PreviewRecord>,
) {
    preview_placement_overrides_for_scene(
        &mut commands,
        &session,
        &root_index,
        &roots,
        &current_scene_id.0,
        &record.0,
    ); // 不构造完整EditorSceneDraft，但仍覆盖与生产相同的scene_id门和回退路径。
}

#[test]
fn unsaved_preview_only_rewrites_an_active_matching_root() {
    let selection = selection();
    let mut changed = record();
    apply_action_to_record(
        &mut changed,
        &selection,
        PlacementWaterAction::SetHorizontal,
    )
    .expect("测试网格赋水合法");
    let mut app = App::new();
    app.insert_resource(EditorAuthoringSession::default())
        .insert_resource(PreviewRecord(changed))
        .insert_resource(PreviewSceneId(scene_id()))
        .init_resource::<PlacementInstanceIndex>()
        .add_observer(register_placement_instance_root)
        .add_observer(unregister_placement_instance_root);
    let adopted_record = record(); // 活动根冻结与稍后写回目标相同的完整结构证据。
    let adopted = app
        .world_mut()
        .spawn((
            ScenePlacementInstance::new(&scene_id(), &adopted_record),
            DynamicWaterMeshOverrides::default(),
        ))
        .id();
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .placement_previews
        .insert(adopted_record.id.clone(), adopted); // 精确预览身份仍需通过夹具中的当前场景复核。
    let mut unrelated_record = record(); // 无关根仅改变稳定ID，其他结构保持合法。
    unrelated_record.id = "other-placement".to_owned(); // 当前选择不得命中此根。
    let unrelated = app
        .world_mut()
        .spawn((
            ScenePlacementInstance::new(&scene_id(), &unrelated_record),
            DynamicWaterMeshOverrides::default(),
        ))
        .id();
    app.add_systems(Update, apply_preview_once);

    app.update();

    assert_eq!(
        app.world()
            .get::<DynamicWaterMeshOverrides>(adopted)
            .expect("已采用根保留覆盖组件")
            .surfaces()
            .len(),
        1,
    ); // 已采用正式根仍获得F12即时预览。
    assert!(
        app.world()
            .get::<ScenePlacementWaterEvidence>(adopted)
            .is_some()
    ); // 首项非空覆盖与保存Query证据marker同批出现。
    assert!(
        app.world()
            .get::<DynamicWaterMeshOverrides>(unrelated)
            .expect("无关活动根保留原覆盖")
            .surfaces()
            .is_empty()
    ); // 其他placement不能被当前选中网格的草稿覆盖改写。
    assert!(
        app.world()
            .get::<ScenePlacementWaterEvidence>(unrelated)
            .is_none()
    ); // 普通无水根不进入保存等待帧的受限Query。

    app.world_mut()
        .resource_mut::<PreviewRecord>()
        .0
        .water_surfaces
        .clear(); // 模拟取消最后一项整网格水绑定。
    app.update(); // 同一预览写回系统应同步移除覆盖证据marker。
    assert!(
        app.world()
            .get::<ScenePlacementWaterEvidence>(adopted)
            .is_none()
    ); // 清空覆盖后根重新成为不会被保存门枚举的普通placement。
}

#[test]
fn clipmap_mesh_water_preview_uses_the_same_placement_root_without_glb_terrain_or_static_water() {
    use feathrise_scene_content::SceneTerrainKind;
    use std::sync::Arc;
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    draft.manifest.terrain.kind = SceneTerrainKind::SparseClipmap;
    draft.manifest.terrain.glb_path.clear();
    draft.terrain_mesh = None;
    draft.heights = None;
    draft.water = None;
    let map = draft.scene_id.clone();
    let original = record();
    let mut app = App::new();
    app.insert_resource(EditorAuthoringSession {
        draft: Some(draft),
        ..default()
    })
    .insert_resource(PreviewRecord(original.clone()))
    .init_resource::<PlacementInstanceIndex>()
    .add_observer(register_placement_instance_root)
    .add_observer(unregister_placement_instance_root)
    .add_systems(
        Update,
        |mut commands: Commands,
         session: Res<EditorAuthoringSession>,
         index: Res<PlacementInstanceIndex>,
         roots: Query<(Entity, &ScenePlacementInstance)>,
         record: Res<PreviewRecord>| {
            preview_placement_overrides(&mut commands, &session, &index, &roots, &record.0);
        },
    );
    let root = app
        .world_mut()
        .spawn((
            ScenePlacementInstance::new(&map, &original),
            DynamicWaterMeshOverrides::default(),
        ))
        .id();
    for (action, expected) in [
        (
            PlacementWaterAction::SetHorizontal,
            Some(ScenePlacementWaterKind::Horizontal),
        ),
        (
            PlacementWaterAction::SetSloped,
            Some(ScenePlacementWaterKind::Sloped),
        ),
        (
            PlacementWaterAction::SetWaterfall,
            Some(ScenePlacementWaterKind::Waterfall),
        ),
        (PlacementWaterAction::Clear, None),
    ] {
        apply_action_to_record(
            &mut app.world_mut().resource_mut::<PreviewRecord>().0,
            &selection(),
            action,
        )
        .unwrap();
        app.update();
        let overrides = app.world().get::<DynamicWaterMeshOverrides>(root).unwrap();
        assert_eq!(
            overrides.surfaces().first().map(|surface| surface.kind),
            expected
        );
        assert_eq!(
            app.world().get::<ScenePlacementWaterEvidence>(root).is_some(),
            expected.is_some()
        );
    } // 三类赋水和清除均通过真正的草稿场景身份入口，无地形GLB或静水文件依赖。
}

#[test]
fn foreign_exact_preview_falls_back_to_current_scene_root_without_cross_scene_write() {
    let selection = selection(); // 两个场景故意共享同一完整placement稳定ID。
    let mut changed = record(); // 当前图草稿的唯一网格覆盖将用于识别串写。
    apply_action_to_record(&mut changed, &selection, PlacementWaterAction::SetSloped)
        .expect("测试网格赋水合法");
    let current_scene_id = scene_id(); // 回退索引只应在这张图中查找同ID活动根。
    let foreign_scene_id =
        SceneId::parse("MAP-placement-water-ui-foreign").expect("另一测试场景ID必须合法"); // 精确预览表故意残留此图实体。
    let mut app = App::new();
    app.insert_resource(EditorAuthoringSession::default())
        .insert_resource(PreviewRecord(changed))
        .insert_resource(PreviewSceneId(current_scene_id.clone()))
        .init_resource::<PlacementInstanceIndex>()
        .add_observer(register_placement_instance_root)
        .add_observer(unregister_placement_instance_root);
    let target_record = record(); // 正式根和异图预览根都冻结同一placement结构身份。
    let current_formal_root = app
        .world_mut()
        .spawn((
            ScenePlacementInstance::new(&current_scene_id, &target_record),
            DynamicWaterMeshOverrides::default(),
        ))
        .id(); // 精确候选被拒绝后，必须通过当前图索引回退到此根。
    let foreign_exact_preview = app
        .world_mut()
        .spawn((
            ScenePlacementInstance::new(&foreign_scene_id, &target_record),
            DynamicWaterMeshOverrides::default(),
        ))
        .id(); // 该根active且ID一致，只有scene_id不匹配。
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .placement_previews
        .insert(target_record.id.clone(), foreign_exact_preview); // 故意注入跨场景精确预览残留引用。
    app.add_systems(Update, apply_preview_once);

    app.update(); // 精确候选失败后仍应在同帧生成当前图正式根延迟写回。

    assert_eq!(
        app.world()
            .get::<DynamicWaterMeshOverrides>(current_formal_root)
            .expect("当前图正式根应保留覆盖组件")
            .surfaces(),
        app.world()
            .resource::<PreviewRecord>()
            .0
            .water_surfaces
            .as_slice(),
    ); // 当前图正式根通过scene_id + placement_id索引定向回退并取得完整覆盖。
    assert!(
        app.world()
            .get::<ScenePlacementWaterEvidence>(current_formal_root)
            .is_some()
    ); // 回退写入同时发布非空保存证据。
    assert!(
        app.world()
            .get::<DynamicWaterMeshOverrides>(foreign_exact_preview)
            .expect("异图精确预览根应保留覆盖组件")
            .surfaces()
            .is_empty()
    ); // 即使active且placement_id完全相同，他图精确实体也不得串写。
    assert!(
        app.world()
            .get::<ScenePlacementWaterEvidence>(foreign_exact_preview)
            .is_none()
    ); // 被拒绝的异图根也不得获取保存证据marker。
}

#[test]
fn hidden_or_retired_root_identity_is_never_author_writable() {
    let current_scene = scene_id(); // 当前草稿场景是精确预览根的唯一跨场景信用。
    let other_scene = feathrise_scene_content::SceneId::parse("MAP-placement-water-ui-other")
        .expect("另一测试场景ID必须合法"); // 同名活动根也不能跨场景接收作者补丁。
    assert!(placement_root_is_author_writable(
        &current_scene,
        "placement-water",
        true,
        &current_scene,
        "placement-water",
    )); // 已采用正式根和编辑器预览根都由活动门明确放行。
    assert!(!placement_root_is_author_writable(
        &current_scene,
        "placement-water",
        false,
        &current_scene,
        "placement-water",
    )); // 隐藏候选或退役旧根即使ID相同也不能接收未保存值。
    assert!(!placement_root_is_author_writable(
        &current_scene,
        "other-placement",
        true,
        &current_scene,
        "placement-water",
    )); // 活动门不能绕过稳定placement身份匹配。
    assert!(!placement_root_is_author_writable(
        &other_scene,
        "placement-water",
        true,
        &current_scene,
        "placement-water",
    )); // 精确预览表若残留另一场景同名活动根，仍必须失败关闭而不是串图补丁。
}

#[test]
fn stable_presentation_frame_does_not_rebuild_text_or_scan_runtime_roots() {
    let mut app = App::new();
    let mut editor = SceneEditorState::default();
    editor.open = true;
    let mut browser = EditorGltfBrowserState::default();
    browser.selection = Some(selection());
    app.insert_resource(editor)
        .insert_resource(browser)
        .init_resource::<EditorAuthoringSession>()
        .init_resource::<PlacementWaterSelectionCache>()
        .init_resource::<PlacementWaterRuntimeSelectionCache>()
        .init_resource::<ScenePlacementRuntimeStatus>();
    let text = app
        .world_mut()
        .spawn((Text::new("尚未同步"), SceneEditorPlacementWaterText))
        .id();
    let runtime_record = record(); // 极值夹具复用同一个只读作者来源，只关心状态查询是否被稳定帧唤醒。
    let runtime_scene_id = scene_id(); // 循环外只解析一次稳定场景身份，测试自身不制造8192次字符串分配。
    for _ in 0..8_192 {
        app.world_mut().spawn((
            ScenePlacementInstance::new(&runtime_scene_id, &runtime_record),
            DynamicWaterMeshOverrideStatus::Ready,
        )); // 最坏正式规模让误入文字系统的第二帧必须走完整状态汇总并覆盖哨兵。
    }
    app.add_systems(
        Update,
        (
            refresh_placement_water_selection_cache,
            sync_placement_water_text,
        )
            .chain(),
    );

    app.update(); // 首帧消费新增资源与状态，建立空水绑定属性文字。
    **app
        .world_mut()
        .entity_mut(text)
        .get_mut::<Text>()
        .expect("测试文字存在") = "稳定帧哨兵".to_owned();
    app.world_mut()
        .resource_mut::<EditorGltfBrowserState>()
        .status = "Scene0候选加载进度变化".to_owned(); // 浏览器Resource产生非语义Changed。
    app.world_mut().resource_mut::<SceneEditorState>().status = "其他作者底栏状态变化".to_owned(); // editor Resource产生非语义Changed。
    app.world_mut()
        .resource_mut::<ScenePlacementRuntimeStatus>()
        .detail = "隐藏候选进度4096/8192".to_owned(); // 正式状态Resource每帧进度Changed也不能唤醒。
    app.update(); // 选择、活动正式修订与作者绑定都未变化。

    assert_eq!(
        app.world()
            .entity(text)
            .get::<Text>()
            .expect("测试文字仍存在")
            .as_str(),
        "稳定帧哨兵",
    ); // 若缓存伪Changed或旧session guard仍存在，系统会扫描8192根并重写该文字。
}
