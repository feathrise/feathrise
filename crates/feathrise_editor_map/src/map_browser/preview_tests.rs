//! 拖拽提示的真实中文字体、字形布局和鼠标坐标回归，不访问作者地图或开启原生窗口。

use super::{
    presentation::{position_map_browser_drag_preview, sync_map_browser_drag_preview},
    scene::map_browser_drag_preview,
};
use crate::components::{MapBrowserDragPreview, MapBrowserDragPreviewText, MapEditorRoot};
use crate::resources::{MapBrowserDragState, MapEditorState};
use crate::types::{MapBrowserDragPress, MapBrowserItem};
use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};
use bevy::prelude::*;
use bevy::text::TextLayoutInfo;
use bevy::ui::{
    ComputedUiRenderTargetInfo, ComputedUiTargetCamera, FocusPolicy, ui_layout_system,
    ui_surface::UiSurface, update::propagate_ui_target_cameras,
};
use bevy::window::PrimaryWindow;

fn fixture(scale: f32, dpi: f32, offset: Vec2) -> (App, Handle<Font>, Entity, Entity) {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin {
            file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets").into(),
            ..default()
        },
        bevy::scene::ScenePlugin,
        bevy::text::TextPlugin,
        bevy::transform::TransformPlugin,
        bevy::mesh::MeshPlugin,
        bevy::camera::visibility::VisibilityPlugin,
    ))
    .init_resource::<Assets<Image>>()
    .init_resource::<UiSurface>()
    .insert_resource(UiScale(scale))
    .insert_resource(MapEditorState {
        open: true,
        ..default()
    })
    .insert_resource(MapBrowserDragState {
        active: true,
        press: Some(MapBrowserDragPress {
            item: MapBrowserItem::Folder("FOLDER-1".into()),
            document_revision: 1,
            display_name: "测试文件夹".into(),
            start_physical: Vec2::ZERO,
        }),
        ..default()
    })
    .add_plugins(HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(
        PostUpdate,
    ))
    .add_plugins(HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(
        PostUpdate,
    ));
    app.configure_sets(
        PostUpdate,
        PropagateSet::<ComputedUiTargetCamera>::default()
            .after(propagate_ui_target_cameras)
            .before(bevy::ui::widget::measure_text_system),
    );
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
            bevy::ui::widget::measure_text_system
                .after(bevy::text::detect_text_needs_rerender)
                .after(bevy::text::load_font_assets_into_font_collection),
            ui_layout_system,
            bevy::ui::widget::text_system,
            position_map_browser_drag_preview,
        )
            .chain(),
    );
    app.add_systems(Startup, map_browser_drag_preview.spawn())
        .add_systems(Update, sync_map_browser_drag_preview);
    let font = app
        .world_mut()
        .resource_mut::<Assets<Font>>()
        .add(Font::from_bytes(
            feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES.to_vec(),
        ));
    let mut window = Window::default();
    window.resolution.set_physical_resolution(1920, 1080);
    window.resolution.set_scale_factor_override(Some(dpi));
    window.set_physical_cursor_position(Some(Vec2::new(100.0, 100.0).into()));
    let window = app.world_mut().spawn((window, PrimaryWindow)).id();
    app.world_mut().spawn((
        Camera2d,
        Camera {
            computed: ComputedCameraValues {
                target_info: Some(RenderTargetInfo {
                    physical_size: UVec2::new(1920, 1080),
                    scale_factor: dpi,
                }),
                ..default()
            },
            viewport: Some(Viewport {
                physical_size: UVec2::new(1920, 1080),
                ..default()
            }),
            ..default()
        },
    ));
    let root = app
        .world_mut()
        .spawn((
            MapEditorRoot,
            Node {
                position_type: PositionType::Absolute,
                left: px(offset.x),
                top: px(offset.y),
                width: px(1920.0 / (scale * dpi) - offset.x * 2.0),
                height: px(1080.0 / (scale * dpi) - offset.y * 2.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
        ))
        .id();
    app.update();
    let world = app.world_mut();
    let popup = world
        .query_filtered::<Entity, With<MapBrowserDragPreview>>()
        .single(world)
        .unwrap();
    world.entity_mut(popup).insert(ChildOf(root));
    for mut text in world.query::<&mut TextFont>().iter_mut(world) {
        text.font = font.clone().into();
    }
    settle(&mut app);
    (app, font, window, popup)
}

fn settle(app: &mut App) {
    for _ in 0..5 {
        app.update();
    }
}

fn bounds(app: &App, entity: Entity) -> Rect {
    Rect::from_center_size(
        app.world()
            .get::<UiGlobalTransform>(entity)
            .unwrap()
            .translation,
        app.world().get::<ComputedNode>(entity).unwrap().size(),
    )
}

fn move_cursor(app: &mut App, window: Entity, position: Vec2) {
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_physical_cursor_position(Some(position.into()));
    app.update(); // 光标与长文本变化必须在本帧完成定位，不能靠后续帧修正。
}

fn assert_text_is_visible(app: &mut App, popup: Entity) {
    let world = app.world_mut();
    let (entity, label, text, node, focus) = world
        .query_filtered::<
            (Entity, &Text, &TextLayoutInfo, &ComputedNode, &FocusPolicy),
            With<MapBrowserDragPreviewText>,
        >()
        .single(world)
        .unwrap();
    assert!(label.as_str().contains("文件夹"));
    assert!(
        !text.glyphs.is_empty(),
        "必须生成真实字形，不能只检查空文本节点"
    );
    assert!(node.size().min_element() > 0.0, "文字不能被压缩成零宽");
    assert!(
        (text.size * text.scale_factor)
            .cmple(node.size() + Vec2::splat(1.1))
            .all()
    );
    assert_eq!(*focus, FocusPolicy::Pass);
    let text_rect = bounds(app, entity);
    let popup_rect = bounds(app, popup);
    assert!(
        popup_rect.contains(text_rect.min) && popup_rect.contains(text_rect.max),
        "文字必须完整留在提示框内"
    );
    assert_eq!(
        *app.world().get::<FocusPolicy>(popup).unwrap(),
        FocusPolicy::Pass
    );
}

#[test]
fn mixed_batch_preview_renders_counts_and_rejection_at_all_ui_scales() {
    for (scale, dpi) in [
        (0.6, 1.0),
        (1.0, 1.0),
        (1.5, 1.0),
        (0.6, 2.0),
        (1.0, 2.0),
        (1.5, 2.0),
    ] {
        let (mut app, _, window, popup) = fixture(scale, dpi, Vec2::ZERO);
        app.world_mut()
            .resource_mut::<MapBrowserDragState>()
            .selection = Some(crate::types::MapBrowserSelection {
            items: std::sync::Arc::new(
                [
                    MapBrowserItem::Map(feathrise_map_content::MapId::parse("MAP-1").unwrap()),
                    MapBrowserItem::Map(feathrise_map_content::MapId::parse("MAP-2").unwrap()),
                    MapBrowserItem::Map(feathrise_map_content::MapId::parse("MAP-3").unwrap()),
                    MapBrowserItem::Folder("FOLDER-1".into()),
                    MapBrowserItem::Folder("FOLDER-2".into()),
                ]
                .into(),
            ),
            maps: 3,
            folders: 2,
            ..default()
        });
        app.world_mut()
            .resource_mut::<MapBrowserDragState>()
            .drop_error = Some("不能把所选文件夹移入自身或子文件夹".into());
        settle(&mut app);
        move_cursor(&mut app, window, Vec2::new(1000.0, 650.0));
        assert_text_is_visible(&mut app, popup);
        let world = app.world_mut();
        let label = world
            .query_filtered::<&Text, With<MapBrowserDragPreviewText>>()
            .single(world)
            .unwrap();
        assert!(label.as_str().contains("3 张地图、2 个文件夹"));
        assert!(label.as_str().contains("无法放置"));
    }
}

#[test]
fn preview_renders_text_and_stays_near_cursor_across_ui_scales_dpi_and_root_offsets() {
    for (scale, dpi) in [
        (1.0, 1.0),
        (1.25, 1.0),
        (1.5, 1.0),
        (0.6, 2.0),
        (1.25, 1.25),
        (1.5, 2.0),
    ] {
        let (mut app, _font, window, popup) = fixture(scale, dpi, Vec2::new(12.0, 8.0));
        for cursor in [
            Vec2::new(100.0, 100.0),
            Vec2::new(650.0, 400.0),
            Vec2::new(1200.0, 750.0),
        ] {
            move_cursor(&mut app, window, cursor);
            assert_text_is_visible(&mut app, popup);
            let rect = bounds(&app, popup);
            assert!(rect.width() > rect.height(), "短提示不能缩成空方框");
            let horizontal = if rect.min.x > cursor.x {
                rect.min.x - cursor.x
            } else {
                cursor.x - rect.max.x
            };
            let vertical = if rect.min.y > cursor.y {
                rect.min.y - cursor.y
            } else {
                cursor.y - rect.max.y
            };
            assert!(
                (horizontal - 12.0 * dpi).abs() <= 1.1,
                "水平漂移 scale={scale} dpi={dpi} cursor={cursor:?} rect={rect:?}"
            );
            assert!(
                (vertical - 12.0 * dpi).abs() <= 1.1,
                "垂直漂移 scale={scale} dpi={dpi} cursor={cursor:?} rect={rect:?}"
            );
        }
    }
}

#[test]
fn long_preview_and_rejection_wrap_inside_root_and_disappear_after_release() {
    let (mut app, _font, window, popup) = fixture(1.25, 2.0, Vec2::ZERO);
    {
        let mut drag = app.world_mut().resource_mut::<MapBrowserDragState>();
        drag.press.as_mut().unwrap().display_name = "完整中文文件夹名称".repeat(6);
        drag.drop_error = Some("不能把文件夹移入自身或子文件夹".into());
    }
    move_cursor(&mut app, window, Vec2::new(1880.0, 1050.0));
    assert_text_is_visible(&mut app, popup);
    let world = app.world_mut();
    let root = world
        .query_filtered::<Entity, With<MapEditorRoot>>()
        .single(world)
        .unwrap();
    let root_rect = bounds(&app, root);
    let popup_rect = bounds(&app, popup);
    assert!(root_rect.contains(popup_rect.min) && root_rect.contains(popup_rect.max));
    app.world_mut().resource_mut::<MapBrowserDragState>().active = false;
    app.update();
    assert_eq!(
        app.world().get::<Node>(popup).unwrap().display,
        Display::None
    );
}
