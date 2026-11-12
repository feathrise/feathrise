//! 骨架层级和旋转边界回归；曲线插值由作者数据模块的共享测试覆盖。

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
fn document_channels_drive_visible_joint_positions_and_bone_direction() {
    let mut document = super::super::data::tests::test_document();
    let rotation = &mut document.clips[0].channels[0];
    rotation.path = "rotation".into();
    rotation.keys[0].value = vec![0.0, 0.0, 0.0, 1.0];
    rotation.keys[1].value = vec![0.0, 1.0, 0.0, 0.0];
    let pose = prepare_pose(Arc::new(document), 0, 1.0);
    assert!(pose.joints[0].translation.abs_diff_eq(Vec3::Y, 1e-5));
    assert!(
        pose.joints[1]
            .translation
            .abs_diff_eq(Vec3::new(1.0, 1.0, 0.0), 1e-5)
    );
    assert!((pose.bones[1].rotation * Vec3::Y).abs_diff_eq(Vec3::X, 1e-5));
}

fn candidate_app(count: usize) -> App {
    let mut app = App::new();
    let roots = [
        app.world_mut().spawn(Visibility::Inherited).id(),
        app.world_mut().spawn(Visibility::Hidden).id(),
    ];
    let entities = (0..count)
        .map(|_| BoneEntities {
            joint: std::array::from_fn(|bank| {
                app.world_mut()
                    .spawn((
                        Transform::default(),
                        MeshMaterial3d::<StandardMaterial>::default(),
                        ChildOf(roots[bank]),
                    ))
                    .id()
            }),
            bone: std::array::from_fn(|bank| {
                app.world_mut()
                    .spawn((
                        Transform::default(),
                        MeshMaterial3d::<StandardMaterial>::default(),
                        ChildOf(roots[bank]),
                    ))
                    .id()
            }),
        })
        .collect();
    let mut runtime = PreviewRuntime {
        roots: Some(roots),
        entities,
        applying: true,
        complete: true,
        joint_material: Some(Handle::default()),
        animated_material: Some(Handle::default()),
        ..default()
    };
    runtime.pose = Some(Arc::new(Pose {
        document: default(),
        clip: 0,
        draft: None,
        time: 0.0,
        local: vec![Transform::default(); count],
        model_local: None,
        world: vec![Mat4::IDENTITY; count],
        joints: vec![Transform::from_xyz(10.0, 0.0, 0.0); count],
        bones: vec![Transform::from_xyz(20.0, 0.0, 0.0); count],
        animated: vec![true; count],
        bounds: AssetPreviewModelBounds {
            minimum: Vec3::ZERO,
            maximum: Vec3::splat(20.0),
        },
    }));
    app.insert_resource(runtime);
    app
}

fn apply_tick(
    mut commands: Commands,
    mut runtime: ResMut<PreviewRuntime>,
    mut transforms: Query<&mut Transform>,
    mut materials: Query<&mut MeshMaterial3d<StandardMaterial>>,
) {
    if runtime.applying {
        apply_candidate(&mut runtime, &mut commands, &mut transforms, &mut materials);
    }
}

#[test]
fn large_pose_keeps_previous_bank_visible_until_every_node_is_complete() {
    let mut app = candidate_app(130);
    app.add_systems(Update, apply_tick);
    let roots = app.world().resource::<PreviewRuntime>().roots.unwrap();
    for expected in [64, 128] {
        app.update();
        let runtime = app.world().resource::<PreviewRuntime>();
        assert_eq!(runtime.cursor, expected);
        assert_eq!(runtime.front, 0);
        assert_eq!(
            *app.world().get::<Visibility>(roots[0]).unwrap(),
            Visibility::Inherited
        );
        assert_eq!(
            *app.world().get::<Visibility>(roots[1]).unwrap(),
            Visibility::Hidden
        );
        for pair in &runtime.entities {
            assert_eq!(
                app.world()
                    .get::<Transform>(pair.joint[0])
                    .unwrap()
                    .translation,
                Vec3::ZERO
            );
        }
    }
    app.update();
    let runtime = app.world().resource::<PreviewRuntime>();
    assert!(!runtime.applying);
    assert_eq!(runtime.front, 1);
    assert_eq!(
        *app.world().get::<Visibility>(roots[0]).unwrap(),
        Visibility::Hidden
    );
    assert_eq!(
        *app.world().get::<Visibility>(roots[1]).unwrap(),
        Visibility::Inherited
    );
    for pair in &runtime.entities {
        assert_eq!(
            app.world()
                .get::<Transform>(pair.joint[1])
                .unwrap()
                .translation,
            Vec3::new(10.0, 0.0, 0.0)
        );
    }
    // 暂停状态没有新候选，保持完整容器，不再次交换或写入骨架。
    app.update();
    assert_eq!(app.world().resource::<PreviewRuntime>().front, 1);
}

#[test]
fn in_place_pose_budget_counts_mapped_joints_and_publishes_the_complete_model() {
    use crate::types::{ModelBank, SkeletonMapping};
    use feathrise_editor_animation_model::retarget::{JointCorrection, RigRetarget};

    fn tick(
        mut commands: Commands,
        mut runtime: ResMut<PreviewRuntime>,
        mut binding: ResMut<BindingState>,
        preview: Res<AssetPreviewState>,
        mut transforms: Query<&mut Transform>,
    ) {
        if runtime.applying {
            apply_in_place_candidate(&mut runtime, &mut binding, &preview, &mut commands, &mut transforms);
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
fn cancellation_hides_both_banks_then_releases_entities_with_a_fixed_budget() {
    fn cancel(mut commands: Commands, mut runtime: ResMut<PreviewRuntime>, mut first: Local<bool>) {
        if !*first {
            retire(&mut runtime, &mut commands);
            *first = true;
        }
        advance_retirement(&mut runtime, &mut commands);
    }
    let mut app = candidate_app(130);
    let roots = app.world().resource::<PreviewRuntime>().roots.unwrap();
    let pairs = app.world().resource::<PreviewRuntime>().entities.clone();
    app.add_systems(Update, cancel);
    app.update();
    for root in roots {
        assert_eq!(
            *app.world().get::<Visibility>(root).unwrap(),
            Visibility::Hidden
        );
    }
    let remaining = pairs
        .iter()
        .flat_map(|pair| pair.joint.into_iter().chain(pair.bone))
        .filter(|entity| app.world().get_entity(*entity).is_ok())
        .count();
    assert_eq!(remaining, 130 * 4 - 64);
    for _ in 0..9 {
        app.update();
    }
    assert!(app.world().resource::<PreviewRuntime>().retiring.is_none());
    for root in roots {
        assert!(app.world().get_entity(root).is_err());
    }
}

#[test]
fn source_failure_hides_published_banks_and_recovery_republishes_only_a_complete_pose() {
    use bevy::ecs::system::RunSystemOnce;
    fn failed(mut commands: Commands, mut runtime: ResMut<PreviewRuntime>) {
        hide_failed_source(&mut runtime, &mut commands);
    }
    let mut app = candidate_app(130);
    {
        let mut runtime = app.world_mut().resource_mut::<PreviewRuntime>();
        runtime.applied = Some((1, 0, 0));
        runtime.cursor = 64;
    }
    let roots = app.world().resource::<PreviewRuntime>().roots.unwrap();
    app.world_mut().run_system_once(failed).unwrap();
    let runtime = app.world().resource::<PreviewRuntime>();
    assert!(!runtime.complete);
    assert!(runtime.applying);
    assert_eq!(runtime.cursor, 0);
    for root in roots {
        assert_eq!(
            *app.world().get::<Visibility>(root).unwrap(),
            Visibility::Hidden
        );
    }
    app.add_systems(Update, apply_tick);
    for _ in 0..2 {
        app.update();
        assert!(!app.world().resource::<PreviewRuntime>().complete);
        for root in roots {
            assert_eq!(
                *app.world().get::<Visibility>(root).unwrap(),
                Visibility::Hidden
            );
        }
    }
    app.update();
    assert!(app.world().resource::<PreviewRuntime>().complete);
    assert_eq!(
        *app.world().get::<Visibility>(roots[1]).unwrap(),
        Visibility::Inherited
    );

    // 编辑已撤销的候选虽然仍持有旧向量，源恢复时也不能重新变成有效画面。
    let mut invalidated = candidate_app(2);
    invalidated.world_mut().run_system_once(failed).unwrap();
    let runtime = invalidated.world().resource::<PreviewRuntime>();
    assert!(!runtime.complete && !runtime.applying);
}

/// 使用自建纯动画GLB和正式预览系统，验证显示的骨架确实随动作变化。
/// 此测试仅建立离屏GPU应用，读取中性夹具并向target写验证图片，不安装游戏或设置保存插件。
#[test]
#[ignore = "需要真实GPU；用--ignored --test-threads=1运行，中性动画夹具已随测试提供"]
fn gpu_animation_only_glb_renders_and_changes_pose() {
    gpu_preview(false, false);
}

#[test]
#[ignore = "需要真实GPU；使用随测试提供的中性五骨骼蒙皮GLB"]
fn gpu_bound_character_skin_changes_without_skeleton_overlay() {
    gpu_preview(true, false);
}

#[test]
#[ignore = "需要真实GPU；检查侧视旋转环及选择覆盖的实际像素"]
fn gpu_pose_tools_move_arrows_and_edge_on_ring_stay_visible() {
    gpu_preview(false, true);
}

fn gpu_preview(bound: bool, handles: bool) {
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

    const ASSET: &str = "animated_rig.glb";
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let asset_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/animation_preview")
        .canonicalize()
        .unwrap();
    let source = asset_root.join(ASSET);
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
    if handles {
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
    }
    if bound {
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
            server.load(ASSET),
            server.load(GltfAssetLabel::Scene(0).from_asset(ASSET)),
        )
    };
    let scene_root = app
        .world_mut()
        .spawn((
            WorldAssetRoot(scene.clone()),
            Transform::default(),
            Visibility::Hidden,
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
    if bound {
        editor.current_mut().unwrap().model_path = Some("skinned_rig.glb".into());
        app.world_mut().spawn((
            DirectionalLight {
                illuminance: 15000.0,
                ..default()
            },
            Transform::from_xyz(3., 4., 5.).looking_at(Vec3::ZERO, Vec3::Y),
            RenderLayers::layer(SCENE_EDITOR_ASSET_PREVIEW_LAYER),
        ));
    }
    app.insert_resource(editor)
        .insert_resource(AssetPreviewState {
            selected_path: Some(ASSET.to_owned()),
            camera: Some(camera),
            _render_target: Some(target.clone()),
            content: AssetPreviewContent::Model {
                handle: scene,
                root: scene_root,
                phase: AssetPreviewPhase::Loading,
            },
            model: Some(AssetPreviewModelSession::new(1, gltf.clone())),
            ..default()
        });
    wait_gpu_pose(&mut app, 0.0, &gltf);
    if handles {
        verify_selection_and_edge_on_ring(&mut app, camera, target, &gltf, &document, &workspace);
        return;
    }
    assert!(
        app.world()
            .resource::<Assets<Gltf>>()
            .get(&gltf)
            .unwrap()
            .meshes
            .is_empty(),
        "回归输入必须是真实无Mesh动画文件"
    );
    if bound {
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
        let entities: Vec<_> = app
            .world()
            .resource::<PreviewRuntime>()
            .entities
            .iter()
            .flat_map(|b| b.joint.into_iter().chain(b.bone))
            .collect();
        for entity in entities {
            app.world_mut()
                .entity_mut(entity)
                .insert(Visibility::Hidden);
        }
        let skin_count = app
            .world_mut()
            .query::<&bevy::mesh::skinning::SkinnedMesh>()
            .iter(app.world())
            .count();
        assert!(skin_count >= 2, "两容器必须各有真实蒙皮网格");
    }
    let start_transforms = published_gpu_joints(&app);
    let first = read_gpu_skeleton(&mut app, target.clone(), None, !bound);
    {
        let mut editor = app.world_mut().resource_mut::<AnimationEditorState>();
        editor.seek(midpoint);
        editor.changed();
    }
    wait_gpu_pose(&mut app, midpoint, &gltf);
    let middle_transforms = published_gpu_joints(&app);
    assert!(
        start_transforms
            .iter()
            .zip(&middle_transforms)
            .any(|(before, after)| !before.translation.abs_diff_eq(after.translation, 1e-4)),
        "真实动作中间时刻没有改变骨节位置"
    );
    let middle = read_gpu_skeleton(&mut app, target, Some(&first), !bound);
    let output = workspace.join(if bound {
        "target/animation-character-preview.png"
    } else {
        "target/animation-skeleton-preview.png"
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

/// 使用正式绘制系统和真实骨架，在完全侧视及两侧小角度检查绿色环，选择和换帧后再查青色连杆。
fn verify_selection_and_edge_on_ring(
    app: &mut App,
    camera: Entity,
    target: Handle<Image>,
    gltf: &Handle<Gltf>,
    document: &AnimationDocument,
    workspace: &Path,
) {
    use feathrise_editor_animation_model::gizmo::GizmoCamera;
    let node = document
        .nodes
        .iter()
        .position(|n| n.name.contains("LeftForeArm"))
        .unwrap();
    let other = document
        .nodes
        .iter()
        .position(|n| n.name.contains("RightForeArm"))
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
        let (origin, parent, extent) = {
            let runtime = app.world().resource::<PreviewRuntime>();
            let pose = runtime.published_pose.as_ref().unwrap();
            (
                pose.joints[selected].translation,
                pose.joints[document.nodes[selected].parent.unwrap()].translation,
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
            let bytes = read_gpu_skeleton(app, target.clone(), None, true);
            let green = [6, 9, 12, 20, 23, 26]
                .into_iter()
                .filter(|&i| sample(&bytes, ring.screen[i], false))
                .count();
            let joint = view.project(origin).unwrap() * 512. + Vec2::splat(8.);
            let link = view.project(origin.lerp(parent, 0.5)).unwrap() * 512.;
            if green >= 4 && sample(&bytes, joint, true) && sample(&bytes, link, true) {
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
    let origin = pose.joints[other].translation;
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
        let bytes = read_gpu_skeleton(app, target.clone(), None, true);
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

fn published_gpu_joints(app: &App) -> Vec<Transform> {
    let runtime = app.world().resource::<PreviewRuntime>();
    runtime
        .entities
        .iter()
        .map(|pair| {
            *app.world()
                .get::<Transform>(pair.joint[runtime.front])
                .unwrap()
        })
        .collect()
}

fn read_gpu_skeleton(
    app: &mut App,
    target: Handle<Image>,
    previous: Option<&[u8]>,
    skeleton: bool,
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
            // BGRA8回读：橙色材质须满足红>绿>蓝；没有UI文字或其他场景可以冒充骨架。
            let orange = bytes
                .chunks_exact(4)
                .filter(|pixel| {
                    pixel[2] > 140
                        && pixel[1] > 70
                        && pixel[2] as u16 > pixel[1] as u16 + 20
                        && pixel[1] as u16 > pixel[0] as u16 + 20
                })
                .count();
            let changed = previous.map_or(usize::MAX, |before| {
                before
                    .chunks_exact(4)
                    .zip(bytes.chunks_exact(4))
                    .filter(|(before, after)| {
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
            if (if skeleton {
                orange > 100
            } else {
                visible > 300
            }) && changed > 100
            {
                break bytes;
            }
            assert!(
                started.elapsed().as_secs() < 15,
                "骨架GPU画面未就绪：橙色像素{orange}，变化像素{changed}"
            );
        }
        assert!(started.elapsed().as_secs() < 20, "骨架GPU纹理回读超时");
    };
    app.world_mut().entity_mut(reader).despawn();
    bytes
}
