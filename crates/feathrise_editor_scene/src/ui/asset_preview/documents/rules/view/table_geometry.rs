//! 用正式字库与 Bevy 的 Text/布局测量复现长 ID 的错误高度，包含窄栏和 DPI 变化。
use super::*;
use bevy::{
    app::{HierarchyPropagatePlugin, PropagateSet},
    camera::{ComputedCameraValues, RenderTargetInfo},
    ui::{
        ComputedUiRenderTargetInfo, ComputedUiTargetCamera, ui_layout_system,
        ui_surface::UiSurface, update::propagate_ui_target_cameras, widget::measure_text_system,
    },
};

fn rect(world: &World, e: Entity) -> Rect {
    let node = world.get::<ComputedNode>(e).unwrap();
    Rect::from_center_size(
        world.get::<UiGlobalTransform>(e).unwrap().translation,
        node.size(),
    )
}

#[test]
fn rules_table_cells_measure_at_their_actual_column_width() {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        bevy::text::TextPlugin,
        bevy::transform::TransformPlugin,
        HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(PostUpdate),
        HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(PostUpdate),
    ))
    .init_resource::<UiSurface>()
    .init_resource::<UiScale>()
    .init_resource::<Studio>()
    .init_asset::<Image>()
    .init_asset::<bevy::image::TextureAtlasLayout>()
    .configure_sets(
        PostUpdate,
        PropagateSet::<ComputedUiRenderTargetInfo>::default()
            .after(propagate_ui_target_cameras)
            .before(measure_text_system),
    )
    .add_systems(
        PostUpdate,
        (
            propagate_ui_target_cameras,
            measure_text_system.after(bevy::text::detect_text_needs_rerender),
            ui_layout_system,
        )
            .chain(),
    )
    .add_systems(Update, fit);
    let font = app
        .world_mut()
        .resource_mut::<Assets<Font>>()
        .add(Font::from_bytes(
            feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES.to_vec(),
        ));
    app.world_mut().spawn((
        Camera2d,
        Camera {
            computed: ComputedCameraValues {
                target_info: Some(RenderTargetInfo {
                    physical_size: UVec2::new(1800, 1080),
                    scale_factor: 1.,
                }),
                ..default()
            },
            ..default()
        },
    ));
    let root = app
        .world_mut()
        .spawn((
            ScrollArea,
            Node {
                width: px(600),
                ..column(5.)
            },
        ))
        .id();
    app.add_systems(Startup, move |mut commands: Commands| {
        commands.entity(root).with_children(|p| {
            p.spawn(Node {
                height: px(90),
                flex_shrink: 0.,
                ..default()
            });
            p.spawn((TablePart::Summary(0), grid())).with_children(|r| {
                cells(r, &font, Some(0));
                row_actions(r, &font, 0, &Handle::default(), &Handle::default());
            });
        });
    });
    app.update();
    let world = app.world_mut();
    for (cell, mut text) in world.query::<(&Cell, &mut Text)>().iter_mut(world) {
        **text = match cell.column {
            Column::Id => "gate_open".into(),
            Column::Kind => "文本".into(),
            Column::Value => "包含很长默认值的普通文本".repeat(12),
        };
    }
    for (width, scale) in [(600., 1.), (215., 1.5), (760., 1.5)] {
        app.world_mut().get_mut::<Node>(root).unwrap().width = px(width);
        app.world_mut().resource_mut::<UiScale>().0 = scale;
        for _ in 0..4 {
            app.update();
        }
        let world = app.world_mut();
        let cells: Vec<_> = world
            .query::<(Entity, &Cell, &ChildOf)>()
            .iter(world)
            .map(|(e, c, p)| {
                (
                    e,
                    c.column,
                    world.get::<ChildOf>(p.parent()).unwrap().parent(),
                )
            })
            .collect();
        assert_eq!(cells.len(), 3);
        let icons: Vec<_> = world
            .query_filtered::<(Entity, &Name), With<ImageNode>>()
            .iter(world)
            .map(|(entity, name)| (entity, name.as_str().to_owned()))
            .collect();
        let icon = |name| {
            rect(world, icons.iter().find(|(_, id)| id == name).unwrap().0)
        };
        let edit = icon("RulesRowEditIcon");
        let delete = icon("RulesRowDeleteIcon");
        assert!(edit.max.x < delete.min.x);
        assert!((edit.center().y - delete.center().y).abs() <= 1.);
        for (e, column, parent) in cells {
            let text = rect(world, e);
            let row = rect(world, parent);
            assert!(text.height() > 0. && row.contains(text.center()));
            assert!(
                text.min.y >= row.min.y - 1. && text.max.y <= row.max.y + 1.,
                "{width}/{scale} {column:?} {text:?} 越出 {row:?}"
            );
            // 标识和默认值都只占一行；长值由列容器裁剪。
            assert!(text.height() < 2. * metrics::FONT_CONTROL * scale);
            assert!((text.center().y - edit.center().y).abs() <= 1.);
            assert!(text.max.x < edit.min.x);
        }
    }
}
