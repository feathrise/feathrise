//! 固定四十行的呈现与左键选择；右键仍由共用菜单冻结当前可见行身份。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::EditorButtonClick;
use super::{components::*, resources::*, rows, types::LayerList};
use crate::{
    authoring::EditorAuthoringSession,
    resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState},
    ui::{
        list_layout,
        scene_sidebar::tree_row_colors,
        scene_context_menu::{SceneContextMenuState, SceneObjectKey, SceneObjectRow},
    },
    viewport::{LayerCandidateState, LayerTarget},
};
use bevy::prelude::*;

pub(in crate::ui) fn input(
    context: crate::ui::scene_context_menu::MenuContext,
    editor: Res<SceneEditorState>,
    menu: Res<SceneContextMenuState>,
    guard: Res<SceneEditorNumericScrubMouseGuard>,
    rows: Query<(&EditorButtonClick, &SceneObjectRow), Changed<EditorButtonClick>>,
    mut selection: ResMut<SceneLayerSelection>,
) {
    selection.0 = None; // 只允许当帧选择，不把隐藏、切换场景前的点击积压到后续会话。
    if guard.blocked()
        || menu.owns_mouse()
        || !context.enabled(&editor)
    {
        return;
    }
    for (interaction, row) in &rows {
        if interaction.is_clicked()
            && let Some(target) = row
                .0
                .as_ref()
                .filter(|target| !matches!(target.key, SceneObjectKey::Placement { .. })
                    && target.current(&editor, &context.session))
        {
            // 地形、子场景和作者层复用右键的稳定行身份。摆放行由 GLB 树处理展开及增减选择。
            selection.0 = Some(target.clone());
            break;
        }
    }
}

fn selected(
    key: &SceneObjectKey,
    active_terrain: Option<&feathrise_scene_content::TerrainId>,
    candidates: Option<&LayerCandidateState>,
    domain: Option<&crate::game_authoring::scene_interaction::SceneInteractionState>,
) -> bool {
    let target = candidates.and_then(|state| state.selected.as_ref());
    match key {
        SceneObjectKey::Water { kind, id, terrain, .. } => {
            terrain.as_ref() == active_terrain && matches!(target, Some(LayerTarget::Water(selected_kind, selected_id)) if kind == selected_kind && id == selected_id)
        }
        SceneObjectKey::Vegetation { id, .. } => {
            matches!(target, Some(LayerTarget::Vegetation(selected)) if selected == id)
        }
        SceneObjectKey::Domain { kind, id, .. } => {
            match kind {
                crate::game_authoring::scene_interaction::DomainItemKind::Layer => matches!(target, Some(LayerTarget::Domain(selected)) if selected == id),
                crate::game_authoring::scene_interaction::DomainItemKind::Object => matches!(target, Some(LayerTarget::Domain(_))) && domain.is_some_and(|state| state.selected.as_ref() == Some(id)),
            }
        }
        _ => false,
    }
}

pub(super) fn sync(
    editor: Res<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    locale: Option<Res<feathrise_editor_ui::localization::EditorLocale>>,
    candidates: Option<Res<LayerCandidateState>>,
    domain: Option<Res<crate::game_authoring::scene_interaction::SceneInteractionState>>,
    mut sections: Query<(&Section, &mut Node), (Without<List>, Without<Content>, Without<Row>)>,
    mut lists: Query<
        (&List, &Children, &mut Node, &mut ScrollPosition),
        (Without<Section>, Without<Content>, Without<Row>),
    >,
    mut contents: Query<&mut Node, (With<Content>, Without<Section>, Without<List>, Without<Row>)>,
    mut rows: Query<
        (
            &Row,
            &UiInteraction,
            &mut SceneObjectRow,
            Option<&mut EditorButtonClick>,
            &mut Node,
            &mut BackgroundColor,
            &mut BorderColor,
            &Children,
        ),
        (Without<Section>, Without<List>, Without<Content>),
    >,
    mut texts: Query<(&mut Text, Option<&Title>)>,
    labels: Query<&Children, With<RowLabel>>,
    mut previous: Local<[Option<(feathrise_scene_content::SceneId, usize)>; 4]>,
) {
    let draft = session.draft.as_ref().filter(|draft| {
        editor.open
            && editor.selected_scene_id.as_ref() == Some(&draft.scene_id)
            && !editor.scene_transition_in_progress
    });
    for (section, mut node) in &mut sections {
        let display = if draft.is_some_and(|draft| section.0.count(draft, domain.as_deref()) > 0) {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    let locale = locale.as_deref().copied().unwrap_or_default();
    for (mut text, title) in &mut texts {
        if let Some(title) = title {
            let count = draft.map_or(0, |draft| title.0.count(draft, domain.as_deref()));
            let label = title.0.label(domain.as_deref());
            let label = match title.0 {
                LayerList::Water | LayerList::Vegetation => locale.editor_label(label),
                _ => label, // 插件定义的场景类别按原文使用。
            };
            text.set_if_neq(Text::new(locale.format("scene.layers.count",
                &[("label", label.into()), ("count", count.to_string())])));
        }
    }
    let mut starts = [0; 4];
    for (list, children, mut viewport, mut scroll) in &mut lists {
        let ordinal = list.0 as usize;
        let count = draft.map_or(0, |draft| list.0.count(draft, domain.as_deref()));
        let identity = draft.map(|draft| (draft.scene_id.clone(), count));
        let reset = previous[ordinal].as_ref().map(|old| &old.0)
            != identity.as_ref().map(|current| &current.0);
        if previous[ordinal] != identity || scroll.is_changed() {
            if let Some(child) = children.first()
                && let Ok(mut content) = contents.get_mut(*child)
            {
                list_layout::fit_scene_object_list(
                    count,
                    reset,
                    None,
                    &mut viewport,
                    &mut content,
                    &mut scroll,
                );
            }
            previous[ordinal] = identity;
        }
        starts[ordinal] =
            ((scroll.y / list_layout::SCENE_OBJECT_ROW_PITCH).floor() as usize).saturating_sub(1);
    }
    for (row, interaction, mut binding, click, mut node, mut background, mut border, children) in &mut rows
    {
        let index = starts[row.list as usize] + row.slot;
        let item = draft.and_then(|_| rows::object(&session, row.list, index, domain.as_deref(), &editor.vegetation_visibility));
        let active = item.as_ref().is_some_and(|(target, _)| {
            selected(&target.key, draft.and_then(|draft| draft.terrain_documents.active.as_ref()).map(|active| &active.definition.id), candidates.as_deref(), domain.as_deref())
        });
        let (fill, outline) = tree_row_colors(active, false, *interaction);
        background.set_if_neq(BackgroundColor(fill));
        border.set_if_neq(outline);
        let display = if item.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        let top = px(index as f32 * list_layout::SCENE_OBJECT_ROW_PITCH);
        if node.top != top {
            node.top = top;
        }
        let (target, label) = item.map_or((None, String::new()), |(target, label)| {
            (Some(target), label)
        });
        if binding.0 != target {
            binding.0 = target;
            if let Some(mut click) = click { click.invalidate(); }
        }
        if let Some(text_entity) = children.iter().find_map(|child| labels.get(child).ok()?.first())
            && let Ok((mut text, _)) = texts.get_mut(*text_entity)
        {
            text.set_if_neq(Text::new(label));
        }
    }
}

/// 行池换绑时同时作废子按钮的未完成手势，避免滚动后作用到另一植被层。
pub(super) fn sync_vegetation_toggles(
    mut commands: Commands,
    session: Res<EditorAuthoringSession>,
    server: Option<Res<AssetServer>>,
    mut images: Local<Option<[Handle<Image>; 6]>>,
    rows: Query<Ref<SceneObjectRow>, With<Row>>,
    mut buttons: Query<(
        Entity, &VegetationRowToggle, &ChildOf, &Children, &UiInteraction,
        Option<&mut EditorButtonClick>, &mut Node, &mut BackgroundColor, &mut BorderColor,
        Has<bevy::ui::InteractionDisabled>,
    )>,
    mut icons: Query<&mut ImageNode>,
) {
    use crate::ui::scene_sidebar::{TreeRowToggle, load_tree_toggle_images, sync_tree_toggle_icon, tree_toggle_colors};
    use bevy::ui::InteractionDisabled;
    if images.is_none() && let Some(server) = server.as_deref() {
        *images = Some(load_tree_toggle_images(server));
    }
    let authoring = session.draft.as_ref()
        .and_then(|draft| draft.vegetation.as_ref())
        .and_then(|vegetation| vegetation.authoring.as_ref());
    for (entity, toggle, parent, children, interaction, click, mut node, mut background, mut border, disabled) in &mut buttons {
        let Ok(row) = rows.get(parent.parent()) else { continue };
        if row.is_changed() && let Some(mut click) = click { click.invalidate(); }
        let item = row.0.as_ref().and_then(|target| {
            let SceneObjectKey::Vegetation { index, id, output: false } = &target.key else { return None };
            let layer = authoring?.layers.get(*index).filter(|layer| layer.id == *id)?;
            Some((layer, target))
        });
        let display = if item.is_some() { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
        let available = !session.saving && !session.model_vegetation_busy && !session.vegetation_cross_busy
            && item.is_some_and(|(layer, _)| toggle.0 != TreeRowToggle::Participation || !layer.locked);
        if available == disabled {
            if available { commands.entity(entity).remove::<InteractionDisabled>(); }
            else { commands.entity(entity).insert(InteractionDisabled); }
        }
        let active = item.is_some_and(|(layer, target)| match toggle.0 {
            TreeRowToggle::Participation => layer.enabled,
            TreeRowToggle::Visibility => !target.hidden,
            TreeRowToggle::Lock => layer.locked,
        });
        let (fill, outline) = tree_toggle_colors(available, active, *interaction);
        background.set_if_neq(fill);
        border.set_if_neq(outline);
        if let Some(images) = images.as_ref() && let Some(child) = children.first()
            && let Ok(mut icon) = icons.get_mut(*child)
        {
            sync_tree_toggle_icon(toggle.0, active, images, &mut icon);
        }
    }
}

/// 角色后台采用以后、旧候选失效检查以前更新正式选择，表现阶段只读取结果。
pub(super) fn refresh_selection(
    editor: Res<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    mut candidates: Option<ResMut<LayerCandidateState>>,
    domain: Option<Res<crate::game_authoring::scene_interaction::SceneInteractionState>>,
    modes: Option<Res<crate::viewport::modes::ViewportModeState>>,
) {
    if editor.open && let Some(candidates) = candidates.as_mut()
    {
        let filter = modes
            .as_ref()
            .map_or_else(Default::default, |state| state.selection_filter(&editor));
        candidates.rebind_author_revision(&editor, &session, domain.as_deref(), filter);
    }
}
