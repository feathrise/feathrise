//! 场景动画在原视口摆姿势，再把当前帧写入位置或旋转通道。

mod controls;
mod input;
mod preview;
#[cfg(feature = "validation")]
pub mod validation;

use bevy::{ecs::system::SystemParam, prelude::*};
use feathrise_editor_core::{EditorHostOverlay, PresetWorkspaceTab};
use feathrise_scene_content::{SceneId, ScenePlacementRecord};
use std::collections::HashMap;
use crate::{
    SceneEditorSystemSet,
    authoring::{EditorAuthoringSession, EditorSceneDraft, placement_pose::placement_rotation_degrees,
        scene_motion::{SceneMotionKey, SceneMotionTrack}},
    resources::SceneEditorState,
    ui::{gltf_browser::{EditorGltfBrowserState, MainSceneSelection}, vegetation_preset_editor::VegetationPresetEditorState},
    viewport::{multi_select::PlacementSelectionSet, placement_move::PlacementMoveState},
};

pub(super) use controls::{entry, workbench};
const FPS: f64 = 30.0;

/// 动画选中身份独立于工作台仍需使用的摆放姿势目标。
pub(super) fn selected_clip_name<'a>(browser: &EditorGltfBrowserState, draft: &'a EditorSceneDraft) -> Option<&'a str> {
    if draft.placement_glb_hash.is_some()
        && matches!(&browser.selected_main_row, Some((scene, MainSceneSelection::Animation)) if scene == &draft.scene_id)
    {
        draft.motion.clip_name()
    } else {
        None
    }
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneMotionButton {
    Open, Close, Add, First, PreviousFrame, TogglePlay, NextFrame, Last, CyclePlayback, Seek,
    SelectPosition, SelectRotation, SelectScale, InsertKey, DeleteKey, ResetPose, ToggleAutoPlay,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneMotionField { Frame, EndFrame }

/// 主场景树双击冻结的片段，交给当前场景原有工作台统一核对和打开。
#[derive(Message)]
pub(super) struct OpenSceneAnimation(pub(super) super::scene_context_menu::SceneObjectTarget);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Channel { Position, Rotation, Scale }

impl Channel {
    fn index(self) -> usize {
        match self { Self::Position => 0, Self::Rotation => 1, Self::Scale => 2 }
    }

    fn keys(self, track: &SceneMotionTrack) -> &[SceneMotionKey] {
        match self { Self::Position => &track.position, Self::Rotation => &track.rotation, Self::Scale => &track.scale }
    }

    fn keys_mut(self, track: &mut SceneMotionTrack) -> &mut Vec<SceneMotionKey> {
        match self { Self::Position => &mut track.position, Self::Rotation => &mut track.rotation, Self::Scale => &mut track.scale }
    }
}

#[derive(Clone, Copy, PartialEq)]
struct SelectedKey { channel: Channel, time: f32 }

#[derive(Clone, PartialEq, Eq)]
struct Target {
    scene_id: SceneId,
    placement_id: String,
}

impl Target {
    fn new(scene: &SceneId, record: &ScenePlacementRecord) -> Self {
        Self { scene_id: scene.clone(), placement_id: record.id.clone() }
    }

    fn matches(&self, scene: &SceneId, id: &str) -> bool {
        &self.scene_id == scene && self.placement_id == id
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SceneAnimationFramePose {
    pub(crate) translation: [f32; 3],
    pub(crate) rotation_degrees: [f32; 3],
    pub(crate) scale: [f32; 3],
}

impl SceneAnimationFramePose {
    fn base(record: &ScenePlacementRecord) -> Self {
        Self { translation: record.translation, rotation_degrees: placement_rotation_degrees(record), scale: record.scale }
    }

    pub(crate) fn rotation(self) -> Quat {
        let [x, y, z] = self.rotation_degrees.map(f32::to_radians);
        Quat::from_euler(EulerRot::XYZ, x, y, z)
    }
}

#[derive(Default)]
struct PoseDraft {
    position: Option<[f32; 3]>,
    rotation: Option<[f32; 3]>,
    scale: Option<[f32; 3]>,
}

/// 旧工作流默认插位置和旋转。缩放可单选，也可用 Ctrl 加入本次插帧。
struct KeyingTracks([bool; 3]);

impl Default for KeyingTracks {
    fn default() -> Self { Self([true, true, false]) }
}

#[derive(Resource, Default)]
pub(crate) struct Workbench {
    scene: Option<SceneId>,
    target: Option<Target>,
    seconds: f64,
    elapsed: f64,
    playing: bool,
    has_clip: bool,
    pose: Option<preview::PreviewPose>,
    // 两个索引交替使用，在选择与热重载换根时只还原已接管的场景对象。
    previews: HashMap<Entity, preview::PreviewPose>,
    current_previews: HashMap<Entity, preview::PreviewPose>,
    sampled: Option<SceneAnimationFramePose>,
    draft: PoseDraft,
    keying_tracks: KeyingTracks,
    selected_key: Option<SelectedKey>,
    ruler_capture: Option<Entity>,
}

impl Workbench {
    pub(crate) fn owns(&self, scene: &SceneId, id: &str) -> bool {
        self.target.as_ref().is_some_and(|target| target.matches(scene, id))
    }

    pub(crate) fn can_pose(&self, scene: &SceneId, id: &str) -> bool {
        self.owns(scene, id) && self.sampled.is_some() && !self.playing
    }

    pub(crate) fn frame_time(&self) -> f64 { self.seconds }

    fn keys_channel(&self, channel: Channel) -> bool { self.keying_tracks.0[channel.index()] }

    fn select_track(&mut self, channel: Channel, additive: bool) {
        let tracks = &mut self.keying_tracks.0;
        if !additive { *tracks = [false; 3]; }
        tracks[channel.index()] = !tracks[channel.index()];
    }

    fn has_keying_tracks(&self) -> bool { self.keying_tracks.0.iter().any(|&selected| selected) }

    pub(crate) fn pose_for(&self, record: &ScenePlacementRecord, scene: &SceneId) -> Option<SceneAnimationFramePose> {
        if !self.owns(scene, &record.id) { return None; }
        let sampled = self.sampled?;
        Some(SceneAnimationFramePose {
            translation: self.draft.position.unwrap_or(sampled.translation),
            rotation_degrees: self.draft.rotation.unwrap_or(sampled.rotation_degrees),
            scale: self.draft.scale.unwrap_or(sampled.scale),
        })
    }

    pub(crate) fn set_position(&mut self, scene: &SceneId, id: &str, value: [f32; 3]) -> bool {
        if !self.can_pose(scene, id) || !Vec3::from_array(value).is_finite() { return false; }
        self.draft.position = self.sampled.filter(|pose| pose.translation != value).map(|_| value);
        true
    }

    pub(crate) fn set_rotation(&mut self, scene: &SceneId, id: &str, value: [f32; 3]) -> bool {
        if !self.can_pose(scene, id) || !Vec3::from_array(value).is_finite() { return false; }
        self.draft.rotation = self.sampled.filter(|pose| pose.rotation_degrees != value).map(|_| value);
        true
    }

    pub(crate) fn set_scale(&mut self, scene: &SceneId, id: &str, value: [f32; 3]) -> bool {
        if !self.can_pose(scene, id) || value.into_iter().any(|axis| !(0.001..=1_000.0).contains(&axis)) { return false; }
        self.draft.scale = self.sampled.filter(|pose| pose.scale != value).map(|_| value);
        true
    }

    fn seek(&mut self, seconds: f64, duration: f32, key: Option<SelectedKey>) {
        let seconds = seconds.clamp(0.0, f64::from(duration));
        // 取另一帧时清除尚未插入的姿势，已写入的通道只由历史修改。
        if self.seconds != seconds || self.playing {
            self.draft = PoseDraft::default();
            self.sampled = None;
        }
        self.seconds = seconds;
        self.elapsed = seconds;
        self.selected_key = key;
        self.playing = false;
    }

    fn target(&mut self, target: Option<Target>) {
        if self.target == target { return; }
        self.target = target;
        self.sampled = None;
        self.draft = PoseDraft::default();
        self.selected_key = None;
        self.ruler_capture = None;
    }

    fn close(&mut self) {
        self.scene = None;
        self.target(None);
        self.seconds = 0.0;
        self.elapsed = 0.0;
        self.playing = false;
        self.has_clip = false;
        self.keying_tracks = KeyingTracks::default();
        // 关窗后由预览归还实际渲染根，以免覆盖新一代摆放。
    }
}

#[derive(SystemParam)]
struct Context<'w> {
    workspace: Res<'w, VegetationPresetEditorState>,
    browser: Res<'w, EditorGltfBrowserState>,
    mesh_edit: Res<'w, crate::authoring::mesh_edit::MeshEditState>,
    selection: Res<'w, PlacementSelectionSet>,
    moving: Res<'w, PlacementMoveState>,
    host: Option<Res<'w, EditorHostOverlay>>,
    host_page: Option<Res<'w, crate::workbench::WorkbenchHostPageState>>,
    media: Option<Res<'w, crate::ui::asset_preview::AssetPreviewWorkspace>>,
}

impl Context<'_> {
    fn scene<'a>(&self, editor: &SceneEditorState, session: &'a EditorAuthoringSession) -> Option<&'a SceneId> {
        if !editor.open || editor.close_requested || editor.scene_transition_in_progress
            || editor.pending_scene_transition.is_some() || self.workspace.active_tab != PresetWorkspaceTab::Scene
            || self.host_page.as_ref().is_some_and(|page| page.active) || self.mesh_edit.active()
            || self.media.as_ref().is_some_and(|media| media.central_active)
        { return None; }
        let draft = session.draft.as_ref()?;
        (editor.selected_scene_id.as_ref() == Some(&draft.scene_id)).then_some(&draft.scene_id)
    }

    fn record<'a>(&self, editor: &SceneEditorState, session: &'a EditorAuthoringSession) -> Option<(&'a SceneId, &'a ScenePlacementRecord)> {
        let scene = self.scene(editor, session)?;
        if self.selection.members.len() > 1 || self.browser.selection.is_some() { return None; }
        let id = self.browser.selected_placement_id.as_deref()?;
        let record = self.placement(session, id)?;
        (record.editor.selectable() && record.terrain_patch.is_none() && record.courtyard.is_none()
            && record.building_preset_id.is_none() && record.water_surfaces.is_empty())
            .then_some((scene, record))
    }

    fn placement<'a>(&self, session: &'a EditorAuthoringSession, id: &str) -> Option<&'a ScenePlacementRecord> {
        if self.browser.duplicate_placement_ids.contains(id) { return None; }
        let index = self.browser.placement_record_index(id, session.placement_structure_revision)?;
        session.draft.as_ref()?.placements.placements.get(index).filter(|record| record.id == id)
    }

    fn can_edit(&self, editor: &SceneEditorState, session: &EditorAuthoringSession) -> bool {
        !session.saving && !editor.new_scene_dialog_open && !editor.scene_switch_confirmation_open()
            && !self.moving.owns_mouse() && self.host.as_ref().is_none_or(|host| !host.active)
    }
}

pub(crate) fn install(app: &mut App) {
    app.init_resource::<Workbench>()
        .add_systems(Update, input::input
            .after(super::scene_context_menu::SceneMenuDispatch)
            // 点插帧会使右栏数值框失焦。先接收同一帧的提交，再采样姿势。
            .after(crate::ui::placement_transform::handle_placement_transform_numeric_input)
            // I、空格及 Delete 在当前时间轴受理后由它消费，不泄漏给可配置场景快捷键。
            .before(crate::ui::actions::handle_editor_shortcuts)
            .before(crate::authoring::apply_undo_redo)
            .run_if(crate::scene_browser_general_input_available)
            .in_set(SceneEditorSystemSet::Input));
    controls::install(app);
}
