//! 点击交互页和视口绘制真实区域，保存与下一进程重开均从场景文档取证。
use super::*;
use crate::authoring::scene_motion::MotionRegionAction;
use crate::authoring::scene_motion::MotionRegion;
use feathrise_editor_ui::InteractionButton;

#[derive(Default, Debug, Clone, Copy)]
pub(super) enum Check {
    #[default]
    Sidebar,
    Tab,
    Draw,
    FirstCorner,
    SecondCorner,
    Adjust,
    BeginDrag(bool),
    Drag { edge: bool, frame: u8, before: MotionRegion, history: usize },
    UndoEdge { before: MotionRegion, after: MotionRegion },
    RedoEdge { before: MotionRegion, after: MotionRegion },
    CheckRedo(MotionRegion),
    Confirm,
    Present(u8),
    Image,
}

pub(super) fn enabled() -> bool {
    std::env::var("FEATHRISE_MOTION_TRIGGER_VALIDATION").as_deref() == Ok("1")
}

/// 只准备有轨道、尚未绑定触发区的独占夹具；区域仍由真实视口输入绘制。
pub fn prepare_unbound_clip(path: &Path) -> Result<String, String> {
    use crate::authoring::scene_motion::{
        SceneMotionDocument, SceneMotionKey, SceneMotionPlayback, SceneMotionTrack,
    };
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let asset_path = "scenes/M1-SAMPLE/scene.glb";
    let placements = feathrise_scene_content::read_scene_placements(&bytes, asset_path)?;
    let mut motion = SceneMotionDocument::from_glb(path, bytes.clone(), &placements)?;
    motion.create(CLIP_NAME)?;
    let id = motion
        .active_clip_id()
        .ok_or("夹具缺少场景片段")?
        .to_owned();
    let record = placements
        .placements
        .iter()
        .find(|r| r.id == PLATFORM)
        .ok_or("夹具缺少平台摆放")?;
    let [x, y, z] = record.translation;
    motion.replace(
        PLATFORM,
        Some(SceneMotionTrack {
            placement_id: PLATFORM.into(),
            duration_seconds: 2.0,
            playback: SceneMotionPlayback::Once,
            position: vec![
                SceneMotionKey {
                    time: 0.0,
                    value: [x, y, z],
                },
                SceneMotionKey {
                    time: 2.0,
                    value: [x, y + 5.0, z],
                },
            ],
            rotation: Vec::new(),
            scale: Vec::new(),
            collider: None,
        }),
    );
    let (bytes, _) = motion.encode_glb(path, bytes, &placements, false)?;
    std::fs::write(path, bytes).map_err(|e| e.to_string())?;
    Ok(id)
}

pub fn read_saved_trigger_regions(path: &Path, id: &str) -> Result<Value, String> {
    let doc = AnimationDocument::load(path)?;
    let clip = doc
        .clips
        .iter()
        .find(|c| {
            doc.clip_extra(c.source_index, "feathrise_clip_id")
                .and_then(Value::as_str)
                == Some(id)
        })
        .ok_or("磁盘主场景缺少指定片段")?;
    doc.clip_extra(clip.source_index, "feathrise_motion_triggers")
        .cloned()
        .ok_or_else(|| "磁盘主场景缺少触发区域".into())
}

pub(super) fn report(world: &World, run: &Run) -> Result<Value, String> {
    let session = world.resource::<EditorAuthoringSession>();
    if session.draft.as_ref().is_none_or(|d| d.motion.dirty)
        || world.resource::<SceneEditorState>().dirty
    {
        return Err("触发区完成后场景仍为未保存状态".into());
    }
    Ok(
        json!({"passed":true,"reopened":run.reopen,"motion_trigger":proof(world)?,"save_sequence":session.successful_save_sequence}),
    )
}

pub(super) fn save_shortcut(world: &mut World, press: bool) -> Result<(), String> {
    use crate::standalone::validation_fixture::key_state;
    use bevy::input::{ButtonState, keyboard::Key};
    let window = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
        .map_err(|e| e.to_string())?;
    if press {
        key_state(
            world,
            window,
            KeyCode::ControlLeft,
            Key::Control,
            ButtonState::Pressed,
        );
        key_state(
            world,
            window,
            KeyCode::KeyS,
            Key::Character("s".into()),
            ButtonState::Pressed,
        );
    } else {
        key_state(
            world,
            window,
            KeyCode::KeyS,
            Key::Character("s".into()),
            ButtonState::Released,
        );
        key_state(
            world,
            window,
            KeyCode::ControlLeft,
            Key::Control,
            ButtonState::Released,
        );
    }
    Ok(())
}

pub(super) fn named_button(world: &mut World, label: &str) -> Option<Entity> {
    let locale = *world.resource::<feathrise_editor_ui::localization::EditorLocale>();
    let label = locale.editor_label(label);
    world
        .query::<(&ChildOf, &Text, &ComputedNode, &InheritedVisibility)>()
        .iter(world)
        .filter(|(_, text, node, v)| text.as_str() == label && visible(node, v))
        .find_map(|(parent, ..)| {
            world
                .get::<InteractionButton>(parent.parent())
                .map(|_| parent.parent())
        })
}

fn corner(world: &mut World, run: &mut Run, offset: Vec2) -> Result<bool, String> {
    let (node, transform) = world
        .query_filtered::<(&ComputedNode, &UiGlobalTransform), With<SceneEditorViewport>>()
        .single(world)
        .map_err(|e| e.to_string())?;
    let position = (transform.translation + node.size() * offset).as_dvec2();
    let window = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
        .map_err(|e| e.to_string())?;
    Ok(pointer_click(
        world,
        &mut run.click,
        window,
        position,
        MouseButton::Left,
    ))
}

pub(super) fn advance(world: &mut World, run: &mut Run) -> Result<bool, String> {
    use Check::*;
    match run.triggers {
        Sidebar | Tab | Draw | Adjust => {
            let (text, next) = match run.triggers {
                Sidebar => ("交互", Tab),
                Tab => ("动画触发", if run.reopen { Confirm } else { Draw }),
                Adjust => ("调整区域", BeginDrag(false)),
                _ => ("绘制新区域", FirstCorner),
            };
            let entity = if matches!(run.triggers, Sidebar) {
                sidebar(
                    world,
                    EditorSidebarTab::Left(crate::ui::sidebars::EditorLeftSidebarTab::Gameplay),
                )
            } else {
                named_button(world, text)
            };
            if click(world, run, entity)? {
                run.triggers = next;
            }
        }
        FirstCorner => {
            if corner(world, run, Vec2::new(-0.20, 0.10))? {
                run.triggers = SecondCorner;
            }
        }
        SecondCorner => {
            if corner(world, run, Vec2::new(-0.08, 0.21))? {
                run.triggers = Adjust;
            }
        }
        BeginDrag(edge) => {
            let before = region(world)?;
            let history = world.resource::<EditorAuthoringSession>().undo.len();
            run.triggers = Drag { edge, frame: 0, before, history };
        }
        Drag { edge, frame, before, history } => {
            use crate::ui::file_manager_validation::{move_cursor, mouse};
            use bevy::input::ButtonState;
            let window = world.query_filtered::<Entity, With<PrimaryWindow>>().single(world).map_err(|e| e.to_string())?;
            let handle = before.center + if edge { Vec3::X * before.half_extents.x } else { Vec3::ZERO };
            let delta = Vec3::new(before.half_extents.x * 0.45, 0., if edge { 0. } else { before.half_extents.z * 0.25 });
            let screen = projected(world, if frame < 2 { handle } else { handle + delta })?;
            match frame {
                0 => move_cursor(world, window, screen),
                1 => mouse(world, window, ButtonState::Pressed),
                2 => move_cursor(world, window, screen),
                3 => {
                    if region(world)? != before || world.resource::<EditorAuthoringSession>().undo.len() != history {
                        return Err("拖动中写入了权威触发区或撤销记录".into());
                    }
                    mouse(world, window, ButtonState::Released);
                }
                _ => {
                    let after = region(world)?;
                    let expected_center = before.center + if edge { delta * 0.5 } else { delta };
                    let expected_half = before.half_extents + if edge { delta * 0.5 } else { Vec3::ZERO };
                    let tolerance = delta.length() * 0.08 + 0.02;
                    if !after.center.abs_diff_eq(expected_center, tolerance) || !after.half_extents.abs_diff_eq(expected_half, tolerance)
                        || world.resource::<EditorAuthoringSession>().undo.len() != history + 1 {
                        return Err(format!("区域拖动 {edge} 未按一次操作提交：{before:?} → {after:?}，期望 {expected_center:?} / {expected_half:?}"));
                    }
                    run.triggers = if edge { UndoEdge { before, after } } else { BeginDrag(true) };
                    return Ok(false);
                }
            }
            run.triggers = Drag { edge, frame: frame + 1, before, history };
        }
        UndoEdge { before, after } | RedoEdge { before, after } => {
            let undo = matches!(run.triggers, UndoEdge {..});
            if !undo && run.click.is_none() && region(world)? != before { return Err("撤销拖边未还原刚平移后的区域".into()); }
            let button = action_button(world, if undo { EditorAction::Undo } else { EditorAction::Redo });
            if click(world, run, button)? {
                run.triggers = if undo { RedoEdge { before, after } } else { CheckRedo(after) };
            }
        }
        CheckRedo(expected) => {
            if region(world)? != expected { return Err("重做拖边未还原最终尺寸".into()); }
            run.triggers = Confirm;
        }
        Confirm => {
            proof(world)?;
            // 等待离屏场景与主窗口合成同一帧，重开时截到已绘制的区域线框。
            run.triggers = Present(3);
        }
        Present(0) => {
            request_screenshot(world, run.directory.join("motion-trigger-region.png"));
            run.triggers = Image;
        }
        Present(frames) => run.triggers = Present(frames - 1),
        Image => {
            if let Some(written) = screenshot_result(world) {
                written?;
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn region(world: &World) -> Result<MotionRegion, String> {
    let id = clip_id(world, CLIP_NAME).ok_or("缺少电梯片段")?;
    world.resource::<EditorAuthoringSession>().draft.as_ref().and_then(|d| d.motion.regions(id))
        .and_then(|r| r.first().copied()).ok_or_else(|| "缺少视口绘制的区域".into())
}

pub(super) fn projected(world: &mut World, position: Vec3) -> Result<DVec2, String> {
    let view = world.resource::<crate::resources::SceneEditorViewportState>();
    let camera = world.get::<Camera>(view.camera).ok_or("缺少视口相机")?;
    let pose = world.get::<Transform>(view.camera).ok_or("缺少相机姿态")?;
    let pixel = camera.world_to_viewport(&GlobalTransform::from(*pose), position).map_err(|e| e.to_string())?;
    let normalized = pixel / view.render_size.as_vec2() - Vec2::splat(0.5);
    let (node, transform) = world.query_filtered::<(&ComputedNode, &UiGlobalTransform), With<SceneEditorViewport>>().single(world).map_err(|e| e.to_string())?;
    Ok((transform.translation + node.size() * normalized).as_dvec2())
}

fn proof(world: &World) -> Result<Value, String> {
    let draft = world
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .ok_or("动画触发验收缺少场景")?;
    let id = clip_id(world, CLIP_NAME).ok_or("动画触发验收缺少电梯片段")?;
    let [area] = draft.motion.regions(id).unwrap_or_default() else {
        return Err("电梯片段未保存唯一的视口触发区".into());
    };
    if !area.valid()
        || (area.center.y - 1.0).abs() > 1e-5
        || area.half_extents.y != 1.0
        || area.enter != MotionRegionAction::Play
        || area.exit != MotionRegionAction::Reverse
    {
        return Err("触发区域尺寸或进出动作未通过核对".into());
    }
    Ok(json!({"clip_id":id,"region":area,"screenshot":"motion-trigger-region.png"}))
}
