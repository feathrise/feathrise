//! 用正式场景目录 BSN、行池和中性目录夹具生成离屏截图，不依赖原游戏场景。

use super::*;
use crate::{components::*, resources::*, types::*};
use bevy::render::{
    gpu_readback::{Readback, ReadbackComplete},
    render_resource::{TextureFormat, TextureUsages},
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[test]
#[ignore = "显式GPU渲染真实场景目录，不打开或接管用户的原生窗口"]
fn scene_directory_gpu_snapshot() {
    let (mut app, window) = page_with_render(true);
    // 使用同一正式格式构造有限目录；公开测试不读取开发者的游戏内容或修改用户场景。
    let catalog: SceneCatalogAsset = serde_json::from_value(serde_json::json!({
        "schema_version": 2,
        "scenes": [
            {"id": "FIXTURE-A", "display_name": "测试平原", "folder": "scenes/FIXTURE-A", "published": true},
            {"id": "FIXTURE-B", "display_name": "测试湖区", "folder": "scenes/FIXTURE-B", "published": false}
        ]
    })).unwrap();
    let document = crate::scene_browser::decode_folder_document(
        &serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "folders": [{"id": "FIXTURE-FOLDER", "display_name": "测试区域", "parent_id": null}],
            "scene_folders": {"FIXTURE-A": "FIXTURE-FOLDER", "FIXTURE-B": "FIXTURE-FOLDER"}
        })).unwrap(),
    )
    .unwrap();
    let current = catalog.scenes.last().unwrap().id.clone();
    let entries = crate::scene_browser::build_browser_tree(
        document.folders.clone(),
        catalog
            .scenes
            .iter()
            .map(|map| SceneBrowserEntry {
                item: SceneBrowserItem::Scene(map.id.clone()),
                display_name: map.display_name.clone(),
                published: map.published,
                depth: 1,
                parent_folder_id: document.scene_folders.get(map.id.as_str()).cloned(),
            })
            .collect(),
    );
    let signature = SceneBrowserIndexSignature {
        folder_revision: 1,
        catalog_revision: 1,
    };
    let mut browser = SceneBrowserState {
        initial_load_resolved: true,
        document_revision: 1,
        write_blocked: None,
        ..default()
    };
    browser.expanded_folder_ids = document
        .folders
        .iter()
        .map(|folder| folder.id.clone())
        .collect();
    browser.document = document;
    browser.select_only(Some(SceneBrowserItem::Scene(current.clone())));
    app.insert_resource(browser)
        .insert_resource(SceneBrowserIndexState {
            total_entries: entries.len(),
            full_entries: Arc::new(entries.clone()),
            entries: Arc::new(entries),
            applied_signature: Some(signature.clone()),
            desired_signature: Some(signature.clone()),
            applied_projection: Some((signature.clone(), 0)),
            desired_projection: Some((signature, 0)),
            ..default()
        })
        .init_resource::<SceneBrowserDragState>()
        .init_resource::<SceneFolderPersistence>()
        .init_resource::<SceneCreationTask>()
        .init_resource::<SceneDeletionTask>()
        .init_resource::<EditorAssetCatalog>()
        .init_resource::<Assets<SceneCatalogAsset>>()
        .init_resource::<Assets<SceneManifestAsset>>();
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_scene_id = Some(current);
    app.world_mut()
        .resource_mut::<SceneEditorSidebarState>()
        .navigator = crate::ui::sidebars::EditorNavigatorTab::Scenes;
    let catalog = app
        .world_mut()
        .resource_mut::<Assets<SceneCatalogAsset>>()
        .add(catalog);
    app.insert_resource(SceneContentRegistry {
        catalog,
        revision: 1,
        scenes: default(),
        last_error: None,
    });
    crate::ui::scene_sidebar::install(&mut app);
    crate::ui::scene_sidebar::install_snapshot(&mut app);
    app.add_systems(
        Update,
        (
            crate::ui::rows::spawn_dynamic_rows,
            crate::ui::rows::sync_dynamic_list_layouts,
            crate::ui::rows::sync_scene_rows,
            crate::scene_browser::sync_scene_browser_root_row,
        )
            .chain()
            .in_set(crate::SceneEditorSystemSet::Presentation),
    );
    crate::ui::theme::install(&mut app);
    app.world_mut()
        .resource_mut::<feathrise_editor_core::EditorSettings>()
        .editor_color_theme = feathrise_editor_core::EditorColorTheme::NeutralGray;
    let (camera, target) = {
        let world = app.world_mut();
        let old = world
            .query_filtered::<Entity, With<ListHost>>()
            .single(world)
            .unwrap();
        let old = world.get::<ChildOf>(old).unwrap().parent();
        world.entity_mut(old).despawn();
        let (camera, target) = world
            .query::<(Entity, &bevy::camera::RenderTarget)>()
            .single(world)
            .unwrap();
        let bevy::camera::RenderTarget::Image(target) = target else {
            panic!("目录截图必须使用独立离屏图像")
        };
        (camera, target.handle.clone())
    };
    let page = app
        .world_mut()
        .spawn_scene(crate::ui::scene_sidebar::scene_page())
        .unwrap()
        .id();
    let host = app
        .world_mut()
        .spawn((
            SceneEditorRoot, // 菜单固定挂在根下，目录正文不再创建另一排常驻操作。
            feathrise_editor_ui::EditorThemeRoot,
            bevy::ui::UiTargetCamera(camera),
            Node {
                width: percent(100),
                height: percent(100),
                min_width: px(0),
                padding: UiRect::all(px(6)),
                ..default()
            },
            BackgroundColor(crate::ui::theme::PANEL_BG),
        ))
        .id();
    app.world_mut().entity_mut(page).insert(ChildOf(host));
    app.world_mut().get_mut::<Camera>(camera).unwrap().viewport = None;
    let font = app.world_mut().resource_mut::<Assets<Font>>().add(Font::from_bytes(
        feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES.to_vec(),
    ));
    let output =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.local/validation");
    std::fs::create_dir_all(&output).unwrap();
    let (menu_scene, menu_folder) = {
        let index = app.world().resource::<SceneBrowserIndexState>();
        (
            index
                .entries
                .iter()
                .find(|entry| matches!(entry.item, SceneBrowserItem::Scene(_)))
                .expect("真实目录应有场景供菜单验收")
                .item
                .clone(),
            index
                .entries
                .iter()
                .find(|entry| matches!(entry.item, SceneBrowserItem::Folder(_)))
                .expect("真实目录应有文件夹供菜单验收")
                .item
                .clone(),
        )
    };
    for (width, height, scale, context) in [
        (360, 520, 1.0, None),
        (232, 600, 1.0, None),
        (360, 650, 1.25, None),
        (360, 520, 1.0, Some(("scene", menu_scene))),
        (360, 520, 1.0, Some(("folder", menu_folder))),
    ] {
        let mut image =
            Image::new_target_texture(width, height, TextureFormat::Rgba8UnormSrgb, None);
        image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        app.world_mut()
            .resource_mut::<Assets<Image>>()
            .insert(target.id(), image)
            .unwrap();
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .resolution
            .set(width as f32, height as f32);
        app.world_mut().resource_mut::<UiScale>().0 = scale;
        if let Some((_, item)) = context.as_ref() {
            app.world_mut()
                .resource_mut::<SceneBrowserState>()
                .select_only(Some(item.clone()));
        }
        {
            let mut snapshot = app
                .world_mut()
                .resource_mut::<crate::ui::scene_sidebar::DirectoryMenuSnapshot>();
            snapshot.active = context.is_some();
            snapshot.anchor = Vec2::new(width as f32, height as f32) / scale - Vec2::splat(3.0);
        }
        for _ in 0..20 {
            let world = app.world_mut();
            for mut text in world.query::<&mut TextFont>().iter_mut(world) {
                text.font = font.clone().into();
            }
            app.update();
            std::thread::sleep(Duration::from_millis(16));
        }
        let icons = app
            .world()
            .resource::<crate::ui::scene_sidebar::SceneBrowserTreeIcons>();
        let images = app.world().resource::<Assets<Image>>();
        assert!(
            [
                &icons.folder,
                &icons.scene,
                &icons.root,
                &icons.expand,
                &icons.collapse
            ]
            .iter()
            .all(|icon| images.contains(icon.id())),
            "目录五类图标必须实际加载"
        );
        let world = app.world_mut();
        if let Some((_, item)) = context.as_ref() {
            crate::ui::scene_sidebar::assert_snapshot_bounds(world, host, item);
        }
        assert_eq!(
            world
                .query_filtered::<Entity, With<SceneEditorSceneRow>>()
                .iter(world)
                .count(),
            crate::constants::SCENE_BROWSER_ROW_POOL_COUNT
        );
        assert_eq!(
            world
                .query_filtered::<Entity, With<crate::ui::scene_sidebar::SceneSidebarActionBinding>>()
                .iter(world)
                .count(),
            5,
            "正式工作台只能保留浮层中的五个原操作，不能复制另一排按钮"
        );
        assert!(
            world
                .query_filtered::<&Text, With<SceneEditorSceneRowText>>()
                .iter(world)
                .all(|text| !text.contains("[MAP-") && !text.contains("[目录]"))
        );
        let (sender, receiver) = std::sync::mpsc::channel();
        world.spawn(Readback::texture(target.clone())).observe(
            move |event: On<ReadbackComplete>, mut commands: Commands| {
                let _ = sender.send(event.data.clone());
                commands.entity(event.entity).try_despawn();
            },
        );
        let started = Instant::now();
        let bytes = loop {
            app.update();
            if let Ok(bytes) = receiver.try_recv() {
                break bytes;
            }
            assert!(started.elapsed().as_secs() < 10);
            std::thread::sleep(Duration::from_millis(16));
        };
        // GPU复制行按256字节对齐；窄栏宽度不是64的倍数，保存前必须去掉每行尾部填充。
        let stride = width as usize * 4;
        let padded_stride = stride.div_ceil(256) * 256;
        let pixels = bytes
            .chunks_exact(padded_stride)
            .take(height as usize)
            .flat_map(|row| row[..stride].iter().copied())
            .collect();
        let filename = context.as_ref().map_or_else(
            || format!("map-directory-{width}-{scale}.png"),
            |(kind, _)| format!("map-directory-menu-{kind}.png"),
        );
        image::RgbaImage::from_raw(width, height, pixels)
            .unwrap()
            .save(output.join(filename))
            .unwrap();
    }
}
