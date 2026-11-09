//! 缩略图身份、网格、后台检查与有界退役的行为回归。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use super::{
    components::*, constants::*, demand::*, jobs::*, presentation::*, resources::*, types::*,
};
use crate::ui::sidebars::{EditorNavigatorTab, MapEditorSidebarState};
use crate::{
    components::*,
    resources::{EditorAssetCatalog, MapEditorNumericScrubMouseGuard, MapEditorState},
    types::EditorAssetEntry,
};
use bevy::{camera::visibility::RenderLayers, prelude::*};
use feathrise_map_content::MapAssetKind;
use feathrise_editor_core::EditorSettings;
use feathrise_editor_core::AssetBrowserViewMode;
use std::sync::Arc;

fn source(stamp: u64) -> SourceInfo {
    SourceInfo {
        primary: FileStamp {
            length: stamp,
            modified: None,
        },
        stamp,
        dependencies: Arc::from([]),
    }
}

fn current_state() -> ThumbnailState {
    ThumbnailState {
        identity: Some(WindowKey {
            folder: "gltf".into(),
            start: 0,
            count: 12,
            revision: 1,
        }),
        epoch: 2,
        paths: vec!["gltf/test.glb".into()],
        ..default()
    }
}

fn job(root: Option<Entity>) -> ThumbnailJob {
    ThumbnailJob {
        epoch: 2,
        path: "gltf/test.glb".into(),
        stamp: 1,
        started: 0.0,
        phase: JobPhase::Scanning,
        target: Handle::default(),
        camera: Entity::PLACEHOLDER,
        await_reload: false,
        refresh_after_load: false,
        root,
        scene: None,
        image: None,
        texture_preview: None,
        owned_mesh: None,
        owned_material: None,
        stack: root.map(NodeCursor::new).into_iter().collect(),
        nodes: 0,
        minimum: Vec3::splat(f32::INFINITY),
        maximum: Vec3::splat(f32::NEG_INFINITY),
        probes: Vec::new(),
    }
}

#[test]
fn late_or_closed_page_does_not_adopt_inspection() {
    let mut state = current_state();
    adopt_inspection(
        &mut state,
        InspectionResult {
            epoch: 1,
            sources: vec![("gltf/test.glb".into(), Ok(source(1)))],
        },
    );
    assert!(state.cache.is_empty());
    state.identity = None;
    adopt_inspection(
        &mut state,
        InspectionResult {
            epoch: 2,
            sources: vec![("gltf/test.glb".into(), Ok(source(1)))],
        },
    );
    assert!(state.cache.is_empty());
}

#[test]
fn cache_reuses_same_source_and_invalidates_changed_source_or_failure() {
    let mut state = current_state();
    state.cache.insert(
        "gltf/test.glb".into(),
        CacheEntry {
            source: Ok(source(1)),
            result: ThumbnailResult::Ready(Handle::default()),
            used: 0,
        },
    );
    adopt_inspection(
        &mut state,
        InspectionResult {
            epoch: 2,
            sources: vec![("gltf/test.glb".into(), Ok(source(1)))],
        },
    );
    assert!(matches!(
        state.cache["gltf/test.glb"].result,
        ThumbnailResult::Ready(_)
    ));
    adopt_inspection(
        &mut state,
        InspectionResult {
            epoch: 2,
            sources: vec![("gltf/test.glb".into(), Ok(source(2)))],
        },
    );
    assert!(matches!(
        state.cache["gltf/test.glb"].result,
        ThumbnailResult::Queued
    ));
    assert!(!state.job_current(&job(None))); // 旧GPU回执不能确认新源文件。
    adopt_inspection(
        &mut state,
        InspectionResult {
            epoch: 2,
            sources: vec![("gltf/test.glb".into(), Err("文件缺失".into()))],
        },
    );
    assert!(matches!(
        state.cache["gltf/test.glb"].result,
        ThumbnailResult::Failed(_)
    ));
}

#[test]
fn cache_limit_protects_current_page_and_evicts_oldest_other_snapshot() {
    let mut state = current_state();
    for index in 0..CACHE_CAPACITY {
        let path = if index == 0 {
            "gltf/test.glb".into()
        } else {
            format!("gltf/{index}.glb")
        };
        state.cache.insert(
            path,
            CacheEntry {
                source: Ok(source(1)),
                result: ThumbnailResult::Ready(Handle::default()),
                used: index as u64,
            },
        );
    }
    state.paths.push("gltf/new.glb".into());
    adopt_inspection(
        &mut state,
        InspectionResult {
            epoch: 2,
            sources: vec![("gltf/new.glb".into(), Ok(source(1)))],
        },
    );
    assert_eq!(state.cache.len(), CACHE_CAPACITY);
    assert!(state.cache.contains_key("gltf/test.glb"));
    assert!(!state.cache.contains_key("gltf/1.glb"));
}

#[test]
fn grid_adapts_columns_and_last_page_uses_only_real_rows() {
    let narrow = grid_layout(260.0, 12);
    let wide = grid_layout(550.0, 12);
    assert_eq!(narrow.columns, 2);
    assert_eq!(grid_layout(218.0, 12).columns, 2); // 默认左栏有效宽度不能退化成一张一行。
    assert_eq!(wide.columns, 4);
    assert_eq!(grid_layout(260.0, 1).height, TILE_HEIGHT);
    assert_eq!(grid_layout(260.0, 0).height, 0.0);
    assert!(wide.height < narrow.height);
    assert!(grid_layout(f32::NAN, 12).width.is_finite());
}

#[test]
fn thumbnail_demand_follows_left_browser_instead_of_right_properties() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(MapEditorState { open: true, ..default() })
        .init_resource::<MapEditorSidebarState>()
        .init_resource::<crate::resources::MapEditorLayoutState>()
        .init_resource::<EditorAssetCatalog>()
        .init_resource::<ThumbnailState>()
        .insert_resource(EditorSettings { asset_browser_view_mode: AssetBrowserViewMode::Thumbnails, ..default() })
        .add_systems(Update, sync_demand);
    app.world_mut().resource_mut::<MapEditorSidebarState>().navigator = EditorNavigatorTab::Assets;
    app.world_mut().resource_mut::<MapEditorSidebarState>().right = crate::ui::sidebars::EditorRightSidebarTab::Tools;
    app.world_mut().resource_mut::<crate::resources::MapEditorLayoutState>().inspector_visible = false;
    app.update();
    assert!(app.world().resource::<ThumbnailState>().identity.is_some());
    app.world_mut().resource_mut::<MapEditorSidebarState>().right = crate::ui::sidebars::EditorRightSidebarTab::Assets;
    app.update();
    assert!(app.world().resource::<ThumbnailState>().identity.is_some());
    app.world_mut().resource_mut::<crate::resources::MapEditorLayoutState>().content_visible = false;
    app.update();
    assert!(app.world().resource::<ThumbnailState>().identity.is_none());
    app.world_mut().resource_mut::<crate::resources::MapEditorLayoutState>().content_visible = true;
    app.world_mut().resource_mut::<MapEditorSidebarState>().navigator = EditorNavigatorTab::Scene;
    app.update();
    assert!(app.world().resource::<ThumbnailState>().identity.is_none());
}

#[test]
fn view_mode_click_respects_shared_guard_and_preserves_asset_and_author_state() {
    let mut app = App::new();
    let mut sidebar = MapEditorSidebarState::default();
    sidebar.navigator = EditorNavigatorTab::Assets;
    app.insert_resource(MapEditorState {
        open: true,
        selected_asset: Some("keep.glb".into()),
        dirty: true,
        ..default()
    })
    .insert_resource(sidebar)
    .init_resource::<MapEditorNumericScrubMouseGuard>()
    .init_resource::<EditorSettings>()
    .add_systems(Update, control_view_mode);
    let button = app
        .world_mut()
        .spawn((
            ViewModeButton(AssetBrowserViewMode::Thumbnails),
            feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed,
        ))
        .id();
    app.world_mut()
        .resource_mut::<MapEditorNumericScrubMouseGuard>()
        .advance(true, true, false);
    app.update();
    app.world_mut()
        .resource_mut::<MapEditorNumericScrubMouseGuard>()
        .advance(false, false, false);
    app.update();
    assert_eq!(
        app.world()
            .resource::<EditorSettings>()
            .asset_browser_view_mode,
        AssetBrowserViewMode::List
    );
    *app.world_mut().get_mut::<UiInteraction>(button).unwrap() = UiInteraction::None;
    app.update();
    *app.world_mut().get_mut::<UiInteraction>(button).unwrap() = UiInteraction::Pressed;
    app.world_mut().entity_mut(button).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    app.update();
    assert_eq!(
        app.world()
            .resource::<EditorSettings>()
            .asset_browser_view_mode,
        AssetBrowserViewMode::Thumbnails
    );
    let editor = app.world().resource::<MapEditorState>();
    assert!(editor.dirty);
    assert_eq!(editor.selected_asset.as_deref(), Some("keep.glb"));
}

#[test]
fn fixed_row_switches_grid_and_back_without_replacing_identity_or_selection() {
    let mut app = App::new();
    let mut catalog = EditorAssetCatalog::default();
    catalog.entries.push(EditorAssetEntry {
        asset_path: "texture.png".into(),
        display_name: "纹理.png".into(),
        is_directory: false,
        kind: MapAssetKind::Texture,
    });
    app.insert_resource(catalog)
        .init_resource::<EditorSettings>()
        .init_resource::<ThumbnailState>()
        .insert_resource(MapEditorState {
            open: true,
            selected_asset: Some("texture.png".into()),
            ..default()
        })
        .add_systems(
            Update,
            (crate::ui::rows::sync_asset_rows, sync_presentation).chain(),
        );
    let pool = app
        .world_mut()
        .spawn((
            MapEditorAssetList,
            Node::default(),
            ComputedNode {
                size: Vec2::new(280.0, 20.0),
                ..default()
            },
        ))
        .id();
    let row = app
        .world_mut()
        .spawn((
            InteractionButton, // 与真实行池一致，由按钮提供悬停和按下反馈。
            MapEditorAssetRow {
                slot: 0,
                entry_index: Some(0),
            },
            Node::default(),
            BorderColor::default(),
            BackgroundColor::default(),
        ))
        .id();
    app.world_mut()
        .commands()
        .entity(row)
        .with_children(|parent| {
            spawn_thumbnail_nodes(parent, 0, &Handle::default());
            parent.spawn((MapEditorAssetRowText, Text::new("[纹理] 纹理.png")));
        });
    app.world_mut().flush();
    let text = app
        .world_mut()
        .query_filtered::<Entity, With<MapEditorAssetRowText>>()
        .single(app.world())
        .unwrap();
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .asset_browser_view_mode = AssetBrowserViewMode::Thumbnails;
    app.update(); // 此处也验证所有可变UI查询的过滤器互斥，无B0001查询冲突。
    assert_eq!(
        app.world().get::<Node>(row).unwrap().position_type,
        PositionType::Absolute
    );
    assert_eq!(
        app.world().get::<Node>(pool).unwrap().height,
        px(TILE_HEIGHT)
    );
    assert_eq!(app.world().get::<Text>(text).unwrap().as_str(), "纹理.png");
    assert_eq!(
        app.world().get::<TextLayout>(text).unwrap().linebreak,
        bevy::text::LineBreak::AnyCharacter
    );
    for (interaction, expected) in [
        (UiInteraction::None, crate::ui::theme::SELECT_BG),
        (UiInteraction::Hovered, crate::ui::theme::SELECT_BG_HOVER),
        (UiInteraction::Pressed, crate::ui::theme::SELECT_BG_PRESSED),
    ] {
        *app.world_mut().get_mut::<UiInteraction>(row).unwrap() = interaction;
        app.update();
        assert_eq!(app.world().get::<BackgroundColor>(row).unwrap().0, expected);
        assert_eq!(
            app.world()
                .resource::<MapEditorState>()
                .selected_asset
                .as_deref(),
            Some("texture.png")
        );
    }
    *app.world_mut().get_mut::<UiInteraction>(row).unwrap() = UiInteraction::None;
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .asset_browser_view_mode = AssetBrowserViewMode::List;
    app.update();
    assert_eq!(
        app.world().get::<Node>(row).unwrap().position_type,
        PositionType::Absolute // 连续列表与缩略图都以全目录绝对索引定位可见行。
    );
    assert_eq!(app.world().get::<Node>(row).unwrap().height, px(feathrise_editor_ui::metrics::ROW_HEIGHT));
    assert_eq!(
        app.world()
            .get::<MapEditorAssetRow>(row)
            .unwrap()
            .entry_index,
        Some(0)
    );
    assert_eq!(app.world().get::<Node>(pool).unwrap().height, px(feathrise_editor_ui::metrics::ROW_HEIGHT));
    assert_eq!(
        app.world().get::<Text>(text).unwrap().as_str(),
        "纹理.png"
    );
    assert_eq!(
        app.world().get::<TextLayout>(text).unwrap().linebreak,
        bevy::text::LineBreak::NoWrap
    );
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .selected_asset = None;
    app.update();
    assert_eq!(
        app.world().get::<BackgroundColor>(row).unwrap().0,
        crate::ui::theme::PANEL_BG
    );
}

#[test]
fn scene_is_hidden_until_all_nodes_are_isolated_and_bounds_are_complete() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
        .init_resource::<Assets<Image>>()
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .init_resource::<EditorAssetCatalog>();
    let root = app
        .world_mut()
        .spawn((ThumbnailRoot, Visibility::Hidden))
        .id();
    let children: Vec<_> = (0..200)
        .map(|_| app.world_mut().spawn(Transform::default()).id())
        .collect();
    app.world_mut().entity_mut(root).add_children(&children);
    let mesh = app
        .world_mut()
        .spawn((
            Mesh3d::default(),
            MeshMaterial3d::<StandardMaterial>::default(),
            GlobalTransform::from_translation(Vec3::new(10.0, 3.0, -2.0)),
            bevy::camera::primitives::Aabb::from_min_max(Vec3::splat(-1.0), Vec3::splat(1.0)),
        ))
        .id();
    app.world_mut().entity_mut(root).add_child(mesh);
    let mut state = current_state();
    state.cache.insert(
        "gltf/test.glb".into(),
        CacheEntry {
            source: Ok(source(1)),
            result: ThumbnailResult::Queued,
            used: 0,
        },
    );
    state.active = Some(job(Some(root)));
    app.insert_resource(state)
        .add_systems(Update, (drive_jobs, advance_scene).chain());
    app.update();
    assert_eq!(
        *app.world().get::<Visibility>(root).unwrap(),
        Visibility::Hidden
    );
    assert!(
        app.world()
            .resource::<ThumbnailState>()
            .active
            .as_ref()
            .unwrap()
            .nodes
            <= NODE_STEPS_PER_FRAME
    );
    for _ in 0..16 {
        app.update();
        if *app.world().get::<Visibility>(root).unwrap() != Visibility::Hidden {
            break;
        }
    }
    assert_eq!(
        *app.world().get::<Visibility>(root).unwrap(),
        Visibility::Inherited
    );
    for entity in children.into_iter().chain([mesh]) {
        assert_eq!(
            app.world().get::<RenderLayers>(entity).unwrap(),
            &RenderLayers::layer(feathrise_editor_core::constants::render_layers::MAP_EDITOR_ASSET_THUMBNAIL_LAYER)
        );
    }
    let current = app
        .world()
        .resource::<ThumbnailState>()
        .active
        .as_ref()
        .unwrap();
    assert_eq!(current.minimum, Vec3::new(9.0, 2.0, -3.0));
    assert!(matches!(
        app.world().resource::<ThumbnailState>().cache["gltf/test.glb"].result,
        ThumbnailResult::Queued
    )); // CPU范围就绪仍不能发布未绘制快照。
}

#[test]
fn wide_tree_retirement_is_bounded_and_does_not_skip_shifted_siblings() {
    let mut app = App::new();
    app.init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>();
    let root = app.world_mut().spawn_empty().id();
    let children: Vec<_> = (0..350)
        .map(|_| app.world_mut().spawn_empty().id())
        .collect();
    app.world_mut().entity_mut(root).add_children(&children);
    let mut state = ThumbnailState::default();
    state.retiring = Some(Retirement {
        stack: vec![NodeCursor::new(root)],
        job: job(Some(root)),
    });
    app.insert_resource(state).add_systems(Update, retire_nodes);
    let before = app.world().entities().len();
    app.update();
    assert!(before - app.world().entities().len() <= NODE_STEPS_PER_FRAME as u32);
    assert!(app.world().get_entity(root).is_ok());
    for _ in 0..16 {
        app.update();
    }
    assert!(app.world().resource::<ThumbnailState>().retiring.is_none());
    assert!(app.world().get_entity(root).is_err());
    assert!(
        children
            .into_iter()
            .all(|entity| app.world().get_entity(entity).is_err())
    );
}

#[test]
fn cube_and_zero_size_images_cannot_enter_flat_thumbnail_material() {
    let mut image = Image::default();
    image.texture_descriptor.size.depth_or_array_layers = 6;
    assert!(flat_image_size(&image).is_err());
    image.texture_descriptor.size.depth_or_array_layers = 1;
    image.texture_descriptor.size.width = 0;
    assert!(flat_image_size(&image).is_err());
}

#[test]
fn cold_texture_refresh_drains_old_parent_events_before_waiting_for_new_modified() {
    use bevy::world_serialization::WorldAsset;
    use bevy::image::{CompressedImageFormats, ImageLoader, ImagePlugin};
    use bevy::asset::LoadState;
    use crate::ui::asset_preview::compatible_texture::CompatiblePreviewTexture;
    let fixture = Fixture::new();
    let write_valid = || image::RgbaImage::from_pixel(16, 8, image::Rgba([20, 80, 160, 255])).save(fixture.root.join("warm.png")).unwrap();
    write_valid();
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin { file_path: fixture.root.to_string_lossy().into_owned(), ..default() }, ImagePlugin::default()))
        .register_asset_loader(ImageLoader::new(CompressedImageFormats::NONE)).init_asset::<WorldAsset>();
    crate::ui::asset_preview::compatible_texture::install(&mut app);
    app.finish(); app.cleanup();
    let parent = app.world().resource::<AssetServer>().load::<CompatiblePreviewTexture>("warm.png");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(12);
    loop {
        app.update();
        if let LoadState::Failed(error) = app.world().resource::<AssetServer>().load_state(parent.id()) {
            assert!(crate::ui::asset_preview::compatible_texture::retry_capacity(app.world().resource::<AssetServer>(), &parent, &error.to_string()), "{error}");
        }
        if app.world().resource::<Assets<CompatiblePreviewTexture>>().contains(parent.id()) { break; }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let image = app.world().resource::<Assets<CompatiblePreviewTexture>>().get(&parent).unwrap().image.clone();
    let mut state = current_state();
    let mut pending = job(None);
    pending.path = "warm.png".into();
    pending.image = Some(image.clone());
    pending.texture_preview = Some(parent.clone());
    pending.await_reload = true;
    pending.refresh_after_load = true;
    state.active = Some(pending);
    app.insert_resource(state);
    let poll = app.world_mut().register_system(poll_reloaded_assets);
    for event in [AssetEvent::Added { id: parent.id() }, AssetEvent::Modified { id: parent.id() }] {
        app.world_mut().resource_mut::<Messages<AssetEvent<CompatiblePreviewTexture>>>().write(event);
    }
    app.world_mut().run_system(poll).unwrap();
    let pending = app.world().resource::<ThumbnailState>().active.as_ref().unwrap();
    assert!(!pending.refresh_after_load);
    assert!(pending.await_reload, "同批旧Modified不能解除刚派发的新reload");
    app.world_mut().resource_mut::<Messages<AssetEvent<Image>>>().write(AssetEvent::Modified { id: image.id() });
    app.world_mut().resource_mut::<Messages<AssetEvent<CompatiblePreviewTexture>>>().write(AssetEvent::LoadedWithDependencies { id: parent.id() });
    app.world_mut().run_system(poll).unwrap();
    assert!(app.world().resource::<ThumbnailState>().active.as_ref().unwrap().await_reload, "图片label或旧依赖完成不能冒充新父发布");
    app.world_mut().resource_mut::<Messages<AssetEvent<CompatiblePreviewTexture>>>().write(AssetEvent::Modified { id: parent.id() });
    app.world_mut().run_system(poll).unwrap();
    assert!(!app.world().resource::<ThumbnailState>().active.as_ref().unwrap().await_reload);
    // 已有旧发布值的失败也必须typed重试同步进入Loading，不能被旧Failed抢先解锁。
    std::fs::write(fixture.root.join("warm.png"), b"invalid image").unwrap();
    app.world().resource::<AssetServer>().reload("warm.png");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(12);
    while !matches!(app.world().resource::<AssetServer>().load_state(parent.id()), LoadState::Failed(_)) {
        app.update();
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    write_valid();
    {
        let mut state = app.world_mut().resource_mut::<ThumbnailState>();
        let pending = state.active.as_mut().unwrap();
        pending.refresh_after_load = true;
        pending.await_reload = false;
    }
    app.world_mut().run_system(poll).unwrap();
    let pending = app.world().resource::<ThumbnailState>().active.as_ref().unwrap();
    assert!(!pending.refresh_after_load && !pending.await_reload);
    assert!(matches!(app.world().resource::<AssetServer>().load_state(parent.id()), LoadState::Loading));
}

#[test]
fn retiring_image_material_detaches_before_bounded_node_retirement() {
    let mut app = App::new();
    app.init_resource::<Assets<Mesh>>().init_resource::<Assets<StandardMaterial>>();
    let material = app.world_mut().resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial {
        base_color_texture: Some(Handle::default()), ..default()
    });
    let root = app.world_mut().spawn_empty().id();
    let children: Vec<_> = (0..300).map(|_| app.world_mut().spawn_empty().id()).collect();
    app.world_mut().entity_mut(root).add_children(&children);
    let mut retired = job(Some(root));
    retired.owned_material = Some(material.clone());
    app.insert_resource(ThumbnailState { retiring: Some(Retirement { stack: vec![NodeCursor::new(root)], job: retired }), ..default() })
        .add_systems(Update, retire_nodes);
    app.update();
    assert!(app.world().resource::<ThumbnailState>().retiring.is_some());
    assert!(app.world().resource::<Assets<StandardMaterial>>().get(&material).unwrap().base_color_texture.is_none(), "隐藏实体不能代替断开RenderApp材质采样");
}

#[test]
fn cancelled_epoch_and_timeout_do_not_publish_ready_receipts() {
    use bevy::asset::AssetPlugin;
    use std::sync::atomic::Ordering;
    for timeout in [false, true] {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<EditorAssetCatalog>()
            .add_systems(Update, (drive_jobs, retire_nodes).chain());
        let camera = app.world_mut().spawn(Camera::default()).id();
        let mut state = current_state();
        state.camera = Some(camera);
        state.cache.insert(
            "gltf/test.glb".into(),
            CacheEntry {
                source: Ok(source(1)),
                result: ThumbnailResult::Queued,
                used: 0,
            },
        );
        let mut candidate = job(None);
        candidate.phase = JobPhase::Rendering;
        let receipt = Arc::new(RenderReceipt::default());
        receipt.ready.store(true, Ordering::Release);
        candidate.probes.push(receipt);
        if timeout {
            candidate.started = -31.0;
        } else {
            state.epoch += 1;
        }
        state.active = Some(candidate);
        app.insert_resource(state);
        app.update();
        let state = app.world().resource::<ThumbnailState>();
        assert!(state.active.is_none());
        assert!(!matches!(
            state.cache["gltf/test.glb"].result,
            ThumbnailResult::Ready(_)
        ));
        assert_eq!(
            matches!(
                state.cache["gltf/test.glb"].result,
                ThumbnailResult::Failed(_)
            ),
            timeout
        );
        assert!(!app.world().get::<Camera>(camera).unwrap().is_active);
        assert!(matches!(
            app.world().get::<bevy::camera::RenderTarget>(camera),
            Some(bevy::camera::RenderTarget::None { .. })
        ));
    }
}

#[test]
fn scroll_window_change_waits_for_visible_rows_to_bind_to_the_current_window() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
        .init_asset::<Image>()
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .add_systems(Update, drive_jobs);
    crate::ui::asset_preview::compatible_texture::install(&mut app);
    let mut catalog = EditorAssetCatalog::default();
    catalog.entries = (0..13)
        .map(|index| EditorAssetEntry {
            asset_path: format!("test-{index}.png"),
            display_name: format!("test-{index}.png"),
            is_directory: false,
            kind: MapAssetKind::Texture,
        })
        .collect();
    catalog.visible_start = 12;
    catalog.visible_count = 1;
    let mut state = current_state();
    state.identity.as_mut().unwrap().start = 12;
    state.identity.as_mut().unwrap().count = 1;
    state.paths = vec!["test-12.png".into()];
    for path in ["test-0.png", "test-12.png"] {
        state.cache.insert(
            path.into(),
            CacheEntry {
                source: Ok(source(1)),
                result: ThumbnailResult::Queued,
                used: 0,
            },
        );
    }
    app.insert_resource(catalog).insert_resource(state);
    app.world_mut().spawn((
        crate::ui::sidebars::MapEditorSidebarPage(crate::ui::sidebars::EditorSidebarTab::Navigator(
            EditorNavigatorTab::Assets,
        )),
        ComputedNode {
            size: Vec2::splat(200.0),
            ..default()
        },
        UiGlobalTransform::default(),
    ));
    let row = app
        .world_mut()
        .spawn((
            MapEditorAssetRow {
                slot: 0,
                entry_index: Some(0),
            },
            ComputedNode {
                size: Vec2::splat(100.0),
                ..default()
            },
            UiGlobalTransform::default(),
        ))
        .id();
    app.update();
    assert!(app.world().resource::<ThumbnailState>().active.is_none());
    assert!(
        app.world()
            .resource::<AssetServer>()
            .get_handle::<Image>("test-0.png")
            .is_none()
    );
    app.world_mut()
        .get_mut::<MapEditorAssetRow>(row)
        .unwrap()
        .entry_index = Some(12);
    app.update();
    assert_eq!(
        app.world()
            .resource::<ThumbnailState>()
            .active
            .as_ref()
            .unwrap()
            .path,
        "test-12.png"
    );
}

#[test]
fn closing_and_reopening_keeps_pending_io_single_flight_and_discards_old_epoch() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    let mut sidebar = MapEditorSidebarState::default();
    sidebar.navigator = EditorNavigatorTab::Assets;
    let mut catalog = EditorAssetCatalog::default();
    catalog.current_folder = "gltf".into();
    catalog.requested_revision = 1;
    catalog.applied_revision = 1;
    catalog.entries.push(EditorAssetEntry {
        asset_path: "gltf/test.glb".into(),
        display_name: "test.glb".into(),
        is_directory: false,
        kind: MapAssetKind::Scene,
    });
    catalog.visible_count = 1;
    let mut state = current_state();
    state.identity.as_mut().unwrap().count = 1;
    state.cache.insert(
        "gltf/test.glb".into(),
        CacheEntry {
            source: Ok(source(1)),
            result: ThumbnailResult::Ready(Handle::default()),
            used: 0,
        },
    );
    state.inspection = Some(bevy::tasks::IoTaskPool::get().spawn(async {
        std::thread::sleep(std::time::Duration::from_millis(100));
        InspectionResult {
            epoch: 2,
            sources: vec![("gltf/test.glb".into(), Ok(source(99)))],
        }
    }));
    app.insert_resource(state)
        .insert_resource(catalog)
        .insert_resource(sidebar)
        .insert_resource(EditorSettings {
            asset_browser_view_mode: AssetBrowserViewMode::Thumbnails,
            ..default()
        })
        .init_resource::<crate::resources::MapEditorLayoutState>()
        .insert_resource(MapEditorState {
            open: true,
            ..default()
        })
        .add_systems(Update, sync_demand);
    app.update();
    app.world_mut().resource_mut::<MapEditorState>().open = false;
    app.update();
    assert!(
        app.world()
            .resource::<ThumbnailState>()
            .inspection
            .is_some()
    );
    app.world_mut().resource_mut::<MapEditorState>().open = true;
    app.update();
    assert!(
        app.world()
            .resource::<ThumbnailState>()
            .inspection
            .is_some()
    );
    app.world_mut()
        .resource_mut::<ThumbnailState>()
        .inspect_after = f64::INFINITY;
    for _ in 0..25 {
        app.update();
        if app
            .world()
            .resource::<ThumbnailState>()
            .inspection
            .is_none()
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let state = app.world().resource::<ThumbnailState>();
    assert!(state.inspection.is_none());
    assert_eq!(
        state.cache["gltf/test.glb"].source.as_ref().unwrap().stamp,
        1
    );
    assert!(matches!(
        state.cache["gltf/test.glb"].result,
        ThumbnailResult::Ready(_)
    ));
}

struct Fixture {
    root: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "rpg-thumbnail-test-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        Self { root }
    }
    fn scene(&self) {
        std::fs::write(self.root.join("scene.gltf"), r#"{"asset":{"version":"2.0"},"buffers":[{"uri":"buffer.bin","byteLength":36}],"bufferViews":[{"buffer":0,"byteLength":36}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[-1,-1,0],"max":[1,1,0]}],"materials":[{"pbrMetallicRoughness":{"baseColorFactor":[0.2,0.8,0.3,1],"metallicFactor":0,"roughnessFactor":0.8},"doubleSided":true}],"meshes":[{"primitives":[{"attributes":{"POSITION":0},"material":0}]}],"nodes":[{"mesh":0,"translation":[3,1,2]}],"scenes":[{"nodes":[0]}],"scene":0}"#).unwrap();
        let vertices: [f32; 9] = [-1.0, -1.0, 0.0, 1.0, -1.0, 0.0, 0.0, 1.0, 0.0];
        let bytes: Vec<u8> = vertices.into_iter().flat_map(f32::to_le_bytes).collect();
        std::fs::write(self.root.join("buffer.bin"), bytes).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // 只清理本测试明确创建的三个文件和空目录，不做递归删除。
        for file in ["scene.gltf", "buffer.bin", "texture.png", "cube.ktx2"] {
            let _ = std::fs::remove_file(self.root.join(file));
        }
        let _ = std::fs::remove_file(self.root.join(crate::ui::scene::FONT_PATH));
        for directory in [
            "fonts/open_source/noto_sans/cjk",
            "fonts/open_source/noto_sans",
            "fonts/open_source",
            "fonts",
        ] {
            let _ = std::fs::remove_dir(self.root.join(directory));
        }
        let _ = std::fs::remove_dir(&self.root);
    }
}

#[test]
fn background_stamp_tracks_external_gltf_dependencies_without_reparsing_unchanged_scene() {
    let fixture = Fixture::new();
    fixture.scene();
    let input = InspectionInput {
        path: "scene.gltf".into(),
        kind: MapAssetKind::Scene,
        previous: None,
    };
    let first = super::io::inspect_page(fixture.root.clone(), 1, vec![input]);
    let info = first.sources[0].1.as_ref().unwrap();
    assert_eq!(info.dependencies.as_ref(), &["buffer.bin".to_owned()]);
    std::fs::write(fixture.root.join("buffer.bin"), [0u8; 40]).unwrap();
    let next = super::io::inspect_page(
        fixture.root.clone(),
        1,
        vec![InspectionInput {
            path: "scene.gltf".into(),
            kind: MapAssetKind::Scene,
            previous: Some(info.clone()),
        }],
    );
    let next = next.sources[0].1.as_ref().unwrap();
    assert_eq!(info.primary, next.primary);
    assert_ne!(info.stamp, next.stamp);
    assert!(Arc::ptr_eq(&info.dependencies, &next.dependencies));
}

#[test]
fn background_header_rejects_cube_before_decoding_pixels() {
    let fixture = Fixture::new();
    let mut header = [0u8; 48];
    header[..12].copy_from_slice(b"\xABKTX 20\xBB\r\n\x1A\n");
    header[20..24].copy_from_slice(&32u32.to_le_bytes());
    header[24..28].copy_from_slice(&32u32.to_le_bytes());
    header[36..40].copy_from_slice(&6u32.to_le_bytes());
    std::fs::write(fixture.root.join("cube.ktx2"), header).unwrap();
    let result = super::io::inspect_page(
        fixture.root.clone(),
        1,
        vec![InspectionInput {
            path: "cube.ktx2".into(),
            kind: MapAssetKind::Texture,
            previous: None,
        }],
    );
    assert!(result.sources[0].1.as_ref().unwrap_err().contains("Cube"));
}

/// 手动GPU验收：真实透明纹理与Deferred Scene0均生成非空快照，下一任务不能改旧图片。
#[test]
#[ignore = "需要真实GPU；cargo test -p feathrise_editor_map --lib gpu_thumbnail_snapshots -- --ignored --test-threads=1"]
fn gpu_thumbnail_snapshots_render_texture_and_deferred_model_and_freeze_previous_image() {
    use bevy::asset::AssetPlugin;
    use bevy::core_pipeline::prepass::{DeferredPrepass, DepthPrepass};
    use bevy::ecs::system::RunSystemOnce;
    use bevy::pbr::DefaultOpaqueRendererMethod;
    use bevy::render::{
        gpu_readback::{Readback, ReadbackComplete},
        render_resource::{TextureFormat, TextureUsages},
    };
    use bevy::window::{ExitCondition, WindowPlugin};
    use bevy::winit::WinitPlugin;

    let fixture = Fixture::new();
    fixture.scene();
    let target_font = fixture.root.join(crate::ui::scene::FONT_PATH);
    std::fs::create_dir_all(target_font.parent().unwrap()).unwrap();
    std::fs::write(target_font, feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES).unwrap(); // 真正渲染正式编辑器中文字体，避免截图缺字仍通过。
    image::RgbaImage::from_pixel(64, 32, image::Rgba([220, 70, 20, 255]))
        .save(fixture.root.join("texture.png"))
        .unwrap();
    // 真实地表索引KTX2曾被原样绑定进PBR而崩溃；必须验证实际解码与GPU烘焙。
    let indices: Vec<_> = (0..128).flat_map(|pixel| [if pixel % 16 < 8 { 0u32 } else { 1 }, 2, 3, 0])
        .flat_map(u32::to_le_bytes).collect();
    let (indices, _) = feathrise_editor_assets::ktx2::encode_surface_maps(&indices, &[200, 40, 15, 0].repeat(128), [16, 8], 4).unwrap();
    std::fs::write(fixture.root.join("indices.ktx2"), indices).unwrap();
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(AssetPlugin {
                file_path: fixture.root.to_string_lossy().into_owned(),
                ..default()
            })
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .disable::<WinitPlugin>(),
    )
    .init_resource::<EditorAssetCatalog>()
    .init_resource::<ThumbnailState>()
    .add_systems(
        Update,
        (poll_reloaded_assets, drive_jobs, retire_nodes).chain(),
    )
    .add_systems(
        PostUpdate,
        advance_scene
            .after(TransformSystems::Propagate)
            .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
    )
    .add_observer(world_ready);
    crate::ui::asset_preview::compatible_texture::install(&mut app);
    super::render::install(&mut app);
    while !app.plugins_state().eq(&bevy::app::PluginsState::Ready) {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    app.finish();
    app.cleanup();
    let mut first = None;
    let mut first_bytes = Vec::new();
    for (index, path) in ["texture.png", "scene.gltf", "indices.ktx2"].into_iter().enumerate() {
        if index == 1 {
            app.world_mut()
                .insert_resource(DefaultOpaqueRendererMethod::deferred());
        }
        app.world_mut()
            .run_system_once(
                move |mut commands: Commands,
                      server: Res<AssetServer>,
                      mut images: ResMut<Assets<Image>>,
                      mut state: ResMut<ThumbnailState>,
                      time: Res<Time<Real>>| {
                    ensure_view(&mut commands, &mut state);
                    let camera = state.camera.unwrap();
                    if index == 1 {
                        commands
                            .entity(camera)
                            .insert((DepthPrepass, DeferredPrepass, Msaa::Off));
                    }
                    let mut target = Image::new_target_texture(
                        IMAGE_SIZE,
                        IMAGE_SIZE,
                        TextureFormat::Bgra8UnormSrgb,
                        None,
                    );
                    target.texture_descriptor.usage |= TextureUsages::COPY_SRC; // 仅测试回读，不进入正式缩略图路径。
                    let target = images.add(target);
                    let mut current = job(None);
                    current.path = path.into();
                    current.camera = camera;
                    current.target = target;
                    current.started = time.elapsed_secs_f64();
                    current.phase = JobPhase::Loading;
                    if index != 1 {
                        current.image = Some(Handle::default());
                        current.texture_preview = Some(server.load(path));
                    } else {
                        let scene =
                            server.load(bevy::gltf::GltfAssetLabel::Scene(0).from_asset(path));
                        current.root = Some(
                            commands
                                .spawn((
                                    ThumbnailRoot,
                                    bevy::world_serialization::WorldAssetRoot(scene.clone()),
                                    Transform::default(),
                                    Visibility::Hidden,
                                ))
                                .id(),
                        );
                        current.scene = Some(scene);
                    }
                    state.epoch = 2;
                    state.identity = Some(WindowKey {
                        folder: String::new(),
                        start: 0,
                        count: 12,
                        revision: 1,
                    });
                    state.paths = vec![path.into()];
                    state.cache.insert(
                        path.into(),
                        CacheEntry {
                            source: Ok(source(1)),
                            result: ThumbnailResult::Queued,
                            used: 0,
                        },
                    );
                    state.active = Some(current);
                },
            )
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(45);
        let mut saw_integer_material = false;
        let target = loop {
            app.update();
            let state = app.world().resource::<ThumbnailState>();
            if index == 2 && let Some(job) = &state.active
                && let Some(material) = job.owned_material.as_ref().and_then(|handle| app.world().resource::<Assets<StandardMaterial>>().get(handle))
            {
                let preview = app.world().resource::<Assets<crate::ui::asset_preview::compatible_texture::CompatiblePreviewTexture>>()
                    .get(job.texture_preview.as_ref().unwrap()).unwrap();
                assert!(preview.description.contains("Rgba32Uint"));
                let image = app.world().resource::<Assets<Image>>().get(material.base_color_texture.as_ref().unwrap()).unwrap();
                assert!(crate::ui::asset_preview::compatible_texture::sample_compatible(image));
                assert_eq!(image.texture_descriptor.format, TextureFormat::Rgba8UnormSrgb);
                saw_integer_material = true;
            }
            match &state.cache[path].result {
                ThumbnailResult::Ready(image) => break image.clone(),
                ThumbnailResult::Failed(error) => panic!("{path}: {error}"),
                _ => assert!(
                    std::time::Instant::now() < deadline,
                    "GPU缩略图没有完成：{path}"
                ),
            }
            std::thread::sleep(std::time::Duration::from_millis(16));
        };
        if index == 2 { assert!(saw_integer_material, "真实整数源没有经过安全PBR材质"); }
        let bytes = read_texture(&mut app, target.clone());
        let mut min = UVec2::splat(u32::MAX);
        let mut max = UVec2::ZERO;
        let mut colored = 0;
        for (pixel, color) in bytes.chunks_exact(4).enumerate() {
            let visible = if index == 0 {
                color[2] as u32 > color[1] as u32 * 3 / 2 && color[2] > 40
            } else if index == 1 {
                color[1] as u32 > color[2] as u32 * 6 / 5 && color[1] > 30
            } else {
                color[2] > 100 && color[1] > 50 && color[0] > 50
            };
            if visible {
                let point = UVec2::new(pixel as u32 % IMAGE_SIZE, pixel as u32 / IMAGE_SIZE);
                min = min.min(point);
                max = max.max(point);
                colored += 1;
            }
        }
        assert!(
            colored > 1000,
            "{path}只有清屏或未绘制内容；彩色像素{colored}"
        );
        if index == 0 {
            let size = max - min + UVec2::ONE;
            assert!(
                (size.x as f32 / size.y as f32 - 2.0).abs() < 0.08,
                "纹理比例被拉伸：{size:?}"
            );
            first = Some(target.clone());
            first_bytes = bytes.clone();
        }
        let output =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.local/validation");
        std::fs::create_dir_all(&output).unwrap();
        let mut rgba = bytes;
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        image::RgbaImage::from_raw(IMAGE_SIZE, IMAGE_SIZE, rgba)
            .unwrap()
            .save(output.join(match index {
                0 => "asset-thumbnail-texture.png",
                1 => "asset-thumbnail-model-deferred.png",
                _ => "asset-thumbnail-integer-texture.png",
            }))
            .unwrap();
        for _ in 0..10 {
            app.update();
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
        assert!(app.world().resource::<ThumbnailState>().active.is_none());
        assert!(
            !app.world()
                .get::<Camera>(app.world().resource::<ThumbnailState>().camera.unwrap())
                .unwrap()
                .is_active
        );
    }
    assert_eq!(
        first_bytes,
        read_texture(&mut app, first.unwrap()),
        "第二张截图覆盖了缓存中的第一张图片"
    );

    // 渲染正式侧栏Scene及正式十二行池，导出两种模式画面供人工核对。
    app.init_resource::<EditorSettings>()
        .init_resource::<crate::resources::MapBrowserIndexState>()
        .insert_resource(MapEditorState {
            open: true,
            ..default()
        })
        .add_systems(
            Update,
            (
                crate::ui::rows::spawn_dynamic_rows,
                crate::ui::rows::sync_dynamic_list_layouts,
                crate::ui::rows::sync_asset_rows,
                sync_presentation,
            )
                .chain(),
        );
    {
        let mut catalog = app.world_mut().resource_mut::<EditorAssetCatalog>();
        catalog.entries = vec![
            EditorAssetEntry {
                asset_path: "folder".into(),
                display_name: "预设目录".into(),
                is_directory: true,
                kind: MapAssetKind::Unsupported,
            },
            EditorAssetEntry {
                asset_path: "texture.png".into(),
                display_name: "横向纹理.png".into(),
                is_directory: false,
                kind: MapAssetKind::Texture,
            },
            EditorAssetEntry {
                asset_path: "scene.gltf".into(),
                display_name: "场景模型.gltf".into(),
                is_directory: false,
                kind: MapAssetKind::Scene,
            },
            EditorAssetEntry {
                asset_path: "preset.json".into(),
                display_name: "预设数据.json".into(),
                is_directory: false,
                kind: MapAssetKind::Data,
            },
        ];
    }
    let target = app.world_mut().resource_mut::<Assets<Image>>().add({
        let mut image = Image::new_target_texture(320, 768, TextureFormat::Bgra8UnormSrgb, None);
        image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        image
    });
    let ui_camera = app
        .world_mut()
        .spawn((
            Camera2d,
            Camera {
                order: 20,
                ..default()
            },
            bevy::camera::RenderTarget::from(target.clone()),
        ))
        .id();
    app.world_mut()
        .run_system_once::<_, (), _>((|| crate::ui::sidebars::left_sidebar(&feathrise_editor_documents::scenes())).spawn())
        .unwrap();
    {
        let world = app.world_mut();
        let mut panes = world.query_filtered::<(Entity, &mut Node), With<MapEditorPane>>();
        let pane = panes.iter_mut(world).next().unwrap().0;
        world
            .entity_mut(pane)
            .insert(bevy::ui::UiTargetCamera(ui_camera));
        world.get_mut::<Node>(pane).unwrap().width = percent(100);
        let mut pages = world.query::<(&crate::ui::sidebars::MapEditorSidebarPage, &mut Node)>();
        for (page, mut node) in pages.iter_mut(world) {
            node.display = if page.0
                == crate::ui::sidebars::EditorSidebarTab::Navigator(EditorNavigatorTab::Assets)
            {
                Display::Flex
            } else {
                Display::None
            };
        }
    }
    for (mode, filename) in [
        (
            AssetBrowserViewMode::Thumbnails,
            "asset-browser-thumbnails.png",
        ),
        (AssetBrowserViewMode::List, "asset-browser-list.png"),
    ] {
        app.world_mut()
            .resource_mut::<EditorSettings>()
            .asset_browser_view_mode = mode;
        for _ in 0..90 {
            app.update();
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
        let bytes = read_texture(&mut app, target.clone());
        let mut rgba = bytes;
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        let output = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/validation")
            .join(filename);
        image::RgbaImage::from_raw(320, 768, rgba)
            .unwrap()
            .save(output)
            .unwrap();
    }

    fn read_texture(app: &mut App, target: Handle<Image>) -> Vec<u8> {
        let image = app
            .world()
            .resource::<Assets<Image>>()
            .get(&target)
            .unwrap();
        let bytes_expected = image.width() as usize * image.height() as usize * 4;
        let (sender, receiver) = std::sync::mpsc::channel();
        app.world_mut().spawn(Readback::texture(target)).observe(
            move |event: On<ReadbackComplete>, mut commands: Commands| {
                let _ = sender.send(event.data.clone());
                commands.entity(event.entity).try_despawn();
            },
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            app.update();
            if let Ok(bytes) = receiver.try_recv() {
                assert_eq!(bytes.len(), bytes_expected);
                return bytes;
            }
            assert!(std::time::Instant::now() < deadline, "GPU回读超时");
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
    }
}
