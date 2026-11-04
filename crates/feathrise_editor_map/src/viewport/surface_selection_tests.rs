//! 地表作者层覆盖的真实配对权重、边界和身份回归。
use super::*;
fn pairs(slots: &[[u32; 4]]) -> Vec<u8> {
    slots
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect()
} // 正式小端RGBA32。
#[test]
fn surface_selection_distinguishes_author_slots_and_zero_weights() {
    let indices = pairs(&[[0, 1, 2, 3]; 4]); // 即使共享预设也按作者索引区分。
    let (image, present) = build_overlay(
        &indices,
        &[255, 0, 0, 0, 128, 127, 0, 0, 0, 255, 0, 0, 0, 0, 0, 0],
        [2, 2],
        1,
    )
    .unwrap();
    let pixels = image.data.unwrap();
    assert!(present);
    assert_eq!(pixels[3], 0); // 非选中层与空像素都透明。
    assert_eq!(pixels[15], 0);
    assert!(pixels[7] >= 48 && pixels[7] < pixels[11]); // 混合贡献真实变化。
    assert_eq!(pixels[11], 160); // 满权重明显可见。
}
#[test]
fn surface_selection_empty_layer_and_invalid_pairs() {
    assert!(
        !build_overlay(&pairs(&[[0; 4]]), &[255, 0, 0, 0], [1, 1], 1)
            .unwrap()
            .1
    ); // 新层没有贡献。
    assert!(build_overlay(&[], &[], [0, 0], 0).is_err());
    assert!(build_overlay(&pairs(&[[0; 4]]), &[0], [1, 1], 0).is_err()); // 失败不能上传半张图。
}
#[test]
fn surface_selection_max_pool_preserves_narrow_ground_with_bounded_texture() {
    let mut weights = vec![0; 1025 * 4]; // 长窄地皮。
    weights[1024 * 4] = 255; // 最末纹素不能在降采样时丢失。
    let (image, present) =
        build_overlay(&pairs(&vec![[9, 0, 0, 0]; 1025]), &weights, [1025, 1], 9).unwrap();
    assert!(present);
    assert!(image.texture_descriptor.size.width <= 512);
    assert_eq!(*image.data.unwrap().last().unwrap(), 160);
}
#[test]
fn surface_selection_revision_can_keep_same_layer_but_reindex_and_map_cannot() {
    let original = Identity {
        map: MapId("test".to_owned()),
        terrain: None,
        layer: "author_a".into(),
        index: 2,
        revision: 1,
    };
    let mut current = original.clone();
    current.revision = 2;
    assert!(original.same_layer(&current)); // 更新期间保持完整旧覆盖。
    assert_ne!(original, current); // 迟到revision不能采用。
    current.index = 1;
    assert!(!original.same_layer(&current)); // 删除挪位必须重新确认。
    current = original.clone();
    current.layer = "author_b".into();
    assert!(!original.same_layer(&current)); // 同预设不同作者不得串层。
    current = original.clone();
    current.map = MapId("other".to_owned());
    assert!(!original.same_layer(&current));
    current = original.clone();
    current.terrain = Some(TerrainId::parse("another-canvas").unwrap());
    assert!(
        !original.same_layer(&current),
        "同图另一网格的同名作者层不能复用旧图"
    );
}

fn clipmap_fixture() -> App {
    use crate::authoring::terrain_documents::TerrainDocumentIdentity;
    let mut app = fixture();
    app.add_plugins(MinimalPlugins)
        .init_resource::<feathrise_scene::DynamicClipmapSurfaceOverlay>()
        .init_resource::<crate::authoring::SurfacePreviewState>();
    let id = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
        object.id = TerrainId::parse("selection-clipmap-a").unwrap();
        object.terrain.kind = feathrise_map_content::MapTerrainKind::SparseClipmap;
        object.transform.translation = [31.0, 7.0, -19.0];
        object.transform.yaw_degrees = 38.0;
        object.transform.scale = [2.0, 3.0, 0.5];
        let id = object.id.clone();
        let manifest = object.source_manifest(&draft.manifest);
        draft.manifest.terrains = Some(Arc::from([object.clone()]));
        draft.terrain_documents.active = Some(TerrainDocumentIdentity {
            definition: Arc::new(object),
            manifest,
            folder: draft.folder.clone(),
        });
        draft.surface_indices = None; // 首轮已采用图；切对象时不借缺失作者源自动重建来掩盖迟到采用。
        draft.surface_weights = None;
        id
    };
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .terrain_canvas_target = Some(id);
    let identity = selection(
        app.world().resource::<MapEditorState>(),
        app.world().resource::<EditorAuthoringSession>(),
        None,
        app.world().resource::<LayerCandidateState>(),
    )
    .unwrap();
    let image = app.world_mut().resource_mut::<Assets<Image>>().add(
        build_overlay(&pairs(&[[0; 4]]), &[255, 0, 0, 0], [1, 1], 0)
            .unwrap()
            .0,
    );
    {
        let mut state = app.world_mut().resource_mut::<SurfaceSelectionOverlay>();
        state.applied = Some(identity.clone());
        state.desired = Some(identity);
        state.image = Some(image);
    }
    *app.world_mut().resource_mut::<TerrainPreviewState>() = Default::default();
    app
}

#[test]
fn clipmap_selected_surface_publishes_actual_image_without_legacy_preview_and_withdraws_on_hide_failure_close()
 {
    let mut app = clipmap_fixture();
    app.update();
    let channel = app
        .world()
        .resource::<feathrise_scene::DynamicClipmapSurfaceOverlay>();
    let selected = channel.selection.as_ref().unwrap();
    let identity = selected.identity.clone();
    assert_eq!(Some(&identity), channel.current.as_ref());
    assert_eq!(
        Some(&selected.image),
        app.world()
            .resource::<SurfaceSelectionOverlay>()
            .image
            .as_ref()
    );
    assert_eq!(
        app.world_mut()
            .query::<&SurfaceSelectionMesh>()
            .iter(app.world())
            .count(),
        0,
        "稀疏地表高亮不能创建无高度位移的平面副本"
    );
    Arc::make_mut(
        &mut app
            .world_mut()
            .resource_mut::<MapEditorState>()
            .terrain_hidden,
    )
    .insert(identity.terrain_id.clone().unwrap());
    app.update();
    assert!(
        app.world()
            .resource::<feathrise_scene::DynamicClipmapSurfaceOverlay>()
            .current
            .is_none()
    );
    assert!(
        app.world()
            .resource::<feathrise_scene::DynamicClipmapSurfaceOverlay>()
            .selection
            .is_none()
    );
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .terrain_hidden = Default::default();
    app.update();
    assert!(
        app.world()
            .resource::<feathrise_scene::DynamicClipmapSurfaceOverlay>()
            .selection
            .is_some()
    );
    app.world_mut()
        .resource_mut::<crate::authoring::SurfacePreviewState>()
        .failed_identity = Some((
        identity.map_id,
        identity.terrain_id,
        identity.surface_revision,
        0,
    ));
    app.update();
    assert!(
        app.world()
            .resource::<feathrise_scene::DynamicClipmapSurfaceOverlay>()
            .current
            .is_none()
    );
    assert!(
        app.world()
            .resource::<feathrise_scene::DynamicClipmapSurfaceOverlay>()
            .selection
            .is_none()
    );
    app.world_mut()
        .resource_mut::<crate::authoring::SurfacePreviewState>()
        .failed_identity = None;
    app.update();
    app.world_mut().resource_mut::<MapEditorState>().open = false;
    app.update();
    assert!(
        app.world()
            .resource::<feathrise_scene::DynamicClipmapSurfaceOverlay>()
            .current
            .is_none()
    );
    assert!(
        app.world()
            .resource::<feathrise_scene::DynamicClipmapSurfaceOverlay>()
            .selection
            .is_none()
    );
}

#[test]
fn same_map_same_layer_delayed_surface_overlay_cannot_be_adopted_by_another_canvas() {
    let mut app = clipmap_fixture();
    app.update();
    let previous = app
        .world()
        .resource::<SurfaceSelectionOverlay>()
        .applied
        .clone()
        .unwrap();
    let image = app
        .world()
        .resource::<SurfaceSelectionOverlay>()
        .image
        .clone()
        .unwrap();
    let count = app.world().resource::<Assets<Image>>().len();
    let completion = previous.clone();
    app.world_mut()
        .resource_mut::<SurfaceSelectionOverlay>()
        .task = Some(AsyncComputeTaskPool::get().spawn(async move {
        (
            completion,
            build_overlay(&pairs(&[[0; 4]]), &[255, 0, 0, 0], [1, 1], 0),
        )
    }));
    let next = TerrainId::parse("selection-clipmap-b").unwrap();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let mut definition = draft
            .terrain_documents
            .active
            .as_ref()
            .unwrap()
            .definition
            .as_ref()
            .clone();
        definition.id = next.clone();
        draft.manifest.terrains = Some(Arc::from([definition.clone()]));
        draft.terrain_documents.active.as_mut().unwrap().definition = Arc::new(definition);
    }
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .terrain_canvas_target = Some(next.clone());
    // 固定次数推进真实异步完成；不无限轮询，不把测试等待放进生产系统。
    for _ in 0..100 {
        app.update();
        if app
            .world()
            .resource::<SurfaceSelectionOverlay>()
            .task
            .is_none()
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let state = app.world().resource::<SurfaceSelectionOverlay>();
    assert!(state.task.is_none(), "小型覆盖worker应在有限测试窗口内完成");
    assert_eq!(state.applied.as_ref(), Some(&previous));
    assert_eq!(state.image.as_ref(), Some(&image));
    assert_eq!(
        app.world().resource::<Assets<Image>>().len(),
        count,
        "迟到完成不能登记Image或替换当前画布"
    );
    let channel = app
        .world()
        .resource::<feathrise_scene::DynamicClipmapSurfaceOverlay>();
    assert_eq!(
        channel.current.as_ref().unwrap().terrain_id.as_ref(),
        Some(&next)
    );
    assert!(
        channel.selection.is_none(),
        "相同Map/层ID/下标/修订仍不足以授权另一网格的覆盖"
    );
}

fn fixture() -> App {
    // 真实系统的无窗口夹具，预置完整覆盖避免依赖线程调度时间。
    use feathrise_map_content::{MAP_SURFACE_SCHEMA_VERSION, MapSurfaceAsset, MapSurfaceLayer};
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    draft.surface = Some(MapSurfaceAsset {
        schema_version: MAP_SURFACE_SCHEMA_VERSION, // 当前格式。
        map_id: draft.map_id.clone(),               // 同地图。
        weight_source_path: "w".into(),             // 无文件IO。
        weight_texture_path: "wt".into(),           // 无文件IO。
        index_source_path: "i".into(),              // 无文件IO。
        index_texture_path: "it".into(),            // 无文件IO。
        resolution: [1, 1],                         // 最小画布。
        layers: vec![
            MapSurfaceLayer {
                id: "first".into(),
                label: "第一层".into(),
                preset_id: "grass".into(),
            },
            MapSurfaceLayer {
                id: "second".into(),
                label: "第二层".into(),
                preset_id: "grass".into(),
            },
        ], // 相同预设，独立作者身份。
    });
    let editor = MapEditorState {
        open: true,
        tool: EditorTool::Surface,
        selected_map_id: Some(draft.map_id.clone()),
        surface_selected_preset_id: Some("grass".into()),
        ..default()
    };
    let session = EditorAuthoringSession {
        draft: Some(draft),
        surface_revision: 3,
        ..default()
    };
    let identity = selection(&editor, &session, None, &LayerCandidateState::default()).unwrap(); // 编辑模式兼容空候选状态。
    let mut app = App::new();
    app.insert_resource(editor)
        .insert_resource(session)
        .init_resource::<LayerCandidateState>() // 系统使用统一悬停目标。
        .init_resource::<SurfaceBrushStroke>()
        .init_resource::<SurfaceStrokeTrail>()
        .init_resource::<Assets<Image>>()
        .init_resource::<Assets<StandardMaterial>>();
    let mesh = Handle::<Mesh>::default();
    app.insert_resource(TerrainPreviewState {
        applied_map_id: Some(identity.map.clone()),
        mesh: Some(mesh),
        ..default()
    });
    app.insert_resource(SurfaceSelectionOverlay {
        desired: Some(identity.clone()),
        applied: Some(identity),
        material: Some(Handle::default()),
        nonempty: true,
        ..default()
    });
    app.add_systems(Update, sync_surface_selection_overlay);
    app
}
#[test]
fn surface_selection_real_system_hides_on_switch_close_and_brush_then_restores() {
    let mut app = fixture();
    app.update();
    let entity = app
        .world()
        .resource::<SurfaceSelectionOverlay>()
        .entity
        .unwrap();
    assert_eq!(
        *app.world().get::<Visibility>(entity).unwrap(),
        Visibility::Inherited
    );
    app.world_mut().resource_mut::<SurfaceBrushStroke>().active = true;
    app.update();
    assert_eq!(
        *app.world().get::<Visibility>(entity).unwrap(),
        Visibility::Hidden
    ); // 画笔优先。
    app.world_mut().resource_mut::<SurfaceBrushStroke>().active = false;
    app.update();
    assert_eq!(
        *app.world().get::<Visibility>(entity).unwrap(),
        Visibility::Inherited
    );
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .selected_surface_layer = 1;
    app.update();
    assert_eq!(
        *app.world().get::<Visibility>(entity).unwrap(),
        Visibility::Hidden
    ); // 同预设另一作者层也不能显示旧面。
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .selected_surface_layer = 0;
    app.world_mut().resource_mut::<MapEditorState>().open = false;
    app.update();
    assert_eq!(
        *app.world().get::<Visibility>(entity).unwrap(),
        Visibility::Hidden
    ); // 游戏相机没有选中色。
    app.world_mut().resource_mut::<MapEditorState>().open = true;
    app.update();
    assert_eq!(
        *app.world().get::<Visibility>(entity).unwrap(),
        Visibility::Inherited
    ); // 暂停恢复复用完整覆盖。
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .surface_selected_preset_id = Some("different".into());
    app.update();
    assert_eq!(
        *app.world().get::<Visibility>(entity).unwrap(),
        Visibility::Hidden
    ); // 无效笔刷目标不能假选旧层。
}

#[test] // 跨类别悬停不能写回地表正式选层，移开必须恢复。
fn surface_hover_uses_exact_author_then_restores_selection() {
    let mut app = fixture(); // 两个共享预设的作者层。
    app.world_mut().resource_mut::<MapEditorState>().tool = EditorTool::Inspect; // 进入统一选择模式。
    let editor = app.world().resource::<MapEditorState>(); // 借用权威编辑器状态。
    let session = app.world().resource::<EditorAuthoringSession>(); // 同地图作者快照。
    let mut candidates = LayerCandidateState::default(); // 初始化真实生产候选资源。
    candidates.selected = Some(LayerTarget::Surface("first".into())); // 正式选第一层。
    candidates.selected_author_index = Some(0); // 后台已解析的缓存位置。
    candidates.hover = Some(LayerTarget::Surface("second".into())); // 暂时预览第二层。
    candidates.hover_author_index = Some(1); // 同预设不共享索引。
    assert_eq!(
        selection(&editor, session, None, &candidates)
            .unwrap()
            .layer,
        "second"
    ); // 真正高亮悬停作者。
    assert_eq!(editor.selected_surface_layer, 0); // 正式侧栏选择保持不变。
    candidates.hover = Some(LayerTarget::Domain("guards".into())); // 跨类型预览。
    assert!(selection(&editor, session, None, &candidates).is_none()); // 不叠加旧地表高亮。
    candidates.hover = None; // 指针移开候选。
    assert_eq!(
        selection(&editor, session, None, &candidates)
            .unwrap()
            .layer,
        "first"
    ); // 恢复正式作者选择。
    candidates.selected_author_index = Some(1); // 模拟删除挪位后旧索引。
    assert!(selection(&editor, session, None, &candidates).is_none()); // 身份失配不串层。
}
