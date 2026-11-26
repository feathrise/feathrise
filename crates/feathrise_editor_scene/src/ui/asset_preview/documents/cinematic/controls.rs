//! 原生表单、轨道选区与任意时间试播。每次确认只产生一笔异步文档历史。
use super::{model::*, view::*, *};
use bevy::{input_focus::InputFocus, text::EditableText, ui::RelativeCursorPosition};
use feathrise_cinematic::{CameraKey, Pose};
use feathrise_editor_ui::{EditorButtonClick, UiInteraction};
pub(super) fn control_pose(t: &Transform) -> Pose {
    Pose {
        translation: t.translation.to_array(),
        rotation: t.rotation.to_array(),
        scale: t.scale.to_array(),
    }
}
fn read_fields(form: &mut Form, fields: &Query<(&Field, &EditableText)>) -> Result<(), String> {
    for (kind, input) in fields {
        if input.is_composing() || !input.pending_edits.is_empty() || input.pending_paste.is_some()
        {
            return Err("请先确认当前输入再操作".into());
        }
        let dest = match kind {
            Field::At => &mut form.at,
            Field::Length => &mut form.length,
            Field::Target => &mut form.target,
            Field::Instance => &mut form.instance,
            Field::Name => &mut form.name,
            Field::Position => &mut form.position,
            Field::Rotation => &mut form.rotation,
            Field::Scale => &mut form.scale,
            Field::Fov => &mut form.fov,
        };
        if input.value() != dest.as_str() {
            *dest = input.value().to_string();
        }
    }
    Ok(())
}
pub(super) fn input(
    mut s: ResMut<Studio>,
    mut docs: ResMut<DocumentState>,
    mut focus: ResMut<InputFocus>,
    actions: Query<(&Action, &EditorButtonClick), Changed<EditorButtonClick>>,
    fields: Query<(&Field, &EditableText)>,
    markers: Query<(&MarkerWidget, &EditorButtonClick), Changed<EditorButtonClick>>,
    vp: Option<Res<crate::resources::SceneEditorViewportState>>,
    authoring: Option<Res<crate::authoring::EditorAuthoringSession>>,
    camera: Query<(&Transform, &Projection)>,
    selected: Option<Res<crate::ui::gltf_browser::EditorGltfBrowserState>>,
    mut exports: MessageWriter<
        feathrise_editor_documents_model::cinematic_export::OpenCinematicExport,
    >,
) {
    if active(&docs).is_none() {
        return;
    }
    if let Some((m, _)) = markers.iter().find(|(_, c)| c.is_clicked())
        && let Some(item) = s
            .timeline
            .as_ref()
            .and_then(|t| t.lanes[m.lane.index()].get(m.index))
            .map(|m| m.item)
    {
        s.select(m.lane, Some(item));
        s.selected_marker = m.index;
        return;
    }
    let Some((&a, _)) = actions.iter().find(|(_, c)| c.is_clicked()) else {
        return;
    };
    if let Err(e) = read_fields(&mut s.form, &fields) {
        s.notice = e;
        return;
    }
    focus.clear();
    let result = (|| -> Result<(), String> {
        if a == Action::Advanced {
            if let Some(d) = docs.current_mut() {
                d.mode = ViewMode::Edit;
            }
            s.previewing = false;
            s.playing = false;
            return Ok(());
        }
        let t = s
            .timeline
            .clone()
            .ok_or("请先修复演出文件，或等待加载完成")?;
        if s.job.is_some() {
            return Err("演出编辑正在处理，请稍候".into());
        }
        match a {
            Action::Lane(l) => s.select(l, None),
            Action::Play => {
                s.playing = !s.playing;
                if s.playing && s.cursor >= t.film.duration {
                    s.cursor = 0.;
                }
                s.previewing = true;
            }
            Action::Stop => {
                s.playing = false;
                s.previewing = false;
            }
            Action::Go => {
                let sec = number(&s.form.at)?;
                s.seek(sec);
            }
            Action::Save => docs.request_save(),
            Action::Export => {
                let d = docs.current().ok_or("请先保存演出")?;
                if d.dirty() || docs.saved_play_pending() || d.external.warning {
                    return Err("请先保存演出并完成磁盘冲突处理，再导出视频".into());
                }
                if super::super::source::is_project(&d.path) {
                    return Err("视频导出需要位于工程资产根内的演出".into());
                }
                exports.write(
                    feathrise_editor_documents_model::cinematic_export::OpenCinematicExport {
                        asset_root: docs.root.clone(),
                        asset_path: d.disk.path.clone(),
                        scene: authoring
                            .as_ref()
                            .and_then(|session| session.draft.as_ref())
                            .map(|draft| draft.scene_id.as_str().to_owned()),
                    },
                );
            }
            Action::Settings => {
                s.settings = true;
                s.form.name = t.film.name.clone();
                s.form.length = t.film.duration.to_string();
                s.control_camera = t.film.control_camera;
                s.control_player = t.film.control_player;
                s.refresh();
            }
            Action::ControlCamera => {
                s.control_camera = !s.control_camera;
                s.refresh();
            }
            Action::ControlPlayer => {
                s.control_player = !s.control_player;
                s.refresh();
            }
            Action::ApplySettings => {
                let e = Edit::Settings {
                    name: s.form.name.trim().to_owned(),
                    duration: number(&s.form.length)?,
                    control_camera: s.control_camera,
                    control_player: s.control_player,
                };
                start(&mut s, &docs, Some(e))?;
            }
            Action::Smooth => {
                s.form.smooth = !s.form.smooth;
                s.refresh();
            }
            Action::Wait => {
                s.form.wait = !s.form.wait;
                s.refresh();
            }
            Action::Pose | Action::Clip | Action::Subtitle | Action::Yarn => {
                s.kind = match a {
                    Action::Clip => Kind::Clip,
                    Action::Yarn => Kind::Yarn,
                    Action::Subtitle => Kind::Subtitle,
                    _ => Kind::Pose,
                };
                s.refresh();
            }
            Action::Previous | Action::Next => {
                let lane = s.lane;
                let data = &t.lanes[lane.index()];
                if data.is_empty() {
                    return Err("此轨道还没有内容".into());
                }
                let i = if let Some(i) = s
                    .selection
                    .map(|_| s.selected_marker)
                    .filter(|i| *i < data.len())
                {
                    if a == Action::Next {
                        (i + 1) % data.len()
                    } else {
                        (i + data.len() - 1) % data.len()
                    }
                } else if a == Action::Next {
                    data.partition_point(|m| m.at < s.cursor)
                        .min(data.len() - 1)
                } else {
                    data.partition_point(|m| m.at <= s.cursor).saturating_sub(1)
                };
                s.select(lane, Some(data[i].item));
                s.selected_marker = i;
            }
            Action::UseSelection => {
                s.form.target = selected
                    .as_ref()
                    .and_then(|s| s.selected_placement_id.clone())
                    .ok_or("请先在场景树选中要绑定的摆放")?;
                s.refresh();
            }
            Action::New | Action::Replace | Action::Capture => {
                let replacement = if a == Action::Replace {
                    Some(s.selection.ok_or("请先选择要更新的标记")?)
                } else {
                    None
                };
                let camera_key = if a == Action::Capture {
                    let v = vp.as_ref().ok_or("等待活动场景视口")?;
                    let (transform, projection) =
                        camera.get(v.camera).map_err(|_| "活动机位不可用")?;
                    let Projection::Perspective(p) = projection else {
                        return Err("请先将场景视口切到透视，再记录镜头".into());
                    };
                    Some(CameraKey {
                        time: number(&s.form.at)?,
                        pose: control_pose(transform),
                        fov_y_degrees: p.fov.to_degrees(),
                        smooth: s.form.smooth,
                    })
                } else {
                    None
                };
                let edit = Edit::Set {
                    lane: s.lane,
                    kind: s.kind,
                    form: s.form.clone(),
                    replace: replacement,
                    camera: camera_key,
                };
                start(&mut s, &docs, Some(edit))?;
            }
            Action::Remove => {
                let item = s.selection.ok_or("请先选定要删除的标记")?;
                start(&mut s, &docs, Some(Edit::Remove(item)))?;
            }
            Action::Undo | Action::Redo => {
                if let Some(d) = docs.current_mut() {
                    d.travel(a == Action::Redo);
                }
            }
            Action::Advanced | Action::PickClip => {}
        };
        Ok(())
    })();
    if let Err(e) = result {
        s.notice = e;
    }
}
/// 文件窗口已在后台按真实 GLB 动画表构造行。只接受同一演出、该次属性与工程仍有效的选择。
pub(super) fn picker(
    mut s: ResMut<Studio>,
    docs: Res<DocumentState>,
    manager: Option<ResMut<crate::ui::file_manager::FileManagerState>>,
    buttons: Query<(&Action, &EditorButtonClick), Changed<EditorButtonClick>>,
    mut picked: MessageReader<PickCinematicClip>,
    mut open: Option<ResMut<Messages<crate::ui::file_manager::OpenAnimationPicker>>>,
) {
    let Some(mut manager) = manager else { return };
    let current = active(&docs).filter(|_| s.kind == Kind::Clip).map(|k| {
        crate::ui::file_manager::ProjectMotionTarget {
            asset_root: docs.root.clone(),
            path: k.path,
            generation: k.generation,
            tab: s.form_revision,
        }
    });
    manager.close_stale_animation_picker(|target| {
        !is_path(&target.path) || Some(target) == current.as_ref()
    });
    if let Some(p) = picked.read().last()
        && Some(&p.target) == current.as_ref()
    {
        manager.finish_animation_picker(&p.target);
        s.form.name = p.name.clone();
        s.refresh();
        return;
    }
    if let Some(target) = current
        && buttons
            .iter()
            .any(|(a, c)| *a == Action::PickClip && c.is_clicked())
        && let Some(open) = open.as_deref_mut()
    {
        open.write(crate::ui::file_manager::OpenAnimationPicker {
            context: manager.service_context(),
            directory: docs.root.clone(),
            target,
        });
    }
}
pub(super) fn seek(
    mut s: ResMut<Studio>,
    docs: Res<DocumentState>,
    mouse: Res<ButtonInput<MouseButton>>,
    rows: Query<(Entity, &LaneWidget, &UiInteraction, &RelativeCursorPosition)>,
    mut drag: Local<Option<Entity>>,
) {
    if active(&docs).is_none() || !mouse.pressed(MouseButton::Left) {
        *drag = None;
        return;
    }
    if drag.is_none() && mouse.just_pressed(MouseButton::Left) {
        *drag = rows
            .iter()
            .find(|(_, _, input, pos)| **input == UiInteraction::Pressed && pos.cursor_over())
            .map(|r| r.0);
    }
    let Some(entity) = *drag else { return };
    let Ok((_, lane, _, pos)) = rows.get(entity) else {
        return;
    };
    let Some(normal) = pos.normalized else { return };
    let Some(t) = &s.timeline else { return };
    let at = feathrise_editor_ui::animation_timeline::cursor_time(0., t.film.duration, normal.x);
    if let Some(l) = lane.0
        && s.lane != l
    {
        s.select(l, None);
    }
    s.seek(at);
    s.form.at = format!("{:.3}", s.cursor);
    s.refresh();
}
