//! 场景树摆放和逻辑组的箭头与行内开关，共用可见行的身份校验。

use super::{EditorGltfBrowserEntryKind, EditorGltfBrowserState, MainSceneGlbRow, SceneEditorGltfBrowserRow, systems::main_glb_draft};
use crate::{
    authoring::{EditorAuthoringSession, placement_deletion::PlacementDeletionIdentity, placement_participation},
    resources::SceneEditorState,
    ui::{
        scene_objects::{SceneObjectState, effective_metadata, effective_participation},
        scene_sidebar::{
            TreeRowToggle, load_tree_branch_images, load_tree_toggle_images, sync_tree_branch_icon,
            sync_tree_toggle_icon, tree_branch_fill, tree_toggle_colors,
        },
    },
};
use bevy::{prelude::*, ui::InteractionDisabled};
use feathrise_editor_ui::{EditorButtonClick, UiInteraction};
use feathrise_scene_content::ScenePlacementRecord;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum BranchTarget {
    MainGlb,
    Entry(usize),
}

struct RowBinding {
    document: PlacementDeletionIdentity,
    revision: u64,
    entries_revision: u64,
    target: BranchTarget,
}

impl RowBinding {
    fn matches(&self, session: &EditorAuthoringSession, row: &SceneEditorGltfBrowserRow, target: BranchTarget) -> bool {
        self.document.matches(&session.placement_deletions) && self.revision == session.revision
            && self.entries_revision == row.entries_revision && self.target == target
    }

    fn sync(
        binding: &mut Option<Self>, session: &EditorAuthoringSession, row: &SceneEditorGltfBrowserRow,
        target: Option<BranchTarget>, click: Option<Mut<'_, EditorButtonClick>>,
    ) {
        let unchanged = match (&*binding, target) {
            (Some(binding), Some(target)) => binding.matches(session, row, target),
            (None, None) => true,
            _ => false,
        };
        if unchanged { return; }
        *binding = target.map(|target| Self {
            document: session.placement_deletions.identity(),
            revision: session.revision, entries_revision: row.entries_revision, target,
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
    pub(super) fn target(
        &self, editor: &SceneEditorState, browser: &EditorGltfBrowserState,
        session: &EditorAuthoringSession, row: &SceneEditorGltfBrowserRow,
    ) -> Option<BranchTarget> {
        let target = if row.main_glb == Some(MainSceneGlbRow::File) {
            main_glb_draft(editor, session)?;
            BranchTarget::MainGlb
        } else {
            BranchTarget::Entry(published_index(browser, session, row)?)
        };
        self.0.as_ref()?.matches(session, row, target).then_some(target)
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
        if !self.binding.as_ref()?.matches(session, row, BranchTarget::Entry(index)) { return None; }
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
        EditorGltfBrowserEntryKind::Group { first_member: Some(index), .. } => (
            *index, browser.signature.get(*index)?.placement_id.as_str(),
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
        let show = matches!(kind, Some(EditorGltfBrowserEntryKind::Placement { .. } | EditorGltfBrowserEntryKind::Group { first_member: Some(_), .. }));
        let display = if show { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
        let record = kind.and_then(|kind| representative(&browser, &session, kind));
        let index = if ready && record.is_some() { published_index(&browser, &session, row) } else { None };
        RowBinding::sync(&mut toggle.binding, &session, row, index.map(BranchTarget::Entry), click);
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

    let expander_images = expander_images.get_or_insert_with(|| load_tree_branch_images(&server));
    let main_glb = main_glb_draft(&editor, &session);
    for (entity, parent, children, mut expander, mut visibility, click, interaction, disabled, mut fill) in &mut expanders {
        let Ok(row) = rows.get(parent.parent()) else { continue; };
        let entry = row.entry_index.and_then(|index| browser.entries.get(index));
        let (collapsed, target) = match (row.main_glb, entry.map(|entry| &entry.kind)) {
            (Some(MainSceneGlbRow::File), _) => (
                main_glb.map(|draft| browser.expanded_main_glb.as_ref() != Some(&draft.scene_id)),
                main_glb.map(|_| BranchTarget::MainGlb),
            ),
            (_, Some(EditorGltfBrowserEntryKind::Group { collapsed, first_member: Some(_), .. })) =>
                (Some(*collapsed), published_index(&browser, &session, row).map(BranchTarget::Entry)),
            (_, Some(EditorGltfBrowserEntryKind::Placement { collapsed, expandable: true, placement_id, .. })) => {
                let target = browser.placement_asset_paths.contains_key(placement_id)
                    .then(|| published_index(&browser, &session, row)).flatten().map(BranchTarget::Entry);
                (Some(*collapsed), target)
            }
            _ => (None, None),
        };
        // 叶子保留箭头所在列，只隐藏这个按钮，文字缩进不会在行池换绑时跳动。
        visibility.set_if_neq(if collapsed.is_some() { Visibility::Inherited } else { Visibility::Hidden });
        let target = target.filter(|_| ready);
        RowBinding::sync(&mut expander.0, &session, row, target, click);
        let available = target.is_some();
        if available == disabled {
            let mut entity = commands.entity(entity);
            if available { entity.remove::<InteractionDisabled>(); }
            else { entity.insert(InteractionDisabled); }
        }
        fill.set_if_neq(tree_branch_fill(available, *interaction));
        if let Some(collapsed) = collapsed && let Some(icon) = children.first()
            && let Ok(mut image) = icons.get_mut(*icon)
        {
            sync_tree_branch_icon(collapsed, available, expander_images, &mut image);
        }
    }
}
