//! 场景树摆放和逻辑组的箭头与行内开关，共用可见行的身份校验。

use super::{EditorGltfBrowserEntryKind, EditorGltfBrowserState, SceneEditorGltfBrowserRow};
use crate::{
    authoring::{EditorAuthoringSession, placement_deletion::PlacementDeletionIdentity, placement_participation},
    resources::SceneEditorState,
    ui::{
        scene_objects::{SceneObjectState, effective_metadata, effective_participation},
        scene_sidebar::{
            TreeRowToggle, load_tree_toggle_images, sync_tree_toggle_icon, tree_toggle_colors,
        },
        theme as colors,
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

    fn sync(
        binding: &mut Option<Self>, session: &EditorAuthoringSession, row: &SceneEditorGltfBrowserRow,
        index: Option<usize>, click: Option<Mut<'_, EditorButtonClick>>,
    ) {
        let unchanged = match (&*binding, index) {
            (Some(binding), Some(index)) => binding.matches(session, row, index),
            (None, None) => true,
            _ => false,
        };
        if unchanged { return; }
        *binding = index.map(|index| Self {
            document: session.placement_deletions.identity(),
            revision: session.revision, entries_revision: row.entries_revision, index,
        }); // 同场景重开或虚拟行换绑后，旧按压不得落在新对象。
        if let Some(mut click) = click { click.invalidate(); }
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

/// 独立于名称区的左侧分支按钮，虚拟行复用时沿用同一释放身份门。
#[derive(Component, Default)]
pub(crate) struct PlacementTreeExpander(Option<RowBinding>);

impl PlacementTreeExpander {
    pub(super) fn target_index(
        &self, browser: &EditorGltfBrowserState, session: &EditorAuthoringSession, row: &SceneEditorGltfBrowserRow,
    ) -> Option<usize> {
        let index = published_index(browser, session, row)?;
        self.0.as_ref()?.matches(session, row, index).then_some(index)
    }
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
    mut expander_images: Local<Option<[Handle<Image>; 2]>>,
    rows: Query<&SceneEditorGltfBrowserRow>,
    mut buttons: Query<(
        Entity, &ChildOf, &Children, &mut PlacementTreeToggle, &mut Node,
        Option<&mut EditorButtonClick>, &UiInteraction, Has<InteractionDisabled>,
        &mut BackgroundColor, &mut BorderColor,
    ), Without<PlacementTreeExpander>>,
    mut expanders: Query<(
        Entity, &ChildOf, &Children, &mut PlacementTreeExpander, &mut Visibility,
        Option<&mut EditorButtonClick>, &UiInteraction, Has<InteractionDisabled>, &mut BackgroundColor,
    ), Without<PlacementTreeToggle>>,
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
        RowBinding::sync(&mut toggle.binding, &session, row, index, click);
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

    let expander_images = expander_images.get_or_insert_with(|| [
        server.load("icons/editor/tree_expand.png"), server.load("icons/editor/tree_collapse.png"),
    ]);
    for (entity, parent, children, mut expander, mut visibility, click, interaction, disabled, mut fill) in &mut expanders {
        let Ok(row) = rows.get(parent.parent()) else { continue; };
        let entry = row.entry_index.and_then(|index| browser.entries.get(index));
        let (collapsed, valid) = match entry.map(|entry| &entry.kind) {
            Some(EditorGltfBrowserEntryKind::Group { collapsed, .. }) => (Some(*collapsed), true),
            Some(EditorGltfBrowserEntryKind::Placement { collapsed, expandable: true, placement_id }) => {
                (Some(*collapsed), browser.placement_asset_paths.contains_key(placement_id))
            }
            _ => (None, false),
        };
        // 叶子保留箭头所在列，只隐藏这个按钮，文字缩进不会在行池换绑时跳动。
        visibility.set_if_neq(if collapsed.is_some() { Visibility::Inherited } else { Visibility::Hidden });
        let index = if ready && valid { published_index(&browser, &session, row) } else { None };
        RowBinding::sync(&mut expander.0, &session, row, index, click);
        let available = index.is_some();
        if available == disabled {
            let mut entity = commands.entity(entity);
            if available { entity.remove::<InteractionDisabled>(); }
            else { entity.insert(InteractionDisabled); }
        }
        fill.set_if_neq(BackgroundColor(match (available, *interaction) {
            (true, UiInteraction::Pressed) => colors::SELECT_BG_PRESSED,
            (true, UiInteraction::Hovered) => colors::SELECT_BG_HOVER,
            _ => Color::NONE,
        }));
        if let Some(collapsed) = collapsed && let Some(icon) = children.first()
            && let Ok(mut image) = icons.get_mut(*icon)
        {
            let handle = &expander_images[usize::from(!collapsed)];
            if &image.image != handle { image.image = handle.clone(); }
            let color = if available { colors::TEXT_PRIMARY } else { colors::TEXT_MUTED };
            if image.color != color { image.color = color; }
        }
    }
}
