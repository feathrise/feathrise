//! 地表柔边覆盖的栅格预算、短点击和生命周期回归。
use super::*; // 验证真实实现而非模拟渲染算法。
use std::sync::Arc; // 复用小型作者草稿夹具。

fn session() -> EditorAuthoringSession {
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121])); // 只构造内存草稿。
    draft.manifest.world_min = [0.0, 0.0]; // 一米对应一个覆盖像素便于判断。
    draft.manifest.world_max = [255.0, 255.0]; // 覆盖整个固定256²平面。
    draft.surface_dirty = true; // 真实待PBR采用的作者修改。
    EditorAuthoringSession {
        draft: Some(draft),
        surface_revision: 4,
        ..default()
    } // 固定可识别的修订。
}

#[test]
fn surface_trail_single_click_has_soft_edges_and_bounded_overlap() {
    let session = session(); // 固定场景范围。
    let mut trail = SurfaceStrokeTrail::default(); // 无图像或渲染上下文也可受理单击。
    let point = Vec3::new(128.0, 0.0, 128.0); // 精确像素中心。
    trail.record(&session, point, point, 10.0, false, 0.04); // 真实连续帧常见弱强度。
    assert!(trail.has_segments()); // 同帧收口也保留即时反馈。
    let center = trail.pixels[128 * DIM + 128]; // 中央柔和但可辨识。
    let edge = trail.pixels[128 * DIM + 137]; // 边缘接近透明。
    assert!(center[3] >= 160 && center[3] <= 224); // 弱刷也必须清晰可见，同时保留柔边和部分底图。
    assert!(edge[3] > 0 && edge[3] < center[3]); // 没有硬边框或空心圆圈。
    for _ in 0..200 {
        trail.record(&session, point, point, 10.0, false, 0.04);
    } // 长时间按住或重复pending。
    assert_eq!(trail.pixels[128 * DIM + 128], center); // 不会因为帧数累加而无限增亮。
    assert!(trail.touched.len() <= PIXELS && trail.dirty.len() <= PIXELS); // 所有列表都受固定像素数约束。
}

#[test]
fn surface_trail_long_stroke_keeps_start_and_tiny_click_is_visible() {
    let session = session(); // 完整256米场景。
    let mut trail = SurfaceStrokeTrail::default(); // 固定大小栅格。
    for x in 0..240 {
        // 超过原先128段上限的长笔。
        trail.record(
            &session,
            Vec3::new(x as f32, 0.0, 30.0),
            Vec3::new(x as f32 + 1.0, 0.0, 30.0),
            2.0,
            false,
            0.1,
        ); // 连续轨迹不截断尾部。
    }
    assert!(trail.pixels[30 * DIM][3] > 0); // 画到后面时起点仍在。
    assert!(trail.pixels[30 * DIM + 240][3] > 0); // 终点也有连续填色。
    trail.clear(); // 下一笔使用同一栅格。
    let point = Vec3::new(128.5, 0.0, 128.5); // 恰好落在四个像素中心之间。
    trail.record(&session, point, point, 0.05, false, 0.1); // 半径远小于一个覆盖像素。
    assert!(trail.has_segments()); // 细单击不会落进采样缝隙。
    assert_eq!(trail.pixels.len(), PIXELS); // 长笔和细笔都不增加画布尺寸。
}

#[test]
fn surface_trail_strength_mode_and_external_revision_are_preserved() {
    let mut session = session(); // 当前作者来源。
    let mut trail = SurfaceStrokeTrail::default(); // 同一覆盖画布。
    let point = Vec3::new(128.0, 0.0, 128.0); // 使用相同落点比较强度。
    trail.record(&session, point, point, 2.0, false, 0.0); // 零强度不是受理修改。
    assert!(!trail.has_segments()); // 不制造虚假的笔刷结果。
    trail.record(&session, point, point, 2.0, false, 0.01); // 弱笔。
    let weak = trail.pixels[128 * DIM + 128]; // 冻结弱笔透明度。
    trail.record(&session, point, point, 2.0, false, 1.0); // 强笔。
    assert!(trail.pixels[128 * DIM + 128][3] > weak[3]); // 实际冻结强度影响覆盖。
    trail.record(&session, point, point, 2.0, true, 0.1); // 擦除改为暖土色。
    assert_ne!(trail.pixels[128 * DIM + 128][..3], weak[..3]); // 擦除与绘制可区分但无刺眼红色。
    session.revision += 1; // 外部撤销或图层结构编辑。
    trail.reconcile(&session); // 实际生命周期检查。
    assert!(!trail.has_segments()); // 旧作者覆盖已清除。
    trail.record(&session, point, point, 2.0, false, 0.1); // 新版本重新受理。
    session.terrain_revision += 1; // 外部改变地形。
    trail.reconcile(&session); // 旧地形投影不能残留。
    assert!(!trail.has_segments()); // 新高度场需要新覆盖来源。
}

fn app() -> App {
    let mut session = session(); // 固定作者草稿。
    session.draft.as_mut().unwrap().manifest.terrain.kind = feathrise_scene_content::SceneTerrainKind::SparseClipmap;
    let scene_id = session.draft.as_ref().unwrap().scene_id.clone(); // 所有预览用同一稳定场景。
    let mut app = App::new(); // 不初始化GPU或窗口。
    app.insert_resource(SceneEditorState {
        open: true,
        tool: crate::types::EditorTool::Surface,
        selected_scene_id: Some(scene_id.clone()),
        ..default()
    }); // 当前F12可见。
    app.init_resource::<TerrainPreviewState>(); // Clipmap反馈不依赖旧高度网格预览。
    app.insert_resource(session); // 当前完整作者会话。
    app.init_resource::<SurfaceBrushStroke>(); // 默认已松笔，等待PBR。
    app.init_resource::<SurfacePreviewState>(); // 当前还没有PBR回执。
    app.init_resource::<SurfaceStrokeTrail>(); // 唯一覆盖资源。
    app.init_resource::<DynamicClipmapSurfaceOverlay>(); // 当前高度地形直接发布正式覆盖通道。
    app.init_resource::<Assets<Image>>(); // CPU资产存储足以验证RGBA图。
    app.init_resource::<Assets<StandardMaterial>>(); // 不运行真正渲染。
    app.add_systems(Update, draw_surface_stroke_trail); // 使用正式显隐和采用逻辑。
    app
}

fn stamp(app: &mut App) {
    app.world_mut()
        .resource_scope(|world, mut trail: Mut<SurfaceStrokeTrail>| {
            let session = world.resource::<EditorAuthoringSession>(); // 同版本实际记录。
            let point = Vec3::new(128.0, 0.0, 128.0); // 固定落点。
            trail.record(session, point, point, 10.0, false, 0.1); // 单击先于显示系统受理。
        });
}

#[test]
fn transformed_trail_keeps_source_uv_world_radius_and_applies_object_pose_once() {
    use crate::authoring::terrain_documents::TerrainDocumentIdentity;
    let mut app = app();
    let (id, pose) = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
        object.id = TerrainId::parse("trail-a").unwrap();
        object.transform = feathrise_scene_content::TerrainObjectTransform {
            translation: [20.0, 6.0, -30.0],
            yaw_degrees: 37.0,
            scale: [2.0, 3.0, 0.5],
            ..Default::default()
        };
        let result = (object.id.clone(), object.transform);
        let source = object.source_manifest(&draft.manifest);
        draft.manifest.terrains = Some(Arc::from([object.clone()]));
        draft.terrain_documents.active = Some(TerrainDocumentIdentity {
            definition: Arc::new(object),
            manifest: source,
            folder: draft.folder.clone(),
        });
        result
    };
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .terrain_canvas_target = Some(id.clone());
    stamp(&mut app);
    app.update();
    let trail = app.world().resource::<SurfaceStrokeTrail>();
    assert!(trail.entity.is_none(), "Clipmap覆盖必须复用正式高度扩展，不能新增平面");
    assert!(
        trail.pixels[(128 + 18) * DIM + 128][3] > 0,
        "源Z缩小后世界圆刷应覆盖更远的源像素"
    );
    assert_eq!(
        trail.pixels[128 * DIM + 134][3],
        0,
        "源X放大后世界圆刷不能横向变宽"
    );
    assert_eq!(
        app.world().resource::<DynamicClipmapSurfaceOverlay>().current.as_ref().unwrap().source_to_world,
        pose.as_transform().to_matrix()
    );
    let next = {
        let session = app.world().resource::<EditorAuthoringSession>();
        let mut object = session
            .draft
            .as_ref()
            .unwrap()
            .terrain_documents
            .active
            .as_ref()
            .unwrap()
            .definition
            .as_ref()
            .clone();
        object.id = TerrainId::parse("trail-b").unwrap();
        Arc::new(object)
    };
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .draft
        .as_mut()
        .unwrap()
        .terrain_documents
        .active
        .as_mut()
        .unwrap()
        .definition = next;
    app.update();
    assert!(
        !app.world().resource::<SurfaceStrokeTrail>().has_segments(),
        "同图切对象时旧笔迹必须失效"
    );
}

#[test]
fn sparse_surface_trail_publishes_existing_rgba_without_legacy_mesh_and_rejects_other_canvas_receipts()
 {
    use crate::authoring::terrain_documents::TerrainDocumentIdentity;
    let mut app = app();
    app.init_resource::<DynamicClipmapSurfaceOverlay>();
    let id = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
        object.id = TerrainId::parse("sparse-trail").unwrap();
        object.terrain.kind = feathrise_scene_content::SceneTerrainKind::SparseClipmap;
        object.transform.translation = [20.0, 6.0, -30.0];
        object.transform.yaw_degrees = 37.0;
        object.transform.scale = [2.0, 3.0, 0.5];
        let id = object.id.clone();
        let manifest = object.source_manifest(&draft.manifest);
        draft.manifest.terrains = Some(Arc::from([object.clone()]));
        draft.terrain_documents.active = Some(TerrainDocumentIdentity {
            definition: Arc::new(object),
            manifest,
            folder: draft.folder.clone(),
        });
        id
    };
    {
        let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
        editor.tool = crate::types::EditorTool::Surface;
        editor.terrain_canvas_target = Some(id.clone());
    }
    *app.world_mut().resource_mut::<TerrainPreviewState>() = Default::default();
    stamp(&mut app);
    app.update();
    let trail = app.world().resource::<SurfaceStrokeTrail>();
    let image = trail
        .image
        .clone()
        .expect("没有旧GLB预览也须生成有界即时覆盖");
    let channel = app.world().resource::<DynamicClipmapSurfaceOverlay>();
    assert_eq!(channel.trail.as_ref().unwrap().image, image);
    let identity = channel.current.as_ref().unwrap().clone();
    assert_eq!(identity.terrain_id, Some(id.clone()));
    assert_eq!(channel.trail.as_ref().unwrap().identity, identity);
    assert_eq!(
        app.world_mut()
            .query::<&SurfaceStrokeOverlay>()
            .iter(app.world())
            .count(),
        0,
        "Clipmap反馈须交给真实高度材质，不能建水平StandardMaterial副本"
    );
    assert!(
        app.world()
            .resource::<Assets<Image>>()
            .get(&image)
            .unwrap()
            .data
            .as_ref()
            .unwrap()
            .chunks_exact(4)
            .any(|pixel| pixel[3] > 0)
    );

    {
        let mut preview = app.world_mut().resource_mut::<SurfacePreviewState>();
        preview.applied_scene_id = Some(identity.scene_id.clone());
        preview.applied_terrain_id = Some(TerrainId::parse("another-canvas").unwrap());
        preview.applied_revision = identity.surface_revision;
        preview.material = Some(Handle::default());
    }
    app.update();
    assert!(
        app.world().resource::<SurfaceStrokeTrail>().has_segments(),
        "同Map/同revision的另一对象PBR回执不能消费当前绿迹"
    );
    app.world_mut()
        .resource_mut::<SurfacePreviewState>()
        .applied_terrain_id = Some(id.clone());
    app.update();
    assert!(
        app.world()
            .resource::<DynamicClipmapSurfaceOverlay>()
            .trail
            .is_none()
    );
    assert!(!app.world().resource::<SurfaceStrokeTrail>().has_segments());

    app.world_mut()
        .resource_mut::<SurfacePreviewState>()
        .material = None;
    stamp(&mut app);
    app.update();
    assert!(
        app.world()
            .resource::<DynamicClipmapSurfaceOverlay>()
            .trail
            .is_some()
    );
    app.world_mut()
        .resource_mut::<SurfacePreviewState>()
        .failed_identity = Some((identity.scene_id, Some(id), identity.surface_revision, 0));
    app.update();
    assert!(
        app.world()
            .resource::<DynamicClipmapSurfaceOverlay>()
            .current
            .is_none()
    );
    assert!(
        app.world()
            .resource::<DynamicClipmapSurfaceOverlay>()
            .trail
            .is_none()
    );
    app.world_mut()
        .resource_mut::<SurfacePreviewState>()
        .failed_identity = None;
    stamp(&mut app);
    app.update();
    app.world_mut().resource_mut::<SceneEditorState>().open = false;
    app.update();
    assert!(
        app.world()
            .resource::<DynamicClipmapSurfaceOverlay>()
            .current
            .is_none()
    );
    assert!(
        app.world()
            .resource::<DynamicClipmapSurfaceOverlay>()
            .trail
            .is_none()
    );
}

#[test]
fn legacy_clipmap_primary_claim_keeps_root_feedback_but_explicit_primary_rejects_root_receipts() {
    let mut app = app();
    app.init_resource::<DynamicClipmapSurfaceOverlay>();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        draft.manifest.terrain.kind = feathrise_scene_content::SceneTerrainKind::SparseClipmap;
        crate::authoring::terrain_documents::ensure_primary_canvas(draft);
        assert!(draft.manifest.terrains.is_none());
        assert_eq!(
            draft
                .terrain_documents
                .active
                .as_ref()
                .unwrap()
                .definition
                .id,
            TerrainId::legacy()
        );
    }
    app.world_mut().resource_mut::<SceneEditorState>().tool = crate::types::EditorTool::Surface;
    *app.world_mut().resource_mut::<TerrainPreviewState>() = Default::default();
    stamp(&mut app);
    app.update();
    let channel = app.world().resource::<DynamicClipmapSurfaceOverlay>();
    let map = channel.current.as_ref().unwrap().scene_id.clone();
    assert!(
        channel.current.as_ref().unwrap().terrain_id.is_none(),
        "旧清单的primary作者认领不能改成尚不存在的实例目标"
    );
    assert!(
        channel.trail.is_some(),
        "没有旧GLB网格也须发布真实Clipmap反馈"
    );
    {
        let mut preview = app.world_mut().resource_mut::<SurfacePreviewState>();
        preview.applied_scene_id = Some(map.clone());
        preview.applied_terrain_id = None;
        preview.applied_revision = 4;
        preview.material = Some(Handle::default());
    }
    app.update();
    assert!(
        !app.world().resource::<SurfaceStrokeTrail>().has_segments(),
        "旧根PBR回执应能消费仅改寻址的primary绿迹"
    );
    app.world_mut()
        .resource_mut::<SurfacePreviewState>()
        .material = None;
    stamp(&mut app);
    app.update();
    app.world_mut()
        .resource_mut::<SurfacePreviewState>()
        .failed_identity = Some((map, None, 4, 0));
    app.update();
    assert!(
        app.world()
            .resource::<DynamicClipmapSurfaceOverlay>()
            .current
            .is_none(),
        "旧根失败回执也须撤回primary绿迹，不能继续冒充有效预览"
    );
    app.world_mut()
        .resource_mut::<SurfacePreviewState>()
        .failed_identity = None;
    stamp(&mut app);
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let object = draft
            .terrain_documents
            .active
            .as_ref()
            .unwrap()
            .definition
            .as_ref()
            .clone();
        draft.manifest.terrains = Some(Arc::from([object]));
        draft.manifest.terrain = feathrise_scene_content::SceneTerrainDescriptor::empty();
    }
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .terrain_canvas_target = Some(TerrainId::legacy());
    app.world_mut()
        .resource_mut::<SurfacePreviewState>()
        .material = Some(Handle::default());
    app.update();
    let channel = app.world().resource::<DynamicClipmapSurfaceOverlay>();
    assert_eq!(
        channel.current.as_ref().unwrap().terrain_id,
        Some(TerrainId::legacy()),
        "显式primary对象必须通过自己的正式实例，不能借root绘制"
    );
    assert!(channel.trail.is_some());
    assert!(
        app.world().resource::<SurfaceStrokeTrail>().has_segments(),
        "显式对象不得接受只带root身份的迟到PBR回执"
    );
}

#[test]
fn surface_trail_old_receipt_preserves_new_stroke_and_ready_reuses_assets() {
    let mut app = app(); // 当前目标revision4。
    stamp(&mut app); // 第一笔实际记录。
    app.update(); // 创建一份有界覆盖图，复用正式Clipmap的高度扩展。
    assert!(app.world().resource::<SurfaceStrokeTrail>().entity.is_none());
    let image = app
        .world()
        .resource::<SurfaceStrokeTrail>()
        .image
        .clone()
        .unwrap(); // 后续不换整图句柄。
    assert_eq!(app.world().resource::<DynamicClipmapSurfaceOverlay>().trail.as_ref().unwrap().image, image);
    let image_count = app.world().resource::<Assets<Image>>().len();
    let material_count = app.world().resource::<Assets<StandardMaterial>>().len();
    let scene_id = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .scene_id
        .clone(); // PBR采用必须同场景。
    {
        let mut preview = app.world_mut().resource_mut::<SurfacePreviewState>(); // 模拟迟到旧PBR。
        preview.applied_scene_id = Some(scene_id); // 同场景但修订落后。
        preview.material = Some(Handle::default()); // 完整PBR材质已有句柄。
        preview.applied_revision = 3; // 比当前笔划早。
    }
    app.update(); // 旧回执不能清新覆盖。
    assert!(app.world().resource::<SurfaceStrokeTrail>().has_segments()); // 最新笔反馈仍存在。
    app.world_mut()
        .resource_mut::<SurfacePreviewState>()
        .applied_revision = 4; // 当前PBR完整到齐。
    app.update(); // 收起辅助覆盖。
    assert!(!app.world().resource::<SurfaceStrokeTrail>().has_segments()); // 最新正式材质接管。
    assert!(app.world().resource::<DynamicClipmapSurfaceOverlay>().trail.is_none());
    stamp(&mut app); // 再次记录待呈现输入。
    app.world_mut()
        .resource_mut::<SurfacePreviewState>()
        .applied_revision = 3; // 保留待更新状态。
    app.update(); // 使用旧覆盖资产。
    assert_eq!(
        app.world().resource::<SurfaceStrokeTrail>().image.as_ref(),
        Some(&image)
    ); // 没有每帧新建Image。
    assert_eq!(app.world().resource::<Assets<Image>>().len(), image_count);
    assert_eq!(app.world().resource::<Assets<StandardMaterial>>().len(), material_count);
    app.world_mut().resource_mut::<SceneEditorState>().open = false; // 关闭F12。
    app.update(); // 立即撤回作者辅助色。
    assert!(app.world().resource::<DynamicClipmapSurfaceOverlay>().trail.is_none());
    assert!(app.world().resource::<DynamicClipmapSurfaceOverlay>().current.is_none());
}

#[test]
#[ignore = "手工测量固定256²满覆盖局部栅格的当前硬件CPU预算"]
fn surface_trail_full_canvas_budget() {
    let session = session(); // 固定256²上界而非场景分辨率。
    let mut trail = SurfaceStrokeTrail::default(); // 复用同一固定分配。
    let point = Vec3::new(128.0, 0.0, 128.0); // 超大笔覆盖整个辅助画布。
    trail.record(&session, point, point, 1000.0, false, 0.1); // 预热一次性分配不混入稳定帧。
    let mut times = Vec::with_capacity(200); // 只存在于显式性能测试中。
    for index in 0..200 {
        let start = std::time::Instant::now(); // 测量整个record和dirty去重路径。
        trail.record(
            std::hint::black_box(&session),
            point,
            point,
            1000.0,
            index % 2 == 0,
            0.1,
        ); // 每次切模式强制所有像素真实变化。
        std::hint::black_box(&trail); // 防止优化移除结果。
        times.push(start.elapsed().as_secs_f64() * 1000.0); // 毫秒CPU时间。
    }
    times.sort_by(f64::total_cmp); // 只在测试结束统计，不进入运行时。
    println!(
        "surface overlay 256² stamp CPU: mean {:.3} ms, p95 {:.3} ms, max {:.3} ms; image upload cap {} KiB/change frame",
        times.iter().sum::<f64>() / times.len() as f64,
        times[189],
        times[199],
        PIXELS * 4 / 1024
    ); // 不是GPU帧时证明。
}

#[test]
fn embedded_surface_trail_projects_original_pixels_resets_after_snapshot_and_reliably_clears() {
    use crate::standalone::embedded_surface_trail::{
        EmbeddedSurfaceTrailState, SurfaceTrailPreviewPlugin, invalidate,
    };
    use feathrise_scene_content::embedded_surface_trail::{
        SURFACE_TRAIL_BLOCK_BYTES, decode_surface_trail,
    };
    use std::time::{Duration, Instant};

    fn wait_ready(app: &mut App) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            app.update();
            let state = app.world().resource::<EmbeddedSurfaceTrailState>();
            assert!(state.error.is_none(), "{:?}", state.error);
            if state.ready.is_some() { return; }
            assert!(Instant::now() < deadline, "原地表辅助投影后台任务没有完成");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    let mut app = app();
    app.add_plugins(MinimalPlugins).add_plugins(SurfaceTrailPreviewPlugin);
    {
        let mut state = app.world_mut().resource_mut::<EmbeddedSurfaceTrailState>();
        state.enabled = true; state.session = 7; state.generation = 1;
    }
    app.world_mut().resource_mut::<SurfaceBrushStroke>().active = true;
    stamp(&mut app); // 使用原生产笔迹栅格，桥接层没有自己的画笔和颜色。
    wait_ready(&mut app);
    let first = app.world().resource::<EmbeddedSurfaceTrailState>().ready.clone().unwrap();
    let patch = decode_surface_trail(&first.bytes).unwrap();
    assert!(patch.visible && patch.reset);
    assert_eq!(patch.rgba.len(), SURFACE_TRAIL_BLOCK_BYTES);
    assert!(app.world().resource::<SurfaceBrushStroke>().active, "不能等松手才有辅助反馈");
    let trail = app.world().resource::<SurfaceStrokeTrail>();
    let actual = app.world().resource::<Assets<Image>>().get(trail.image.as_ref().unwrap()).unwrap();
    let offset = usize::from(patch.block) * SURFACE_TRAIL_BLOCK_BYTES;
    assert_eq!(patch.rgba, actual.data.as_ref().unwrap()[offset..offset + SURFACE_TRAIL_BLOCK_BYTES]);
    assert!(patch.rgba.chunks_exact(4).any(|pixel| pixel[3] > 0));

    // ACK只确认已发送块；整图采用使客体旧纹理退役后，同一来源必须重新建立基础图。
    app.world_mut().resource_mut::<EmbeddedSurfaceTrailState>().acknowledged_sequence = first.sequence;
    invalidate(app.world_mut());
    wait_ready(&mut app);
    let next = app.world().resource::<EmbeddedSurfaceTrailState>().ready.clone().unwrap();
    let fresh = decode_surface_trail(&next.bytes).unwrap();
    assert!(fresh.reset && fresh.source_epoch > patch.source_epoch);
    assert!(next.sequence > first.sequence);
    app.world_mut().resource_mut::<EmbeddedSurfaceTrailState>().acknowledged_sequence = first.sequence;
    app.update(); // 旧ACK不能确认新代首块。
    assert_eq!(app.world().resource::<EmbeddedSurfaceTrailState>().ready.as_ref().unwrap().sequence, next.sequence);

    app.world_mut().resource_mut::<SceneEditorState>().open = false;
    wait_ready(&mut app);
    let clear = app.world().resource::<EmbeddedSurfaceTrailState>().ready.clone().unwrap();
    let clear_patch = decode_surface_trail(&clear.bytes).unwrap();
    assert!(!clear_patch.visible && clear_patch.reset && clear_patch.rgba.is_empty());
    assert!(clear_patch.source_epoch > fresh.source_epoch);
    app.world_mut().resource_mut::<EmbeddedSurfaceTrailState>().acknowledged_sequence = clear.sequence;
    app.update();
    assert!(!app.world().resource::<EmbeddedSurfaceTrailState>().pending);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert!(session.undo.is_empty());
    assert_eq!(session.successful_save_sequence, 0);

    app.world_mut().resource_mut::<SceneEditorState>().open = true;
    stamp(&mut app);
    wait_ready(&mut app);
    app.world_mut().resource_mut::<SceneEditorState>().open = false;
    wait_ready(&mut app); // 旧图clear尚未ACK时，完整快照负责撤回客体图层。
    invalidate(app.world_mut());
    app.update();
    assert!(!app.world().resource::<EmbeddedSurfaceTrailState>().pending,
        "完整快照已清旧辅助，不能在空图保留旧clear永久等待");
}
