//! 两个独立的真实编辑器进程通过原文件菜单、轨道按钮、原生输入和 GPU 回读验证创建、存盘与重开。
use super::{
    view::{Action, Field, Subtitle},
    *,
};
use crate::ui::{
    file_manager::{
        FileAction, FileManagerButton, FileManagerContextRequest, FileNameInput, OpenManagedFile,
    },
    file_manager_validation::{click, evidence, fixture_focus, mouse, move_cursor, visible},
    sidebars::{EditorLeftSidebarTab, EditorSidebarTab, SceneEditorSidebarTabButton},
};
use bevy::{
    app::AppExit,
    text::{EditableText, TextEdit},
    window::PrimaryWindow,
};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
const NAME: &str = "交付验收.cinematic.json";
#[derive(Resource)]
struct Run {
    work: PathBuf,
    root: PathBuf,
    phase: String,
    stage: usize,
    scene_requested: bool,
    hover: Option<Entity>,
    pointer: Option<bevy::math::DVec2>,
    drag_step: u8,
    since: Instant,
    click: Option<(Entity, bool)>,
    capture: Option<evidence::CaptureTask>,
    shot: Option<Value>,
    base: Option<Transform>,
}
pub(crate) fn install(app: &mut App, work: PathBuf, phase: String) -> Result<(), String> {
    if !matches!(phase.as_str(), "edit" | "reopen")
        || std::fs::read(work.join(".cinematic-author-fixture"))
            .ok()
            .as_deref()
            != Some(b"cinematic-author-v1")
    {
        return Err("演出作者验收归属或阶段无效".into());
    }
    app.insert_resource(Run {
        root: work.join("project/assets"),
        work,
        phase,
        stage: 0,
        scene_requested: false,
        hover: None,
        pointer: None,
        drag_step: 0,
        since: Instant::now(),
        click: None,
        capture: None,
        shot: None,
        base: None,
    })
    .add_systems(PreUpdate, hold.before(bevy::ui::UiSystems::Focus))
    .add_systems(
        Update,
        fixture_focus.before(crate::SceneEditorSystemSet::Input),
    )
    .add_systems(Last, drive);
    Ok(())
}
fn hold(world: &mut World) {
    let run = world.resource::<Run>();
    let point = run.pointer.or_else(|| {
        run.click
            .map(|(e, _)| e)
            .or(run.hover)
            .and_then(|e| world.get::<UiGlobalTransform>(e))
            .map(|p| p.translation.as_dvec2())
    });
    if let Some(pt) = point
        && let Some(w) = world
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .iter(world)
            .next()
    {
        move_cursor(world, w, pt);
    }
}
fn action(world: &mut World, r: &mut Run, w: Entity, a: Action) -> bool {
    let e = visible(world, |b: &Action| *b == a);
    click(world, &mut r.click, w, e)
}
fn type_in<T: Component>(world: &mut World, select: impl Fn(&T) -> bool, value: &str) -> bool {
    let Some(e) = visible(world, select) else {
        return false;
    };
    let Some(mut t) = world.get_mut::<EditableText>(e) else {
        return false;
    };
    if t.value() == value {
        return true;
    }
    if t.pending_edits.is_empty() {
        t.pending_edits
            .extend([TextEdit::SelectAll, TextEdit::Insert(value.into())]);
    }
    false
}
fn set(world: &mut World, field: Field, v: &str) -> bool {
    type_in(world, |f: &Field| *f == field, v)
}
fn stable(world: &World) -> bool {
    world
        .get_resource::<Studio>()
        .is_some_and(|s| s.job.is_none() && s.timeline.is_some())
}
fn drive(world: &mut World) {
    world.resource_scope(|world, mut run: Mut<Run>| {
        if run.stage == usize::MAX {
            return;
        }
        let Some(w) = world
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .iter(world)
            .next()
        else {
            return;
        };
        let result = if run.since.elapsed() > Duration::from_secs(35) {
            let state = world.get_resource::<Studio>();
            let doc = world
                .get_resource::<DocumentState>()
                .and_then(|d| d.current())
                .map(|d| (&d.path, &d.status));
            Err(format!(
                "步骤 {} 等待超时；演出 {:?}，文档 {:?}，状态 {:?}，文件操作 {:?}，回执 {:?}，按压 {:?}",
                run.stage,
                state.map(|s| (&s.notice, s.lane, s.kind, s.cursor, s.job.is_some())),
                doc,
                world
                    .get_resource::<crate::resources::SceneEditorState>()
                    .map(|s| &s.status),
                world.get_resource::<crate::ui::file_manager::FileManagerState>().map(|s|&s.notice),
                world.get_resource::<feathrise_editor_project_browser_model::file_operations::FileOperationService>()
                    .and_then(|service|service.snapshot()).and_then(|s|s.completed.last().map(|r|format!("{:?} {:?}",r.kind,r.error))),
                run.click.map(|(e,b)|(e,b,world.get::<feathrise_editor_ui::UiInteraction>(e)))
            ))
        } else {
            step(world, &mut run, w)
        };
        match result {
            Ok(true) => {
                run.stage += 1;
                run.since = Instant::now();
            }
            Ok(false) => {}
            Err(e) => finish(world, &mut run, Some(e)),
        }
    });
}
fn step(world: &mut World, r: &mut Run, w: Entity) -> Result<bool, String> {
    if !world
        .get_resource::<crate::resources::SceneEditorState>()
        .is_some_and(|e| e.open && !e.scene_transition_in_progress)
    {
        return Ok(false);
    }
    let path = r.root.join(NAME);
    if !r.scene_requested {
        world
            .resource_mut::<crate::resources::SceneEditorState>()
            .queue_scene_transition(
                feathrise_scene_content::SceneId::parse("M1-SAMPLE")?,
                "打开本轮剧情演出夹具场景",
            );
        r.scene_requested = true;
        return Ok(false);
    }
    if !world
        .resource::<crate::authoring::EditorAuthoringSession>()
        .draft
        .as_ref()
        .is_some_and(|d| d.scene_id.as_str() == "M1-SAMPLE")
    {
        return Ok(false);
    }
    if r.phase == "reopen" {
        return Ok(match r.stage {
            0 => {
                world.write_message(OpenManagedFile { path });
                true
            }
            1 => {
                if !stable(world) {
                    return Ok(false);
                }
                let s = world.resource::<Studio>();
                let f = &s.timeline.as_ref().unwrap().film;
                if f.camera.len() != 2
                    || f.actors.len() != 1
                    || f.objects.len() != 1
                    || f.dialogue.len() != 2
                {
                    return Err("新进程四轨回读结果不完整".into());
                }
                true
            }
            2 => action(world, r, w, Action::Lane(Lane::Dialogue)),
            3 => action(world, r, w, Action::Next),
            4 => {
                let Some(field) = visible(world, |f: &Field| *f == Field::Name) else {
                    return Ok(false);
                };
                let shaped = world
                    .get::<bevy::text::TextLayoutInfo>(field)
                    .is_some_and(|layout| layout.glyphs.len() >= 5);
                if !shaped {
                    return Ok(false);
                }
                if !world
                    .query_filtered::<&Text, With<Subtitle>>()
                    .iter(world)
                    .any(|t| t.as_str().contains("守卫接过剑"))
                {
                    return Ok(false);
                }
                let bounds = Rect::from_center_size(
                    world.get::<UiGlobalTransform>(field).unwrap().translation,
                    world.get::<ComputedNode>(field).unwrap().size(),
                );
                let interior = URect::from_corners(
                    (bounds.min + Vec2::splat(4.)).ceil().as_uvec2(),
                    (bounds.max - Vec2::splat(4.)).floor().as_uvec2(),
                );
                let Some(shot) = evidence::capture_field(
                    world,
                    &mut r.capture,
                    &r.work.join("cinematic-reopen.png"),
                    Some(interior),
                )?
                else {
                    return Ok(false);
                };
                r.shot = Some(shot);
                true
            }
            5 => drag_camera(world, r, w),
            6 => camera_time(world).is_some_and(|t| (t - 3.5).abs() < 0.02),
            7 => action(world, r, w, Action::Undo),
            8 => camera_time(world) == Some(4.),
            9 => action(world, r, w, Action::Redo),
            10 => camera_time(world).is_some_and(|t| (t - 3.5).abs() < 0.02),
            11 => action(world, r, w, Action::Undo),
            12 => camera_time(world) == Some(4.) && action(world, r, w, Action::Export),
            13 => {
                if visible(world, |name: &Name| {
                    name.as_str() == "CinematicExportDialog"
                })
                .is_some()
                {
                    finish(world, r, None)
                }
                false
            }
            _ => false,
        });
    }
    if (6..=31).contains(&r.stage) && !stable(world) {
        return Ok(false);
    }
    let ready = match r.stage {
        0 => {
            if world
                .get_resource::<crate::resources::SceneEditorViewportState>()
                .is_none()
            {
                return Ok(false);
            }
            world.write_message(FileManagerContextRequest {
                position: Vec2::new(160., 155.),
                directory: r.root.clone(),
                sources: default(),
                open_directory: r.root.clone(),
                is_directory: Some(true),
            });
            true
        }
        1 => {
            let Some(e) = visible(world, |b: &FileManagerButton| {
                b.window.is_none() && b.action == FileAction::NewMenu
            }) else {
                return Ok(false);
            };
            let at = world
                .get::<UiGlobalTransform>(e)
                .unwrap()
                .translation
                .as_dvec2();
            move_cursor(world, w, at);
            r.hover = Some(e);
            let shown = visible(world, |b: &FileManagerButton| {
                b.action == FileAction::NewCinematic
            })
            .is_some();
            if shown {
                r.hover = None;
            }
            shown
        }
        2 => {
            let b = visible(world, |b: &FileManagerButton| {
                b.window.is_none() && b.action == FileAction::NewCinematic
            });
            click(world, &mut r.click, w, b)
        }
        3 => type_in(world, |_: &FileNameInput| true, NAME),
        4 => {
            let b = visible(world, |b: &FileManagerButton| {
                b.action == FileAction::Confirm
            });
            click(world, &mut r.click, w, b)
        }
        5 => stable(world),
        6 => {
            let e = world
                .resource::<crate::resources::SceneEditorViewportState>()
                .camera;
            r.base = world.get::<Transform>(e).copied();
            action(world, r, w, Action::Capture)
        }
        7 => {
            if world
                .resource::<Studio>()
                .timeline
                .as_ref()
                .unwrap()
                .film
                .camera
                .len()
                != 1
            {
                return Ok(false);
            }
            set(world, Field::At, "4") && set(world, Field::Position, "8, 3, 7")
        }
        8 => action(world, r, w, Action::New),
        9 => action(world, r, w, Action::Lane(Lane::Actor)),
        10 => {
            set(world, Field::Target, "guard_point#0")
                && set(world, Field::Position, "4, 0, 3")
                && set(world, Field::Rotation, "0, 90, 0")
        }
        11 => action(world, r, w, Action::New),
        12 => action(world, r, w, Action::Clip),
        13 => {
            set(world, Field::Target, "guard_point#0")
                && set(world, Field::Name, "Walk")
                && set(world, Field::Length, "2")
        }
        14 => action(world, r, w, Action::New),
        15 => action(world, r, w, Action::Lane(Lane::Object)),
        16 => set(world, Field::Target, "sword_rack") && set(world, Field::Position, "2, 1, 3"),
        17 => action(world, r, w, Action::New),
        18 => action(world, r, w, Action::Lane(Lane::Dialogue)),
        19 => {
            set(world, Field::Target, "line_guard")
                && set(world, Field::Name, "守卫接过剑")
                && set(world, Field::Length, "3")
        }
        20 => action(world, r, w, Action::New),
        21 => action(world, r, w, Action::Yarn),
        22 => {
            set(world, Field::Target, "yarn_guard")
                && set(world, Field::Name, "GuardDelivery")
                && set(world, Field::At, "3")
        }
        23 => action(world, r, w, Action::New),
        24 => action(world, r, w, Action::Save),
        25 => {
            let d = world.resource::<DocumentState>();
            d.current()
                .is_some_and(|d| !d.dirty() && d.status.contains("已保存"))
        }
        26 => set(world, Field::At, "2"),
        27 => action(world, r, w, Action::Go),
        28 => {
            let s = world.resource::<Studio>();
            let t = &s.timeline.as_ref().unwrap().film;
            let Some(sample) = t.sample(2.).camera else {
                return Err("未能重建中段机位".into());
            };
            let cam = world
                .resource::<crate::resources::SceneEditorViewportState>()
                .camera;
            let current = world.get::<Transform>(cam).ok_or("场景机位消失")?;
            if current
                .translation
                .distance(Vec3::from_array(sample.pose.translation))
                > 0.01
            {
                return Ok(false);
            }
            if !world
                .query_filtered::<&Text, With<Subtitle>>()
                .iter(world)
                .any(|t| t.as_str().contains("守卫接过剑"))
            {
                return Ok(false);
            }
            let Some(shot) =
                evidence::capture(world, &mut r.capture, &r.work.join("cinematic-edit.png"))?
            else {
                return Ok(false);
            };
            r.shot = Some(shot);
            true
        }
        29 => action(world, r, w, Action::Play),
        30 => {
            if world.resource::<Studio>().cursor <= 2. {
                return Ok(false);
            }
            action(world, r, w, Action::Stop)
        }
        31 => {
            let cam = world
                .resource::<crate::resources::SceneEditorViewportState>()
                .camera;
            world
                .get::<Transform>(cam)
                .zip(r.base.as_ref())
                .is_some_and(|(a, b)| a.translation.distance(b.translation) < 0.01)
        }
        32 => {
            let b = visible(world, |b: &SceneEditorSidebarTabButton| {
                b.0 == EditorSidebarTab::Left(EditorLeftSidebarTab::Gameplay)
            });
            click(world, &mut r.click, w, b)
        }
        33 => type_in(
            world,
            |b: &super::binding::BindingField| *b == super::binding::BindingField::Target,
            "east_gate",
        ),
        34 => {
            let b = visible(world, |_: &super::binding::Bind| true);
            click(world, &mut r.click, w, b)
        }
        35 => {
            if !world
                .resource::<DocumentState>()
                .current()
                .is_some_and(|d| d.disk.project_rules && d.dirty())
            {
                return Ok(false);
            }
            let b = visible(world, |a: &super::super::components::DocumentAction| {
                *a == super::super::components::DocumentAction::Save
            });
            click(world, &mut r.click, w, b)
        }
        36 => {
            if world
                .resource::<DocumentState>()
                .current()
                .is_some_and(|d| d.disk.project_rules && !d.dirty() && d.status.contains("已保存"))
            {
                finish(world, r, None)
            }
            false
        }
        _ => false,
    };
    Ok(ready)
}
fn finish(world: &mut World, r: &mut Run, error: Option<String>) {
    let data = json!({"phase":r.phase,"passed":error.is_none(),"stage":r.stage,"error":error,"screenshot":r.shot,
        "input":"原新建菜单、文件提示框、四轨可视按钮、原生 EditableText 编辑队列和 Bevy 鼠标事件","os_input_verified":false});
    let ok = evidence::report(&r.work.join(format!("cinematic-{}.json", r.phase)), &data).is_ok();
    r.stage = usize::MAX;
    world.write_message(if ok && error.is_none() {
        AppExit::Success
    } else {
        AppExit::error()
    });
}

fn camera_time(world: &World) -> Option<f32> {
    if !stable(world) {
        return None;
    }
    world
        .resource::<Studio>()
        .timeline
        .as_ref()?
        .film
        .camera
        .get(1)
        .map(|k| k.time)
}
fn drag_camera(world: &mut World, r: &mut Run, w: Entity) -> bool {
    use bevy::input::ButtonState;
    use feathrise_editor_ui::UiInteraction;
    let Some(marker) = visible(world, |m: &view::MarkerWidget| {
        m.lane == Lane::Camera && m.index == 1
    }) else {
        return false;
    };
    let Some(row) = world.get::<ChildOf>(marker).map(|c| c.parent()) else {
        return false;
    };
    let from = world
        .get::<UiGlobalTransform>(marker)
        .unwrap()
        .translation
        .as_dvec2();
    let dx = world.get::<ComputedNode>(row).unwrap().size().x as f64 * 0.1;
    let to = from - bevy::math::DVec2::new(dx, 0.);
    match r.drag_step {
        0 => {
            r.pointer = Some(from);
            move_cursor(world, w, from);
            r.drag_step += 1;
        }
        1 if world.get::<UiInteraction>(marker) == Some(&UiInteraction::Hovered) => {
            mouse(world, w, ButtonState::Pressed);
            r.drag_step += 1;
        }
        2 if world.get::<UiInteraction>(marker) == Some(&UiInteraction::Pressed) => {
            r.pointer = Some(to);
            move_cursor(world, w, to);
            r.drag_step += 1;
        }
        3 if (world.resource::<Studio>().cursor - 3.5).abs() < 0.02 => {
            mouse(world, w, ButtonState::Released);
            r.pointer = None;
            r.drag_step += 1;
            return true;
        }
        _ => {}
    }
    false
}
