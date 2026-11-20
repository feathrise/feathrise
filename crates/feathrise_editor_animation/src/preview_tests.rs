//! 骨架层级和旋转边界回归；曲线插值由作者数据模块的共享测试覆盖。

use bevy::camera::visibility::RenderLayers;
use feathrise_editor_core::constants::render_layers::SCENE_EDITOR_ASSET_PREVIEW_LAYER;
use feathrise_editor_ui::UiInteraction;
use super::*;

fn node(parent: Option<usize>) -> Node {
    Node {
        name: "骨骼".into(),
        parent,
        translation: [0.0, 0.0, 0.0],
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: [1.0; 3],
    }
}

#[test]
fn unordered_parent_chain_preserves_animated_translation_and_scale() {
    let nodes = vec![node(Some(2)), node(None), node(Some(1))];
    let local = vec![
        Transform::from_xyz(0.0, 1.0, 0.0),
        Transform::from_xyz(5.0, 0.0, 0.0).with_scale(Vec3::splat(2.0)),
        Transform::from_xyz(0.0, 2.0, 0.0),
    ];
    let world = world_matrices(&nodes, &local);
    assert_eq!(
        world[0].transform_point3(Vec3::ZERO),
        Vec3::new(5.0, 6.0, 0.0)
    );
}

#[test]
fn invalid_parent_and_cycles_finish_with_finite_transforms() {
    let nodes = vec![node(Some(1)), node(Some(0)), node(Some(99))];
    let local = vec![Transform::from_xyz(0.0, 1.0, 0.0); 3];
    assert!(
        world_matrices(&nodes, &local)
            .iter()
            .all(|matrix| matrix.is_finite())
    );
}

#[test]
fn sampled_base_rotation_is_normalized_and_zero_is_safe() {
    let mut nodes = vec![node(None), node(None)];
    nodes[0].rotation = [0.0, 0.0, 2.0, 2.0];
    nodes[1].rotation = [0.0; 4];
    let sampled = sampled_local(&nodes, &[], 0.0);
    assert!((sampled[0].rotation.length() - 1.0).abs() < 1e-6);
    assert_eq!(sampled[1].rotation, Quat::IDENTITY);
}

#[test]
fn document_channels_drive_world_joint_positions() {
    let mut document = super::super::data::tests::test_document();
    let rotation = &mut document.clips[0].channels[0];
    rotation.path = "rotation".into();
    rotation.keys[0].value = vec![0.0, 0.0, 0.0, 1.0];
    rotation.keys[1].value = vec![0.0, 1.0, 0.0, 0.0];
    let pose = prepare_pose(Arc::new(document), 0, 1.0);
    let parent = pose.world[0].transform_point3(Vec3::ZERO);
    let child = pose.world[1].transform_point3(Vec3::ZERO);
    assert!(parent.abs_diff_eq(Vec3::Y, 1e-5));
    assert!(
        child.abs_diff_eq(Vec3::new(1.0, 1.0, 0.0), 1e-5)
    );
}

#[test]
fn paused_seek_withdraws_the_old_candidate_before_publication() {
    use crate::resources::Session;

    for (playing, draft) in [(false, false), (true, false), (false, true)] {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .add_systems(Update, drive);
        let source = PathBuf::from("fixture.glb");
        let scene = app.world_mut().spawn_empty().id();
        let mut editor = AnimationEditorState {
            active: Some(source.clone()),
            visible: true,
            ..default()
        };
        editor.sessions.insert(source.clone(), Session::new(crate::data::tests::test_document()));
        let published = Arc::new(prepare_pose(editor.document_snapshot().unwrap(), 0, 0.0));
        let request = (editor.revision(), editor.active_clip(), pose_request_time(&editor));
        let identity = SourceIdentity { path: source, revision: editor.source_revision(), scene };
        app.insert_resource(PreviewRuntime {
            identity: Some(identity),
            applied: Some(request),
            pose: Some(published.clone()),
            published_pose: Some(published.clone()),
            applying: true,
            cursor: NODES_PER_FRAME,
            complete: true,
            ..default()
        });

        editor.seek(1.0);
        editor.playing = playing;
        if draft {
            editor.current_mut().unwrap().pose = Some((0, Transform::IDENTITY));
        }
        assert_eq!(editor.revision(), request.0);
        app.insert_resource(editor).insert_resource(AssetPreviewState {
            render_visible: true,
            selected_path: Some("fixture.glb".into()),
            content: AssetPreviewContent::Model {
                handle: Handle::default(), root: scene, phase: AssetPreviewPhase::Ready,
            },
            ..default()
        });
        app.update();

        // 暂停定位会撤下未完成的旧候选，仍保留上一份完整画面等候新帧。
        let keep_candidate = playing || draft;
        let runtime = app.world().resource::<PreviewRuntime>();
        assert_eq!(runtime.applied, keep_candidate.then_some(request));
        assert_eq!(runtime.applying, keep_candidate);
        assert_eq!(runtime.cursor, if keep_candidate { NODES_PER_FRAME } else { 0 });
        assert!(Arc::ptr_eq(runtime.published_pose.as_ref().unwrap(), &published));
    }
}

#[test]
fn pose_budget_counts_mapped_joints_and_publishes_the_complete_model() {
    use crate::types::{ModelBank, SkeletonMapping};
    use feathrise_editor_animation_model::retarget::{JointCorrection, RigRetarget};

    fn tick(
        mut commands: Commands,
        mut runtime: ResMut<PreviewRuntime>,
        mut binding: ResMut<BindingState>,
        preview: Res<AssetPreviewState>,
        mut transforms: Query<&mut Transform>,
    ) {
        if runtime.applying
            && apply_model_candidate(&mut runtime, &binding, &mut transforms, true)
        {
            publish_in_place_scene(&mut runtime, &mut binding, &preview, &mut commands);
        }
    }

    // 复现 66 个源节点映射到 49 个关节的动作，末关节仍位于第 64 号源节点。
    let sparse: Vec<_> = (0..15).chain(23..39).chain(47..65).collect();
    assert_eq!(sparse.len(), 49);
    for mapped in [sparse, (0..65).collect()] {
        let mut app = App::new();
        let scene = app.world_mut().spawn(Visibility::Inherited).id();
        let banks: [ModelBank; 2] = std::array::from_fn(|_| {
            let root = app.world_mut().spawn(Visibility::Hidden).id();
            let nodes = mapped.iter().map(|&index| {
                let entity = app.world_mut().spawn((Transform::IDENTITY, ChildOf(root))).id();
                (index, entity)
            }).collect();
            ModelBank { root: Some(root), nodes, scanned: true, ..default() }
        });
        let target = banks[1].root.unwrap();
        let last_joint = banks[1].nodes[&64];
        let mut document = super::super::data::tests::test_document();
        document.nodes.resize_with(66, || node(None));
        let mut pose = prepare_pose(Arc::new(document), 0, 0.25);
        let mut corrections = vec![None; pose.local.len()];
        for &index in &mapped {
            corrections[index] = Some(JointCorrection {
                rest: Transform::from_xyz(1.0, 0.0, 0.0),
                before: Quat::IDENTITY,
                after: Quat::IDENTITY,
            });
        }
        let rig = RigRetarget { corrections, clips: vec![true], bounds: pose.bounds };
        pose.model_local = Some(rig.sample(&pose.local));
        app.insert_resource(PreviewRuntime { pose: Some(Arc::new(pose)), applying: true, ..default() })
            .insert_resource(BindingState {
                mapping: Some(SkeletonMapping {
                    names: mapped.iter().map(|&index| (format!("关节{index}"), index)).collect(),
                    retarget: Some(Arc::new(rig)),
                    embedded_nodes: Default::default(),
                }),
                banks,
                ..default()
            })
            .insert_resource(AssetPreviewState {
                content: AssetPreviewContent::Model {
                    handle: Handle::default(),
                    root: scene,
                    phase: AssetPreviewPhase::Ready,
                },
                ..default()
            })
            .add_systems(Update, tick);

        app.update();
        if mapped.len() > NODES_PER_FRAME {
            let runtime = app.world().resource::<PreviewRuntime>();
            assert_eq!(runtime.cursor, NODES_PER_FRAME);
            assert!(runtime.published_pose.is_none());
            assert_eq!(app.world().get::<Visibility>(scene), Some(&Visibility::Inherited));
            assert_eq!(app.world().get::<Visibility>(target), Some(&Visibility::Hidden));
            assert_eq!(app.world().get::<Transform>(last_joint).unwrap().translation, Vec3::ZERO);
            app.update();
        }

        let runtime = app.world().resource::<PreviewRuntime>();
        assert_eq!(runtime.cursor, mapped.len());
        assert_eq!(runtime.front, 1);
        assert!(!runtime.applying);
        assert_eq!(runtime.published_pose.as_ref().unwrap().time, 0.25);
        assert_eq!(app.world().get::<Visibility>(scene), Some(&Visibility::Hidden));
        assert_eq!(app.world().get::<Visibility>(target), Some(&Visibility::Inherited));
        let binding = app.world().resource::<BindingState>();
        assert!(binding.active);
        for &entity in binding.banks[1].nodes.values() {
            assert_eq!(app.world().get::<Transform>(entity).unwrap().translation, Vec3::X);
        }
    }
}

#[test]
fn failed_source_can_only_restart_a_current_pose() {
    let document = Arc::new(crate::data::tests::test_document());
    let pose = Arc::new(prepare_pose(document, 0, 0.0));
    for request in [Some((1, 0, 0)), None] {
        let mut runtime = PreviewRuntime {
            pose: Some(pose.clone()),
            published_pose: Some(pose.clone()),
            complete: true,
            cursor: NODES_PER_FRAME,
            applied: request,
            ..default()
        };
        hide_failed_source(&mut runtime);
        assert!(!runtime.complete);
        assert_eq!(runtime.cursor, 0);
        assert_eq!(runtime.applying, request.is_some());
    }
}

/// 使用自建纯动画GLB和正式预览系统，验证显示的骨架确实随动作变化。
/// 此测试仅建立离屏GPU应用，读取中性夹具并向target写验证图片，不安装游戏或设置保存插件。
#[test]
#[ignore = "需要真实GPU；用--ignored --test-threads=1运行，中性动画夹具已随测试提供"]
fn gpu_animation_only_glb_renders_and_changes_pose() {
    gpu_preview(GpuPreview::Motion);
}

#[test]
#[ignore = "需要真实GPU；使用随测试提供的中性五骨骼蒙皮GLB"]
fn gpu_bound_character_skin_changes_with_optional_skeleton_overlay() {
    gpu_preview(GpuPreview::Bound);
}

#[test]
#[ignore = "需要真实GPU；检查同源内嵌动作的实际蒙皮和浅色骨架"]
fn gpu_embedded_animation_drives_skin_and_keeps_editable() {
    gpu_preview(GpuPreview::Embedded);
}

#[test]
#[ignore = "需要真实GPU；检查侧视旋转环及选择覆盖的实际像素"]
fn gpu_pose_tools_move_arrows_and_edge_on_ring_stay_visible() {
    gpu_preview(GpuPreview::Tools);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GpuPreview {
    Motion,
    Bound,
    Embedded,
    Tools,
}

fn gpu_preview(mode: GpuPreview) {
    use feathrise_editor_animation_model::{host::AnimationHost, view_math::{AssetPreviewView, apply_model_preview_frame}};
    use super::super::resources::Session;
    use bevy::{
        asset::AssetPlugin,
        camera::RenderTarget,
        render::render_resource::{TextureFormat, TextureUsages},
        window::{ExitCondition, WindowPlugin},
        winit::WinitPlugin,
        world_serialization::WorldAsset,
    };

    let bound = mode == GpuPreview::Bound;
    let embedded = mode == GpuPreview::Embedded;
    let asset = if embedded { "skinned_animated_rig.glb" } else { "animated_rig.glb" };
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let asset_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/animation_preview")
        .canonicalize()
        .unwrap();
    let source = asset_root.join(asset);
    let document = AnimationDocument::load(&source).expect("中性动画GLB应可读取");
    assert!(!document.clips.is_empty() && document.nodes.len() > 1);
    let midpoint = document.clips[0].duration() * 0.5;
    assert!(midpoint > 0.0);
    // 相机同时覆盖两次采样范围，死亡动作倒地后仍须留在同一画面中，图像差异不能来自重取景。
    let document = Arc::new(document);
    let first_pose = prepare_pose(document.clone(), 0, 0.0);
    let middle_pose = prepare_pose(document.clone(), 0, midpoint);
    let minimum = first_pose.bounds.minimum.min(middle_pose.bounds.minimum);
    let maximum = first_pose.bounds.maximum.max(middle_pose.bounds.maximum);
    let mut camera_transform = Transform::default();
    let mut projection = Projection::Perspective(PerspectiveProjection::default());
    apply_model_preview_frame(
        minimum,
        maximum,
        1.0,
        AssetPreviewView::default(),
        &mut camera_transform,
        &mut projection,
    );

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .set(AssetPlugin {
                file_path: asset_root.to_string_lossy().into_owned(),
                ..default()
            })
            .disable::<WinitPlugin>()
            .disable::<bevy::log::LogPlugin>(),
    );
    app.init_resource::<PreviewRuntime>()
        .add_systems(Update, drive);
    super::super::pose::install_visuals(&mut app);
    app.world_mut().spawn((
        AssetPreviewCanvas,
        ComputedNode {
            size: Vec2::splat(512.),
            inverse_scale_factor: 1.,
            ..default()
        },
        bevy::ui::RelativeCursorPosition::default(),
        UiInteraction::None,
    ));
    if bound || embedded {
        app.insert_resource(AnimationHost { asset_root: asset_root.clone(), ..default() });
        super::super::binding::install(&mut app);
    }
    app.finish();
    app.cleanup();
    let target = app.world_mut().resource_mut::<Assets<Image>>().add({
        let mut image = Image::new_target_texture(512, 512, TextureFormat::Bgra8UnormSrgb, None);
        image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        image
    });
    let camera = app
        .world_mut()
        .spawn((
            Camera3d::default(),
            AssetPreviewCamera,
            Camera {
                clear_color: ClearColorConfig::Custom(Color::srgb(0.018, 0.022, 0.028)),
                ..default()
            },
            RenderTarget::from(target.clone()),
            camera_transform,
            projection,
            RenderLayers::layer(SCENE_EDITOR_ASSET_PREVIEW_LAYER),
        ))
        .id();
    let (gltf, scene): (Handle<Gltf>, Handle<WorldAsset>) = {
        let server = app.world().resource::<AssetServer>();
        (
            server.load(asset),
            server.load(GltfAssetLabel::Scene(0).from_asset(asset)),
        )
    };
    let scene_root = app
        .world_mut()
        .spawn((
            WorldAssetRoot(scene.clone()),
            Transform::default(),
            if embedded { Visibility::Inherited } else { Visibility::Hidden },
            RenderLayers::layer(SCENE_EDITOR_ASSET_PREVIEW_LAYER),
        ))
        .id();
    let mut editor = AnimationEditorState {
        active: Some(source.clone()),
        visible: true,
        ..default()
    };
    editor
        .sessions
        .insert(source, Session::new((*document).clone()));
    if bound || embedded {
        let session = editor.current_mut().unwrap();
        // 让像素差异独立于选骨手柄；内嵌动作仍需同时出现模型和浅色骨架。
        session.selected_node = None;
        if bound {
            session.model_path = Some("skinned_rig.glb".into());
        }
        app.world_mut().spawn((
            DirectionalLight {
                illuminance: 15000.0,
                ..default()
            },
            Transform::from_xyz(3., 4., 5.).looking_at(Vec3::ZERO, Vec3::Y),
            RenderLayers::layer(SCENE_EDITOR_ASSET_PREVIEW_LAYER),
        ));
    }
    let mut model = AssetPreviewModelSession::new(1, gltf.clone());
    model.scene_mesh_count = usize::from(embedded);
    app.insert_resource(editor)
        .insert_resource(AssetPreviewState {
            render_visible: true,
            selected_path: Some(asset.to_owned()),
            camera: Some(camera),
            _render_target: Some(target.clone()),
            content: AssetPreviewContent::Model {
                handle: scene,
                root: scene_root,
                phase: if embedded { AssetPreviewPhase::Ready } else { AssetPreviewPhase::Loading },
            },
            model: Some(model),
            ..default()
        });
    wait_gpu_pose(&mut app, 0.0, &gltf);
    if mode == GpuPreview::Tools {
        verify_selection_and_edge_on_ring(&mut app, camera, target, &gltf, &document, &workspace);
        return;
    }
    assert_eq!(
        app.world()
            .resource::<Assets<Gltf>>()
            .get(&gltf)
            .unwrap()
            .meshes
            .is_empty(),
        !embedded,
        "纯动作和内嵌动作分别需要无网格及有网格的真实 GLB"
    );
    if bound || embedded {
        let start = std::time::Instant::now();
        while !app
            .world()
            .resource::<super::super::binding::BindingState>()
            .active
        {
            app.update();
            std::thread::sleep(std::time::Duration::from_millis(16));
            assert!(
                start.elapsed().as_secs() < 40,
                "绑定未完成：{}",
                app.world().resource::<AnimationEditorState>().model_status
            );
        }
        wait_gpu_pose(&mut app, 0.0, &gltf);
        if embedded {
            assert!(app.world().resource::<super::super::binding::BindingState>().embedded());
            assert!(app.world().resource::<AnimationEditorState>().editable());
            assert_eq!(app.world().get::<Visibility>(scene_root), Some(&Visibility::Hidden));
        }
        let skin_count = app
            .world_mut()
            .query::<&bevy::mesh::skinning::SkinnedMesh>()
            .iter(app.world())
            .count();
        assert!(skin_count >= 2, "两容器必须各有真实蒙皮网格");
    }
    let start_joints = published_gpu_joints(&app, mode);
    let bound_overlay = bound.then(|| {
        // 同一完整角色帧先显示浅色骨架，再用正式的权重视口归属门取得纯网格对照。
        let started = std::time::Instant::now();
        while !super::super::pose::animation_overlay_submitted(app.world()) {
            app.update();
            std::thread::sleep(std::time::Duration::from_millis(16));
            assert!(started.elapsed().as_secs() < 20, "兼容绑定未提交浅色骨架");
        }
        let rendered = read_gpu_preview(&mut app, target.clone(), None, GpuPreview::Motion);
        app.world_mut().resource_mut::<AnimationHost>().weights_own_viewport = true;
        rendered
    });
    let first = read_gpu_preview(&mut app, target.clone(), bound_overlay.as_deref(), mode);
    {
        let mut editor = app.world_mut().resource_mut::<AnimationEditorState>();
        editor.seek(midpoint);
        editor.changed();
    }
    wait_gpu_pose(&mut app, midpoint, &gltf);
    let middle_joints = published_gpu_joints(&app, mode);
    assert!(
        start_joints
            .iter()
            .zip(&middle_joints)
            .any(|(before, after)| !before.abs_diff_eq(*after, 1e-4)),
        "真实动作中间时刻没有改变骨节位置"
    );
    let middle = read_gpu_preview(&mut app, target, Some(&first), mode);
    assert_eq!(super::super::pose::animation_overlay_submitted(app.world()), !bound);
    let output = workspace.join(match mode {
        GpuPreview::Bound => "target/animation-character-preview.png",
        GpuPreview::Embedded => "target/animation-embedded-preview.png",
        _ => "target/animation-skeleton-preview.png",
    });
    std::fs::create_dir_all(output.parent().unwrap()).unwrap();
    let mut rgba = middle;
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    image::RgbaImage::from_raw(512, 512, rgba)
        .unwrap()
        .save(&output)
        .unwrap();
    println!("纯动画真实GPU验证图片：{}", output.display());
}

/// 使用正式绘制系统检查侧视旋转环，以及随选骨和换帧更新的青色菱形与出射骨段。
fn verify_selection_and_edge_on_ring(
    app: &mut App,
    camera: Entity,
    target: Handle<Image>,
    gltf: &Handle<Gltf>,
    document: &AnimationDocument,
    workspace: &Path,
) {
    use feathrise_editor_animation_model::{gizmo::GizmoCamera, skeleton::JointGlyph};
    let node = document
        .nodes
        .iter()
        .position(|n| n.name.contains("LeftUpperArm"))
        .unwrap();
    let other = document
        .nodes
        .iter()
        .position(|n| n.name.contains("RightUpperArm"))
        .unwrap();
    for (step, selected, tilt) in [(0, node, -0.03), (1, node, 0.0), (2, other, 0.03)] {
        {
            let mut editor = app.world_mut().resource_mut::<AnimationEditorState>();
            editor.pose_tool = super::super::types::PoseTool::Rotate;
            editor.current_mut().unwrap().select_node(selected);
            if step == 2 {
                editor.seek(0.5);
            }
            editor.changed();
        }
        wait_gpu_pose(app, if step == 2 { 0.5 } else { 0.0 }, gltf);
        let child = document.nodes.iter().position(|node| node.parent == Some(selected)).unwrap();
        let (origin, tip, extent) = {
            let runtime = app.world().resource::<PreviewRuntime>();
            let pose = runtime.published_pose.as_ref().unwrap();
            (
                pose.world[selected].transform_point3(Vec3::ZERO),
                pose.world[child].transform_point3(Vec3::ZERO),
                (pose.bounds.maximum - pose.bounds.minimum).max_element() * 1.1,
            )
        };
        let transform = Transform::from_translation(origin + Vec3::new(0., tilt, 3.) * extent)
            .looking_at(origin, Vec3::Y);
        let projection = Projection::Orthographic(OrthographicProjection {
            scaling_mode: bevy::camera::ScalingMode::FixedVertical {
                viewport_height: extent,
            },
            ..OrthographicProjection::default_3d()
        });
        let view = GizmoCamera::from_projection(
            &projection,
            &GlobalTransform::from(transform),
            Vec2::splat(512.),
            1.,
        )
        .unwrap();
        app.world_mut()
            .entity_mut(camera)
            .insert((transform, projection));
        let selected_glyph = JointGlyph::project(view, origin, true).unwrap();
        let child_glyph = JointGlyph::project(view, tip, false).unwrap();
        let [base, _, end] = selected_glyph.screen_bone_to(child_glyph).unwrap();
        let ring = feathrise_editor_animation_model::gizmo::rotation_rings(view, origin)[1]
            .take()
            .unwrap();
        let sample = |bytes: &[u8], point: Vec2, cyan: bool| {
            let p = (point + Vec2::splat(256.)).round().as_ivec2();
            (-4..=4).any(|dy| {
                (-4..=4).any(|dx| {
                    let x = p.x + dx;
                    let y = p.y + dy;
                    if !(0..512).contains(&x) || !(0..512).contains(&y) {
                        return false;
                    }
                    let b = &bytes[((y * 512 + x) * 4) as usize..][..4];
                    if cyan {
                        b[0] > 160 && b[1] > 160 && b[2] < 130
                    } else {
                        b[1] > 160 && b[0] < 150 && b[2] < 150
                    }
                })
            })
        };
        let started = std::time::Instant::now();
        let bytes = loop {
            let bytes = read_gpu_preview(app, target.clone(), None, GpuPreview::Tools);
            let green = [6, 9, 12, 20, 23, 26]
                .into_iter()
                .filter(|&i| sample(&bytes, ring.screen[i], false))
                .count();
            let diamond = selected_glyph.screen + Vec2::new(1., -1.) * (selected_glyph.radius * 0.5);
            if green >= 4 && sample(&bytes, diamond, true) && sample(&bytes, base.lerp(end, 0.5), true) {
                break bytes;
            }
            assert!(
                started.elapsed().as_secs() < 20,
                "侧视环或所选点线未呈现：步骤{step}，绿色采样{green}"
            );
        };
        if step == 1 {
            let mut rgba = bytes;
            for p in rgba.chunks_exact_mut(4) {
                p.swap(0, 2);
            }
            let path = workspace.join("target/animation-selection-edge-on.png");
            image::RgbaImage::from_raw(512, 512, rgba)
                .unwrap()
                .save(&path)
                .unwrap();
            println!("侧视环与所选骨骼GPU图片：{}", path.display());
        }
    }
    // 同一真实骨架通过正式工具动作切到移动，检查三根箭杆和箭翼，不能把骨架线误当作移动手柄。
    super::super::control::action(
        &mut app.world_mut().resource_mut::<AnimationEditorState>(),
        super::super::components::AnimationAction::MovePose,
    );
    wait_gpu_pose(app, 0.5, gltf);
    let pose = app
        .world()
        .resource::<PreviewRuntime>()
        .published_pose
        .as_ref()
        .unwrap();
    let origin = pose.world[other].transform_point3(Vec3::ZERO);
    let extent = (pose.bounds.maximum - pose.bounds.minimum).max_element();
    let transform = Transform::from_translation(origin + Vec3::new(1.8, 1., 3.) * extent)
        .looking_at(origin, Vec3::Y);
    let projection = Projection::Orthographic(OrthographicProjection {
        scaling_mode: bevy::camera::ScalingMode::FixedVertical {
            viewport_height: extent,
        },
        ..OrthographicProjection::default_3d()
    });
    let view = GizmoCamera::from_projection(
        &projection,
        &GlobalTransform::from(transform),
        Vec2::splat(512.),
        1.,
    )
    .unwrap();
    let arrows = feathrise_editor_animation_model::gizmo::move_arrows(view, origin);
    assert!(arrows.iter().all(Option::is_some));
    app.world_mut()
        .entity_mut(camera)
        .insert((transform, projection));
    let started = std::time::Instant::now();
    let mut bytes = loop {
        let bytes = read_gpu_preview(app, target.clone(), None, GpuPreview::Tools);
        let visible = arrows.iter().enumerate().all(|(axis, arrow)| {
            let [a, b, c, d] = arrow.as_ref().unwrap().screen;
            [a.lerp(b, 0.65), b.lerp(c, 0.7), b.lerp(d, 0.7)]
                .into_iter()
                .all(|point| {
                    let p = (point + Vec2::splat(256.)).round().as_ivec2();
                    (-3..=3).any(|dy| {
                        (-3..=3).any(|dx| {
                            let x = p.x + dx;
                            let y = p.y + dy;
                            if !(0..512).contains(&x) || !(0..512).contains(&y) {
                                return false;
                            }
                            let pixel = &bytes[((y * 512 + x) * 4) as usize..][..4];
                            let channel = 2 - axis; // BGRA回读对应世界XYZ红绿蓝。
                            pixel[channel] > 170
                                && (0..3)
                                    .filter(|i| *i != channel)
                                    .all(|i| pixel[channel] as i16 - pixel[i] as i16 > 50)
                        })
                    })
                })
        });
        if visible {
            break bytes;
        }
        assert!(
            started.elapsed().as_secs() < 20,
            "切到移动后没有显示三轴箭杆和箭翼"
        );
    };
    for p in bytes.chunks_exact_mut(4) {
        p.swap(0, 2);
    }
    image::RgbaImage::from_raw(512, 512, bytes)
        .unwrap()
        .save(workspace.join("target/animation-move-arrows.png"))
        .unwrap();
}

fn wait_gpu_pose(app: &mut App, position: f32, gltf: &Handle<Gltf>) {
    let started = std::time::Instant::now();
    loop {
        app.update();
        std::thread::sleep(std::time::Duration::from_millis(16));
        let runtime = app.world().resource::<PreviewRuntime>();
        let editor = app.world().resource::<AnimationEditorState>();
        let ready = runtime.complete
            && !runtime.applying
            && runtime.applied == Some((editor.revision(), 0, position.to_bits()));
        if ready
            && app
                .world()
                .resource::<AssetServer>()
                .is_loaded_with_dependencies(gltf.id())
        {
            return;
        }
        assert!(
            started.elapsed().as_secs() < 30,
            "真实GLB骨架姿态发布超时：{}",
            app.world().resource::<AssetPreviewState>().status
        );
    }
}

fn published_gpu_joints(app: &App, mode: GpuPreview) -> Vec<Vec3> {
    let world = app.world();
    let runtime = world.resource::<PreviewRuntime>();
    if matches!(mode, GpuPreview::Bound | GpuPreview::Embedded) {
        let binding = world.resource::<super::super::binding::BindingState>();
        let bank = &binding.banks[runtime.front];
        assert_eq!(world.get::<Visibility>(bank.root.unwrap()), Some(&Visibility::Inherited));
        let mut nodes: Vec<_> = bank.nodes.iter().collect();
        nodes.sort_unstable_by_key(|(index, _)| **index);
        return nodes.into_iter().map(|(_, &entity)| world.get::<GlobalTransform>(entity).unwrap().translation()).collect();
    }
    runtime.published_pose.as_ref().unwrap().world.iter()
        .map(|matrix| matrix.transform_point3(Vec3::ZERO))
        .collect()
}

fn read_gpu_preview(
    app: &mut App,
    target: Handle<Image>,
    previous: Option<&[u8]>,
    mode: GpuPreview,
) -> Vec<u8> {
    use bevy::render::gpu_readback::{Readback, ReadbackComplete};
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let reader = app
        .world_mut()
        .spawn(Readback::texture(target))
        .observe(move |event: On<ReadbackComplete>| {
            let _ = sender.try_send(event.data.clone());
        })
        .id();
    let started = std::time::Instant::now();
    let bytes = loop {
        app.update();
        std::thread::sleep(std::time::Duration::from_millis(16));
        if let Ok(bytes) = receiver.try_recv() {
            assert_eq!(bytes.len(), 512 * 512 * 4);
            // BGRA8 的独立相机没有 UI 或日光背景，直接核对浅色骨架像素和旧橙色像素。
            let pale = bytes.chunks_exact(4).filter(|pixel| {
                let color = &pixel[..3];
                let low = *color.iter().min().unwrap();
                let high = *color.iter().max().unwrap();
                low > 120 && high - low < 42
            }).count();
            let orange = bytes
                .chunks_exact(4)
                .filter(|pixel| {
                    pixel[2] > 140
                        && pixel[1] > 70
                        && pixel[2] as u16 > pixel[1] as u16 + 20
                        && pixel[1] as u16 > pixel[0] as u16 + 20
                })
                .count();
            // 内嵌动作夹具的蓝色网格与浅色线条分离，必须观测到网格覆盖范围真的随帧变化。
            let blue_mesh = |pixel: &[u8]| {
                pixel[0] > 80 && pixel[0].saturating_sub(pixel[1]) > 24 && pixel[0].saturating_sub(pixel[2]) > 48
            };
            let changed = previous.map_or(usize::MAX, |before| {
                before
                    .chunks_exact(4)
                    .zip(bytes.chunks_exact(4))
                    .filter(|(before, after)| {
                        if mode == GpuPreview::Embedded {
                            return blue_mesh(before) != blue_mesh(after);
                        }
                        before[..3]
                            .iter()
                            .zip(&after[..3])
                            .any(|(before, after)| before.abs_diff(*after) > 12)
                    })
                    .count()
            });
            let visible = bytes
                .chunks_exact(4)
                .filter(|p| p[0].max(p[1]).max(p[2]) > 80)
                .count();
            let ready = match mode {
                GpuPreview::Bound => visible > 300,
                GpuPreview::Embedded => pale > 100 && orange == 0
                    && bytes.chunks_exact(4).filter(|pixel| blue_mesh(pixel)).count() > 300,
                GpuPreview::Motion | GpuPreview::Tools => pale > 100 && orange == 0,
            };
            if ready && changed > 100 {
                break bytes;
            }
            assert!(
                started.elapsed().as_secs() < 15,
                "预览 GPU 画面未就绪：浅色像素{pale}，橙色像素{orange}，变化像素{changed}"
            );
        }
        assert!(started.elapsed().as_secs() < 20, "骨架GPU纹理回读超时");
    };
    app.world_mut().entity_mut(reader).despawn();
    bytes
}
