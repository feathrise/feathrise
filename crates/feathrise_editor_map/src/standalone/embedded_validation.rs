//! 常驻视口的显式有限作者验收端口：只驱动原创建请求和原撤销按钮，不替代业务实现。

use feathrise_editor_ui::UiInteraction;
use crate::{
    authoring::{
        EditorAuthoringSession, EditorCommand,
        basic_shapes::{BasicShapeState, CuboidCreationRequest},
    },
    components::MapEditorActionButton,
    resources::MapEditorState,
    types::EditorAction,
};
use bevy::prelude::*;
use feathrise_map_content::{MapId, MapPlacementAssetTarget};

#[path = "embedded_validation_presentation.rs"]
mod presentation;
pub use presentation::{presentation_probe, PresentationProbe};
#[path = "embedded_validation_live.rs"]
pub mod live;

#[path = "embedded_validation_tool_previews.rs"]
pub mod tool_previews;

#[derive(Resource)]
struct Installed;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PlacementProbe {
    pub id: String,
    pub translation: [f32; 3],
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AuthorProbe {
    pub revision: u64,
    pub save_sequence: u64,
    pub undo_len: usize,
    pub redo_len: usize,
    pub dirty: bool,
    pub last_added_id: Option<String>,
    pub placements: Vec<PlacementProbe>,
}

/// 宿主先核验本轮自建工程的 owner / provenance；普通编译没有本模块，普通会话不安装。
pub fn install(app: &mut App) {
    app.insert_resource(Installed);
    live::install(app);
}

pub fn probe(world: &World) -> Result<AuthorProbe, String> {
    if !world.contains_resource::<Installed>() {
        return Err("常驻验收作者端口未安装".into());
    }
    let session = world
        .get_resource::<EditorAuthoringSession>()
        .ok_or("作者会话未就绪")?;
    let draft = session.draft.as_ref().ok_or("作者草稿未就绪")?;
    if draft.map_id.as_str() != "M1-SAMPLE" || draft.placements.placements.len() > 64 {
        return Err("常驻验收只允许本轮有限 M1-SAMPLE 夹具，不接受任意作者地图".into());
    }
    Ok(AuthorProbe {
        revision: session.revision,
        save_sequence: session.successful_save_sequence,
        undo_len: session.undo.len(),
        redo_len: session.redo.len(),
        dirty: world.resource::<MapEditorState>().dirty,
        last_added_id: match session.undo.last() {
            Some(EditorCommand::PlacementAdded(record)) => Some(record.id.clone()),
            _ => None,
        },
        placements: draft
            .placements
            .placements
            .iter()
            .map(|record| PlacementProbe {
                id: record.id.clone(),
                translation: record.translation,
            })
            .collect(),
    })
}

pub fn request_cuboid(world: &mut World, position: [f32; 3]) -> Result<(), String> {
    let before = probe(world)?;
    if !super::preview_ready_for(world, "M1-SAMPLE") {
        return Err("原作者视口尚未就绪".into());
    }
    if let Some(reason) = super::saved_play_blocker(world) {
        return Err(reason);
    }
    let mut shapes = world.resource_mut::<BasicShapeState>();
    if shapes.creation_request.is_some() {
        return Err("原坐标创建请求已占用".into());
    }
    shapes.creation_identity = Some((MapId::parse("M1-SAMPLE")?, before.revision));
    shapes.creation_request = Some(CuboidCreationRequest {
        target: MapPlacementAssetTarget::DEFAULT_CUBOID,
        position: Some(Vec3::from_array(position)),
    });
    Ok(())
}

pub fn pulse_history(world: &mut World, redo: bool) -> Result<Option<Entity>, String> {
    let _ = probe(world)?;
    let action = if redo {
        EditorAction::Redo
    } else {
        EditorAction::Undo
    };
    let Some(entity) = world
        .query::<(Entity, &MapEditorActionButton)>()
        .iter(world)
        .find_map(|(entity, button)| (button.0 == action).then_some(entity))
    else {
        return Ok(None);
    };
    if world.get::<bevy::ui::InteractionDisabled>(entity).is_some() {
        return Ok(None);
    }
    world.entity_mut(entity).insert((
        UiInteraction::Pressed,
        feathrise_editor_ui::EditorButtonClick::triggered(),
    ));
    Ok(Some(entity))
}

pub fn release_pulse(world: &mut World, entity: Entity) {
    if let Ok(mut entity) = world.get_entity_mut(entity) {
        entity.insert((
            UiInteraction::None,
            feathrise_editor_ui::EditorButtonClick::default(),
        ));
    }
}
