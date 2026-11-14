//! 仅测试特性开放的中性场景夹具与受限观测口；不把旧游戏测试或整个作者模块编入依赖。

use super::{EditorAuthoringSession, EditorSceneDraft};
use feathrise_scene_content::{
    SceneId, SceneManifestAsset, ScenePlacementsAsset, ScenePortalsAsset, SceneTerrainDescriptor,
    SceneTerrainKind,
};
use feathrise_terrain::{TerrainMeshInput, build_terrain_editing_index};
use std::sync::Arc;

/// 其他作者域测试共用的11×11动态GLB草稿夹具。
pub fn test_heightfield_draft(heights: Arc<Vec<u16>>) -> EditorSceneDraft {
    assert_eq!(heights.len(), 121);
    let scene_id = SceneId("EDGE-TEST".to_owned());
    let positions = (0..11)
        .flat_map(|z| {
            let heights = Arc::clone(&heights);
            (0..11).map(move |x| [x as f32, heights[z * 11 + x] as f32, z as f32])
        })
        .collect::<Vec<_>>();
    let mut indices = Vec::with_capacity(600);
    for z in 0..10u32 {
        for x in 0..10u32 {
            let nw = z * 11 + x;
            indices.extend_from_slice(&[nw, nw + 11, nw + 1, nw + 1, nw + 11, nw + 12]);
        }
    }
    let mesh = TerrainMeshInput { positions, indices };
    let editing_index = build_terrain_editing_index(&mesh).expect("测试GLB必须建立拓扑索引");
    EditorSceneDraft {
        scene_id: scene_id.clone(),
        domain: None,
        preserved_asset_paths: Default::default(),
        folder: "scenes/EDGE-TEST".to_owned(),
        placement_glb_hash: None,
        water_asset_path: "scenes/EDGE-TEST/water.scene_water.json".to_owned(),
        terrain_ground: None,
        terrain_documents: Default::default(),
        manifest: SceneManifestAsset {
            vegetation_paint_groups: Default::default(),
            terrains: None,
            subscenes: Default::default(),
            schema_version: feathrise_scene_content::SCENE_SCHEMA_VERSION,
            id: scene_id.clone(),
            display_name: "边界笔刷测试".to_owned(),
            notes: String::new(), // 测试旧场景缺省备注。
            runtime_scene: "data_driven".to_owned(),
            world_min: [0.0, 0.0],
            world_max: [10.0, 10.0],
            default_spawn: [5.0, 0.0, 5.0],
            environment: Default::default(),
            puddles: None, // 新图及旧图夹具默认没有积水，首次有效落笔才创建。
            terrain: SceneTerrainDescriptor {
                resolved_stitches: Default::default(),
                collision_lods: Vec::new(), // GLB测试不消费稀疏碰撞配置。
                display_lods: None,         // GLB测试不消费SparseClipmap显示LOD。
                kind: SceneTerrainKind::Gltf,
                collision_mode: Default::default(), // 高度场测试不启用Clipmap物理。
                player_collision_preload_radius_meters:
                    feathrise_scene_content::DEFAULT_PLAYER_COLLISION_PRELOAD_RADIUS_METERS, // 旧路线不读取。
                role_collision_preload_radius_meters: None, // 旧路线不登记稀疏粗级半径。
                player_collision_chunk_cells: None,
                role_collision_chunk_cells: None,
                player_collision_span_meters: None, // 测试旧路线不读取联动设置。
                role_collision_span_meters: None,   // 测试旧路线不读取联动设置。
                glb_path: "scenes/EDGE-TEST/terrain/terrain.glb".to_owned(),
                height_path: None,
                height_texture_path: None,
                resolution: [11, 11],
                base_height: 0.0,
                height_scale: u16::MAX as f32,
            },
            asset_roots: Vec::new(),
        },
        placements: ScenePlacementsAsset {
            scene_id: scene_id.clone(),
            ..Default::default()
        },
        placement_water_surface_count: 0,
        portals: ScenePortalsAsset {
            scene_id,
            ..Default::default()
        },
        terrain_mesh: Some(Arc::new(mesh)),
        terrain_editing_index: Some(Arc::new(editing_index)),
        terrain_source_sha256: Some([7; 32]),
        terrain_overwrite_authorized_sha256: None,
        heights: Some(heights),
        surface: None,
        puddle_mask: None,    // 尚未加载或创建独立R8作者源。
        puddles_dirty: false, // 未编辑积水不增加额外保存责任。
        surface_weights: None,
        surface_indices: None,
        vegetation: None,
        vegetation_densities: Default::default(),
        vegetation_nonzero_layers: Default::default(),
        vegetation_density_stats: Default::default(),
        vegetation_author_densities: Default::default(),
        vegetation_conflicts: Default::default(),
        water: None,
        water_masks: Default::default(),
        boundaries: None,
        terrain_dirty: false,
        surface_dirty: false,
        vegetation_dirty_layers: Default::default(),
        vegetation_author_dirty_layers: Default::default(),
        water_dirty_layers: Default::default(),
        environment_dirty: false,
        water_catalog_registration_required: false,
        water_registration_source_hash: None, // GLB夹具没有旧Clipmap水域补登记责任。
        vegetation_registration_source_hash: None, // GLB夹具没有旧Clipmap补登记责任。
        boundary_catalog_registration_required: false,
        boundaries_dirty: false,
    }
}

/// 当前高度地形的已采用查询夹具；作者后端为Clipmap，承托仍由真实三角面查询。
/// 只验证通用交互的测试无需写稀疏页，不能把中性GLB夹具当成已就绪地形。
pub fn test_clipmap_draft(heights: Arc<Vec<u16>>, revision: u64) -> EditorSceneDraft {
    let mut draft = test_heightfield_draft(heights);
    draft.manifest.terrain.kind = SceneTerrainKind::SparseClipmap;
    draft.manifest.terrain.height_path = Some("scenes/EDGE-TEST/terrain/height.sparse.json".into());
    draft.manifest.terrains = Some(draft.manifest.terrain_objects_for_edit());
    refresh_test_clipmap_ground(&mut draft, revision);
    draft
}

/// 模拟同代来源到达或缺失时同时更换真实查询；仅改旧heights不能撤销已采用快照。
pub fn refresh_test_clipmap_ground(draft: &mut EditorSceneDraft, revision: u64) {
    use bevy::prelude::Vec3;
    let Some(heights) = draft.heights.as_ref() else {
        draft.terrain_ground = None;
        return;
    };
    let source = draft.terrain_manifest();
    let [width, height] = source.terrain.resolution;
    assert_eq!(heights.len(), width as usize * height as usize);
    let mut mesh = TerrainMeshInput {
        positions: Vec::with_capacity(heights.len()),
        indices: Vec::with_capacity(((width - 1) * (height - 1) * 6) as usize),
    };
    for z in 0..height {
        for x in 0..width {
            let y = source.terrain.base_height
                + heights[(z * width + x) as usize] as f32 / u16::MAX as f32
                    * source.terrain.height_scale;
            mesh.positions.push([
                source.world_min[0] + (source.world_max[0] - source.world_min[0]) * x as f32 / (width - 1) as f32,
                y,
                source.world_min[1] + (source.world_max[1] - source.world_min[1]) * z as f32 / (height - 1) as f32,
            ]);
            if x + 1 < width && z + 1 < height {
                let nw = z * width + x;
                mesh.indices.extend_from_slice(&[nw, nw + width, nw + 1, nw + 1, nw + width, nw + width + 1]);
            }
        }
    }
    let collider = avian3d::prelude::Collider::trimesh(
        mesh.positions.iter().copied().map(Vec3::from_array).collect(),
        mesh.indices.chunks_exact(3).map(|triangle| [triangle[0], triangle[1], triangle[2]]).collect(),
    );
    let objects = draft.manifest.terrain_objects_for_edit();
    assert_eq!(objects.len(), 1, "规则高度夹具只支持一个独立地形");
    let mut ground = super::terrain_ground::TerrainGroundSnapshot::test_meshes(
        draft.scene_id.clone(), &objects, collider,
    );
    Arc::get_mut(&mut ground).unwrap().revision = revision;
    draft.terrain_ground = Some(ground);
    draft.terrain_editing_index = Some(Arc::new(build_terrain_editing_index(&mesh).unwrap()));
    draft.terrain_mesh = Some(Arc::new(mesh));
}

/// 只轮询正式贴地系统；后台批次完整返回前不预先标记完成或执行无关输入系统。
pub(crate) fn drive_test_follow_to_completion(
    app: &mut bevy::prelude::App,
) -> super::PlacementGroundingReadiness {
    use bevy::ecs::system::RunSystemOnce;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        app.world_mut().run_system_once(super::reconcile_following_placements).unwrap();
        let session = app.world().resource::<EditorAuthoringSession>();
        let draft = session.draft.as_ref().unwrap();
        let readiness = app.world().resource::<super::PlacementTerrainFollowState>().readiness_for(
            &draft.scene_id, session.terrain_revision, session.placement_structure_revision,
            draft.placements.placements.len(),
        );
        if !matches!(readiness, super::PlacementGroundingReadiness::Waiting { .. }) {
            return readiness;
        }
        assert!(std::time::Instant::now() < deadline, "真实贴地worker未完整归还测试槽");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// 跨 crate 回归只设置历史前缀，不能直接访问或替换普通编辑器画布容器。
pub fn set_scoped_history(draft: &mut EditorSceneDraft, count: usize) {
    draft.terrain_documents.scoped_through = count;
}
pub fn scoped_history(draft: &EditorSceneDraft) -> usize {
    draft.terrain_documents.scoped_through
}

/// 运行停放前同一个文档转场门，供实际外部消费者检查当帧输入事实的发布顺序。
pub fn scene_document_gate_busy(world: &mut bevy::prelude::World) -> bool {
    use bevy::ecs::system::RunSystemOnce;
    world
        .run_system_once(
            |transition: crate::ui::scene_documents::SceneDocumentTransition| transition.busy(),
        )
        .unwrap()
}

/// 安装原主地形身份，使私有作者回归仍覆盖真实的画布历史包装。
pub fn activate_primary_terrain(
    draft: &mut EditorSceneDraft,
) -> feathrise_scene_content::TerrainId {
    let object = draft.manifest.terrain_objects_for_edit()[0].clone();
    let id = object.id.clone();
    draft.manifest.terrains = Some(Arc::from([object.clone()]));
    draft.terrain_documents.active = Some(super::terrain_documents::TerrainDocumentIdentity {
        manifest: object.source_manifest(&draft.manifest),
        definition: Arc::new(object),
        folder: draft.folder.clone(),
    });
    id
}
pub fn scope_terrain_history(session: &mut EditorAuthoringSession) {
    super::terrain_documents::scope_history(session);
}

/// 使用真正的保存门和场景入口；测试接口不公开它们内部的全部任务和环境资源。
pub fn needs_save_before_scene_switch(
    session: &EditorAuthoringSession,
    editor: &crate::resources::SceneEditorState,
) -> bool {
    session.needs_save_before_scene_switch(editor, &super::GlobalEnvironmentAuthoringState::default())
}
pub fn request_selected_scene_entry(
    editor: &mut crate::resources::SceneEditorState,
    catalog: &feathrise_scene_content::SceneCatalogAsset,
    session: &EditorAuthoringSession,
) {
    crate::ui::rows::request_selected_scene_entry(
        editor,
        &crate::resources::SceneDeletionTask::default(),
        Some(catalog),
        session,
        &super::GlobalEnvironmentAuthoringState::default(),
        false,
    );
}

/// 与插件使用同一退役系统，私有UI回归不再维持一份角色专属后台析构器。
pub fn poll_authoring_retirement(retirement: bevy::prelude::ResMut<super::AuthoringRetirement>) {
    super::retirement::poll(retirement);
}

/// 只排入真实碰撞查询，不制造命中值；冻结与普通点击相同的场景、相机和作者身份。
pub fn queue_ground_click(
    world: &mut bevy::prelude::World,
    origin: bevy::prelude::Vec3,
    direction: bevy::prelude::Vec3,
) {
    use super::terrain_ground::{
        CursorGroundSnapshot, CursorIdentity, CursorRay, TerrainGroundCursor,
    };
    use bevy::prelude::*;
    let session = world.resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().expect("点击夹具须先装载场景");
    let editor = world.resource::<crate::resources::SceneEditorState>();
    let identity = CursorIdentity {
        scene_id: draft.scene_id.clone(),
        revision: session.revision,
        tool: editor.tool,
        asset: None,
        terrain_revision: session.terrain_revision,
        ground_snapshot: draft
            .terrain_ground
            .as_ref()
            .map(CursorGroundSnapshot::capture),
        asset_target: None,
        portal: None,
        camera: Mat4::IDENTITY,
        camera_entity: Entity::PLACEHOLDER,
        projection: Mat4::IDENTITY,
        view: editor.view,
        orthographic: false,
        extent: 40.0_f32.to_bits(),
        size: UVec2::new(600, 400),
        visibility: Arc::as_ptr(&editor.terrain_hidden) as usize,
        relocation: None,
        translation_snap: None,
    };
    let ray = CursorRay {
        origin,
        direction,
        epoch: 1,
    };
    world.init_resource::<TerrainGroundCursor>();
    let mut ground = world.resource_mut::<TerrainGroundCursor>();
    ground.ray = Some(ray);
    ground.identity = Some(identity.clone());
    ground.capture_click(identity, ray, None);
}
pub fn poll_ground_cursor(world: &mut bevy::prelude::World) {
    super::terrain_ground::cursor_for_tests(world);
}
pub fn ground_click_ready(world: &bevy::prelude::World) -> bool {
    world
        .resource::<super::terrain_ground::TerrainGroundCursor>()
        .click_terrain
        .is_some()
}

/// 私有作者回归安装真实导航消费者，仍由普通场景读取统一鼠标所有权事实。
pub fn install_viewport_navigation(app: &mut bevy::prelude::App) {
    use bevy::prelude::*;
    app.init_resource::<super::WaterNetworkEditState>()
        .add_systems(
            Update,
            crate::viewport::control_editor_viewport
                .after(crate::game_authoring::scene_interaction::SceneInteractionAdoption),
        );
}

pub fn set_editor_view(
    editor: &mut crate::resources::SceneEditorState,
    viewport: Option<&mut crate::resources::SceneEditorViewportState>,
    view: crate::types::EditorView,
) {
    crate::viewport::set_editor_view(editor, viewport, view);
}

/// 只提供真实数值拖动门的推进；不开放内部blocked字段或复制状态机。
pub fn advance_numeric_mouse_guard(
    guard: &mut crate::resources::SceneEditorNumericScrubMouseGuard,
    active: bool,
    pressed: bool,
    released: bool,
) {
    guard.advance(active, pressed, released);
}

/// 无窗口回归只构造正式视口元数据；纹理与驻留场景身份仍留在普通编辑器内部。
pub fn viewport_state(
    scene_center: bevy::prelude::Vec3,
    scene_half_extents: bevy::prelude::Vec3,
) -> crate::resources::SceneEditorViewportState {
    use bevy::prelude::*;
    crate::resources::SceneEditorViewportState {
        _image: Handle::default(),
        camera: Entity::PLACEHOLDER,
        center: Vec3::ZERO,
        scene_center,
        scene_half_extents,
        extent: 50.0,
        yaw: 0.0,
        pitch: -0.6,
        framed_scene_id: None,
        render_size: UVec2::new(600, 400),
    }
}
