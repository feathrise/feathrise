//! 通过原生场景树点击单个静水体的三个右侧按钮，并核对相邻静水体的隔离。

use super::{click_control, row_has_icon, ui_visible};
use crate::{
    authoring::{EditorAuthoringSession, layer_categories::water_layer_state, terrain_documents::root_water},
    types::WaterAuthoringFeatureKind::StillWater,
    ui::{
        scene_context_menu::{SceneObjectKey, SceneObjectRow}, scene_layers::{LayerCategoryVisibility, StillWaterRowToggle},
        scene_sidebar::TreeRowToggle,
    },
};
use bevy::{prelude::*, ui::InteractionDisabled};
use serde_json::{Value, json};

const IDS: [&str; 2] = ["scene-tree-water-target", "scene-tree-water-neighbor"];
pub(super) const SCREENSHOT: &str = "scene-tree-still-water-toggles.png";
type State = (bool, bool, bool); // 参与、作者显隐、锁定，按来源及稳定水体 ID 读取。
const DEFAULT: State = (true, false, false);
const CLICKS: [(TreeRowToggle, State, &str); 3] = [
    (TreeRowToggle::Visibility, (true, true, false), "icons/editor/mesh_hidden.png"),
    (TreeRowToggle::Participation, (false, true, false), "icons/editor/mesh_disabled.png"),
    (TreeRowToggle::Lock, (false, true, true), "icons/editor/mesh_locked.png"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::standalone::validation_fixture) enum Step { Click(usize, u8), Verify(usize) }

fn states(world: &World) -> Option<[State; 2]> {
    let draft = world.resource::<EditorAuthoringSession>().draft.as_ref()?;
    let layers = &root_water::listed_source(draft, None)?.layers;
    if !IDS.iter().all(|id| layers.iter().any(|layer| layer.id == *id)) { return None; }
    let visibility = world.resource::<LayerCategoryVisibility>();
    Some(IDS.map(|id| {
        let (enabled, locked) = water_layer_state(draft, None, id);
        (enabled, visibility.water_hidden(&draft.scene_id, None, id), locked)
    }))
}

pub(super) fn proof(world: &World) -> Option<Value> {
    let [target, neighbor] = states(world)?;
    let record = |id: &str, (enabled, hidden, locked): State| json!({
        "id":id,"enabled":enabled,"hidden":hidden,"locked":locked,
    });
    Some(json!({"target":record(IDS[0], target),"neighbor":record(IDS[1], neighbor)}))
}

fn button(world: &mut World, id: &str, action: TreeRowToggle) -> Option<Entity> {
    world.query::<(Entity, &StillWaterRowToggle, &ChildOf)>().iter(world)
        .find_map(|(entity, toggle, parent)| {
            let row = world.get::<SceneObjectRow>(parent.parent())?.0.as_ref()?;
            (toggle.0 == action && ui_visible(world, entity)
                && matches!(&row.key, SceneObjectKey::Water { kind: StillWater, terrain: None, id: row_id, .. } if row_id == id))
                .then_some(entity)
        })
}

pub(super) fn advance(world: &mut World, step: &mut Step, revision: &mut u64) -> Result<bool, String> {
    *step = match *step {
        Step::Click(index, frame) => {
            if index == 0 && frame == 0 {
                let Some(baseline) = states(world) else { return Ok(false); };
                if baseline != [DEFAULT; 2] { return Err("静水子行验收夹具的初始状态不符".into()); }
                for id in IDS {
                    for action in TreeRowToggle::ALL {
                        let Some(control) = button(world, id, action) else { return Ok(false); };
                        if world.get::<InteractionDisabled>(control).is_some() { return Ok(false); }
                    }
                }
            }
            let Some(control) = button(world, IDS[0], CLICKS[index].0) else { return Ok(false); };
            if world.get::<InteractionDisabled>(control).is_some() { return Ok(false); }
            click_control(world, control, frame)?.map_or(Step::Verify(index), |next| Step::Click(index, next))
        }
        Step::Verify(index) => {
            let Some([target, neighbor]) = states(world) else { return Ok(false); };
            if neighbor != DEFAULT { return Err("操作目标静水体时，邻近静水体的状态被改变".into()); }
            let (action, expected, icon) = CLICKS[index];
            if target != expected { return Ok(false); }
            let Some(control) = button(world, IDS[0], action) else { return Ok(false); };
            if !row_has_icon(world, control, icon) { return Ok(false); }
            let actual_revision = world.resource::<EditorAuthoringSession>().revision;
            let expected_revision = if action == TreeRowToggle::Visibility { *revision } else { revision.wrapping_add(1) };
            if actual_revision != expected_revision {
                return Err(format!("静水子行 {action:?} 修订不符，实际为 {actual_revision}，预期为 {expected_revision}"));
            }
            *revision = actual_revision;
            if index + 1 == CLICKS.len() { return Ok(true); }
            Step::Click(index + 1, 0)
        }
    };
    Ok(false)
}
