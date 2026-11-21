//! 本文件只复制进本轮生成的独占游戏，使用实际 glTF 播放器、SDK 场景和 Tnua 角色电机。

use avian3d::prelude::*;
use bevy::{
    animation::graph::{AnimationGraph, AnimationGraphHandle, AnimationNodeIndex},
    app::AnimationSystems,
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    world_serialization::{WorldAssetRoot, WorldInstanceReady},
};
use bevy_tnua::{
    TnuaControllerPlugin, TnuaScheme, TnuaUserControlsSystems,
    builtins::{TnuaBuiltinJump, TnuaBuiltinJumpConfig, TnuaBuiltinWalk, TnuaBuiltinWalkConfig},
    prelude::{TnuaConfig, TnuaController},
};
use bevy_tnua_avian3d::{TnuaAvian3dPlugin, TnuaAvian3dSensorShape};
use feathrise_runtime::{GameView, project::PrimarySceneView};
use feathrise_scene::{ScenePlacementInstance, ScenePlacementRuntimeStatus};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const HERO: &str = "roles/nested/hero.glb";
const PLATFORM: &str = "validation-elevator";
const FLOAT_HEIGHT: f32 = 0.95;

pub struct ProbePlugin {
    pub view: GameView,
}

#[derive(TnuaScheme)]
#[scheme(basis = TnuaBuiltinWalk)]
enum RiderControls {
    Jump(TnuaBuiltinJump),
}

#[derive(Component)]
struct Rider;

#[derive(Resource)]
struct Probe {
    output: PathBuf,
    started: Instant,
    lift_travel: f32,
    lift_rotation: Quat,
    frame: f32,
    hips: Vec3,
    arm: Quat,
    animation: Option<(Entity, AnimationNodeIndex)>,
    seeked: bool,
    pose: Option<Value>,
    model: Option<Entity>,
    rider: Option<Entity>,
    aboard: Option<(f32, f32)>,
    top: f32,
    rotation_reached: bool,
    max_gap_error: f32,
    ascent_samples: u32,
    descent_samples: u32,
    previous_height: Option<f32>,
    jump_seconds: f32,
    jump_from: Option<(f32, f32)>,
    jumped: Option<(f32, f32)>,
    finishing: bool,
}

impl Plugin for ProbePlugin {
    fn build(&self, app: &mut App) {
        let output = PathBuf::from(
            std::env::var_os("FEATHRISE_ANIMATION_WORKSPACES_GAME_DIR")
                .expect("游戏探针需要本轮独占目录"),
        );
        let expected: Value =
            serde_json::from_slice(&fs::read(output.join("expected.json")).expect("缺少作者报告"))
                .expect("作者报告无效");
        let frame = expected["pose"]["time"].as_f64().expect("缺少姿势帧") as f32;
        let hips = Vec3::from_array(
            serde_json::from_value(expected["pose"]["hips_translation"].clone())
                .expect("骨盆位置无效"),
        );
        let arm = Quat::from_array(
            serde_json::from_value(expected["pose"]["arm_rotation"].clone()).expect("左臂旋转无效"),
        );
        let positions = expected["motion"]["track"]["position"].as_array().expect("缺少平台位置键");
        let lift_travel = (positions.last().unwrap()["value"][1].as_f64().unwrap()
            - positions[0]["value"][1].as_f64().unwrap()) as f32;
        let lift_rotation = Quat::from_array(serde_json::from_value(expected["motion"]["preview_rotation"].clone())
            .expect("缺少编辑器实测平台旋转"));
        app.add_plugins((
            super::controller::ProjectScenePlugin { view: self.view },
            TnuaControllerPlugin::<RiderControls>::new(FixedUpdate),
            TnuaAvian3dPlugin::new(FixedUpdate),
        ))
        .insert_resource(Probe {
            output,
            started: Instant::now(),
            lift_travel,
            lift_rotation,
            frame,
            hips,
            arm,
            animation: None,
            seeked: false,
            pose: None,
            model: None,
            rider: None,
            aboard: None,
            top: f32::NEG_INFINITY,
            rotation_reached: false,
            max_gap_error: 0.0,
            ascent_samples: 0,
            descent_samples: 0,
            previous_height: None,
            jump_seconds: 0.0,
            jump_from: None,
            jumped: None,
            finishing: false,
        })
        .insert_resource(bevy::winit::WinitSettings::continuous())
        .add_systems(Startup, spawn_model)
        .add_systems(Update, spawn_rider)
        .add_systems(FixedUpdate, drive_character.in_set(TnuaUserControlsSystems))
        .add_systems(PostUpdate, observe_game.after(AnimationSystems));
    }
}

fn spawn_model(mut commands: Commands, mut probe: ResMut<Probe>, assets: Res<AssetServer>) {
    // 同一份保存的动作在平台尚未就绪时也能由游戏播放器独立运行。
    probe.model = Some(
        commands
            .spawn((
                WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(HERO))),
                Transform::from_xyz(0.0, 0.5, 0.0),
                Visibility::Inherited,
            ))
            .observe(play_saved_clip)
            .id(),
    );
}

fn spawn_rider(
    mut commands: Commands,
    mut probe: ResMut<Probe>,
    mut configs: ResMut<Assets<<RiderControls as TnuaScheme>::Config>>,
    platforms: Query<(&ScenePlacementInstance, &RigidBody, &Position), Without<RigidBodyDisabled>>,
) {
    if probe.rider.is_some() {
        return;
    }
    let Some((_, _, position)) = platforms.iter().find(|(instance, body, _)| {
        instance.id() == PLATFORM && instance.is_active() && **body == RigidBody::Kinematic
    }) else {
        return;
    };
    let config = configs.add(RiderControlsConfig {
        basis: TnuaBuiltinWalkConfig {
            float_height: FLOAT_HEIGHT,
            cling_distance: 0.35,
            ..default()
        },
        jump: TnuaBuiltinJumpConfig {
            height: 1.12,
            ..default()
        },
    });
    // 驾驶输入只交给 Tnua；角色从生成起就由 Avian 动态体驱动，模型只是动态体的子实体。
    let rider = commands
        .spawn((
            Rider,
            RigidBody::Dynamic,
            Collider::capsule(0.29, 0.9),
            LockedAxes::ROTATION_LOCKED,
            Visibility::Inherited,
            Transform::from_translation(position.0 + Vec3::Y * (0.25 + FLOAT_HEIGHT)),
            TnuaController::<RiderControls>::default(),
            TnuaConfig::<RiderControls>(config),
            TnuaAvian3dSensorShape(Collider::cylinder(0.27, 0.0)),
        ))
        .id();
    if let Some(model) = probe.model {
        commands
            .entity(model)
            .insert((ChildOf(rider), Transform::from_xyz(0.0, -FLOAT_HEIGHT, 0.0)));
    }
    probe.rider = Some(rider);
}

fn play_saved_clip(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    mut probe: ResMut<Probe>,
    children: Query<&Children>,
    mut players: Query<&mut AnimationPlayer>,
    assets: Res<AssetServer>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    for entity in children.iter_descendants(ready.entity) {
        if let Ok(mut player) = players.get_mut(entity) {
            let clip = assets.load(GltfAssetLabel::Animation(0).from_asset(HERO));
            let (graph, animation) = AnimationGraph::from_clip(clip);
            commands
                .entity(entity)
                .insert(AnimationGraphHandle(graphs.add(graph)));
            player.play(animation).repeat().set_speed(0.75);
            probe.animation = Some((entity, animation));
            break;
        }
    }
}

fn drive_character(
    mut probe: ResMut<Probe>,
    mut riders: Query<&mut TnuaController<RiderControls>, With<Rider>>,
    time: Res<Time<Fixed>>,
) {
    let Ok(mut controller) = riders.single_mut() else {
        return;
    };
    controller.initiate_action_feeding();
    if probe.jump_seconds > 0.0 {
        controller.action(RiderControls::Jump(TnuaBuiltinJump::default()));
        probe.jump_seconds -= time.delta_secs();
    }
}

fn observe_game(world: &mut World) {
    world.resource_scope(|world, mut probe: Mut<Probe>| {
        if probe.finishing {
            return;
        }
        sample_animation(world, &mut probe);
        sample_elevator(world, &mut probe);
        if (probe.pose.is_some() && probe.jumped.is_some() && probe.rotation_reached)
            || probe.started.elapsed() > Duration::from_secs(45)
        {
            let passed = probe.pose.is_some() && probe.jumped.is_some() && probe.rotation_reached;
            let mut report = json!({
                "passed":passed,
                "animation":probe.pose,
                "platform":{"id":PLATFORM,"up_and_down_with_rider":probe.jump_from.is_some(),
                    "jumped_off":probe.jumped.is_some(),"ascent_samples":probe.ascent_samples,
                    "descent_samples":probe.descent_samples,"top_y":probe.top,"aboard":probe.aboard,
                    "max_rider_height_gap_error":probe.max_gap_error,"jump_from":probe.jump_from,
                    "jumped":probe.jumped,"rotation_reached":probe.rotation_reached},
                "error":(!passed).then_some("超时前缺少两根骨骼、平台旋转或完整载人往返的证据"),
            });
            probe.finishing = true;
            let output = probe.output.clone();
            if !passed {
                report["observed"] = diagnostics(world, &probe);
                write_result(world, &output, report);
                return;
            }
            world.spawn(Screenshot::primary_window()).observe(
                move |event: On<ScreenshotCaptured>, mut commands: Commands| {
                    let mut report = report.clone();
                    if let Err(error) = event
                        .image
                        .clone()
                        .try_into_dynamic()
                        .map_err(|error| error.to_string())
                        .and_then(|image| {
                            image
                                .to_rgb8()
                                .save(output.join("game-motion.png"))
                                .map_err(|error| error.to_string())
                        })
                    {
                        report["passed"] = json!(false);
                        report["error"] = json!(error);
                    }
                    let output = output.clone();
                    commands.queue(move |world: &mut World| write_result(world, &output, report));
                },
            );
        }
    });
}

fn diagnostics(world: &mut World, probe: &Probe) -> Value {
    let placements = world
        .get_resource::<ScenePlacementRuntimeStatus>()
        .map(|status| format!("{status:?}"));
    let physics = world
        .get_resource::<Time<Physics>>()
        .map(|time| time.is_paused());
    let mut query = world.query::<(
        &ScenePlacementInstance,
        Option<&RigidBody>,
        Option<&Position>,
        Option<&InheritedVisibility>,
        Option<&ChildOf>,
        Has<RigidBodyDisabled>,
    )>();
    let entities: Vec<_> = query.iter(world).map(|(instance, body, position, visibility, parent, disabled)| json!({
        "id":instance.id(),"active":instance.is_active(),"body":body.map(|body| format!("{body:?}")),
        "position":position.map(|position| position.0.to_array()),
        "visible":visibility.map(|visible| visible.get()),"disabled":disabled,
        "parent_visible":parent.and_then(|parent| world.get::<InheritedVisibility>(parent.parent()))
            .map(|visible| visible.get()),
    })).collect();
    let rider = probe.rider.and_then(|entity| {
        let position = world.get::<Position>(entity)?.0;
        let controller = world.get::<TnuaController<RiderControls>>(entity)?;
        Some(json!({"position":position.to_array(),
            "standing_on":controller.basis_memory.standing_on_entity().map(|entity| entity.to_bits())}))
    });
    json!({"placement_status":placements,"physics_paused":physics,"placements":entities,
        "rider":rider,"animation_player":probe.animation.is_some(),"animation_seeked":probe.seeked})
}

fn write_result(world: &mut World, output: &Path, mut report: Value) {
    report["source"] = json!("Bevy AnimationPlayer, Avian Position and Tnua standing body");
    let written = serde_json::to_vec_pretty(&report)
        .map_err(|error| error.to_string())
        .and_then(|bytes| {
            fs::write(output.join("game-motion.json"), bytes).map_err(|error| error.to_string())
        });
    if let Err(error) = &written {
        error!("游戏验收报告写入失败：{error}");
    }
    world.write_message(if written.is_ok() && report["passed"] == true {
        AppExit::Success
    } else {
        AppExit::error()
    });
}

fn sample_animation(world: &mut World, probe: &mut Probe) {
    if probe.pose.is_some() {
        return;
    }
    let Some((player, clip)) = probe.animation else {
        return;
    };
    let (Some(hips), Some(arm)) = ({
        let mut query = world.query::<(&Name, &Transform)>();
        let mut hips = None;
        let mut arm = None;
        for (name, transform) in query.iter(world) {
            match name.as_str() {
                "mixamorig:Hips" => hips = Some(transform.translation),
                "mixamorig:LeftArm" => arm = Some(transform.rotation),
                _ => (),
            }
        }
        (hips, arm)
    }) else {
        return;
    };
    let Some(mut player) = world.get_mut::<AnimationPlayer>(player) else {
        return;
    };
    let Some(playing) = player.animation_mut(clip) else {
        return;
    };
    // 先证明正常向前播放改变了两个关节，之后用同一个播放器在保存帧精确取样。
    if !probe.seeked {
        if playing.seek_time() > 0.15
            && hips.x.abs() > 0.035
            && arm.angle_between(Quat::IDENTITY) > 0.06
        {
            playing.set_seek_time(probe.frame).pause();
            probe.seeked = true;
        }
    } else if hips.distance(probe.hips) < 0.002 && arm.dot(probe.arm).abs() > 0.9999 {
        probe.pose = Some(
            json!({"sampled_both_bones":true,"advanced_before_seek":true,"time":probe.frame,
            "hips_translation":hips.to_array(),"arm_rotation":arm.to_array()}),
        );
    }
}

fn sample_elevator(world: &mut World, probe: &mut Probe) {
    let mut platforms = world.query_filtered::<(
        Entity,
        &ScenePlacementInstance,
        &RigidBody,
        &Position,
        &Rotation,
        &Transform,
    ), Without<RigidBodyDisabled>>();
    let Some((root, _, _, platform, rotation, displayed)) =
        platforms.iter(world).find(|(_, instance, body, _, _, _)| {
            instance.id() == PLATFORM && instance.is_active() && **body == RigidBody::Kinematic
        })
    else {
        return;
    };
    let y = platform.0.y;
    if (displayed.translation.y - y).abs() > 0.12 {
        return;
    }
    let rotation = rotation.0;
    let displayed_rotation = displayed.rotation;
    let mut riders =
        world.query_filtered::<(&Position, &TnuaController<RiderControls>), With<Rider>>();
    let Ok((rider, controller)) = riders.single(world) else {
        return;
    };
    let relative = rider.0.y - y;
    let supported = controller
        .basis_memory
        .standing_on_entity()
        .is_some_and(|hit| {
            hit == root
                || world
                    .get::<ColliderOf>(hit)
                    .is_some_and(|collider| collider.body == root)
        });
    if let Some((_, base)) = probe.jump_from {
        if (!supported || controller.is_airborne().unwrap_or(false)) && relative > base + 0.28 {
            probe.jumped = Some((y, rider.0.y));
        }
        return;
    }
    if !supported {
        return;
    }
    let (origin, gap) = *probe.aboard.get_or_insert((y, relative));
    let previous = probe.previous_height.replace(y).unwrap_or(y);
    probe.top = probe.top.max(y);
    probe.max_gap_error = probe.max_gap_error.max((relative - gap).abs());
    probe.ascent_samples += u32::from(y > previous + 0.001);
    probe.descent_samples += u32::from(y < previous - 0.001);
    let travel = probe.lift_travel;
    if (y - origin - travel).abs() < 0.04 && rotation.angle_between(probe.lift_rotation) < 0.04
        && displayed_rotation.angle_between(rotation) < 0.04 && rotation.angle_between(Quat::IDENTITY) > 0.3
    {
        probe.rotation_reached = true; // 同一个主 GLB 的旋转键同时控制实际刚体和视口模型。
    }
    let frame = Vec3::new(0.0, (origin + probe.top) * 0.5 + 0.6, 0.0);
    let mut camera = world.query_filtered::<&mut Transform, With<PrimarySceneView>>();
    if let Ok(mut view) = camera.single_mut(world) {
        *view = Transform::from_translation(frame + Vec3::new(6.4, 2.7, 8.2))
            .looking_at(frame, Vec3::Y);
    }
    if probe.top - origin > travel - 0.035
        && (y - origin).abs() < 0.02
        && probe.ascent_samples >= 10
        && probe.descent_samples >= 10
        && probe.max_gap_error < 0.29
    {
        probe.jump_from = Some((y, relative));
        // Tnua 将结束供给视为松键并缩短跳跃，保持一段真实按键时长再验收脱离。
        probe.jump_seconds = 0.30;
    }
}
