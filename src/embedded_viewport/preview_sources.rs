//! 四个原作者域的窄适配；帧内只读固定状态与Arc，来源自身负责后台准备、边沿和失效。
use bevy::prelude::*;
use feathrise_editor_protocol::PreviewKind;
use feathrise_editor_scene::standalone::{
    embedded_author_preview::{self as author, EmbeddedAuthorPreviewState},
    embedded_placement_preview::{self as placement, EmbeddedPlacementPreview},
    embedded_surface_trail::{self as surface, EmbeddedSurfaceTrailState},
    embedded_vegetation::{self as vegetation, EmbeddedVegetationState},
};
use std::sync::Arc;

pub(super) struct Offer {
    pub kind: PreviewKind,
    pub session: u64,
    pub generation: u64,
    pub sequence: u64,
    pub scene_id: String,
    pub format: &'static str,
    pub bytes: Arc<Vec<u8>>,
}

pub(super) fn install(app: &mut App) {
    placement::install(app);
    app.add_plugins((
        vegetation::VegetationPreviewPlugin,
        author::AuthorPreviewPlugin,
        surface::SurfaceTrailPreviewPlugin,
    ));
}

pub(super) fn configure(world: &mut World, enabled: bool, session: u64, generation: u64) {
    macro_rules! set {
        ($state:ty) => {
            if let Some(mut state) = world.get_resource_mut::<$state>() {
                state.enabled = enabled;
                state.session = session;
                state.generation = generation;
            }
        };
    }
    set!(EmbeddedPlacementPreview);
    set!(EmbeddedVegetationState);
    set!(EmbeddedAuthorPreviewState);
    set!(EmbeddedSurfaceTrailState);
}

pub(super) fn error(world: &World) -> Option<String> {
    world
        .get_resource::<EmbeddedPlacementPreview>()
        .and_then(|state| state.error.clone())
        .or_else(|| {
            world
                .get_resource::<EmbeddedVegetationState>()
                .and_then(|state| state.error.clone())
        })
        .or_else(|| {
            world
                .get_resource::<EmbeddedAuthorPreviewState>()
                .and_then(|state| state.error.clone())
        })
        .or_else(|| {
            world
                .get_resource::<EmbeddedSurfaceTrailState>()
                .and_then(|state| state.error.clone())
        })
}

pub(super) fn offers(world: &World) -> Vec<Offer> {
    let mut result = Vec::with_capacity(4);
    if let Some(ready) = placement::peek(world) {
        result.push(Offer {
            kind: PreviewKind::Placement,
            session: ready.session,
            generation: ready.generation,
            sequence: ready.sequence,
            scene_id: ready.scene_id.clone(),
            format: placement::PLACEMENT_PREVIEW_FORMAT,
            bytes: ready.bytes.clone(),
        });
    }
    macro_rules! offer {
        ($state:ty,$kind:ident,$format:expr) => {
            // ACK可在来源自己的Update之前到达。ready留给来源推进版本，宿主此帧不能再发旧序号。
            if let Some(ready) = world
                .get_resource::<$state>()
                .and_then(|state| state.ready.as_ref().filter(|ready| ready.sequence > state.acknowledged_sequence))
            {
                result.push(Offer {
                    kind: PreviewKind::$kind,
                    session: ready.session,
                    generation: ready.generation,
                    sequence: ready.sequence,
                    scene_id: ready.scene_id.clone(),
                    format: $format,
                    bytes: ready.bytes.clone(),
                });
            }
        };
    }
    offer!(
        EmbeddedVegetationState,
        Vegetation,
        feathrise_scene_content::embedded_preview::VEGETATION_PREVIEW_FORMAT
    );
    offer!(
        EmbeddedAuthorPreviewState,
        AuthorDomain,
        feathrise_scene_content::embedded_author_preview::AUTHOR_PREVIEW_FORMAT
    );
    offer!(
        EmbeddedSurfaceTrailState,
        SurfaceTrail,
        feathrise_scene_content::embedded_surface_trail::SURFACE_TRAIL_FORMAT
    );
    result
}

pub(super) fn acknowledge(world: &mut World, kind: PreviewKind, sequence: u64) {
    macro_rules! ack {
        ($state:ty) => {
            if let Some(mut state) = world.get_resource_mut::<$state>() {
                state.acknowledged_sequence = sequence;
            }
        };
    }
    match kind {
        PreviewKind::Placement => {
            placement::acknowledge(world, sequence);
        }
        PreviewKind::Vegetation => {
            ack!(EmbeddedVegetationState);
        }
        PreviewKind::AuthorDomain => {
            ack!(EmbeddedAuthorPreviewState);
        }
        PreviewKind::SurfaceTrail => {
            ack!(EmbeddedSurfaceTrailState);
        }
    }
}

pub(super) fn settled(world: &World) -> bool {
    world
        .get_resource::<EmbeddedPlacementPreview>()
        .is_none_or(|state| !state.pending && state.error.is_none())
        && world
            .get_resource::<EmbeddedVegetationState>()
            .is_none_or(|state| !state.pending && state.ready.is_none() && state.error.is_none())
        && world
            .get_resource::<EmbeddedAuthorPreviewState>()
            .is_none_or(|state| !state.pending && state.ready.is_none() && state.error.is_none())
        && world
            .get_resource::<EmbeddedSurfaceTrailState>()
            .is_none_or(|state| !state.pending && state.ready.is_none() && state.error.is_none())
}

pub(super) fn invalidate(world: &mut World) {
    vegetation::invalidate(world);
    author::invalidate(world);
    surface::invalidate(world);
}

#[cfg(feature = "validation")]
pub(super) fn diagnostic(world: &World) -> serde_json::Value {
    use serde_json::json;
    macro_rules! source {
        ($state:ty) => { world.get_resource::<$state>().map(|state| json!({
            "enabled":state.enabled,"session":state.session,"generation":state.generation,
            "pending":state.pending,"ack":state.acknowledged_sequence,"error":state.error,
            "ready":state.ready.as_ref().map(|ready|json!({"sequence":ready.sequence,
                "scene_id":ready.scene_id,"bytes":ready.bytes.len()}))
        })) };
    }
    json!({"vegetation":source!(EmbeddedVegetationState),
        "author_domain":source!(EmbeddedAuthorPreviewState),
        "surface_trail":source!(EmbeddedSurfaceTrailState),
        "placement":world.get_resource::<EmbeddedPlacementPreview>().map(|state|json!({
            "enabled":state.enabled,"pending":state.pending,"error":state.error}))})
}

#[cfg(test)]
mod tests {
    use super::*;
    use vegetation::EmbeddedVegetationPreview;

    #[test]
    fn embedded_author_domain_ack_is_not_replayed_by_the_next_host_poll() {
        let mut world = World::new();
        let mut state = EmbeddedAuthorPreviewState::default();
        state.ready = Some(Arc::new(author::EmbeddedAuthorPreview {
            session: 7,
            generation: 1,
            sequence: 2,
            scene_id: "preview-map".into(),
            bytes: Arc::new(vec![1]),
        }));
        world.insert_resource(state);
        assert_eq!(offers(&world)[0].sequence, 2);
        acknowledge(&mut world, PreviewKind::AuthorDomain, 2);
        assert!(offers(&world).is_empty());
    }

    #[test]
    fn embedded_preview_ack_cannot_reoffer_before_the_source_consumes_its_ready_slot() {
        let mut world = World::new();
        let mut state = EmbeddedVegetationState::default();
        state.ready = Some(Arc::new(EmbeddedVegetationPreview {
            session: 7,
            generation: 1,
            sequence: 3,
            scene_id: "preview-map".into(),
            source_revision: 8,
            structure_revision: 1,
            bytes: Arc::new(vec![1]),
        }));
        world.insert_resource(state);
        assert_eq!(offers(&world).len(), 1);
        acknowledge(&mut world, PreviewKind::Vegetation, 3);
        assert!(
            world.resource::<EmbeddedVegetationState>().ready.is_some(),
            "来源尚未运行，必须留其自行推进已采用版本"
        );
        assert!(
            offers(&world).is_empty(),
            "同帧消费真实ACK后不能重新发送旧正文"
        );
        assert!(!settled(&world), "来源尚未结清时不能提前进入预览");
        let mut state = world.resource_mut::<EmbeddedVegetationState>();
        let ready = state.ready.as_ref().unwrap();
        state.ready = Some(Arc::new(EmbeddedVegetationPreview {
            session: ready.session,
            generation: ready.generation,
            sequence: 4,
            scene_id: ready.scene_id.clone(),
            source_revision: 9,
            structure_revision: 1,
            bytes: ready.bytes.clone(),
        }));
        assert_eq!(offers(&world)[0].sequence, 4, "新的生产结果仍可发送");
    }
}
