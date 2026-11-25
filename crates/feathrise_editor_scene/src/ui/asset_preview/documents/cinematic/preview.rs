//! 取样不运行 Yarn 或任务。镜头借用中央离屏相机，物件按当前场景的稳定摆放索引查找；离开试播归还姿态。
use super::{
    view::{FilmImage, Subtitle},
    *,
};
use crate::{
    authoring::EditorAuthoringSession, resources::SceneEditorViewportState,
    ui::placement_water::PlacementInstanceIndex,
};
use bevy::{
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
    ui::RelativeCursorPosition,
};
use feathrise_cinematic::{CameraPose, Pose};
use feathrise_scene::ScenePlacementInstance;
mod objects;
/// 宿主只得到已校验文档的不可变原字节与当前时刻，嵌入工程按协议呈现真实游戏演员。
#[derive(Resource, Default)]
pub struct CinematicPreview {
    pub active: Option<CinematicPreviewFrame>,
}
pub struct CinematicPreviewFrame {
    pub bytes: Arc<Vec<u8>>,
    pub id: String,
    pub time: f32,
    pub playing: bool,
}
pub(super) fn install(app: &mut App) {
    app.init_resource::<CinematicPreview>()
        .init_resource::<Preview>()
        .add_systems(
            Update,
            orbit
                .after(super::controls::input)
                .before(crate::viewport::sync_editor_viewport_camera)
                .in_set(crate::SceneEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            apply
                .after(super::sync)
                .after(crate::viewport::sync_editor_viewport_camera)
                .in_set(crate::SceneEditorSystemSet::Presentation),
        );
}
fn orbit(
    s: Res<Studio>,
    docs: Res<DocumentState>,
    vp: Option<ResMut<SceneEditorViewportState>>,
    image: Query<&RelativeCursorPosition, With<FilmImage>>,
    motion: Res<AccumulatedMouseMotion>,
    wheel: Res<AccumulatedMouseScroll>,
    buttons: Res<ButtonInput<MouseButton>>,
) {
    if s.previewing
        || active(&docs).is_none()
        || !image
            .single()
            .is_ok_and(RelativeCursorPosition::cursor_over)
    {
        return;
    }
    let Some(mut v) = vp else { return };
    if buttons.pressed(MouseButton::Right) && motion.delta != Vec2::ZERO {
        v.yaw -= motion.delta.x * 0.006;
        v.pitch = (v.pitch + motion.delta.y * 0.006).clamp(-1.55, 1.55);
    }
    if buttons.pressed(MouseButton::Middle) && motion.delta != Vec2::ZERO {
        let right = Vec3::new(v.yaw.cos(), 0., -v.yaw.sin());
        let up = Vec3::new(
            -v.yaw.sin() * v.pitch.sin(),
            v.pitch.cos(),
            -v.yaw.cos() * v.pitch.sin(),
        );
        let shift = (-right * motion.delta.x + up * motion.delta.y) * v.extent
            / v.render_size.y.max(1) as f32;
        v.center += shift;
    }
    if wheel.delta.y != 0. {
        v.extent = (v.extent * (-wheel.delta.y * 0.1).exp()).max(0.1);
    }
}
struct TargetPose {
    target: String,
    instance: Option<String>,
    pose: Pose,
}
struct Picture {
    time: f32,
    camera: Option<CameraPose>,
    objects: Vec<TargetPose>,
    subtitle: String,
}
#[derive(Resource, Default)]
struct Preview {
    work: Option<(Arc<Timeline>, Task<Picture>)>,
    frame: Option<(Arc<Timeline>, Arc<Picture>)>,
    camera: Option<(Entity, Transform, Projection)>,
    objects: objects::ObjectPreview,
}
fn as_transform(p: Pose) -> Transform {
    Transform {
        translation: p.translation.into(),
        rotation: Quat::from_array(p.rotation).normalize(),
        scale: p.scale.into(),
    }
}
fn prepare(t: &Timeline, time: f32) -> Picture {
    let sample = t.film.sample(time);
    let objects = sample
        .objects
        .into_iter()
        .filter_map(|s| {
            Some(TargetPose {
                target: s.target.to_owned(),
                instance: s.instance.map(str::to_string),
                pose: s.pose?,
            })
        })
        .collect();
    let subtitle = sample
        .dialogue
        .iter()
        .filter_map(|d| {
            d.text
                .clone()
                .or_else(|| d.yarn.as_ref().map(|n| format!("Yarn · {n}")))
        })
        .collect::<Vec<_>>()
        .join("\n");
    Picture {
        time,
        camera: sample.camera,
        objects,
        subtitle,
    }
}
fn apply(
    s: Res<Studio>,
    docs: Res<DocumentState>,
    mut public: ResMut<CinematicPreview>,
    mut state: ResMut<Preview>,
    editor: Res<crate::resources::SceneEditorState>,
    session: Option<Res<EditorAuthoringSession>>,
    idx: Option<Res<PlacementInstanceIndex>>,
    vp: Option<Res<SceneEditorViewportState>>,
    mut cameras: Query<
        (&mut Transform, &mut Projection),
        With<crate::components::SceneEditorCamera>,
    >,
    mut roots: Query<
        (&ScenePlacementInstance, &mut Transform),
        Without<crate::components::SceneEditorCamera>,
    >,
    mut subtitle: Query<&mut Text, With<Subtitle>>,
) {
    let timeline = if editor.open && s.previewing && active(&docs).as_ref() == s.key.as_ref() {
        s.timeline.as_ref()
    } else {
        None
    };
    if let Some((t, job)) = state.work.as_mut()
        && let Some(p) = check_ready(job)
    {
        let current =
            timeline.is_some_and(|a| Arc::ptr_eq(a, t)) && (s.playing || p.time == s.cursor);
        let (t, _) = state.work.take().unwrap();
        if current {
            state.frame = Some((t, Arc::new(p)));
        }
    }
    if let Some(t) = timeline {
        if public.active.as_ref().is_none_or(|p| {
            !Arc::ptr_eq(&p.bytes, &t.bytes) || p.time != s.cursor || p.playing != s.playing
        }) {
            public.active = Some(CinematicPreviewFrame {
                bytes: t.bytes.clone(),
                id: t.film.id.clone(),
                time: s.cursor,
                playing: s.playing,
            });
        }
        let ready = state
            .frame
            .as_ref()
            .is_some_and(|(a, p)| Arc::ptr_eq(a, t) && p.time == s.cursor);
        if !ready && state.work.is_none() {
            let t = t.clone();
            let time = s.cursor;
            let input = t.clone();
            state.work = Some((
                t,
                AsyncComputeTaskPool::get().spawn(async move { prepare(&input, time) }),
            ));
        }
    } else {
        public.active.take();
        state.frame.take();
    }
    // 已切出文档、向首键之前定位或原目标被替换时，只归还本预览写入的分量。
    let current = state
        .frame
        .take()
        .filter(|(t, _)| timeline.is_some_and(|a| Arc::ptr_eq(a, t)));
    let cam = current.as_ref().and_then(|(_, p)| p.camera);
    if let Some((cam_id, source, projection)) = state.camera.take() {
        if let Ok((mut transform, mut p)) = cameras.get_mut(cam_id) {
            *transform = source;
            *p = projection;
        }
    }
    if let (Some(cam), Some(v)) = (cam, vp)
        && let Ok((mut transform, mut projection)) = cameras.get_mut(v.camera)
    {
        state.camera = Some((v.camera, *transform, projection.clone()));
        *transform = as_transform(cam.pose);
        let mut p = match &*projection {
            Projection::Perspective(p) => p.clone(),
            _ => default(),
        };
        p.fov = cam.fov_y_degrees.to_radians();
        *projection = Projection::Perspective(p);
    }
    let scene = session
        .as_deref()
        .and_then(|session| session.draft.as_ref())
        .map(|draft| &draft.scene_id);
    state
        .objects
        .apply(current.as_ref(), scene, idx.as_deref(), &mut roots);
    for mut text in &mut subtitle {
        let value = current.as_ref().map_or("", |(_, p)| p.subtitle.as_str());
        if text.as_str() != value {
            **text = value.into();
        }
    }
    state.frame = current;
}
