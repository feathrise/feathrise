//! 场景树摆放和逻辑组的行内开关，绑定当前可见行，沿用高度地形图标。

use super::{EditorGltfBrowserEntryKind, EditorGltfBrowserState, SceneEditorGltfBrowserRow};
use crate::{
    authoring::{EditorAuthoringSession, placement_deletion::PlacementDeletionIdentity, placement_participation},
    resources::SceneEditorState,
    ui::{
        scene_objects::{SceneObjectState, effective_metadata, effective_participation},
        scene_sidebar::{
            TreeRowToggle, load_tree_toggle_images, sync_tree_toggle_icon, tree_toggle_colors,
        },
    },
};
use bevy::{prelude::*, ui::InteractionDisabled};
use feathrise_editor_ui::{EditorButtonClick, UiInteraction};
use feathrise_scene_content::ScenePlacementRecord;

struct RowBinding {
    document: PlacementDeletionIdentity,
    revision: u64,
    entries_revision: u64,
    index: usize,
}

impl RowBinding {
    fn matches(&self, session: &EditorAuthoringSession, row: &SceneEditorGltfBrowserRow, index: usize) -> bool {
        self.document.matches(&session.placement_deletions) && self.revision == session.revision
            && self.entries_revision == row.entries_revision && self.index == index
    }
}

fn published_index(
    browser: &EditorGltfBrowserState, session: &EditorAuthoringSession, row: &SceneEditorGltfBrowserRow,
) -> Option<usize> {
    let draft = session.draft.as_ref()?;
    if !browser.placement_index_ready(session.placement_structure_revision)
        || browser.entries_build.is_some()
        || browser.observation.scene_objects_revision != session.scene_objects_revision
        || browser.observation.draft_scene_id.as_deref() != Some(draft.scene_id.as_str())
        || row.entries_revision != browser.published_entries_revision
    {
        return None;
    }
    row.entry_index
}

#[derive(Component)]
pub(crate) struct PlacementTreeToggle {
    pub(crate) kind: TreeRowToggle,
    binding: Option<RowBinding>,
}

impl PlacementTreeToggle {
    pub(super) fn new(kind: TreeRowToggle) -> Self {
        Self { kind, binding: None }
    }

    pub(crate) fn target<'a>(
        &self, browser: &'a EditorGltfBrowserState, session: &EditorAuthoringSession,
        row: &SceneEditorGltfBrowserRow,
    ) -> Option<&'a EditorGltfBrowserEntryKind> {
        let index = published_index(browser, session, row)?;
        if !self.binding.as_ref()?.matches(session, row, index) { return None; }
        Some(&browser.entries.get(index)?.kind)
    }
}

fn representative<'a>(
    browser: &EditorGltfBrowserState, session: &'a EditorAuthoringSession, kind: &EditorGltfBrowserEntryKind,
) -> Option<&'a ScenePlacementRecord> {
    let (index, id) = match kind {
        EditorGltfBrowserEntryKind::Placement { placement_id, .. } => (
            browser.placement_record_index(placement_id, session.placement_structure_revision)?,
            placement_id.as_str(),
        ),
        EditorGltfBrowserEntryKind::Group { first_member, .. } => (
            *first_member, browser.signature.get(*first_member)?.placement_id.as_str(),
        ),
        _ => return None,
    };
    session.draft.as_ref()?.placements.placements.get(index).filter(|record| record.id == id)
}

pub(crate) fn sync(
    mut commands: Commands,
    browser: Res<EditorGltfBrowserState>,
    editor: Res<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    objects: Res<SceneObjectState>,
    server: Res<AssetServer>,
    mut images: Local<Option<[Handle<Image>; 6]>>,
    rows: Query<&SceneEditorGltfBrowserRow>,
    mut buttons: Query<(
        Entity, &ChildOf, &Children, &mut PlacementTreeToggle, &mut Node,
        Option<&mut EditorButtonClick>, &UiInteraction, Has<InteractionDisabled>,
        &mut BackgroundColor, &mut BorderColor,
    )>,
    mut icons: Query<&mut ImageNode>,
) {
    let images = images.get_or_insert_with(|| load_tree_toggle_images(&server));
    let ready = editor.open && !editor.close_requested && !editor.new_scene_dialog_open
        && !editor.scene_transition_in_progress && editor.pending_scene_transition.is_none()
        && editor.scene_switch_confirmation.is_none() && !session.saving && objects.accepts_commands()
        && session.draft.as_ref().is_some_and(|draft| editor.selected_scene_id.as_ref() == Some(&draft.scene_id));
    for (entity, parent, children, mut toggle, mut node, click, interaction, disabled, mut fill, mut border) in &mut buttons {
        let Ok(row) = rows.get(parent.parent()) else { continue; };
        let kind = row.entry_index.and_then(|index| browser.entries.get(index)).map(|entry| &entry.kind);
        let show = matches!(kind, Some(EditorGltfBrowserEntryKind::Placement { .. } | EditorGltfBrowserEntryKind::Group { .. }));
        let display = if show { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
        let record = kind.and_then(|kind| representative(&browser, &session, kind));
        let index = if ready && record.is_some() { published_index(&browser, &session, row) } else { None };
        let same_binding = match (&toggle.binding, index) {
            (Some(binding), Some(index)) => binding.matches(&session, row, index),
            (None, None) => true,
            _ => false,
        };
        if !same_binding {
            toggle.binding = index.map(|index| RowBinding {
                // 同一个场景重开也要拒收旧文档上的按压。
                document: session.placement_deletions.identity(),
                revision: session.revision, entries_revision: row.entries_revision, index,
            });
            if let Some(mut click) = click { click.invalidate(); } // 虚拟行换绑后旧手势不得转派给新对象。
        }
        let metadata = record.map(|record| effective_metadata(Some(&objects), &session, record));
        let available = index.is_some() && (toggle.kind != TreeRowToggle::Participation
            || placement_participation::available() && metadata.is_some_and(|metadata| !metadata.locked));
        if available == disabled {
            let mut entity = commands.entity(entity);
            if available { entity.remove::<InteractionDisabled>(); }
            else { entity.insert(InteractionDisabled); }
        }
        let active = match toggle.kind {
            TreeRowToggle::Participation => record.is_some_and(|record| effective_participation(Some(&objects), &session, record)),
            TreeRowToggle::Visibility => metadata.is_some_and(|metadata| !metadata.hidden),
            TreeRowToggle::Lock => metadata.is_some_and(|metadata| metadata.locked),
        };
        let (color, outline) = tree_toggle_colors(available, active, *interaction);
        fill.set_if_neq(color);
        border.set_if_neq(outline);
        if let Some(icon) = children.first() && let Ok(mut image) = icons.get_mut(*icon) {
            sync_tree_toggle_icon(toggle.kind, active, images, &mut image);
        }
    }
}
