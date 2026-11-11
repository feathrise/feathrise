//! 捕获原工具已经发布的语义状态，宿主仅桥接纯值，不依赖工具内部或生成线段。

use super::embedded_snapshot::EmbeddedSnapshotState;
use crate::viewport::tool_preview::AuthorToolFrame;
use bevy::prelude::*;
use feathrise_author_tools::AuthorToolPreview;

#[derive(Resource, Default)]
pub struct EmbeddedAuthorTools {
    pub revision: u64,
    pub scene_id: Option<String>,
    pub generation: u64,
    pub preview: AuthorToolPreview,
    pub error: Option<String>,
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<AuthorToolFrame>()
        .init_resource::<EmbeddedAuthorTools>()
        .add_systems(Last, capture);
}

fn capture(
    snapshot: Res<EmbeddedSnapshotState>,
    session: Option<Res<crate::authoring::EditorAuthoringSession>>,
    frame: Res<AuthorToolFrame>,
    mut tools: ResMut<EmbeddedAuthorTools>,
) {
    let mut preview = if snapshot.enabled { frame.preview.clone() } else { default() };
    let mut error = if snapshot.enabled { frame.error.clone() } else { None };
    if let Err(reason) = preview.validate() {
        preview.items.clear();
        error = Some(format!("作者工具预览无效：{reason}"));
    }
    let scene_id = session.as_deref().and_then(|session| session.draft.as_ref())
        .map(|draft| draft.scene_id.as_str().to_owned());
    if tools.revision != snapshot.revision || tools.scene_id != scene_id || tools.preview != preview || tools.error != error {
        tools.revision = snapshot.revision;
        tools.scene_id = scene_id;
        tools.preview = preview;
        tools.error = error;
        tools.generation = tools.generation.wrapping_add(1).max(1);
    }
}
