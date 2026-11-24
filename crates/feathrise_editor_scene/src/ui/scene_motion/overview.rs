//! 片段总览用原时间轴行展示场景动画的对象和合并关键帧。

use bevy::{prelude::*, ui::InteractionDisabled};
use feathrise_editor_ui::{EditorButtonClick, animation_timeline::KEY_SLOTS};
use feathrise_scene_content::{SceneId, ScenePlacementAssetTarget, ScenePlacementRecord};

use super::{Channel, Context, SceneMotionButton, Workbench};
use crate::{
    authoring::{EditorAuthoringSession, scene_motion::SceneMotionTrack},
    resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState},
    ui::{
        scene_context_menu::{SceneContextMenuState, SceneObjectKey, SceneObjectTarget},
        scene_layers::SceneLayerSelection,
        sidebars::SceneEditorSidebarState,
    },
};

// 与现有位置、旋转、缩放三行共用同一批控件，组数跟随实际对象数量。
pub(super) const ROWS: usize = 3;

pub(super) fn page(requested: usize, count: usize) -> usize {
    requested.min(count.div_ceil(ROWS).saturating_sub(1))
}

#[derive(Clone)]
struct Binding {
    scene: SceneId,
    clip: String,
    placement: String,
}

#[derive(Component, Clone)]
pub(super) struct Row {
    pub(super) channel: Channel,
    binding: Option<Binding>,
}

impl Row {
    pub(super) fn new(channel: Channel) -> Self {
        Self { channel, binding: None }
    }

    /// 只在换页、换片段或换对象时重新绑定。旧按钮的按下不能提交给新对象。
    pub(super) fn bind(&mut self, target: Option<(&SceneId, &str, &str)>) -> bool {
        let current = self.binding.as_ref().map(|id| (&id.scene, id.clip.as_str(), id.placement.as_str()));
        if current == target { return false; }
        self.binding = target.map(|(scene, clip, placement)| Binding {
            scene: scene.clone(), clip: clip.to_owned(), placement: placement.to_owned(),
        });
        true
    }
}

pub(super) fn name(record: &ScenePlacementRecord) -> &str {
    if !record.editor.name.is_empty() { &record.editor.name }
    else if matches!(record.asset_target, ScenePlacementAssetTarget::Cuboid { .. }) { "立方体" }
    else if record.asset_target.is_builtin() { "网格对象" }
    else { record.asset_path.rsplit('/').next().filter(|name| !name.is_empty()).unwrap_or(&record.id) }
}

#[derive(Default)]
pub(super) struct Summary {
    source: Option<(SceneId, String)>,
    stamp: (u64, u64, usize, u32),
    times: [Vec<f32>; ROWS],
}

impl Summary {
    pub(super) fn update(
        &mut self, source: Option<(&SceneId, &str)>, session: &EditorAuthoringSession, page: usize,
        duration: f32, tracks: [Option<&SceneMotionTrack>; ROWS],
    ) {
        let current = self.source.as_ref().map(|(scene, clip)| (scene, clip.as_str()));
        let stamp = (session.revision, session.successful_save_sequence, page, duration.to_bits());
        if current == source && self.stamp == stamp { return; }
        self.source = source.map(|(scene, clip)| (scene.clone(), clip.to_owned()));
        self.stamp = stamp;
        self.times = tracks.map(|track| track.map_or_else(Vec::new, |track| summary_times(track, duration)));
    }

    pub(super) fn time(&self, channel: Channel, slot: usize) -> Option<f32> {
        self.times[channel.index()].get(slot).copied()
    }
}

fn summary_times(track: &SceneMotionTrack, duration: f32) -> Vec<f32> {
    let channels = [&track.position, &track.rotation, &track.scale];
    let mut next = [0; 3];
    let mut times = Vec::new();
    // 三条有序轨道同帧只画一颗菱形，总览不从任意一个通道猜测唯一键。
    loop {
        let time = channels.iter().zip(next).filter_map(|(keys, index)| keys.get(index))
            .map(|key| key.time).min_by(f32::total_cmp);
        let Some(time) = time else { return times; };
        if times.len() == KEY_SLOTS { break; }
        times.push(time);
        for (keys, index) in channels.iter().zip(&mut next) {
            if keys.get(*index).is_some_and(|key| key.time == time) { *index += 1; }
        }
    }
    // 密集总览按时间区间归并，二分查找避免在显示时扫描整段动画。
    times.clear();
    let step = duration / (KEY_SLOTS - 1) as f32;
    for slot in 0..KEY_SLOTS - 1 {
        let first = slot as f32 * step;
        let end = (slot + 1) as f32 * step;
        let key = channels.iter()
            .filter_map(|keys| keys.get(keys.partition_point(|key| key.time < first)))
            .map(|key| key.time).filter(|time| *time < end).min_by(f32::total_cmp);
        times.extend(key);
    }
    if let Some(last) = channels.iter().filter_map(|keys| keys.last()).map(|key| key.time).max_by(f32::total_cmp)
        && times.last() != Some(&last)
    {
        times.push(last);
    }
    times
}

pub(super) fn select(
    context: Context, editor: Res<SceneEditorState>, session: Res<EditorAuthoringSession>,
    numeric: Res<SceneEditorNumericScrubMouseGuard>, menu: Res<SceneContextMenuState>,
    mut workbench: ResMut<Workbench>, mut selection: ResMut<SceneLayerSelection>,
    mut sidebar: Option<ResMut<SceneEditorSidebarState>>,
    rows: Query<(&Row, &SceneMotionButton, &EditorButtonClick), (Changed<EditorButtonClick>, Without<InteractionDisabled>)>,
) {
    if numeric.blocked() || menu.owns_mouse() || selection.0.is_some() || !context.can_edit(&editor, &session)
        || !workbench.shows_objects(context.record(&editor, &session).is_some())
    {
        return;
    }
    let Some((row, binding)) = rows.iter().find_map(|(row, action, click)| {
        (*action == SceneMotionButton::ChooseObject && click.is_clicked())
            .then(|| row.binding.as_ref().map(|binding| (row, binding))).flatten()
    }) else { return; };
    let Some(scene) = context.scene(&editor, &session) else { return; };
    let Some(motion) = session.draft.as_ref().map(|draft| &draft.motion) else { return; };
    let slot = page(workbench.object_page, motion.keyed_track_count()) * ROWS + row.channel.index();
    if &binding.scene != scene || workbench.scene.as_ref() != Some(scene)
        || workbench.clip_id.as_deref() != Some(binding.clip.as_str())
        || motion.active_clip_id() != Some(binding.clip.as_str())
        || !motion.keyed_track_at(slot).is_some_and(|track| track.placement_id == binding.placement)
    {
        return;
    }
    let Some(record) = context.placement(&session, &binding.placement).filter(|record| Context::can_target(record))
        else { return; };
    let Some(index) = context.browser.placement_record_index(&record.id, session.placement_structure_revision)
        else { return; };
    // 正式场景选择会同步树、属性和视口。只切换显示的轨道，当前播放头继续使用同一片段。
    selection.0 = Some(SceneObjectTarget {
        scene_id: scene.clone(), revision: session.revision,
        key: SceneObjectKey::Placement { index, id: record.id.clone(), mesh: None },
        name: name(record).to_owned(), hidden: false, locked: false, copyable: true,
    });
    workbench.show_objects = false;
    if let Some(sidebar) = sidebar.as_mut() { sidebar.reveal_scene_properties(); }
}

#[cfg(feature = "validation")]
pub(super) fn visible_row_for_validation(world: &mut World, placement_id: &str) -> Option<(Entity, usize)> {
    use bevy::ui::ComputedNode;

    let (button, channel) = world.query_filtered::<(Entity, &Row, &InheritedVisibility, &ComputedNode), Without<InteractionDisabled>>()
        .iter(world).find_map(|(entity, row, visibility, node)| {
            (row.binding.as_ref().is_some_and(|binding| binding.placement == placement_id)
                && visibility.get() && node.size().min_element() > 1.0)
                .then_some((entity, row.channel))
        })?;
    let count = world.query::<(&super::controls::Marker, &InheritedVisibility, &ComputedNode, &Node)>()
        .iter(world).filter(|(marker, visibility, layout, node)|
            marker.channel == channel && visibility.get() && layout.size().min_element() > 1.0
                && node.display != Display::None).count();
    Some((button, count))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authoring::scene_motion::{SceneMotionKey, SceneMotionPlayback};

    fn track(position: &[u32], rotation: &[u32], scale: &[u32]) -> SceneMotionTrack {
        let keys = |frames: &[u32]| frames.iter().map(|&frame| SceneMotionKey { time: frame as f32, value: [0.0; 3] }).collect();
        SceneMotionTrack {
            placement_id: "mesh".into(), duration_seconds: 120.0, playback: SceneMotionPlayback::Once,
            position: keys(position), rotation: keys(rotation), scale: keys(scale), collider: None,
        }
    }

    #[test]
    fn overview_combines_same_frame_keys_and_keeps_each_distinct_time() {
        assert_eq!(summary_times(&track(&[0, 4, 10], &[0, 6, 10], &[4, 12]), 12.0), [0.0, 4.0, 6.0, 10.0, 12.0]);
    }

    #[test]
    fn overview_dense_keys_keep_both_endpoints_and_bound_only_display_slots() {
        let frames: Vec<_> = (0..=120).collect();
        let track = track(&frames, &frames, &[]);
        let times = summary_times(&track, track.duration_seconds);
        assert_eq!(times.len(), KEY_SLOTS);
        assert_eq!((times.first(), times.last()), (Some(&0.0), Some(&120.0)));
        assert!(times.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(track.position.len(), 121);
        assert_eq!(page(10, 0), 0);
        assert_eq!(page(10, 8), 2);
    }
}
