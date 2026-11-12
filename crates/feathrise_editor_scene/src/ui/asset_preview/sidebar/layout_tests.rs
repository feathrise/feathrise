//! 原生中文字体下资产侧栏的宽度、缩放和固定实体布局回归。

use super::*;
use crate::components::{
    SceneEditorActionButton, SceneEditorAssetFolderText, SceneEditorAssetList, SceneEditorAssetListTitle, SceneEditorAssetRow,
    SceneEditorAssetRowText, SceneEditorLeftPanelScroll, SceneEditorRightPanelScroll,
};
use crate::types::{EditorAction, EditorAssetEntry};
use crate::ui::asset_preview::{
    documents::DocumentSource, presentation::*, resources::AssetPreviewNavigation,
    subasset_presentation::sync_asset_preview_subassets, types::*,
};
use crate::ui::rows::{spawn_dynamic_rows, sync_asset_rows, sync_dynamic_list_layouts};
use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};
use bevy::gltf::{Gltf, GltfMesh};
use bevy::text::EditableText;
use bevy::ui::{
    ComputedUiRenderTargetInfo, ComputedUiTargetCamera, ui_layout_system, ui_surface::UiSurface,
    update::propagate_ui_target_cameras,
};
use feathrise_editor_ui::EditorUiPlugin;
use feathrise_scene_content::SceneAssetKind;
use feathrise_editor_core::EditorSettings;
use feathrise_editor_core::AssetBrowserViewMode;

#[derive(Resource)]
struct TestFont(Handle<Font>);

fn entries(count: usize) -> Vec<EditorAssetEntry> {
    (0..count)
        .map(|index| EditorAssetEntry {
            asset_path: format!("gltf/{}模型{index}.glb", "完整中文目录/".repeat(8)),
            display_name: format!("{}模型{index}.glb", "很长的中文模型名称".repeat(12)),
            is_directory: false,
            kind: SceneAssetKind::Scene,
        })
        .collect()
}

/// 仅运行原Scene、行池及表现桥；字体真实测量，不启动磁盘扫描、模型加载或RTT。
fn page() -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        bevy::scene::ScenePlugin,
        bevy::text::TextPlugin,
        bevy::transform::TransformPlugin,
        EditorUiPlugin,
    ));
    app.init_resource::<UiSurface>()
        .insert_resource(UiScale(1.0))
        .init_resource::<EditorSettings>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<crate::resources::SceneBrowserIndexState>()
        .init_resource::<SceneEditorNumericScrubMouseGuard>()
        .init_resource::<SceneEditorSidebarState>()
        .init_resource::<AssetPreviewNavigation>()
        .init_asset::<Gltf>()
        .init_asset::<GltfMesh>()
        .init_asset::<Image>();
    app.world_mut()
        .resource_mut::<SceneEditorSidebarState>()
        .right = EditorRightSidebarTab::Assets;
    app.world_mut().resource_mut::<SceneEditorSidebarState>().navigator = crate::ui::sidebars::EditorNavigatorTab::Assets;
    let files = entries(13);
    let selected = files[0].asset_path.clone();
    let mut catalog = EditorAssetCatalog::default();
    catalog.current_folder = "gltf".into();
    catalog.entries = files;
    catalog.requested_revision = 1;
    catalog.applied_revision = 1;
    catalog.feedback = Some(AssetCatalogFeedback {
        revision: 1,
        result: Ok(()),
    });
    app.insert_resource(catalog)
        .insert_resource(SceneEditorState {
            open: true,
            selected_asset: Some(selected.clone()),
            ..default()
        });
    let mut model = AssetPreviewModelSession::new(17, Handle::default());
    model.catalog.phase = AssetPreviewCatalogPhase::Ready;
    model.catalog.choices = (0..9)
        .map(|index| AssetPreviewPrimitiveChoice {
            id: AssetPreviewPrimitiveId {
                mesh: index,
                primitive: 0,
            },
            material: Some(index + 1),
            requires_skinning: false,
        })
        .collect();
    model.catalog.status = "完整子资产目录 · 九个静态网格".into();
    let choice = model.catalog.choices[2];
    model.target = AssetPreviewModelTarget::Primitive(choice.id);
    model.selected_primitive = Some(choice);
    app.insert_resource(AssetPreviewState {
        selected_path: Some(selected),
        content: AssetPreviewContent::Model {
            handle: Handle::default(),
            root: Entity::PLACEHOLDER,
            phase: AssetPreviewPhase::Failed,
        },
        status: "加载失败：请检查这个完整的中文资源路径与原材质。".repeat(12),
        model: Some(model),
        ..default()
    });
    app.add_systems(Startup, (
        (|| super::super::scene::assets_page(&feathrise_editor_documents::scenes())).spawn(),
        (|| {
            super::super::scene::asset_properties_page(
                &feathrise_editor_documents::scenes(),
                &feathrise_editor_weights::scenes(),
                &feathrise_editor_animation::scenes(),
            )
        }).spawn(),
    )); // 真实左右两棵子树各建立一次，共同验证相同可用列宽下的字段排版。
    app.add_systems(
        Update,
        (
            spawn_dynamic_rows,
            sync_dynamic_list_layouts,
            sync_asset_rows,
            sync_sidebar,
            sync_asset_preview_presentation,
            sync_asset_preview_controls,
            sync_asset_preview_subassets,
        )
            .chain(),
    );
    crate::ui::asset_thumbnails::install_presentation_test(&mut app);
    app.add_plugins((
        HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(PostUpdate),
        HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(PostUpdate),
    ));
    app.configure_sets(
        PostUpdate,
        PropagateSet::<ComputedUiRenderTargetInfo>::default()
            .after(propagate_ui_target_cameras)
            .before(bevy::ui::widget::measure_text_system),
    );
    app.add_systems(
        PostUpdate,
        (
            propagate_ui_target_cameras,
            bevy::ui::widget::measure_text_system.after(bevy::text::detect_text_needs_rerender),
            ui_layout_system,
        )
            .chain(),
    );
    let font = app
        .world_mut()
        .resource_mut::<Assets<Font>>()
        .add(Font::from_bytes(
            feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES.to_vec(),
        ));
    app.insert_resource(TestFont(font));
    app.world_mut().spawn((
        Camera2d,
        Camera {
            computed: ComputedCameraValues {
                target_info: Some(RenderTargetInfo {
                    physical_size: UVec2::splat(4096),
                    scale_factor: 1.0,
                }),
                ..default()
            },
            viewport: Some(Viewport {
                physical_size: UVec2::splat(4096),
                ..default()
            }),
            ..default()
        },
    ));
    app.update();
    let world = app.world_mut();
    let root = world
        .query_filtered::<Entity, With<SceneEditorRightPanelScroll>>()
        .single(world)
        .unwrap();
    let host = world
        .spawn(Node {
            width: px(340),
            height: px(400),
            min_width: px(0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .id();
    world.entity_mut(root).insert(ChildOf(host));
    world.get_mut::<Node>(root).unwrap().display = Display::Flex;
    let browser = world.query_filtered::<(Entity, &crate::ui::project_browser::ProjectBrowserContent), With<SceneEditorLeftPanelScroll>>()
        .iter(world).find_map(|(entity, view)| (view.0 == crate::ui::project_browser::ProjectBrowserView::Assets).then_some(entity)).unwrap();
    world.entity_mut(browser).insert(ChildOf(host));
    world.get_mut::<Node>(browser).unwrap().display = Display::Flex;
    for mut text in world
        .query_filtered::<&mut Text, With<SceneEditorAssetFolderText>>()
        .iter_mut(world)
    {
        **text = format!("assets/{}/", "需要完整显示的中文目录".repeat(16));
    }
    settle(&mut app);
    (app, host)
}

fn settle(app: &mut App) {
    for _ in 0..6 {
        app.update();
        let font = app.world().resource::<TestFont>().0.clone();
        let world = app.world_mut();
        for mut text in world.query::<&mut TextFont>().iter_mut(world) {
            if text.font != font.clone().into() {
                text.font = font.clone().into();
            }
        }
    }
    app.update();
}

#[test]
fn original_file_primitive_and_native_document_entities_remain_unique() {
    let (mut app, _) = page();
    let world = app.world_mut();
    assert_eq!(
        world
            .query_filtered::<Entity, With<SceneEditorRightPanelScroll>>()
            .iter(world)
            .count(),
        1
    );
    assert!(
        world
            .query_filtered::<Entity, With<SceneEditorAssetRow>>()
            .iter(world)
            .count() >= crate::ui::rows::ASSET_ROW_COUNT,
        "资产保留最小可见池；大视口可以增容，不能再把十二槽当作目录页数"
    );
    assert_eq!(
        world
            .query_filtered::<Entity, With<AssetPreviewPrimitiveRow>>()
            .iter(world)
            .count(),
        5
    );
    assert_eq!(
        world
            .query_filtered::<Entity, With<AssetPreviewSceneRow>>()
            .iter(world)
            .count(),
        1
    );
    assert_eq!(
        world
            .query_filtered::<Entity, (With<DocumentSource>, With<EditableText>)>()
            .iter(world)
            .count(),
        1
    );
    assert_eq!(
        world
            .query_filtered::<Entity, With<AssetPreviewPanel>>()
            .iter(world)
            .count(),
        1
    );
    assert_eq!(
        world
            .query_filtered::<Entity, With<AssetPreviewCanvas>>()
            .iter(world)
            .count(),
        1
    );
    let mut actions: Vec<_> = world
        .query::<&SceneEditorActionButton>()
        .iter(world)
        .map(|button| button.0)
        .collect();
    actions.sort_by_key(|action| format!("{action:?}"));
    assert_eq!(actions.len(), 2);
    for expected in [
        EditorAction::AssetFolderUp,
        EditorAction::RefreshAssets,
    ] {
        assert!(actions.contains(&expected));
    }
    assert!(
        world
            .query_filtered::<Entity, With<AssetPreviewCamera>>()
            .iter(world)
            .next()
            .is_none()
    );
}

#[test]
fn stable_rows_keep_text_generation_when_unrelated_editor_status_changes() {
    let (mut app, _) = page();
    for mode in [AssetBrowserViewMode::List, AssetBrowserViewMode::Thumbnails] {
        app.world_mut()
            .resource_mut::<EditorSettings>()
            .asset_browser_view_mode = mode;
        settle(&mut app);
        let mut labels = app
            .world_mut()
            .query_filtered::<(Entity, Ref<Text>), With<SceneEditorAssetRowText>>();
        let before: Vec<_> = labels
            .iter(app.world())
            .map(|(entity, text)| (entity, text.last_changed()))
            .collect();
        for index in 0..4 {
            app.world_mut().resource_mut::<SceneEditorState>().status =
                format!("其他工具反馈 {index}");
            app.update();
        }
        let after: Vec<_> = labels
            .iter(app.world())
            .map(|(entity, text)| (entity, text.last_changed()))
            .collect();
        assert_eq!(before, after, "列表和卡片名称不能逐帧来回改写 {mode:?}");
    }
}

#[test]
fn both_modes_fit_real_cjk_font_at_four_widths_scales_and_dpi_without_rebuilding() {
    let (mut app, host) = page();
    let world = app.world_mut();
    let refresh = world.query::<(Entity, &SceneEditorActionButton)>().iter(world)
        .find_map(|(entity, action)| (action.0 == EditorAction::RefreshAssets).then_some(entity)).unwrap();
    let thumbnails = world.query_filtered::<Entity, With<crate::ui::asset_thumbnails::ViewModeButton>>()
        .single(world).unwrap();
    let folder = world.query_filtered::<Entity, With<SceneEditorAssetFolderText>>().single(world).unwrap();
    let toolbar = world.get::<ChildOf>(refresh).unwrap().parent();
    assert_eq!(world.get::<ChildOf>(thumbnails).unwrap().parent(), toolbar);
    assert_eq!(world.get::<ChildOf>(folder).unwrap().parent(), world.get::<ChildOf>(toolbar).unwrap().parent());
    let rows: Vec<_> = world
        .query_filtered::<Entity, With<SceneEditorAssetRow>>()
        .iter(world)
        .collect();
    let primitives: Vec<_> = world
        .query_filtered::<Entity, With<AssetPreviewPrimitiveRow>>()
        .iter(world)
        .collect();
    let source = world
        .query_filtered::<Entity, (With<DocumentSource>, With<EditableText>)>()
        .single(world)
        .unwrap();
    let source_parent = world.get::<ChildOf>(source).unwrap().parent();
    let panel = world
        .query_filtered::<Entity, With<AssetPreviewPanel>>()
        .single(world)
        .unwrap();
    let selected = world.resource::<SceneEditorState>().selected_asset.clone();
    let target = world
        .resource::<AssetPreviewState>()
        .model
        .as_ref()
        .unwrap()
        .target;
    for mode in [AssetBrowserViewMode::List, AssetBrowserViewMode::Thumbnails] {
        app.world_mut()
            .resource_mut::<EditorSettings>()
            .asset_browser_view_mode = mode;
        for width in [260.0, 320.0, 420.0, 560.0] {
            app.world_mut().get_mut::<Node>(host).unwrap().width = px(width);
            for scale in [0.6, 1.0, 1.5] {
                app.world_mut().resource_mut::<UiScale>().0 = scale;
                for dpi in [1.0, 2.0] {
                    let world = app.world_mut();
                    for mut camera in world.query::<&mut Camera>().iter_mut(world) {
                        camera.computed.target_info.as_mut().unwrap().scale_factor = dpi;
                    }
                    settle(&mut app);
                    let world = app.world_mut();
                    let size = world.get::<ComputedNode>(host).unwrap().size().x;
                    assert!((size - width * scale * dpi).abs() < 1.1);
                    let center = world.get::<UiGlobalTransform>(host).unwrap().translation.x;
                    let refresh_position = world.get::<UiGlobalTransform>(refresh).unwrap().translation;
                    let thumbnail_position = world.get::<UiGlobalTransform>(thumbnails).unwrap().translation;
                    let thumbnail_size = world.get::<ComputedNode>(thumbnails).unwrap().size();
                    assert!((refresh_position.y - thumbnail_position.y).abs() <= 1.0);
                    assert!((thumbnail_size.x - 22.0 * scale * dpi).abs() <= 1.1);
                    assert!(thumbnail_position.x + thumbnail_size.x / 2.0 <= center + size / 2.0 + 1.1);
                    assert!(rows.iter().all(|entity| world.get::<SceneEditorAssetRow>(*entity).is_some()),
                        "窗口与模式变化可以扩池，不能重建原行实体");
                    let current_rows = world.query_filtered::<Entity, With<SceneEditorAssetRow>>().iter(world).collect::<Vec<_>>();
                    for entity in current_rows.iter().chain(primitives.iter()) {
                        let node = world.get::<Node>(*entity).unwrap();
                        if node.display == Display::None {
                            continue;
                        }
                        let computed = world.get::<ComputedNode>(*entity).unwrap();
                        let position = world
                            .get::<UiGlobalTransform>(*entity)
                            .unwrap()
                            .translation
                            .x;
                        assert!(
                            position - computed.size().x / 2.0 >= center - size / 2.0 - 1.1,
                            "左侧溢出 {width}/{scale}/{dpi}/{mode:?}"
                        );
                        assert!(
                            position + computed.size().x / 2.0 <= center + size / 2.0 + 1.1,
                            "右侧溢出 {width}/{scale}/{dpi}/{mode:?}"
                        );
                    }
                    for (marker, computed, position) in world
                        .query::<(&AssetSidebarText, &ComputedNode, &UiGlobalTransform)>()
                        .iter(world)
                    {
                        if computed.is_empty() {
                            continue;
                        }
                        assert!(
                            position.translation.x + computed.size().x / 2.0
                                <= center + size / 2.0 + 1.1,
                            "完整文件信息溢出 {:?}",
                            marker.0
                        );
                    }
                    for (computed, position) in world.query_filtered::<(&ComputedNode, &UiGlobalTransform), With<SceneEditorAssetFolderText>>().iter(world) {
                        assert!(position.translation.x + computed.size().x / 2.0 <= center + size / 2.0 + 1.1);
                        assert!(position.translation.y - computed.size().y / 2.0 > thumbnail_position.y);
                        assert!(computed.size().y > 20.0 * scale * dpi, "长中文路径应换行");
                    }
                    let copy = world.query_filtered::<(Entity, &ComputedNode, &UiGlobalTransform), With<AssetSidebarCopyPath>>().single(world).unwrap();
                    assert!((copy.1.size().y - 26.0 * scale * dpi).abs() <= 1.1);
                    let copy_text = world.get::<Children>(copy.0).unwrap()[0];
                    assert!(
                        (world
                            .get::<UiGlobalTransform>(copy_text)
                            .unwrap()
                            .translation
                            .y
                            - copy.2.translation.y)
                            .abs()
                            <= 1.1,
                        "按钮文字必须垂直居中"
                    );
                    assert_eq!(
                        world.get::<ChildOf>(source).unwrap().parent(),
                        source_parent
                    );
                    for (layout, computed) in world.query_filtered::<(&TextLayout, &ComputedNode), With<SceneEditorAssetRowText>>().iter(world) {
                        if computed.is_empty() { continue; } // 缩略图需要的槽更少；未绑定隐藏行保留旧样式，不能当成当前可见内容。
                        assert_eq!(layout.linebreak, if mode == AssetBrowserViewMode::List {
                            bevy::text::LineBreak::NoWrap
                        } else { bevy::text::LineBreak::AnyCharacter });
                        assert!(computed.size().x <= size + 1.1);
                    }
                    assert!(world.get::<AssetPreviewPanel>(panel).is_some());
                    assert_eq!(world.resource::<SceneEditorState>().selected_asset, selected);
                    assert_eq!(
                        world
                            .resource::<AssetPreviewState>()
                            .model
                            .as_ref()
                            .unwrap()
                            .target,
                        target
                    );
                }
            }
        }
    }
}

#[test]
fn empty_and_deep_directory_use_full_height_and_low_window_keeps_original_scroll() {
    let (mut app, host) = page();
    let world = app.world_mut();
    let root = world
        .query_filtered::<Entity, With<SceneEditorRightPanelScroll>>()
        .single(world)
        .unwrap();
    let source = world
        .query_filtered::<Entity, (With<DocumentSource>, With<EditableText>)>()
        .single(world)
        .unwrap();
    let browser = world.query::<(Entity, &crate::ui::project_browser::ProjectBrowserContent)>().iter(world)
        .find_map(|(entity, content)| (content.0 == crate::ui::project_browser::ProjectBrowserView::Assets).then_some(entity)).unwrap();
    app.world_mut().get_mut::<Node>(host).unwrap().height = px(160);
    app.world_mut().get_mut::<ScrollPosition>(root).unwrap().y = 49.0;
    for mode in [AssetBrowserViewMode::List, AssetBrowserViewMode::Thumbnails] {
        app.world_mut()
            .resource_mut::<EditorSettings>()
            .asset_browser_view_mode = mode;
        for count in [0_usize, 1, 12, 13, 192] {
            {
                let mut catalog = app.world_mut().resource_mut::<EditorAssetCatalog>();
                catalog.entries = entries(count);
                catalog.requested_revision = catalog.requested_revision.wrapping_add(1);
                catalog.applied_revision = catalog.requested_revision;
            }
            app.world_mut().get_mut::<ScrollPosition>(browser).unwrap().y = 0.;
            settle(&mut app);
            let world = app.world_mut();
            let pool = world
                .query_filtered::<(&Node, &ComputedNode), With<SceneEditorAssetList>>()
                .single(world)
                .unwrap();
            assert_eq!(
                pool.0.display,
                if count == 0 {
                    Display::None
                } else {
                    Display::Flex
                }
            );
            if mode == AssetBrowserViewMode::List {
                let height = count as f32 * feathrise_editor_ui::metrics::ROW_HEIGHT + count.saturating_sub(1) as f32 * 2.0;
                assert_eq!(pool.0.height, px(height));
            } else {
                let grid = crate::ui::asset_thumbnails::grid_layout(pool.1.size().x * pool.1.inverse_scale_factor(), count);
                let rows = count.div_ceil(grid.columns);
                let height = rows as f32 * crate::ui::asset_thumbnails::TILE_HEIGHT
                    + rows.saturating_sub(1) as f32 * crate::ui::asset_thumbnails::TILE_GAP;
                assert_eq!(pool.0.height, px(height), "缩略图滚动高度覆盖完整目录，而非可见池容量");
                if count == 1 { assert!(pool.1.size().y < 160.0, "单文件只保留一行卡片"); }
            }
            let catalog = world.resource::<EditorAssetCatalog>();
            let visible = catalog.visible_count;
            assert!(visible <= count);
            if count == 0 { assert_eq!(visible, 0); } else { assert!(visible > 0); }
            assert_eq!(
                world
                    .query::<(&SceneEditorAssetRow, &Node)>()
                    .iter(world)
                    .filter(|(_, node)| node.display != Display::None)
                    .count(),
                visible
            );
            assert!(world.get::<EditableText>(source).is_some());
            assert_eq!(world.get::<ScrollPosition>(root).unwrap().y, 49.0);
            for text in world.query_filtered::<&Text, With<SceneEditorAssetListTitle>>().iter(world) {
                assert!(!text.contains('页'), "连续目录摘要不能再显示页码：{}", text.0);
            }
            if count > 13 {
                let pool_size = world.query_filtered::<Entity, With<SceneEditorAssetRow>>().iter(world).count();
                assert!(pool_size < count, "目录数据不能各自创建一个UI行实体");
                let viewport = world.get::<ComputedNode>(browser).unwrap();
                let maximum = (viewport.content_size().y - viewport.size().y).max(0.) * viewport.inverse_scale_factor();
                world.get_mut::<ScrollPosition>(browser).unwrap().y = maximum;
                settle(&mut app);
                let world = app.world_mut();
                let item = world.query::<(&SceneEditorAssetRow, &Node, &ComputedNode, &UiGlobalTransform)>().iter(world)
                    .find_map(|(row, node, computed, transform)| (row.entry_index == Some(count - 1) && node.display != Display::None)
                        .then(|| Rect::from_center_size(transform.translation, computed.size())))
                    .expect("滚动到末尾必须绑定原十二槽以外的最后一个文件");
                let viewport = world.get::<ComputedNode>(browser).unwrap();
                let clip = Rect::from_center_size(world.get::<UiGlobalTransform>(browser).unwrap().translation, viewport.size());
                assert!(item.max.y > clip.min.y && item.min.y < clip.max.y,
                    "低窗口最后一项也须实际可见；超高缩略图可部分显示：{item:?}/{clip:?}");
                assert_eq!(world.query_filtered::<Entity, With<SceneEditorAssetRow>>().iter(world).count(), pool_size,
                    "同一视口滚动不能继续增加UI行实体");
                assert_eq!(world.get::<ScrollPosition>(root).unwrap().y, 49.0,
                    "工程目录滚动不得改动独立右侧属性滚动");
            }
        }
    }
}
