//! 使用正式PBR网格、元素Gizmo和中文视口工具栏验收点边面编辑，不启动原生窗口或读写作者地图。

use feathrise_editor_ui::InteractionButton;
use super::{geometry::world_vertices, render::draw_elements, resources::MeshEditState, types::*};
use crate::{
    components::{MapEditorRoot, MapEditorViewport},
    resources::{MapEditorState, MapEditorViewportState},
    viewport::{
        MapEditorBrushGizmos, configure_map_editor_brush_gizmos,
        modes::{MeshEditToolbarRoot, ViewportModeState, install_snapshot_controls},
        placement_move::MoveCamera,
    },
};
use bevy::{
    camera::visibility::RenderLayers,
    prelude::*,
    render::{
        gpu_readback::{Readback, ReadbackComplete},
        render_resource::{TextureFormat, TextureUsages},
    },
    window::ExitCondition,
    winit::WinitPlugin,
};
use feathrise_map_content::{MapId, MapPlacementAssetTarget, MapPlacementRecord};

const WIDTH: u32 = 1_280; // BGRA行宽保持256字节对齐，回读无额外行填充。
const HEIGHT: u32 = 720;

fn frames(app: &mut App, count: usize) {
    for _ in 0..count {
        app.update();
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}

fn snapshot(app: &mut App, target: Handle<Image>, name: &str) -> Vec<u8> {
    let (sender, receiver) = std::sync::mpsc::channel();
    app.world_mut().spawn(Readback::texture(target)).observe(
        move |event: On<ReadbackComplete>, mut commands: Commands| {
            let _ = sender.send(event.data.clone());
            commands.entity(event.entity).try_despawn();
        },
    );
    let started = std::time::Instant::now();
    let mut bytes = loop {
        app.update();
        if let Ok(bytes) = receiver.try_recv() {
            break bytes;
        }
        assert!(started.elapsed().as_secs() < 15, "点边面GPU快照回读超时");
        std::thread::sleep(std::time::Duration::from_millis(16));
    };
    assert_eq!(bytes.len(), (WIDTH * HEIGHT * 4) as usize);
    for pixel in bytes.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/mesh-edit-validation");
    std::fs::create_dir_all(&directory).unwrap();
    image::RgbaImage::from_raw(WIDTH, HEIGHT, bytes.clone())
        .unwrap()
        .save(directory.join(name))
        .unwrap();
    bytes
}

fn assert_toolbar(app: &mut App, kind: MeshElementKind) {
    let world = app.world_mut();
    let root = world
        .query_filtered::<Entity, With<MeshEditToolbarRoot>>()
        .single(world)
        .unwrap();
    let root_rect = Rect::from_center_size(
        world.get::<UiGlobalTransform>(root).unwrap().translation,
        world.get::<ComputedNode>(root).unwrap().size(),
    );
    assert!(root_rect.min.x >= 0.0 && root_rect.max.x <= WIDTH as f32);
    assert!(root_rect.min.y >= 0.0 && root_rect.max.y <= HEIGHT as f32);
    let mut labels = Vec::new();
    let mut selection_caption = false;
    for button in world.get::<Children>(root).unwrap().iter() {
        let node = world.get::<ComputedNode>(button).unwrap();
        if node.size().min_element() <= 0.0 {
            continue;
        }
        let rect = Rect::from_center_size(
            world.get::<UiGlobalTransform>(button).unwrap().translation,
            node.size(),
        );
        assert!(rect.min.x >= root_rect.min.x - 1.0 && rect.max.x <= root_rect.max.x + 1.0);
        assert!(rect.min.y >= root_rect.min.y - 1.0 && rect.max.y <= root_rect.max.y + 1.0);
        if world.get::<InteractionButton>(button).is_none() {
            // 正式只读计数与按钮并列；需要检查其字形和真实回显，不能假设它也有按钮子树。
            let text = world.get::<Text>(button).unwrap();
            assert_eq!(text.as_str(), format!("已选 1 {}", kind.label()));
            assert!(
                !world
                    .get::<bevy::text::TextLayoutInfo>(button)
                    .unwrap()
                    .glyphs
                    .is_empty()
            );
            selection_caption = true;
            continue;
        }
        let text_entity = world
            .get::<Children>(button)
            .unwrap()
            .iter()
            .next()
            .unwrap();
        let text = world.get::<Text>(text_entity).unwrap();
        let layout = world
            .get::<bevy::text::TextLayoutInfo>(text_entity)
            .unwrap();
        assert!(
            !layout.glyphs.is_empty(),
            "正式中文工具栏必须加载并绘制字形"
        );
        assert!(layout.size.x * layout.scale_factor <= node.size().x + 1.1);
        if text.as_str()
            == format!(
                "{} {}",
                kind.label(),
                match kind {
                    MeshElementKind::Vertex => 1,
                    MeshElementKind::Edge => 2,
                    MeshElementKind::Face => 3,
                }
            )
        {
            assert_eq!(
                world.get::<BackgroundColor>(button).unwrap().0,
                crate::ui::theme::SELECT_BG,
                "真实工具栏必须回显当前点边面状态"
            );
        }
        labels.push(text.as_str().to_owned());
    }
    assert!(selection_caption, "正式工具栏必须回显实际元素选择计数");
    let mut expected = vec!["返回对象 Tab", "点 1", "边 2", "面 3", "全选 A", "框选 B"];
    if kind == MeshElementKind::Face { expected.push("挤出 E"); }
    expected.push("更多");
    assert_eq!(labels, expected);
}

fn orange_pixels_near(bytes: &[u8], point: Vec2, radius: i32) -> usize {
    let mut count = 0;
    for y in (point.y as i32 - radius).max(0)..=(point.y as i32 + radius).min(HEIGHT as i32 - 1) {
        for x in (point.x as i32 - radius).max(0)..=(point.x as i32 + radius).min(WIDTH as i32 - 1)
        {
            let offset = ((y as u32 * WIDTH + x as u32) * 4) as usize;
            let pixel = &bytes[offset..offset + 3];
            if pixel[0] > 150 && pixel[1] > 55 && pixel[1] < 220 && pixel[2] < 110 {
                count += 1;
            }
        }
    }
    count
}

#[test]
#[ignore = "需要真实GPU；点边面中文工具栏、变形PBR网格和正式选择覆盖离屏验收"]
fn mesh_element_vertex_edge_face_gpu_snapshots() {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .set(AssetPlugin {
                file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets").into(),
                watch_for_changes_override: Some(false),
                ..default()
            })
            .disable::<WinitPlugin>()
            .disable::<bevy::log::LogPlugin>(),
    )
    .add_plugins(feathrise_editor_ui::EditorThemePlugin)
    .init_gizmo_group::<MapEditorBrushGizmos>()
    .insert_resource(ClearColor(Color::srgb(0.075, 0.09, 0.11)))
    .insert_resource(MapEditorState {
        open: true,
        ..default()
    })
    .init_resource::<ViewportModeState>()
    .init_resource::<MeshEditState>()
    .add_systems(Startup, configure_map_editor_brush_gizmos)
    .add_systems(Update, draw_elements);
    install_snapshot_controls(&mut app);
    crate::ui::theme::install(&mut app);
    app.finish();
    app.cleanup();

    let target = app.world_mut().resource_mut::<Assets<Image>>().add({
        let mut image =
            Image::new_target_texture(WIDTH, HEIGHT, TextureFormat::Bgra8UnormSrgb, None);
        image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        image
    });
    let camera = app
        .world_mut()
        .spawn((
            Camera3d::default(),
            bevy::camera::RenderTarget::from(target.clone()),
            RenderLayers::from_layers(&[0, crate::viewport::MAP_EDITOR_BRUSH_RENDER_LAYER]),
            AmbientLight {
                brightness: 180.0,
                ..default()
            },
            Msaa::Sample4,
        ))
        .id();
    let map = MapId::parse("MAP-mesh-edit-gpu").unwrap();
    let viewport = MapEditorViewportState {
        _image: target.clone(),
        camera,
        center: Vec3::new(0.0, 1.2, 0.0),
        scene_center: Vec3::Y,
        scene_half_extents: Vec3::new(3.0, 3.0, 3.0),
        extent: 8.0,
        yaw: 0.65,
        pitch: 0.42,
        framed_map_id: Some(map.clone()),
        render_size: UVec2::new(WIDTH, HEIGHT),
    };
    // 与正式MoveCamera共享pose，覆盖和PBR绝不能分别使用一组看似接近的测试矩阵。
    let (projection, transform) = crate::viewport::editor_camera_pose(false, &viewport);
    app.world_mut()
        .entity_mut(camera)
        .insert((projection, transform));
    app.insert_resource(viewport);
    app.world_mut().spawn((
        MapEditorRoot,
        MapEditorViewport,
        bevy::ui::UiTargetCamera(camera),
        Node {
            width: px(WIDTH as f32),
            height: px(HEIGHT as f32),
            ..default()
        },
    ));

    let mut asset_target = MapPlacementAssetTarget::DEFAULT_CUBOID
        .into_editable()
        .unwrap();
    let MapPlacementAssetTarget::EditableCuboid {
        ref mut vertices_half_mm,
        ..
    } = asset_target
    else {
        unreachable!()
    };
    vertices_half_mm[7][1] += 1_600; // 把一个共享角点上移0.8m，三角形法线必须真实反映非平面顶面。
    assert!(asset_target.validate().is_ok());
    let record: MapPlacementRecord = serde_json::from_value(serde_json::json!({
        "id":"mesh-edit-gpu", "asset_path":"", "asset_target":asset_target,
        "translation":[0,1,0], "yaw_degrees":0, "scale":[1,1,1],
    }))
    .unwrap();
    let vertices = world_vertices(&record).unwrap();
    let mesh = app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(feathrise_scene::editable_cuboid_mesh(
            asset_target.editable_vertices_half_mm().unwrap(),
        ));
    let material = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial {
            base_color: Color::srgb(0.55, 0.61, 0.69),
            perceptual_roughness: 0.7,
            ..default()
        });
    app.world_mut().spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::from_xyz(0.0, 1.0, 0.0),
    ));
    let ground = app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(Plane3d::default().mesh().size(20.0, 20.0));
    let ground_material = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial {
            base_color: Color::srgb(0.18, 0.21, 0.25),
            perceptual_roughness: 1.0,
            ..default()
        });
    app.world_mut().spawn((
        Mesh3d(ground),
        MeshMaterial3d(ground_material),
        Transform::from_xyz(0.0, -0.005, 0.0),
    ));
    app.world_mut().spawn((
        DirectionalLight {
            illuminance: 8_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 5.0).looking_at(Vec3::Y, Vec3::Y),
    ));
    app.world_mut().resource_mut::<MeshEditState>().target = Some(EditTarget {
        map_id: map,
        index: 0,
        author_revision: 0,
        structure_revision: 0,
        record,
    });
    frames(&mut app, 45);
    let mut snapshots = Vec::new();
    for (kind, element, name, point) in [
        (
            MeshElementKind::Vertex,
            7,
            "mesh-edit-vertex.png",
            vertices[7],
        ),
        (
            MeshElementKind::Edge,
            3,
            "mesh-edit-edge.png",
            (vertices[6] + vertices[7]) * 0.5,
        ),
        (
            MeshElementKind::Face,
            5,
            "mesh-edit-face.png",
            (vertices[4] + vertices[5] + vertices[7]) / 3.0,
        ),
    ] {
        {
            let mut state = app.world_mut().resource_mut::<MeshEditState>();
            state.kind = kind;
            state.selected = 1 << element;
            state.hover = None;
        }
        frames(&mut app, 5);
        assert_toolbar(&mut app, kind);
        let pixel = {
            let editor = app.world().resource::<MapEditorState>();
            let viewport = app.world().resource::<MapEditorViewportState>();
            let camera = MoveCamera::new(editor, viewport, 1.0).unwrap();
            (camera.project(point).unwrap() + Vec2::splat(0.5))
                * Vec2::new(WIDTH as f32, HEIGHT as f32)
        };
        let bytes = snapshot(&mut app, target.clone(), name);
        assert!(
            orange_pixels_near(&bytes, pixel, 12) > 5,
            "{}选择必须由正式Gizmo绘制在真实几何投影处，不能只有工具栏状态",
            kind.label()
        );
        if kind == MeshElementKind::Face {
            let boundary = {
                let editor = app.world().resource::<MapEditorState>();
                let viewport = app.world().resource::<MapEditorViewportState>();
                let camera = MoveCamera::new(editor, viewport, 1.0).unwrap();
                (camera.project((vertices[6] + vertices[7]) * 0.5).unwrap() + Vec2::splat(0.5))
                    * Vec2::new(WIDTH as f32, HEIGHT as f32)
            };
            assert!(
                orange_pixels_near(&bytes, boundary, 8) > 5,
                "面选择必须显示橙色真实边界，不能只留下中心小标记"
            );
        }
        snapshots.push(bytes);
    }
    for pair in snapshots.windows(2) {
        let changed = pair[0]
            .chunks_exact(4)
            .zip(pair[1].chunks_exact(4))
            .enumerate()
            .filter(|(index, (a, b))| *index >= (WIDTH * 100) as usize && a[..3] != b[..3])
            .count();
        assert!(
            changed > 60,
            "点边面切换必须实际改变三维覆盖，不能只更换中文按钮填色"
        );
    }
    // 点模式中原竖栏工具和三维手柄同时存在；实际像素必须跟随选中点，不能仅更换工具图标。
    for (tool, selected, name) in [
        (MeshEditTool::Move, 1 << 7, "mesh-edit-tool-move.png"),
        (MeshEditTool::Scale, 1 << 7, "mesh-edit-tool-scale.png"),
        (
            MeshEditTool::Rotate,
            0b1100_1100,
            "mesh-edit-tool-rotate.png",
        ),
    ] {
        {
            let mut state = app.world_mut().resource_mut::<MeshEditState>();
            state.kind = MeshElementKind::Vertex;
            state.selected = selected;
            state.tool = tool;
            state.handle_hover = Some(if matches!(tool,MeshEditTool::Move|MeshEditTool::Scale) {
                MoveConstraint::X
            } else {
                MoveConstraint::View
            });
        }
        frames(&mut app, 5);
        let pixel = {
            let camera = MoveCamera::new(
                app.world().resource::<MapEditorState>(),
                app.world().resource::<MapEditorViewportState>(),
                1.0,
            )
            .unwrap();
            let origin = super::geometry::selection_center(&vertices, selected as u64).unwrap();
            let point = if tool == MeshEditTool::Move {
                let arrow = super::transform_math::move_arrows(camera, origin)[0]
                    .take()
                    .unwrap();
                arrow.world[0].lerp(arrow.world[1], 0.65)
            } else if tool == MeshEditTool::Scale {
                let handle=super::transform_math::scale_handles(camera,origin)[0].take().unwrap();
                handle.world[2].lerp(handle.world[3],0.5) // 采缩放轴端真实方帽，不把移动箭头像素当作缩放验收。
            } else {
                let ring = super::transform_math::rotation_rings(camera, origin)[3]
                    .take()
                    .unwrap();
                // 取环的画面最高点；下半环可能被所编辑的实体表面正常遮挡，不能把遮挡当成未绘制。
                let index = ring
                    .screen
                    .iter()
                    .enumerate()
                    .min_by(|(_, a), (_, b)| a.y.total_cmp(&b.y))
                    .unwrap()
                    .0;
                ring.world[index]
            };
            (camera.project(point).unwrap() + Vec2::splat(0.5))
                * Vec2::new(WIDTH as f32, HEIGHT as f32)
        };
        let bytes = snapshot(&mut app, target.clone(), name);
        assert!(
            orange_pixels_near(&bytes, pixel, 9) > 5,
            "所选点的{}手柄必须真实可见",
            tool.label()
        );
    }
    assert!(
        !app.world().resource::<MapEditorState>().dirty,
        "离屏验收不生成任何作者事务"
    );
}
