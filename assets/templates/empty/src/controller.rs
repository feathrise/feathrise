//! 新工程的场景视图与 Bevy 原生自由相机；编辑器在运行阶段交出相机和键鼠。
use bevy::camera_controller::free_camera::{FreeCamera, FreeCameraPlugin, FreeCameraState};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use feathrise_runtime::{GameView, project::ProjectViewOrigin};
use feathrise_runtime::project::{PrimarySceneView, SceneRuntimeObserver};
use feathrise_runtime::project::embedded::{EmbeddedProjectState, ProjectGameplay, project_simulation_active};

pub struct ProjectScenePlugin {
    pub view: GameView,
}
#[derive(Resource)]
struct View(GameView);

impl Plugin for ProjectScenePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(View(self.view))
            .add_plugins(FreeCameraPlugin)
            // 工程自定义移动/AI/交互加入此集合；嵌入编辑态暂停，预览后在同一World恢复。
            .configure_sets(Update, ProjectGameplay.run_if(project_simulation_active))
            .add_systems(Startup, spawn_observer)
            // 原生控制器先于 Update 在 RunFixedMainLoop 运行，必须在 PreUpdate 关闭其许可；
            // 不能只把自己的 Update 系统放进 ProjectGameplay，就误认为原生控制器也被暂停。
            .add_systems(PreUpdate, synchronize_controls.after(bevy::input::InputSystems))
            .add_systems(Update, (initialize_view, synchronize_origin).chain().before(ProjectGameplay))
            .add_systems(Update, synchronize_observer.in_set(ProjectGameplay));
        #[cfg(feature = "function_hotpatch")]
        app.add_systems(Update, synchronize_speed.before(ProjectGameplay));
    }
}

fn spawn_observer(mut commands: Commands, origin: Res<ProjectViewOrigin>) {
    commands.spawn((SceneRuntimeObserver, Transform::from_translation(origin.0)));
}

fn synchronize_origin(origin: Res<ProjectViewOrigin>,
    mut observers: Query<&mut Transform, With<SceneRuntimeObserver>>) {
    // 常驻工程先建空 World，再接收作者场景；驻留观察点须随新场景出生点更新。
    if !origin.is_changed() { return; }
    for mut transform in &mut observers { transform.translation = origin.0; }
}

fn synchronize_controls(
    project: Option<Res<EmbeddedProjectState>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<&mut FreeCameraState, With<PrimarySceneView>>,
) {
    let enabled = project_simulation_active(project)
        && windows.single().is_ok_and(|window| window.focused);
    for mut state in &mut cameras {
        if enabled && !state.enabled {
            // 编辑器曾借用同一台主相机；恢复时由原生控制器重新读取已归还的运行姿态，
            // 避免缓存的作者 yaw/pitch 让第一次环视跳回错误方向。滚轮设定的速度保留。
            let speed_multiplier = state.speed_multiplier;
            *state = FreeCameraState::default();
            state.speed_multiplier = speed_multiplier;
        }
        state.enabled = enabled;
        if !enabled {
            // 原生控制器使用真实时间；暂停必须清掉惯性及轴向动画，返回后才不会自行滑动。
            state.velocity = Vec3::ZERO;
            state.rotation_curve = None;
        }
    }
}

fn synchronize_observer(
    cameras: Query<&Transform, (With<PrimarySceneView>, Without<SceneRuntimeObserver>)>,
    mut observers: Query<&mut Transform, With<SceneRuntimeObserver>>,
) {
    let Ok(camera) = cameras.single() else { return; };
    // 自由相机就是空模板的实际观察点；移动后植被、碰撞代理和资源流送应跟随它。
    for mut observer in &mut observers { observer.translation = camera.translation; }
}

fn initialize_view(mut commands: Commands, view: Res<View>, origin: Res<ProjectViewOrigin>,
    mut cameras: Query<(Entity, &mut Transform), Added<PrimarySceneView>>) {
    // 两种元数据只决定进入时的观察角度；进入后都可自由飞行，不附带角色或碰撞电机。
    let (horizontal, vertical, distance) = match view.0 {
        GameView::ThirdPerson => (0.0_f32, -0.3_f32, 5.0),
        GameView::FixedTopDown => (std::f32::consts::FRAC_PI_4, -55.0_f32.to_radians(), 18.0),
    };
    let target = origin.0 + Vec3::Y * 0.8;
    let offset = Vec3::new(
        -horizontal.sin() * vertical.cos(), -vertical.sin(), horizontal.cos() * vertical.cos(),
    ) * distance;
    // 场景生命周期拥有唯一主相机；只在此相机附加控制器，切换场景时不能另叠一台窗口视图。
    for (entity, mut transform) in &mut cameras {
        *transform = Transform::from_translation(target + offset).looking_at(target, Vec3::Y);
        let mut controls = FreeCameraState::default();
        controls.enabled = false; // 下一帧按预览/独立运行及窗口焦点开门，作者编辑时始终不抢输入。
        commands.entity(entity).insert((FreeCamera { walk_speed: walk_speed(), run_speed: run_speed(), ..default() }, controls));
    }
}

// 可热更新的纯数值边界；函数签名、控制器类型和系统注册保持稳定。
fn walk_speed() -> f32 { 12.0 }
fn run_speed() -> f32 { 15.0 }

#[cfg(feature = "function_hotpatch")]
fn synchronize_speed(mut cameras: Query<&mut FreeCamera, With<PrimarySceneView>>) {
    let (walk, run) = bevy::app::hotpatch::call(|| (walk_speed(), run_speed()));
    for mut camera in &mut cameras {
        // 更新现有组件而非重建相机；FreeCameraState的速度倍率、惯性和运行姿态继续保留。
        camera.walk_speed = walk;
        camera.run_speed = run;
    }
}
