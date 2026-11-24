//! 固定四十行的呈现与左键选择；右键仍由共用菜单冻结当前可见行身份。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::EditorButtonClick;
use super::{catalog::VegetationCatalogAssets, components::*, inventory::LayerInventory, resources::*, rows, types::LayerList};
use crate::{
    authoring::EditorAuthoringSession,
    resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState},
    ui::{
        gltf_browser::EditorGltfBrowserState,
        list_layout,
        scene_sidebar::{load_tree_branch_images, sync_tree_branch_icon, tree_branch_fill, tree_row_colors},
        scene_context_menu::{SceneContextMenuState, SceneObjectKey, SceneObjectRow, SceneObjectTarget},
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
                .filter(|target| !matches!(target.key, SceneObjectKey::Placement { .. }
                    | SceneObjectKey::Group { .. } | SceneObjectKey::MainSceneAnimation(_)
                    | SceneObjectKey::SourceGlbAnimation { .. })
                    && target.current(&editor, &context.session))
        {
            // 地形、子场景和作者层复用右键的稳定行身份。动画双击直接打开工作台，保留原对象选择。
            selection.0 = Some(target.clone());
            break;
        }
    }
}

fn selected(
    key: &SceneObjectKey,
    draft: &crate::authoring::EditorSceneDraft,
    candidates: Option<&LayerCandidateState>,
    domain: Option<&crate::game_authoring::scene_interaction::SceneInteractionState>,
) -> bool {
    let target = candidates.and_then(|state| state.selected.as_ref());
    let active_terrain = draft.terrain_documents.active.as_ref().map(|active| &active.definition.id);
    let current_canvas = candidates.is_some_and(|state| state.author_canvas_matches(draft));
    match key {
        SceneObjectKey::Water { kind, id, terrain, .. } => {
            current_canvas && terrain.as_ref() == active_terrain
                && matches!(target, Some(LayerTarget::Water(selected_kind, selected_id)) if kind == selected_kind && id == selected_id)
        }
        SceneObjectKey::Vegetation { id, terrain, .. } => {
            current_canvas && terrain.as_ref() == active_terrain
                && matches!(target, Some(LayerTarget::Vegetation(selected)) if selected == id)
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
    selection: (Option<Res<LayerCandidateState>>, Option<Res<EditorGltfBrowserState>>),
    domain: Option<Res<crate::game_authoring::scene_interaction::SceneInteractionState>>,
    assets: VegetationCatalogAssets,
    presentation: (Option<Res<LayerTreeFolds>>, Option<Res<LayerCategoryVisibility>>),
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
        (Without<Section>, Without<List>, Without<Content>, Without<SectionSelection>),
    >,
    mut headers: Query<(&SectionSelection, &UiInteraction, &mut SceneObjectRow, &mut EditorButtonClick,
        &mut BackgroundColor, &mut BorderColor), Without<Row>>,
    mut texts: Query<(&mut Text, Option<&Title>)>,
    labels: Query<&Children, With<RowLabel>>,
    mut previous: Local<[Option<(feathrise_scene_content::SceneId, usize)>; 4]>,
    mut inventory: Local<LayerInventory>,
) {
    let (candidates, browser) = selection;
    let (folds, visibility) = presentation;
    let draft = session.draft.as_ref().filter(|draft| {
        editor.open
            && editor.selected_scene_id.as_ref() == Some(&draft.scene_id)
            && !editor.scene_transition_in_progress
    });
    if let Some(draft) = draft { inventory.sync(&session, draft, Some(&assets)); }
    let collapsed = |list| draft.is_some_and(|draft|
        folds.as_ref().is_some_and(|folds| folds.collapsed(&draft.scene_id, list)));
    for (section, mut node) in &mut sections {
        let display = if draft.is_some_and(|draft| section.0.count(draft, &inventory, domain.as_deref()) > 0) {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    let locale = locale.as_deref().copied().unwrap_or_default();
    for (category, interaction, mut binding, mut click, mut background, mut border) in &mut headers {
        let category = category.0;
        let visible = draft.filter(|_| inventory.count(category == super::AuthorLayerCategory::Water) > 0);
        let scene = visible.map(|draft| &draft.scene_id);
        if binding.0.as_ref().map(|target| &target.scene_id) != scene {
            click.invalidate(); // 收起/重绑时不能用旧场景按下对应新行的名称。
            binding.0 = scene.map(|scene| SceneObjectTarget {
                scene_id: scene.clone(), revision: session.revision, key: SceneObjectKey::AuthorCategory(category),
                name: category.label().to_owned(), hidden: false, locked: false, copyable: false,
            });
        } else if let Some(target) = binding.0.as_mut() && target.revision != session.revision {
            target.revision = session.revision;
        }
        let selected = scene.and_then(|scene| browser.as_ref()?.selected_author_category(scene)) == Some(category);
        let (fill, outline) = tree_row_colors(selected, false, *interaction);
        background.set_if_neq(BackgroundColor(fill));
        border.set_if_neq(outline);
    }
    for (mut text, title) in &mut texts {
        if let Some(title) = title {
            let count = draft.map_or(0, |draft| title.0.count(draft, &inventory, domain.as_deref()));
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
        let count = draft.map_or(0, |draft| list.0.count(draft, &inventory, domain.as_deref()));
        if collapsed(list.0) {
            // 收起只退出布局，列表位置继续属于本场景和本分类。
            if viewport.display != Display::None { viewport.display = Display::None; }
            continue;
        }
        let identity = draft.map(|draft| (draft.scene_id.clone(), count));
        let reset = previous[ordinal].as_ref().map(|old| &old.0)
            != identity.as_ref().map(|current| &current.0);
        if previous[ordinal] != identity || scroll.is_changed() || count > 0 && viewport.display == Display::None {
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
        let item = draft.filter(|_| !collapsed(row.list))
            .and_then(|_| rows::object(&session, &inventory, Some(&assets), row.list, index, domain.as_deref(),
                (&editor.vegetation_visibility, visibility.as_deref())));
        let active = item.as_ref().is_some_and(|(target, _)| {
            draft.is_some_and(|draft| selected(&target.key, draft, candidates.as_deref(), domain.as_deref()))
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

/// 行池换绑时同时作废子按钮的未完成手势，避免滚动后作用到另一图层。
pub(super) fn sync_vegetation_toggles(
    mut commands: Commands,
    session: Res<EditorAuthoringSession>,
    assets: VegetationCatalogAssets,
    visibility: Option<Res<LayerCategoryVisibility>>,
    server: Option<Res<AssetServer>>,
    mut images: Local<Option<[Handle<Image>; 6]>>,
    rows: Query<Ref<SceneObjectRow>, With<Row>>,
    mut buttons: Query<(
        Entity, Option<&VegetationRowToggle>, Option<&StillWaterRowToggle>, &ChildOf, &Children, &UiInteraction,
        Option<&mut EditorButtonClick>, &mut Node, &mut BackgroundColor, &mut BorderColor,
        Has<bevy::ui::InteractionDisabled>,
    ), Or<(With<VegetationRowToggle>, With<StillWaterRowToggle>)>>,
    mut icons: Query<&mut ImageNode>,
) {
    use crate::ui::scene_sidebar::{TreeRowToggle, load_tree_toggle_images, sync_tree_toggle_icon, tree_toggle_colors};
    use bevy::ui::InteractionDisabled;
    if images.is_none() && let Some(server) = server.as_deref() {
        *images = Some(load_tree_toggle_images(server));
    }
    for (entity, vegetation, water, parent, children, interaction, click, mut node, mut background, mut border, disabled) in &mut buttons {
        let Ok(row) = rows.get(parent.parent()) else { continue };
        if row.is_changed() && let Some(mut click) = click { click.invalidate(); }
        let Some(toggle) = vegetation.map(|item| item.0).or_else(|| water.map(|item| item.0)) else { continue };
        let item = row.0.as_ref().and_then(|target| {
            let draft = session.draft.as_ref()?;
            if let Some((terrain, id)) = still_water_target(draft, &target.key) {
                use crate::authoring::layer_categories;
                let (enabled, locked) = layer_categories::water_layer_state(draft, terrain, id);
                let group_locked = layer_categories::category_state(draft, false).1;
                let visible = !visibility.as_deref().is_some_and(|visibility| visibility.water_hidden(&target.scene_id, terrain, id));
                let available = layer_categories::water_layer_supported(draft) && !session.water_preset_delete_pending
                    && (toggle == TreeRowToggle::Visibility || !group_locked)
                    && (toggle != TreeRowToggle::Participation || !locked);
                return Some((enabled, visible, locked, available));
            }
            let SceneObjectKey::Vegetation { index, id, output: false, .. } = &target.key else { return None };
            let authoring = assets.row_definition(draft, &target.key)?.authoring.as_ref()?;
            let layer = authoring.layers.get(*index).filter(|layer| layer.id == *id)?;
            let available = !session.model_vegetation_busy && !session.vegetation_cross_busy
                && (toggle != TreeRowToggle::Participation || !layer.locked);
            Some((layer.enabled, !target.hidden, layer.locked, available))
        });
        let display = if item.is_some() { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
        let available = !session.saving && item.is_some_and(|(_, _, _, available)| available);
        if available == disabled {
            if available { commands.entity(entity).remove::<InteractionDisabled>(); }
            else { commands.entity(entity).insert(InteractionDisabled); }
        }
        let active = item.is_some_and(|(enabled, visible, locked, _)| match toggle {
            TreeRowToggle::Participation => enabled,
            TreeRowToggle::Visibility => visible,
            TreeRowToggle::Lock => locked,
        });
        let (fill, outline) = tree_toggle_colors(available, active, *interaction);
        background.set_if_neq(fill);
        border.set_if_neq(outline);
        if let Some(images) = images.as_ref() && let Some(child) = children.first()
            && let Ok(mut icon) = icons.get_mut(*child)
        {
            sync_tree_toggle_icon(toggle, active, images, &mut icon);
        }
    }
}

fn still_water_target<'a>(draft: &crate::authoring::EditorSceneDraft, target: &'a SceneObjectKey)
    -> Option<(Option<&'a feathrise_scene_content::TerrainId>, &'a str)>
{
    let SceneObjectKey::Water { kind: crate::types::WaterAuthoringFeatureKind::StillWater, terrain, id, index } = target else { return None; };
    let terrain = terrain.as_ref();
    let water = crate::authoring::terrain_documents::root_water::listed_source(draft, terrain)?;
    water.layers.get(*index).filter(|layer| &layer.id == id)?;
    Some((terrain, id))
}

pub(super) fn toggle_still_water(
    mut contexts: ParamSet<(crate::ui::scene_context_menu::MenuContext, ResMut<EditorAuthoringSession>)>,
    mut editor: ResMut<SceneEditorState>,
    menu: Res<SceneContextMenuState>,
    guard: Res<SceneEditorNumericScrubMouseGuard>,
    rows: Query<&SceneObjectRow, With<Row>>,
    buttons: Query<(&EditorButtonClick, &StillWaterRowToggle, &ChildOf), (Changed<EditorButtonClick>, Without<bevy::ui::InteractionDisabled>)>,
    mut visibility: ResMut<LayerCategoryVisibility>,
) {
    use crate::authoring::{EditorCommand, layer_categories};
    use crate::ui::scene_sidebar::TreeRowToggle;
    if guard.blocked() || menu.owns_mouse() || !contexts.p0().enabled(&editor) { return; }
    let mut session = contexts.p1();
    if session.water_preset_delete_pending { return; }
    let Some((toggle, target)) = buttons.iter().filter(|(click, _, _)| click.is_clicked())
        .find_map(|(_, toggle, parent)| Some((toggle.0, rows.get(parent.parent()).ok()?.0.as_ref()?))) else { return; };
    if !target.current(&editor, &session) { return; }
    let Some(draft) = session.draft.as_mut().filter(|draft| layer_categories::water_layer_supported(draft)) else { return; };
    let Some((terrain, id)) = still_water_target(draft, &target.key) else { return; };
    let before = layer_categories::water_layer_state(draft, terrain, id);
    let after = match toggle {
        TreeRowToggle::Visibility => {
            let hidden = visibility.water_hidden(&target.scene_id, terrain, id);
            visibility.set_water_hidden(&target.scene_id, terrain, id, !hidden);
            return;
        }
        _ if layer_categories::category_state(draft, false).1 => return,
        TreeRowToggle::Participation if before.1 => return,
        TreeRowToggle::Participation => (!before.0, before.1),
        TreeRowToggle::Lock => (before.0, !before.1),
    };
    layer_categories::set_water_layer_state(draft, terrain, id, after);
    session.undo.push(EditorCommand::StillWaterLayerChanged { terrain: terrain.cloned(), layer_id: id.to_owned(), before, after });
    session.redo.clear();
    session.mark_changed(&mut editor);
}

pub(super) fn fold_sections(
    context: crate::ui::scene_context_menu::MenuContext,
    editor: Res<SceneEditorState>,
    menu: Res<SceneContextMenuState>,
    guard: Res<SceneEditorNumericScrubMouseGuard>,
    arrows: Query<(&SectionArrow, &EditorButtonClick), Changed<EditorButtonClick>>,
    mut folds: ResMut<LayerTreeFolds>,
) {
    if guard.blocked() || menu.owns_mouse() || !context.enabled(&editor) { return; }
    let Some(scene) = editor.selected_scene_id.as_ref().filter(|scene|
        context.session.draft.as_ref().is_some_and(|draft| &draft.scene_id == *scene)) else { return; };
    if let Some((arrow, _)) = arrows.iter().find(|(arrow, click)|
        click.is_clicked() && arrow.scene.as_ref() == Some(scene)) {
        folds.toggle(scene, arrow.list);
    }
}

pub(super) fn sync_section_arrows(
    editor: Res<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    folds: Res<LayerTreeFolds>,
    server: Option<Res<AssetServer>>,
    mut images: Local<Option<[Handle<Image>; 2]>>,
    mut arrows: Query<(&mut SectionArrow, &UiInteraction, &mut EditorButtonClick, &Children, &mut BackgroundColor)>,
    mut icons: Query<&mut ImageNode>,
) {
    let scene = session.draft.as_ref().filter(|draft| editor.open
        && !editor.scene_transition_in_progress && editor.selected_scene_id.as_ref() == Some(&draft.scene_id))
        .map(|draft| &draft.scene_id);
    if images.is_none() && let Some(server) = server.as_deref() {
        *images = Some(load_tree_branch_images(server));
    }
    for (mut arrow, interaction, mut click, children, mut background) in &mut arrows {
        if arrow.scene.as_ref() != scene {
            arrow.scene = scene.cloned();
            click.invalidate(); // 场景交接后的箭头不能采用上一场景还没松开的手势。
        }
        background.set_if_neq(tree_branch_fill(scene.is_some(), *interaction));
        let collapsed = scene.is_some_and(|scene| folds.collapsed(scene, arrow.list));
        if let Some(images) = &*images && let Some(child) = children.first()
            && let Ok(mut image) = icons.get_mut(*child)
        {
            sync_tree_branch_icon(collapsed, scene.is_some(), images, &mut image);
        }
    }
}

pub(super) fn toggle_categories(
    mut contexts: ParamSet<(crate::ui::scene_context_menu::MenuContext, ResMut<EditorAuthoringSession>)>,
    mut editor: ResMut<SceneEditorState>,
    menu: Res<SceneContextMenuState>,
    guard: Res<SceneEditorNumericScrubMouseGuard>,
    buttons: Query<(&SectionToggle, &EditorButtonClick), (Changed<EditorButtonClick>, Without<bevy::ui::InteractionDisabled>)>,
    mut visibility: ResMut<LayerCategoryVisibility>,
) {
    use crate::authoring::{EditorCommand, layer_categories};
    use crate::ui::scene_sidebar::TreeRowToggle;
    let Some(scene) = editor.selected_scene_id.as_ref() else { return; };
    let Some((button, _)) = buttons.iter().find(|(button, click)| click.is_clicked() && button.scene.as_ref() == Some(scene)) else { return; };
    if guard.blocked() || menu.owns_mouse() || !contexts.p0().enabled(&editor) { return; }
    let vegetation = button.list == LayerList::Vegetation;
    let mut session = contexts.p1();
    let Some(draft) = session.draft.as_mut().filter(|draft| &draft.scene_id == scene) else { return; };
    let before = layer_categories::category_state(draft, vegetation);
    let after = match button.toggle {
        TreeRowToggle::Visibility => { visibility.toggle(scene, vegetation); return; }
        TreeRowToggle::Participation if before.1 => return,
        TreeRowToggle::Participation => (!before.0, before.1),
        TreeRowToggle::Lock => (before.0, !before.1),
    };
    // 清单只修改分类自身，未加载的子项也受整组约束，成员状态与各份作者资产保持原样。
    if let Err(error) = layer_categories::set_state(draft, vegetation, after) {
        editor.status = error;
        return;
    }
    session.undo.push(EditorCommand::LayerCategoryChanged { vegetation, before, after });
    session.redo.clear();
    session.mark_changed(&mut editor);
}

pub(super) fn sync_category_toggles(
    mut commands: Commands,
    editor: Res<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    visibility: Res<LayerCategoryVisibility>,
    server: Res<AssetServer>,
    mut images: Local<Option<[Handle<Image>; 6]>>,
    mut buttons: Query<(Entity, &mut SectionToggle, &UiInteraction, &mut EditorButtonClick, &Children,
        &mut Node, Has<bevy::ui::InteractionDisabled>, &mut BackgroundColor, &mut BorderColor)>,
    mut icons: Query<&mut ImageNode>,
) {
    use crate::authoring::layer_categories;
    use crate::ui::scene_sidebar::{TreeRowToggle, load_tree_toggle_images, sync_tree_toggle_icon, tree_toggle_colors};
    let draft = session.draft.as_ref().filter(|draft| editor.open && !editor.scene_transition_in_progress
        && editor.selected_scene_id.as_ref() == Some(&draft.scene_id));
    let scene = draft.map(|draft| &draft.scene_id);
    let images = images.get_or_insert_with(|| load_tree_toggle_images(&server));
    let supports = draft.is_some_and(layer_categories::supported);
    for (entity, mut button, interaction, mut click, children, mut node, disabled, mut fill, mut border) in &mut buttons {
        if button.scene.as_ref() != scene {
            button.scene = scene.cloned();
            click.invalidate();
        }
        let display = if scene.is_some() { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
        let vegetation = button.list == LayerList::Vegetation;
        let (participates, locked) = draft.map_or((true, false), |draft| layer_categories::category_state(draft, vegetation));
        let (available, active) = match button.toggle {
            TreeRowToggle::Participation => (supports && !locked, participates),
            TreeRowToggle::Visibility => (supports, !scene.is_some_and(|scene| visibility.hidden(scene, vegetation))),
            TreeRowToggle::Lock => (supports, locked),
        };
        let available = available && !session.saving;
        if available == disabled {
            if available { commands.entity(entity).remove::<bevy::ui::InteractionDisabled>(); }
            else { commands.entity(entity).insert(bevy::ui::InteractionDisabled); }
        }
        let (color, outline) = tree_toggle_colors(available, active, *interaction);
        fill.set_if_neq(color);
        border.set_if_neq(outline);
        if let Some(icon) = children.first() && let Ok(mut image) = icons.get_mut(*icon) {
            sync_tree_toggle_icon(button.toggle, active, images, &mut image);
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
