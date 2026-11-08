//! 完整摆放根的编辑器显隐覆盖；保留外部最新Visibility，关闭前有界恢复。

use crate::authoring::EditorAuthoringSession;
use crate::authoring::placement_deletion::{PlacementDeletionIndex, PlacementDeletionSuppression};
use crate::authoring::placement_preview_ownership::components::PlacementPreviewSuppression;
use crate::resources::MapEditorState;
use crate::ui::gltf_browser::EditorGltfBrowserState;
use bevy::prelude::*;
use feathrise_scene::MapPlacementInstance;
use std::collections::{BTreeSet, HashMap};
use super::types::PlacementVisibilityIntent;

#[derive(Component)]
pub(super) struct EditorObjectVisibility {
    original: Visibility,
}

/// 作者预览和场景树隐藏共用底层显隐；不能把另一覆盖层产生的Hidden当作正式原值。
pub(crate) fn placement_base_visibility(world: &World, root: Entity) -> Visibility {
    world.get::<EditorObjectVisibility>(root).map(|previous| previous.original)
        .or_else(|| world.get::<PlacementPreviewSuppression>(root).map(|suppression| suppression.original_visibility))
        .or_else(|| world.get::<PlacementDeletionSuppression>(root).map(|suppression| suppression.original_visibility))
        .or_else(|| world.get::<Visibility>(root).copied()).unwrap_or(Visibility::Inherited)
}

/// 冷重载的新根可能尚未轮到有界显隐同步；归还作者遮蔽时仍须立即服从当前场景树标记。
fn editor_hides_root(world: &World, root: Entity) -> bool {
    let Some(editor) = world.get_resource::<MapEditorState>() else { return false; };
    if !editor.open || editor.close_requested { return false; }
    let Some(instance) = world.get::<MapPlacementInstance>(root) else { return false; };
    let Some(session) = world.get_resource::<EditorAuthoringSession>() else { return false; };
    let Some(draft) = session.draft.as_ref().filter(|draft| &draft.map_id == instance.map_id()) else { return false; };
    let Some(browser) = world.get_resource::<EditorGltfBrowserState>() else { return false; };
    if !browser.placement_index_ready(session.placement_structure_revision) {
        return true; // 完整索引尚未确认时先保留单根隐藏，后续dirty同步再决定是否解除。
    }
    browser.placement_record_index(instance.id(), session.placement_structure_revision)
        .and_then(|index| draft.placements.placements.get(index))
        .filter(|record| record.id == instance.id()).is_some_and(|record| record.editor.hidden)
}

/// 释放预览只归还自己的覆盖；直接改值不发布新的外部显隐意图，行隐藏仍保留Hidden。
pub(crate) fn restore_placement_base_visibility(world: &mut World, root: Entity, original: Visibility) {
    if world.get_entity(root).is_err() { return; }
    let overridden = world.get::<EditorObjectVisibility>(root).is_some() || editor_hides_root(world, root);
    if overridden {
        world.entity_mut(root).insert(EditorObjectVisibility { original });
    }
    let masked = world.get::<PlacementPreviewSuppression>(root).is_some()
        || world.get::<PlacementDeletionSuppression>(root).is_some();
    let restored = if overridden || masked { Visibility::Hidden } else { original };
    if let Some(mut visibility) = world.get_mut::<Visibility>(root) {
        *visibility = restored;
    }
    if let Some(mut state) = world.get_resource_mut::<ObjectVisibilityState>() {
        if overridden { state.overridden.insert(root); }
        state.dirty.insert(root); // 定向唤醒，沿用每帧64根预算，不新增完整场景扫描。
    }
}

#[derive(Resource, Default)]
pub(crate) struct ObjectVisibilityState {
    roots: BTreeSet<Entity>,
    intents: HashMap<Entity, PlacementVisibilityIntent>, // 与根同生共灭；不逐帧扫描或依赖会回绕的change tick。
    dirty: BTreeSet<Entity>,
    overridden: BTreeSet<Entity>,
    deletion_roots: BTreeSet<Entity>, // 删除票据可在无预览时存活；关闭F12只定向遍历这些根以归还过期覆盖。
    context: Option<(bool, Option<feathrise_map_content::MapId>, u64, u64, u64)>,
    sweeping: bool,
    after: Option<Entity>,
}

impl ObjectVisibilityState {
    pub(crate) fn visibility_intent(&self, root: Entity) -> Option<PlacementVisibilityIntent> {
        self.intents.get(&root).cloned()
    }

    pub(crate) fn visibility_intent_matches(&self, root: Entity, frozen: &PlacementVisibilityIntent) -> bool {
        self.intents.get(&root).is_some_and(|current| current.same_as(frozen))
    }

    pub(crate) fn restored(&self) -> bool {
        self.overridden.is_empty()
    }
}

pub(super) fn register_root(
    event: On<Insert<MapPlacementInstance>>,
    mut state: ResMut<ObjectVisibilityState>,
) {
    state.roots.insert(event.entity);
    state.intents.entry(event.entity).or_default(); // 同根正常重施MPI不换票；Remove后重新登记才取得新身份。
    state.dirty.insert(event.entity);
}

pub(super) fn remove_root(
    event: On<Remove<MapPlacementInstance>>,
    mut state: ResMut<ObjectVisibilityState>,
) {
    state.roots.remove(&event.entity);
    state.intents.remove(&event.entity);
    state.dirty.remove(&event.entity);
    state.overridden.remove(&event.entity);
}

pub(super) fn register_deletion_suppression(
    event: On<Add<PlacementDeletionSuppression>>,
    mut state: ResMut<ObjectVisibilityState>,
) {
    state.deletion_roots.insert(event.entity);
    state.dirty.insert(event.entity);
}

pub(super) fn remove_deletion_suppression(
    event: On<Remove<PlacementDeletionSuppression>>,
    mut state: ResMut<ObjectVisibilityState>,
) {
    state.deletion_roots.remove(&event.entity);
    state.dirty.insert(event.entity);
}

fn release_expired_deletion(commands: &mut Commands, root: Entity) {
    commands.queue(move |world: &mut World| {
        crate::authoring::placement_deletion::release_expired_suppression(world, root);
    }); // 真正执行时重验scope/ticket；过期检测不能捕获旧布尔值后移除后来接管的mask。
}

pub(super) fn remember_external_visibility(
    event: On<Insert<Visibility>>,
    mut commands: Commands,
    mut roots: Query<
        (&MapPlacementInstance, &mut Visibility, Option<&mut EditorObjectVisibility>,
            Option<&mut PlacementPreviewSuppression>, Option<&mut PlacementDeletionSuppression>),
        With<MapPlacementInstance>,
    >,
    mut state: ResMut<ObjectVisibilityState>,
) {
    if let Ok((instance, mut visibility, previous, suppression, deletion)) = roots.get_mut(event.entity) {
        state.intents.insert(event.entity, PlacementVisibilityIntent::default()); // 同值Insert也是新外部意图，不能靠枚举值猜测归还责任。
        let external = *visibility;
        if let Some(mut previous) = previous {
            previous.original = external;
        }
        if let Some(mut suppression) = suppression {
            suppression.original_visibility = external; // 自身遮蔽只直接改值；真实Insert仍能记录删除或新外部显隐意图。
            *visibility = Visibility::Hidden; // 外部恢复不得越过仍有效的独立作者预览。
        }
        if let Some(mut deletion) = deletion {
            deletion.original_visibility = external;
            *visibility = Visibility::Hidden; // 两个独立原因共享真实底层意图，解除其中一个不能复活旧根。
        }
        if instance.asset_target().is_builtin() {
            feathrise_scene::set_builtin_placement_collision_enabled(&mut commands, event.entity, *visibility != Visibility::Hidden);
        } // 正式立方体共享批次刚体，只隐藏根不会停用子碰撞，需同步唯一盒碰撞。
        state.dirty.insert(event.entity);
    } // 删除、退役和变换写入的显隐意图仍有效，不能在关闭时复活已删除根。
}

pub(super) fn sync_visibility(
    mut commands: Commands,
    editor: Res<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    browser: Res<EditorGltfBrowserState>,
    deletions: Option<Res<PlacementDeletionIndex>>,
    mut state: ResMut<ObjectVisibilityState>,
    mut roots: Query<(
        &MapPlacementInstance,
        &mut Visibility,
        Option<&EditorObjectVisibility>,
        Option<&PlacementPreviewSuppression>,
        Option<&PlacementDeletionSuppression>,
    )>,
) {
    let enabled = editor.open && !editor.close_requested;
    let context = (
        enabled,
        session.draft.as_ref().map(|draft| draft.map_id.clone()),
        session.scene_objects_revision,
        session.placement_structure_revision,
        deletions.as_deref().map_or(0, PlacementDeletionIndex::revision),
    );
    if state.context.as_ref() != Some(&context) {
        state.context = Some(context.clone());
        state.sweeping = true;
        state.after = None;
    }
    let mut work = Vec::with_capacity(64);
    // 恢复优先使用真实覆盖根，空地图或无覆盖时不等待完整场景索引扫描。
    if !enabled {
        work.extend(state.overridden.iter().take(64).copied());
    }
    if state.sweeping && work.len() < 64 {
        use std::ops::Bound::{Excluded, Unbounded};
        let start = state.after.map(Excluded).unwrap_or(Unbounded);
        let remaining = 64 - work.len();
        let sweep = if enabled { &state.roots } else { &state.deletion_roots };
        let selected: Vec<_> = sweep.range((start, Unbounded)).take(remaining).copied().collect();
        state.after = selected.last().copied().or(state.after);
        if selected.len() < remaining {
            state.sweeping = false;
        }
        for entity in selected {
            if !work.contains(&entity) { work.push(entity); }
        } // 与行隐藏恢复共用64额度；关闭时不扫描普通根，也不在稳定帧反复从首个删除根开始。
    }
    while work.len() < 64 {
        let Some(entity) = state.dirty.pop_first() else {
            break;
        };
        if !work.contains(&entity) {
            work.push(entity);
        }
    }
    for entity in work {
        let Ok((instance, mut visibility, previous, suppression, deletion)) = roots.get_mut(entity) else {
            state.overridden.remove(&entity);
            if state.deletion_roots.contains(&entity) { release_expired_deletion(&mut commands, entity); }
            continue;
        };
        let masked = suppression.is_some() || deletion.is_some();
        if masked {
            *visibility = Visibility::Hidden; // 作者覆盖独立于F12开关和场景树索引，任何显隐刷新都不能复活正式旧根。
        }
        let mut hide = false;
        if enabled && context.1.as_ref() == Some(instance.map_id()) {
            if !browser.placement_index_ready(session.placement_structure_revision) {
                state.dirty.insert(entity);
                if instance.asset_target().is_builtin() {
                    feathrise_scene::set_builtin_placement_collision_enabled(&mut commands, entity, *visibility != Visibility::Hidden);
                }
                if deletion.is_some() { release_expired_deletion(&mut commands, entity); }
                continue; // 旧索引不得跨图或跨数组代次决定显隐。
            }
            hide = browser
                .placement_record_index(instance.id(), session.placement_structure_revision)
                .and_then(|index| session.draft.as_ref()?.placements.placements.get(index))
                .filter(|record| record.id == instance.id())
                .is_some_and(|record| record.editor.hidden);
        }
        if hide {
            if previous.is_none() {
                commands.entity(entity).insert(EditorObjectVisibility {
                    original: suppression.map(|suppression| suppression.original_visibility)
                        .or_else(|| deletion.map(|deletion| deletion.original_visibility)).unwrap_or(*visibility),
                });
                state.overridden.insert(entity);
            }
            if *visibility != Visibility::Hidden {
                *visibility = Visibility::Hidden;
            }
        } else if let Some(previous) = previous {
            *visibility = if masked { Visibility::Hidden } else { previous.original };
            commands.entity(entity).remove::<EditorObjectVisibility>();
            state.overridden.remove(&entity);
        }
        if instance.asset_target().is_builtin() {
            feathrise_scene::set_builtin_placement_collision_enabled(&mut commands, entity, *visibility != Visibility::Hidden);
        }
        if deletion.is_some() { release_expired_deletion(&mut commands, entity); }
        // 行隐藏的移除先落地，再归还删除覆盖，避免旧行组件让最终显隐永久停在Hidden。
    }
}
