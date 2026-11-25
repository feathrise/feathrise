//! 「交互」的动画触发区。两次点选定义水平底面的对角点，作者可精确指定底面和高度。

mod controls;
mod handles;
use super::{GameplaySidebarPage, GameplaySidebarState, systems::GameplaySidebarInputGate};
use crate::{
    authoring::{
        AuthoringRetirement, EditorAuthoringSession, EditorCommand,
        scene_motion::{MotionRegion, MotionRegionAction},
    },
    components::SceneEditorViewport,
    resources::{SceneEditorState, SceneEditorViewportState},
    types::EditorTool,
    ui::sidebars::{EditorLeftSidebarTab, SceneEditorSidebarState},
    viewport::{
        SceneEditorBrushGizmos,
        modes::{ViewportMode, ViewportModeState},
    },
};
use bevy::{prelude::*, ui::RelativeCursorPosition};
pub(super) use controls::panel;
use feathrise_editor_ui::{
    EditorButtonClick, NumericInputCommitOutcome, NumericInputCommitted, NumericValue,
};
use feathrise_scene_content::SceneId;

#[derive(Component, Clone, Copy)]
enum Action {
    PreviousClip,
    NextClip,
    PreviousRegion,
    NextRegion,
    Draw,
    Redraw,
    Adjust,
    Remove,
    Enter,
    Exit,
}
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Dimension {
    Floor,
    Height,
}

#[derive(Clone, PartialEq)]
struct Binding {
    scene: SceneId,
    clip: String,
    index: usize,
    revision: u64,
}
#[derive(Clone)]
struct Drawing {
    target: Binding,
    replace: bool,
    anchor: Option<Vec3>,
}
#[derive(Resource)]
struct Tool {
    clip: usize,
    region: usize,
    bound: Option<Binding>,
    drawing: Option<Drawing>,
    adjusting: bool,
    drag: Option<handles::Drag>,
    floor: f32,
    height: f32,
    enter: MotionRegionAction,
    exit: MotionRegionAction,
}
impl Default for Tool {
    fn default() -> Self {
        Self {
            clip: 0,
            region: 0,
            bound: None,
            drawing: None,
            adjusting: false,
            drag: None,
            floor: 0.0,
            height: 2.0,
            enter: MotionRegionAction::Play,
            exit: MotionRegionAction::Reverse,
        }
    }
}
impl MotionRegionAction {
    fn next(self) -> Self {
        use MotionRegionAction::*;
        match self {
            None => Play,
            Play => Reverse,
            Reverse => Pause,
            Pause => Stop,
            Stop => None,
        }
    }
    fn label(self) -> &'static str {
        use MotionRegionAction::*;
        match self {
            None => "无",
            Play => "正向播放",
            Reverse => "反向播放",
            Pause => "暂停",
            Stop => "停止",
        }
    }
}
impl Tool {
    fn bound_is_current(&self, session: &EditorAuthoringSession) -> bool {
        let Some(draft) = &session.draft else {
            return self.bound.is_none();
        };
        let Some((clip, _)) = draft.motion.clip_at(self.clip) else {
            return self.bound.is_none();
        };
        self.bound.as_ref().is_some_and(|b| {
            b.scene == draft.scene_id
                && b.clip == clip
                && b.revision == session.revision
                && b.index
                    == self.region.min(
                        draft
                            .motion
                            .clip_regions_at(self.clip)
                            .len()
                            .saturating_sub(1),
                    )
        })
    }
    fn binding(&self, session: &EditorAuthoringSession) -> Option<Binding> {
        let draft = session.draft.as_ref()?;
        let (clip, _) = draft.motion.clip_at(self.clip)?;
        Some(Binding {
            scene: draft.scene_id.clone(),
            clip: clip.into(),
            revision: session.revision,
            index: self.region.min(
                draft
                    .motion
                    .clip_regions_at(self.clip)
                    .len()
                    .saturating_sub(1),
            ),
        })
    }
    fn box_between(&self, a: Vec3, b: Vec3) -> Option<MotionRegion> {
        let low = Vec3::new(a.x.min(b.x), self.floor, a.z.min(b.z));
        let high = Vec3::new(a.x.max(b.x), self.floor + self.height, a.z.max(b.z));
        let area = MotionRegion {
            center: low * 0.5 + high * 0.5,
            half_extents: (high - low) * 0.5,
            enter: self.enter,
            exit: self.exit,
        };
        area.valid().then_some(area)
    }
}

pub(super) fn install(app: &mut App) {
    let on_page =
        |pages: Res<GameplaySidebarState>| pages.page == GameplaySidebarPage::MotionTrigger;
    let needs_input = |pages: Res<GameplaySidebarState>, tool: Res<Tool>| {
        pages.page == GameplaySidebarPage::MotionTrigger || tool.drawing.is_some() || tool.adjusting
    };
    app.init_resource::<Tool>()
        .add_systems(
            Update,
            (controls::setup.run_if(on_page), input.run_if(needs_input))
                .chain()
                .after(super::systems::handle_tabs)
                .before(crate::SceneEditorSystemSet::Tasks),
        )
        .add_systems(
            Update,
            (controls::sync, draw)
                .chain()
                .run_if(on_page)
                .run_if(
                    |editor: Res<SceneEditorState>, sidebar: Res<SceneEditorSidebarState>| {
                        editor.open && sidebar.tool_page_visible(EditorLeftSidebarTab::Gameplay)
                    },
                )
                .in_set(crate::SceneEditorSystemSet::Presentation),
        );
}

fn change(
    session: &mut EditorAuthoringSession,
    editor: &mut SceneEditorState,
    retirement: &mut AuthoringRetirement,
    target: &Binding,
    index: usize,
    before: Option<MotionRegion>,
    after: Option<MotionRegion>,
) {
    if before == after || session.saving || session.revision != target.revision {
        return;
    }
    let Some(draft) = session
        .draft
        .as_mut()
        .filter(|d| d.scene_id == target.scene)
    else {
        return;
    };
    match draft
        .motion
        .change_region(&target.clip, index, before, after)
    {
        Ok(()) => {
            session.undo.push(EditorCommand::SceneMotionRegionChanged {
                clip_id: target.clip.clone(),
                index,
                before,
                after,
            });
            retirement.push(std::mem::take(&mut session.redo));
            session.mark_changed(editor);
            editor.status = "已修改动画触发区域".into();
        }
        Err(error) => editor.status = error,
    }
}

#[derive(bevy::ecs::system::SystemParam)]
struct Pointer<'w, 's> {
    viewport: Option<Res<'w, SceneEditorViewportState>>,
    canvas: Query<'w, 's, &'static RelativeCursorPosition, With<SceneEditorViewport>>,
    cameras: Query<'w, 's, (&'static Camera, &'static Transform)>,
    windows: Query<'w, 's, &'static Window, With<bevy::window::PrimaryWindow>>,
}
impl Pointer<'_, '_> {
    fn on_plane(&self, y: f32) -> Option<Vec3> {
        let view = self.viewport.as_ref()?;
        let cursor = self.canvas.single().ok()?;
        if !cursor.cursor_over() {
            return None;
        }
        let pixel = (cursor.normalized? + Vec2::splat(0.5)) * view.render_size.as_vec2();
        let (camera, pose) = self.cameras.get(view.camera).ok()?;
        let ray = camera
            .viewport_to_world(&GlobalTransform::from(*pose), pixel)
            .ok()?;
        let t = ray.intersect_plane(Vec3::Y * y, InfinitePlane3d::new(Vec3::Y))?;
        Some(ray.get_point(t))
    }
}

#[allow(clippy::too_many_arguments)]
fn input(
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut retirement: ResMut<AuthoringRetirement>,
    sidebar: Res<SceneEditorSidebarState>,
    pages: Res<GameplaySidebarState>,
    gate: GameplaySidebarInputGate,
    mut mode: ResMut<ViewportModeState>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    pointer: Pointer,
    mut tool: ResMut<Tool>,
    buttons: Query<(&EditorButtonClick, &Action), Changed<EditorButtonClick>>,
    mut numbers: MessageReader<NumericInputCommitted>,
    fields: Query<&Dimension>,
) {
    let active = editor.open
        && !editor.close_requested
        && !editor.save_requested
        && !editor.new_scene_dialog_open
        && !editor.scene_transition_in_progress
        && editor.scene_switch_confirmation.is_none()
        && !session.saving
        && sidebar.tool_page_visible(EditorLeftSidebarTab::Gameplay)
        && pages.page == GameplaySidebarPage::MotionTrigger;
    if !active || !tool.bound_is_current(&session) || keys.just_pressed(KeyCode::Escape) {
        tool.drawing = None;
        if !active || keys.just_pressed(KeyCode::Escape) {
            tool.adjusting = false;
        }
        tool.drag = None;
        numbers.clear();
        return;
    }
    if mode.mode != ViewportMode::Edit || editor.tool != EditorTool::MotionTrigger {
        tool.drawing = None;
        tool.adjusting = false;
        tool.drag = None;
    }
    if numbers.is_empty()
        && !buttons.iter().any(|(click, _)| click.is_clicked())
        && !mouse.just_pressed(MouseButton::Left)
        && tool.drag.is_none()
    {
        return;
    }
    let Some(target) = tool.bound.clone() else {
        numbers.clear();
        return;
    };
    let draft = session.draft.as_ref().unwrap();
    if editor.selected_scene_id.as_ref() != Some(&draft.scene_id) {
        tool.drawing = None;
        tool.adjusting = false;
        tool.drag = None;
        numbers.clear();
        return;
    }
    let clips = draft.motion.clip_count();
    let regions = draft.motion.clip_regions_at(tool.clip);
    let count = regions.len();
    let existing = regions.get(target.index).copied();
    for event in numbers.read() {
        if event.outcome != NumericInputCommitOutcome::Accepted || !event.changed {
            continue;
        }
        let Ok(field) = fields.get(event.entity) else {
            continue;
        };
        let NumericValue::F32(value) = event.value else {
            continue;
        };
        if !value.is_finite() || *field == Dimension::Height && value <= 0.0 {
            continue;
        }
        match field {
            Dimension::Floor => tool.floor = value,
            Dimension::Height => tool.height = value,
        }
        if tool.drawing.is_none()
            && let Some(before) = existing
        {
            let mut after = before;
            after.half_extents.y = tool.height * 0.5;
            after.center.y = tool.floor + after.half_extents.y;
            change(
                &mut session,
                &mut editor,
                &mut retirement,
                &target,
                target.index,
                existing,
                Some(after),
            );
        }
    }
    if gate.blocked() {
        tool.drag = None;
        return;
    }
    for (click, action) in &buttons {
        if !click.is_clicked() {
            continue;
        }
        tool.adjusting = false;
        tool.drag = None;
        match action {
            Action::PreviousClip | Action::NextClip => {
                tool.clip = cycle(tool.clip, clips, matches!(action, Action::NextClip));
                tool.region = 0;
                tool.drawing = None;
            }
            Action::PreviousRegion | Action::NextRegion => {
                tool.region = cycle(target.index, count, matches!(action, Action::NextRegion));
                tool.drawing = None;
            }
            Action::Adjust => {
                if existing.is_some() {
                    mode.mode = ViewportMode::Edit;
                    mode.remember_tool(EditorTool::MotionTrigger);
                    editor.tool = EditorTool::MotionTrigger;
                    tool.drawing = None;
                    tool.adjusting = true;
                    editor.status = "拖动中心移动区域，拖动四边改变宽深；Esc 取消".into();
                }
            }
            Action::Draw | Action::Redraw => {
                let replace = matches!(action, Action::Redraw);
                if replace && existing.is_none() {
                    continue;
                }
                mode.mode = ViewportMode::Edit;
                mode.remember_tool(EditorTool::MotionTrigger);
                editor.tool = EditorTool::MotionTrigger;
                tool.drawing = Some(Drawing {
                    target: target.clone(),
                    replace,
                    anchor: None,
                });
                editor.status = "请在视口点选底面的两个对角，Esc 取消".into();
            }
            Action::Remove => {
                tool.drawing = None;
                change(
                    &mut session,
                    &mut editor,
                    &mut retirement,
                    &target,
                    target.index,
                    existing,
                    None,
                );
            }
            Action::Enter | Action::Exit => {
                if matches!(action, Action::Enter) {
                    tool.enter = tool.enter.next();
                } else {
                    tool.exit = tool.exit.next();
                }
                if tool.drawing.is_none()
                    && let Some(before) = existing
                {
                    let after = MotionRegion {
                        enter: tool.enter,
                        exit: tool.exit,
                        ..before
                    };
                    change(
                        &mut session,
                        &mut editor,
                        &mut retirement,
                        &target,
                        target.index,
                        existing,
                        Some(after),
                    );
                }
            }
        }
        return;
    }
    if tool.adjusting {
        if let Some(before) = existing
            && let Some(after) = handles::update(&mut tool.drag, before, &pointer, &mouse, mode.pointer_over_controls)
        {
            change(&mut session, &mut editor, &mut retirement, &target, target.index, Some(before), Some(after));
        }
        return;
    }
    let Some(drawing) = tool.drawing.clone() else {
        return;
    };
    if drawing.target != target
        || mode.mode != ViewportMode::Edit
        || editor.tool != EditorTool::MotionTrigger
    {
        tool.drawing = None;
        return;
    }
    if mode.pointer_over_controls || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(point) = pointer.on_plane(tool.floor) else {
        return;
    };
    if let Some(anchor) = drawing.anchor {
        let Some(area) = tool.box_between(anchor, point) else {
            editor.status = "两个对角需要围成有面积的底面".into();
            return;
        };
        let index = if drawing.replace { target.index } else { count };
        change(
            &mut session,
            &mut editor,
            &mut retirement,
            &target,
            index,
            if drawing.replace { existing } else { None },
            Some(area),
        );
        tool.region = index;
        tool.drawing = None;
    } else {
        tool.drawing.as_mut().unwrap().anchor = Some(point);
        editor.status = "请点选触发区域的另一角".into();
    }
}

fn cycle(index: usize, count: usize, forward: bool) -> usize {
    if count == 0 {
        return 0;
    }
    if forward {
        (index + 1) % count
    } else {
        (index + count - 1) % count
    }
}

fn draw(
    tool: Res<Tool>,
    session: Res<EditorAuthoringSession>,
    pointer: Pointer,
    mut gizmos: Gizmos<SceneEditorBrushGizmos>,
) {
    let area = if let Some(drag) = &tool.drag {
        Some(drag.preview)
    } else if let Some(d) = &tool.drawing {
        d.anchor
            .zip(pointer.on_plane(tool.floor))
            .and_then(|(a, b)| tool.box_between(a, b))
    } else {
        tool.bound.as_ref().and_then(|b| {
            session
                .draft
                .as_ref()?
                .motion
                .clip_regions_at(tool.clip)
                .get(b.index)
                .copied()
        })
    };
    if let Some(area) = area {
        if tool.adjusting {
            handles::draw(area, &pointer, &mut gizmos);
        }
        gizmos.cube(
            Transform::from_translation(area.center).with_scale(area.half_extents * 2.0),
            Color::srgb(0.20, 0.86, 0.85),
        );
    }
}
