//! 积水作者完整生命周期回归：旧图默认、创建撤销、事务失败和重新加载。
use super::*; // 使用生产入口验证快照与IO。
use feathrise_map_content::MapPuddleDescriptor;
use std::sync::Arc; // 同一版本的指针身份需要精确验证。 // 唯一作者数据协议。
fn painted() -> PuddleSnapshot {
    // 小尺寸夹具覆盖完整像素而不增加测试成本。
    PuddleSnapshot {
        // 有效地图私有R8和KTX2配对。
        descriptor: Some(MapPuddleDescriptor {
            source_path: "maps/EDGE-TEST/puddles.potential.r8".into(),
            texture_path: "maps/EDGE-TEST/puddles.potential.ktx2".into(),
            resolution: [2, 2],
        }), // 使用生产路径规则。
        mask: Some(Arc::new(vec![0, 64, 128, 255])), // 零值与部分、满潜力共同覆盖。
    } // 返回完整版本。
} // 结束夹具。

fn puddle_brush_app(clipmap: bool) -> bevy::prelude::App {
    use bevy::prelude::*;
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<PuddleAuthoringState>()
        .init_resource::<crate::resources::MapEditorNumericScrubMouseGuard>()
        .init_resource::<crate::resources::MapBrowserState>()
        .init_resource::<crate::resources::MapBrowserDragState>()
        .init_resource::<crate::authoring::TerrainExternalChangeState>()
        .init_resource::<crate::viewport::VegetationGroundCursor>()
        .init_resource::<feathrise_scene::DynamicClipmapGroundSurface>();
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    if clipmap {
        draft.manifest.terrain.kind = feathrise_map_content::MapTerrainKind::SparseClipmap;
    }
    app.insert_resource(crate::resources::MapEditorState {
        open: true,
        tool: crate::types::EditorTool::Puddles,
        selected_map_id: Some(draft.map_id.clone()),
        brush_cursor_world: Some(Vec3::new(5., 40., 5.)),
        brush_radius: 2.,
        brush_strength: 1.,
        ..Default::default()
    })
    .insert_resource(crate::authoring::EditorAuthoringSession {
        draft: Some(draft),
        ..Default::default()
    })
    .add_systems(Update, apply_puddle_brush);
    app.world_mut().spawn((
        crate::components::MapEditorViewport,
        bevy::ui::RelativeCursorPosition {
            cursor_over: true,
            normalized: Some(Vec2::splat(0.5)),
        },
    )); // 故意带有旧通用点，不提供正式Clipmap快照。
    app
}

fn finish_puddle_test(app: &mut bevy::prelude::App) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while app.world().resource::<PuddleAuthoringState>().busy() {
        app.update();
        assert!(
            std::time::Instant::now() < deadline,
            "小型积水任务必须在五秒内收口"
        );
        std::thread::sleep(std::time::Duration::from_millis(1)); // 仅测试线程等待真实后台池。
    }
}

#[test]
fn locked_terrain_rejects_a_puddle_result_that_finishes_after_the_click() {
    use bevy::prelude::*;
    let mut app = puddle_brush_app(false);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    assert!(app.world().resource::<PuddleAuthoringState>().busy());
    app.world_mut()
        .resource_mut::<crate::resources::MapEditorState>()
        .terrain_locked
        .insert(feathrise_map_content::TerrainId::legacy());
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    finish_puddle_test(&mut app);
    let session = app
        .world()
        .resource::<crate::authoring::EditorAuthoringSession>();
    assert!(session.draft.as_ref().unwrap().puddle_mask.is_none());
    assert!(session.undo.is_empty());
}

#[test]
fn transformed_puddle_pixels_follow_the_world_circle_and_preserve_source_addresses() {
    use crate::authoring::terrain_documents::{PaintMetricSpace, TerrainDocumentIdentity};
    use bevy::prelude::*;
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
    object.transform = feathrise_map_content::TerrainObjectTransform {
        translation: [20.0, 6.0, -30.0],
        yaw_degrees: 37.0,
        scale: [2.0, 3.0, 0.5],
        ..Default::default()
    };
    let matrix = object.transform.as_transform().to_matrix();
    draft.terrain_documents.active = Some(TerrainDocumentIdentity {
        definition: Arc::new(object),
        manifest: draft.manifest.clone(),
        folder: draft.folder.clone(),
    });
    let metric = PaintMetricSpace::from_draft(&draft);
    let center = Vec3::new(5.0, 0.0, 5.0);
    let point = metric.point(center);
    let mut pixels = vec![0; 121];
    assert!(helpers::paint(
        &mut pixels,
        [11, 11],
        metric.bounds(draft.terrain_manifest().world_min),
        metric.bounds(draft.terrain_manifest().world_max),
        point,
        types::Sample {
            world: point,
            radius: 2.0,
            amount: 1.0,
            erase: false
        }
    ));
    for z in 0..11 {
        for x in 0..11 {
            let distance = matrix
                .transform_point3(Vec3::new(x as f32, 0.0, z as f32))
                .distance(matrix.transform_point3(center));
            let expected = ((1.0 - distance / 2.0).clamp(0.0, 1.0) * 255.0).round() as u8;
            assert!(
                pixels[z * 11 + x].abs_diff(expected) <= 1,
                "世界圆刷积水 ({x},{z})"
            );
        }
    }
}

#[test]
fn clipmap_puddle_controls_and_observation_views_cannot_arm_a_waiting_press() {
    use crate::types::EditorView;
    use crate::viewport::modes::ViewportModeState;
    use bevy::prelude::*;
    for (view, controls) in [
        (EditorView::Perspective, true),
        (EditorView::Front, false),
        (EditorView::Side, false),
    ] {
        let mut app = puddle_brush_app(true);
        app.init_resource::<ViewportModeState>();
        app.world_mut()
            .resource_mut::<ViewportModeState>()
            .pointer_over_controls = controls;
        app.world_mut()
            .resource_mut::<crate::resources::MapEditorState>()
            .view = view;
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        assert!(
            app.world()
                .resource::<PuddleAuthoringState>()
                .pending_ground
                .is_none()
        );
        assert!(!app.world().resource::<PuddleAuthoringState>().busy());
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        app.world_mut()
            .resource_mut::<ViewportModeState>()
            .pointer_over_controls = false;
        app.world_mut()
            .resource_mut::<crate::resources::MapEditorState>()
            .view = EditorView::Top;
        app.update(); // 进入有效视图后仍不能继承控件后的按压意图。
        assert!(
            app.world()
                .resource::<PuddleAuthoringState>()
                .pending_ground
                .is_none()
        );
        {
            let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
            mouse.release(MouseButton::Left);
            mouse.clear();
            mouse.press(MouseButton::Left);
        }
        app.update();
        assert!(
            app.world()
                .resource::<PuddleAuthoringState>()
                .pending_ground
                .is_some()
        );
    }
}

#[test]
fn clipmap_puddles_wait_without_creating_a_canvas_and_release_cancels_start() {
    use bevy::prelude::*;
    let mut app = puddle_brush_app(true);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    assert!(
        app.world()
            .resource::<PuddleAuthoringState>()
            .pending_ground
            .is_some()
    );
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    for _ in 0..3 {
        app.update();
    }
    let state = app.world().resource::<PuddleAuthoringState>();
    assert!(!state.busy() && state.task.is_none() && state.pending.is_none());
    let session = app
        .world()
        .resource::<crate::authoring::EditorAuthoringSession>();
    assert!(session.draft.as_ref().unwrap().manifest.puddles.is_none());
    assert!(session.draft.as_ref().unwrap().puddle_mask.is_none());
    assert!(session.undo.is_empty());
    assert_eq!(session.revision, 0);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.update();
    assert!(
        app.world()
            .resource::<PuddleAuthoringState>()
            .pending_ground
            .is_none()
    );
    assert!(
        app.world()
            .resource::<crate::authoring::EditorAuthoringSession>()
            .undo
            .is_empty()
    );
}

#[test]
fn clipmap_puddle_waiting_press_is_cancelled_by_tool_map_height_or_control() {
    use bevy::prelude::*;
    for change in 0..4 {
        let mut app = puddle_brush_app(true);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        match change {
            0 => {
                app.world_mut()
                    .resource_mut::<crate::resources::MapEditorState>()
                    .tool = crate::types::EditorTool::Inspect
            }
            1 => {
                app.world_mut()
                    .resource_mut::<crate::authoring::EditorAuthoringSession>()
                    .draft
                    .as_mut()
                    .unwrap()
                    .map_id = feathrise_map_content::MapId("other".into())
            }
            2 => {
                app.world_mut()
                    .resource_mut::<crate::authoring::EditorAuthoringSession>()
                    .terrain_revision += 1
            }
            _ => app
                .world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::ControlLeft),
        }
        app.update();
        let state = app.world().resource::<PuddleAuthoringState>();
        assert!(state.pending_ground.is_none() && !state.busy());
        assert!(
            app.world()
                .resource::<crate::authoring::EditorAuthoringSession>()
                .draft
                .as_ref()
                .unwrap()
                .manifest
                .puddles
                .is_none()
        );
    }
}

#[test]
fn accepted_puddle_sample_finishes_after_release_as_one_creation_undo() {
    use bevy::prelude::*;
    let mut app = puddle_brush_app(false);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update(); // 真实后台任务在有效GLB点上受理首次创建。
    assert!(
        app.world()
            .resource::<PuddleAuthoringState>()
            .task
            .is_some()
    );
    {
        let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        mouse.release(MouseButton::Left);
        mouse.clear();
    }
    finish_puddle_test(&mut app);
    let session = app
        .world()
        .resource::<crate::authoring::EditorAuthoringSession>();
    assert_eq!(session.undo.len(), 1);
    let crate::authoring::EditorCommand::PuddlesChanged { before, after } =
        session.undo.last().unwrap()
    else {
        panic!("整笔应生成一条积水历史");
    };
    assert!(before.descriptor.is_none() && before.mask.is_none()); // 撤销首笔可以恢复未创建。
    assert_eq!(after.descriptor.as_ref().unwrap().resolution, [1024, 1024]);
    assert!(after.mask.as_ref().unwrap()[512 * 1024 + 512] > 0);
    assert!(after.is_current(session.draft.as_ref().unwrap())); // 历史after正是实际完整采用的版本。
}

fn install_completed_puddle(
    app: &mut bevy::prelude::App,
    ground_content: Option<[u64; 7]>,
) -> Arc<Vec<u8>> {
    let (map_id, revision, source) = {
        let mut session = app
            .world_mut()
            .resource_mut::<crate::authoring::EditorAuthoringSession>();
        let revision = session.revision;
        let draft = session.draft.as_mut().unwrap();
        apply_snapshot(draft, &painted());
        draft.puddles_dirty = false;
        (
            draft.map_id.clone(),
            revision,
            PuddleSnapshot {
                descriptor: draft.manifest.puddles.clone(),
                mask: draft.puddle_mask.clone(),
            },
        )
    };
    let old = source.mask.clone().unwrap();
    let result = super::types::PaintResult {
        map_id: map_id.clone(),
        revision,
        ground_content,
        source: source.clone(),
        snapshot: PuddleSnapshot {
            descriptor: source.descriptor.clone(),
            mask: Some(Arc::new(vec![255; 4])),
        },
        changed: true,
    };
    *app.world_mut().resource_mut::<PuddleAuthoringState>() = PuddleAuthoringState {
        map_id: Some(map_id),
        before: Some(source.clone()),
        current: Some(source),
        expected_revision: revision,
        ground_content,
        completed: Some(result),
        released: true,
        ..Default::default()
    }; // 模拟唯一CPU槽已经完成，采用仍走真实生产系统。
    old
}

#[test]
fn completed_clipmap_puddle_waits_for_height_then_rejects_new_author_revision() {
    let mut app = puddle_brush_app(true);
    let old = install_completed_puddle(&mut app, Some([1; 7]));
    app.update();
    assert!(
        app.world()
            .resource::<PuddleAuthoringState>()
            .completed
            .is_some()
    );
    assert!(app.world().resource::<PuddleAuthoringState>().busy()); // 暂无高度身份不丢候选，也不归还单飞槽。
    assert!(Arc::ptr_eq(
        &old,
        app.world()
            .resource::<crate::authoring::EditorAuthoringSession>()
            .draft
            .as_ref()
            .unwrap()
            .puddle_mask
            .as_ref()
            .unwrap()
    ));
    {
        let mut session = app
            .world_mut()
            .resource_mut::<crate::authoring::EditorAuthoringSession>();
        session.revision += 1;
        session.terrain_revision += 1;
    }
    app.update();
    assert!(!app.world().resource::<PuddleAuthoringState>().busy());
    let session = app
        .world()
        .resource::<crate::authoring::EditorAuthoringSession>();
    assert!(Arc::ptr_eq(
        &old,
        session
            .draft
            .as_ref()
            .unwrap()
            .puddle_mask
            .as_ref()
            .unwrap()
    ));
    assert!(session.undo.is_empty() && !session.draft.as_ref().unwrap().puddles_dirty);
}

#[test]
fn completed_puddle_cannot_overwrite_a_reloaded_source_or_mix_old_history() {
    let mut app = puddle_brush_app(false);
    install_completed_puddle(&mut app, None);
    app.world_mut()
        .resource_mut::<PuddleAuthoringState>()
        .changed = true; // 模拟已有完整采用块的旧笔。
    let reloaded = Arc::new(vec![9; 4]);
    app.world_mut()
        .resource_mut::<crate::authoring::EditorAuthoringSession>()
        .draft
        .as_mut()
        .unwrap()
        .puddle_mask = Some(reloaded.clone()); // 保持计数相同以验证实际来源身份。
    app.update();
    let session = app
        .world()
        .resource::<crate::authoring::EditorAuthoringSession>();
    assert!(Arc::ptr_eq(
        &reloaded,
        session
            .draft
            .as_ref()
            .unwrap()
            .puddle_mask
            .as_ref()
            .unwrap()
    ));
    assert!(session.undo.is_empty()); // 不能拿旧before和外部新像素拼成一条历史。
    assert!(!app.world().resource::<PuddleAuthoringState>().busy());
}
#[test] // 验证旧图不会自动铺满积水。
fn old_maps_and_creation_undo_redo_preserve_other_domains() {
    // 使用真实统一草稿夹具。
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121])); // 未创建积水层。
    assert!(draft.manifest.puddles.is_none() && draft.puddle_mask.is_none()); // 缺省即无积水。
    let terrain = draft.terrain_mesh.clone().unwrap(); // 其他作者源精确身份。
    let before = PuddleSnapshot::default(); // 首次创建前没有图像。
    let after = painted(); // 首次已完成R8。
    apply_snapshot(&mut draft, &after); // 用户完成首笔。
    assert!(Arc::ptr_eq(
        draft.puddle_mask.as_ref().unwrap(),
        after.mask.as_ref().unwrap()
    )); // 采用不复制。
    apply_snapshot(&mut draft, &before); // 一次撤销恢复未创建状态。
    assert!(draft.manifest.puddles.is_none() && draft.puddle_mask.is_none()); // 描述与像素共同撤销。
    apply_snapshot(&mut draft, &after); // 重做恢复同一完整版本。
    assert_eq!(
        draft.puddle_mask.as_ref().unwrap().as_slice(),
        &[0, 64, 128, 255]
    ); // 每个潜力值精确恢复。
    assert!(Arc::ptr_eq(draft.terrain_mesh.as_ref().unwrap(), &terrain)); // 地形与静水、植被数据不受积水笔影响。
    assert!(draft.water_masks.is_empty() && draft.vegetation_author_densities.is_empty()); // 没有偷偷复用其他作者域。
} // 结束历史回归。
#[test] // 验证真实共享事务和重新打开作者源。
fn saved_source_reloads_and_encoding_failure_keeps_previous_files() {
    // 失败必须发生在正式发布前。
    let root = std::env::temp_dir().join(format!(
        "puddle-author-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )); // 每次测试独享明确临时目录。
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121])); // 真实草稿形状。
    apply_snapshot(&mut draft, &painted()); // 提交完整作者画布。
    let mut writes = Vec::new(); // 统一事务冻结写项。
    prepare_writes(&root, &draft, &mut writes).unwrap(); // raw和KTX2一起准备。
    super::super::save::transaction::commit_prepared_writes(writes)
        .map_err(|error| error.message().to_owned())
        .unwrap(); // 运行实际stage与原子替换。
    let source = root.join("maps/EDGE-TEST/puddles.potential.r8"); // 已保存的准确文件。
    let texture = root.join("maps/EDGE-TEST/puddles.potential.ktx2"); // 同版GPU派生图。
    let old_texture = std::fs::read(&texture).unwrap(); // 记录完整原派生文件。
    draft.puddle_mask = None; // 模拟重新打开时尚未读取R8。
    read_source(&root, &mut draft).unwrap(); // 使用正式后台IO读取助手。
    assert_eq!(
        draft.puddle_mask.as_ref().unwrap().as_slice(),
        &[0, 64, 128, 255]
    ); // 保存和加载没有量化变化。
    draft.puddle_mask = Some(Arc::new(vec![255; 4])); // 后续作者新版本。
    let mut writes = Vec::new(); // 新一批保存写项。
    prepare_writes(&root, &draft, &mut writes).unwrap(); // 新raw和新纹理均先stage。
    writes.push(super::super::save::transaction::PreparedWrite::deferred(
        root.join("failure.bin"),
        || Err("注入后续编码失败".into()),
    )); // 真实事务末尾故障。
    assert!(super::super::save::transaction::commit_prepared_writes(writes).is_err()); // 整批禁止发布。
    assert_eq!(std::fs::read(source).unwrap(), vec![0, 64, 128, 255]); // 源仍是上一有效版本。
    assert_eq!(std::fs::read(texture).unwrap(), old_texture); // 派生图同样没有混版。
    std::fs::remove_dir_all(root).unwrap(); // 只清理本测试明确创建的唯一临时目录。
} // 结束事务回归。
#[test] // 拒绝坏输入而非让GPU或笔刷越界。
fn descriptor_and_source_length_are_validated_before_writes() {
    // 不执行任何正式IO。
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121])); // 准备有效图。
    let mut version = painted(); // 默认合法小画布。
    version.descriptor.as_mut().unwrap().resolution = [4097, 2]; // 超出明确纹理上限。
    assert!(version.descriptor.as_ref().unwrap().validate().is_err()); // 大图失败可诊断。
    apply_snapshot(&mut draft, &painted()); // 回到合法描述。
    draft.puddle_mask = Some(Arc::new(vec![0; 3])); // 缺失一个像素。
    let mut writes = Vec::new(); // 必须在加入任何写项前失败。
    assert!(prepare_writes(std::path::Path::new("assets"), &draft, &mut writes).is_err()); // 读取与保存都严格校验长度。
    assert!(writes.is_empty()); // 没有部分写项进入事务。
} // 结束输入校验回归。
#[test] // 连续笔迹不能因后台背压丢失中间区域。
fn bounded_segment_paint_is_continuous_and_erase_keeps_outside_unchanged() {
    // 复用生产纯CPU核心。
    let mut pixels = vec![0; 121]; // 11×11一米一个纹素。
    let sample = super::types::Sample {
        world: bevy::prelude::Vec3::new(9.0, 0.0, 5.0),
        radius: 1.5,
        amount: 1.0,
        erase: false,
    }; // 快速拖动的单个合并尾段。
    assert!(super::helpers::paint(
        &mut pixels,
        [11, 11],
        [0.0; 2],
        [10.0; 2],
        bevy::prelude::Vec3::new(1.0, 0.0, 5.0),
        sample
    )); // 一个后台块连续补齐整段。
    assert!((1..=9).all(|x| pixels[5 * 11 + x] == 255)); // 圆心轨迹没有断笔。
    assert_eq!(pixels[0], 0); // 扩展包围盒以外严格保留。
    let erase = super::types::Sample {
        erase: true,
        ..sample
    }; // 同一路径反向擦除语义。
    assert!(super::helpers::paint(
        &mut pixels,
        [11, 11],
        [0.0; 2],
        [10.0; 2],
        bevy::prelude::Vec3::new(1.0, 0.0, 5.0),
        erase
    )); // 擦除不接触其他作者域。
    assert!((1..=9).all(|x| pixels[5 * 11 + x] == 0)); // 满强度路径精确回到无积水。
    assert_eq!(pixels[0], 0); // 擦除仍保持范围外值。
} // 结束连续笔迹回归。
#[test] // 相同像素Arc不能掩盖地图或尺寸身份变化。
fn preview_identity_includes_map_and_resolution() {
    // 使用常数时间的生产完成门。
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121])); // 原地图。
    apply_snapshot(&mut draft, &painted()); // 完整2×2作者版本。
    let state = PuddleAuthoringState {
        preview_source: draft.puddle_mask.clone(),
        preview_map: Some(draft.map_id.clone()),
        preview_resolution: Some([2, 2]),
        ..Default::default()
    }; // 模拟已采用GPU版本。
    assert!(!state.preview_pending_for(&draft)); // 完整身份匹配时不重复准备。
    draft.manifest.puddles.as_mut().unwrap().resolution = [4, 1]; // 同Arc重新声明不同形状。
    assert!(state.preview_pending_for(&draft)); // 尺寸变化必须重建GPU图像。
    draft.manifest.puddles.as_mut().unwrap().resolution = [2, 2]; // 恢复原形状。
    draft.map_id = feathrise_map_content::MapId("ANOTHER-MAP".into()); // 同Arc移入另一地图。
    assert!(state.preview_pending_for(&draft)); // 相同像素也不能跨图误判Ready。
} // 结束完整身份回归。
#[test] // 世界范围只改变UV映射，不需要复制同一画布。
fn preview_republishes_world_bounds_without_rebuilding_pixels() {
    // 无图像任务的常数帧发布回归。
    use bevy::prelude::*; // 最小无窗口ECS应用。
    let mut app = App::new(); // 不安装渲染或窗口插件。
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121])); // 真实地图草稿。
    apply_snapshot(&mut draft, &painted()); // 已完成作者R8。
    let mut images = Assets::<Image>::default(); // CPU资产集合足够验证句柄采用。
    let image = images.add(Image::default()); // 本测试不验证GPU数据内容。
    let state = PuddleAuthoringState {
        preview_source: draft.puddle_mask.clone(),
        preview_map: Some(draft.map_id.clone()),
        preview_resolution: Some([2, 2]),
        image: Some(image.clone()),
        ..Default::default()
    }; // 已准备相同版本。
    app.insert_resource(state)
        .insert_resource(images)
        .insert_resource(crate::resources::MapEditorState {
            open: true,
            ..Default::default()
        })
        .insert_resource(crate::authoring::EditorAuthoringSession {
            draft: Some(draft),
            ..Default::default()
        })
        .init_resource::<feathrise_map_content::MapPuddleLiveOverride>()
        .add_systems(Update, sync_puddle_preview); // 只运行小资源发布。
    app.update(); // 首次发布原范围。
    let revision = app
        .world()
        .resource::<feathrise_map_content::MapPuddleLiveOverride>()
        .revision; // 保存已采用身份。
    app.world_mut()
        .resource_mut::<crate::authoring::EditorAuthoringSession>()
        .draft
        .as_mut()
        .unwrap()
        .manifest
        .world_max = [20.0, 30.0]; // 模拟尺寸作者修改但像素不变。
    app.update(); // 必须发布新世界UV端点。
    let live = app
        .world()
        .resource::<feathrise_map_content::MapPuddleLiveOverride>(); // 检查公共渲染合同。
    assert_eq!(live.world_max, Vec2::new(20.0, 30.0)); // 两个轴都正确更新。
    assert!(live.revision > revision); // 身份换代唤醒正式材质同步。
    assert_eq!(live.mask, Some(image)); // 无须重新分配同一像素图。
    assert!(
        app.world()
            .resource::<PuddleAuthoringState>()
            .preview_task
            .is_none()
    ); // 无后台重复计算。
} // 结束边界变更回归。

#[test] // 作者预览不能省略正式保存中已有的远景Mip。
fn preview_uses_the_same_complete_r8_mips_as_saved_texture() {
    let source = vec![64; 16]; // 4×4常数潜力，所有Mip都应保持64。
    let image = super::image::build(&source, [4, 4]).unwrap(); // 生产后台同源构建。
    assert_eq!(image.texture_descriptor.mip_level_count, 3); // 必须包含4×4、2×2、1×1。
    assert_eq!(
        image.texture_descriptor.format,
        bevy::render::render_resource::TextureFormat::R8Unorm
    ); // 禁止sRGB和RGBA替代。
    assert!(
        image
            .data
            .as_ref()
            .unwrap()
            .iter()
            .all(|&value| value == 64)
    ); // 全部层级保持同一线性数据语义。
} // 结束同源Mip回归。

#[test] // 保存成功但正式旧Image仍驻留时不得闪回旧画布。
fn saved_live_mask_remains_until_scene_acknowledges_content() {
    use bevy::prelude::*; // 无窗口最小ECS应用。
    use feathrise_map_content::{MapPuddleLiveOverride, MapPuddleSaveHandoff}; // 与正式桥接完全相同的交接协议。
    let mut app = App::new(); // 不创建渲染设备。
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121])); // 标准测试地图。
    apply_snapshot(&mut draft, &painted()); // 本次已保存R8的完整内容。
    draft.puddles_dirty = false; // 模拟IO事务成功后已经清脏。
    let mut images = Assets::<Image>::default(); // 仅验证CPU句柄所有权。
    let image = images.add(Image::default()); // 已完成上传的作者图像替身。
    let state = PuddleAuthoringState {
        // 与本次草稿相同的完整GPU版本。
        preview_source: draft.puddle_mask.clone(), // 源Arc身份。
        preview_map: Some(draft.map_id.clone()),   // 地图身份。
        preview_resolution: Some([2, 2]),          // 尺寸身份。
        image: Some(image.clone()),                // 当前完整图像。
        ..Default::default()                       // 没有未完成笔划或图像任务。
    };
    let handoff = MapPuddleSaveHandoff {
        // 正式场景尚未完成本次内容比对。
        map_id: Some(draft.map_id.clone()), // 等待当前地图。
        texture_path: Some(
            draft
                .manifest
                .puddles
                .as_ref()
                .unwrap()
                .texture_path
                .clone(),
        ), // 本次派生文件。
        expected_mask: draft.puddle_mask.clone(), // 本次精确R8。
        revision: 7,                        // 固定保存票据。
    };
    app.insert_resource(state)
        .insert_resource(images)
        .insert_resource(handoff)
        .insert_resource(crate::resources::MapEditorState::default())
        .insert_resource(crate::authoring::EditorAuthoringSession {
            draft: Some(draft),
            ..Default::default()
        })
        .init_resource::<MapPuddleLiveOverride>()
        .add_systems(Update, sync_puddle_preview); // F12已经关闭且没有dirty，唯一保持理由是待交接内容。
    app.update(); // 旧文件尚未被证明，继续保留作者画面。
    assert_eq!(
        app.world().resource::<MapPuddleLiveOverride>().mask,
        Some(image)
    ); // 无闪回旧文件空图。
    assert!(!app.world().resource::<MapPuddleLiveOverride>().preview); // 玩法不显示蓝色作者诊断。
    app.world_mut()
        .resource_mut::<MapPuddleSaveHandoff>()
        .map_id = None; // 模拟正式scene精确比对并原子采用后的回执。
    app.update(); // 只有此时归还正式来源。
    assert!(
        app.world()
            .resource::<MapPuddleLiveOverride>()
            .map_id
            .is_none()
    ); // 没有永久hold作者图掩盖外部热更新。
} // 结束保存内容交接回归。
