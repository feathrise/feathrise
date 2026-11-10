//! 连续虚拟列表回显、引用增删、后台循环校验与源场景文档入口；不复制任何内部资产。

use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::EditorButtonClick;
use bevy::{prelude::*, tasks::{IoTaskPool, futures_lite::future}};
use feathrise_scene_content::{SceneCatalogAsset, SceneInstanceAuthorPreview, SceneInstanceId, SceneInstancePlacement,
    SceneInstanceTransform, SceneManifestAsset, SCENE_CATALOG_ASSET_PATH, validate_saved_scene_instances};
use feathrise_editor_ui::{ActiveNumericInput, ActiveNumericInputScrub, NumericInputDisabled,
    NumericInputSetValue, NumericInputValue, NumericValue};
use std::sync::Arc;
use super::{components::*, resources::*};
use crate::{authoring::{EditorAuthoringSession, EditorCommand, GlobalEnvironmentAuthoringState},
    resources::{SceneBrowserState, SceneDeletionTask, SceneEditorState}};

pub(super) fn enabled(editor: &SceneEditorState, session: &EditorAuthoringSession, state: &SubsceneEditorState) -> bool {
    editor.open && !session.saving && !editor.save_requested && !editor.close_requested
        && !editor.new_scene_dialog_open && !editor.scene_transition_in_progress
        && editor.pending_scene_transition.is_none() && editor.scene_switch_confirmation.is_none()
        && state.pending.is_none() && state.rename.is_none() && session.draft.is_some()
}

pub(crate) fn apply(session: &mut EditorAuthoringSession, editor: &mut SceneEditorState,
    after: Arc<[SceneInstancePlacement]>) {
    let draft = session.draft.as_mut().unwrap();
    let before = draft.manifest.subscenes.clone();
    if before == after { return; }
    draft.manifest.subscenes = after.clone();
    // 整体引用同样进入原线性历史，撤销不触碰源场景自己的作者值。
    session.undo.push(EditorCommand::SubscenesChanged { before, after });
    session.redo.clear();
    session.mark_changed(editor);
}

fn validate_add(manifest: SceneManifestAsset) -> Result<(), String> {
    let root = feathrise_editor_core::workspace_asset_root();
    let _lock = feathrise_scene_content::ContentAuthorLock::acquire(&root)?; // 与源图保存和删除共享正式作者锁。
    let bytes = std::fs::read(root.join(SCENE_CATALOG_ASSET_PATH)).map_err(|error| error.to_string())?;
    let catalog: SceneCatalogAsset = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    validate_saved_scene_instances(&root, &catalog, &manifest).map(|_| ())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn input(mut editor: ResMut<SceneEditorState>, mut session: ResMut<EditorAuthoringSession>,
    mut state: ResMut<SubsceneEditorState>,
    registry: Option<Res<feathrise_scene_content::SceneContentRegistry>>, catalogs: Res<Assets<SceneCatalogAsset>>, deletion: Res<SceneDeletionTask>,
    global: Res<GlobalEnvironmentAuthoringState>,
    modes: Res<crate::viewport::modes::ViewportModeState>,
    characters: Res<crate::game_authoring::scene_interaction::SceneInteractionState>,
    mut context: ParamSet<(crate::viewport::placement_move::MoveInputContext,
        (ResMut<crate::viewport::LayerCandidateState>, ResMut<SceneBrowserState>))>,
    menu: Option<Res<crate::ui::scene_context_menu::SceneContextMenuState>>,
    sidebars: Option<Res<crate::ui::sidebars::SceneEditorSidebarState>>,
    layout: Option<Res<crate::resources::SceneEditorLayoutState>>,
    rows: Query<(&EditorButtonClick, &Row), Changed<EditorButtonClick>>,
    buttons: Query<(&EditorButtonClick, &ActionButton), (With<InteractionButton>, Changed<EditorButtonClick>)>) {
    if !enabled(&editor, &session, &state) || state.scrub.is_some() || !context.p0().scene_commands_available(&editor) { return; }
    let (mut candidates, mut browser) = context.p1();
    let tree_visible = sidebars.as_ref().is_none_or(|state| state.scene_tree_visible())
        && layout.as_ref().is_none_or(|state| state.content_shown());
    let properties_visible = sidebars.as_ref().is_none_or(|state| state.right == crate::ui::sidebars::EditorRightSidebarTab::Scene)
        && layout.as_ref().is_none_or(|state| state.inspector_shown());
    let creation_visible = sidebars.as_ref().is_none_or(|state| state.tool_page_visible(crate::ui::sidebars::EditorLeftSidebarTab::Gameplay))
        && layout.as_ref().is_none_or(|state| state.inspector_shown());
    let catalog = registry.as_deref().and_then(|registry| catalogs.get(&registry.catalog));
    for (interaction, row) in &rows {
        if tree_visible && interaction.is_clicked() && modes.mode == crate::viewport::modes::ViewportMode::Select {
            if let Some(record) = session.draft.as_ref().and_then(|draft| draft.manifest.subscenes.get(state.list_start + row.0)) {
                state.scene_id = Some(session.draft.as_ref().unwrap().scene_id.clone());
                state.selected = Some(record.id.clone()); candidates.confirm_subscene(&record.id,
                    &session, characters.observed_generation, modes.selection_filter(&editor));
            }
        }
    }
    let menu_action = menu.as_ref().and_then(|menu| menu.ready.as_ref()).and_then(|request| {
        use crate::ui::scene_context_menu::{SceneMenuAction, SceneObjectKey};
        let SceneObjectKey::Subscene { index, .. } = &request.target.key else { return None; };
        if !request.target.current(&editor, &session) || !request.target.allows(request.action) { return None; }
        let action = match request.action { SceneMenuAction::Copy => Action::Duplicate,
            SceneMenuAction::Delete => Action::Remove, _ => return None };
        Some((action, Some(*index)))
    }); // 原引用增删流程接收精确下标，不能借旧属性选择改派右键动作。
    let actions = buttons.iter().filter_map(|(interaction, action)|
        (interaction.is_clicked()).then_some((action.0, None))).chain(menu_action);
    for (action, menu_index) in actions {
        // 创建按钮迁到玩法页；隐藏的旧按钮不能借场景树选择继续提交作者事务。
        if menu_index.is_none() && !(if matches!(action, Action::Add) { creation_visible } else { properties_visible }) { continue; }
        let draft = session.draft.as_ref().unwrap();
        let index = menu_index.or_else(|| state.selected.as_ref().and_then(|id| draft.manifest.subscenes.iter().position(|record| &record.id == id)));
        match action {
            Action::OpenSource => {
                let Some(index) = index else { continue; };
                let source = draft.manifest.subscenes[index].source_scene_id.clone();
                editor.scene_list_selection_id = Some(source.clone());
                browser.select_only(Some(crate::types::SceneBrowserItem::Scene(source)));
                crate::ui::rows::request_selected_scene_entry(&mut editor, &deletion,
                    catalog, &session, &global, true);
            }
            Action::Remove => {
                let Some(index) = index else { continue; };
                let mut references = draft.manifest.subscenes.to_vec(); references.remove(index);
                apply(&mut session, &mut editor, references.into()); state.selected = None; candidates.clear_for_scene_tree();
            }
            Action::Add | Action::Duplicate => {
                let mut references = draft.manifest.subscenes.to_vec();
                let mut manifest = draft.manifest.clone(); let scene_id = draft.scene_id.clone();
                let mut record = if matches!(action, Action::Duplicate) {
                    let Some(index) = index else { continue; }; references[index].clone()
                } else {
                    let Some(source) = editor.scene_list_selection_id.clone() else {
                        editor.status = "请先在场景目录里选择源场景".into(); continue;
                    };
                    let label = catalog.and_then(|catalog| catalog.scenes.iter()
                        .find(|entry| entry.id == source)).map(|entry| entry.display_name.clone()).unwrap_or_else(|| source.to_string());
                    SceneInstancePlacement { id: SceneInstanceId("pending".into()), source_scene_id: source,
                        label, transform: SceneInstanceTransform::default() }
                };
                // 本地唯一身份与源场景ID分离，同一源可拥有任意合法的不同引用。
                loop {
                    session.next_serial = session.next_serial.wrapping_add(1);
                    record.id = SceneInstanceId(format!("sub-{}", session.next_serial));
                    if references.iter().all(|other| other.id != record.id) { break; }
                }
                if matches!(action, Action::Duplicate) { record.transform.translation[0] += 2.0; }
                let selected = record.id.clone(); references.push(record);
                let references: Arc<[SceneInstancePlacement]> = references.into();
                manifest.subscenes = references.clone();
                state.pending = Some(Pending { scene_id, revision: session.revision, selected, references,
                    task: IoTaskPool::get().spawn(async move { validate_add(manifest) }) });
                editor.status = "正在检查完整子场景引用链".into();
            }
        }
    }
}

pub(super) fn poll(mut editor: ResMut<SceneEditorState>, mut session: ResMut<EditorAuthoringSession>, mut state: ResMut<SubsceneEditorState>, modes: Res<crate::viewport::modes::ViewportModeState>,
    deletion: Res<SceneDeletionTask>,
    characters: Res<crate::game_authoring::scene_interaction::SceneInteractionState>,
    mut candidates: ResMut<crate::viewport::LayerCandidateState>) {
    let Some(pending) = state.pending.as_mut() else { return; };
    let Some(result) = future::block_on(future::poll_once(&mut pending.task)) else { return; };
    let pending = state.pending.take().unwrap();
    if deletion.busy() || session.saving || editor.save_requested || editor.close_requested || session.revision != pending.revision || session.draft.as_ref().is_none_or(|draft| draft.scene_id != pending.scene_id) {
        editor.status = "引用校验期间父场景已变更，请重新添加".into(); return;
    }
    match result {
        Ok(()) => { apply(&mut session, &mut editor, pending.references); if modes.mode == crate::viewport::modes::ViewportMode::Select { candidates.confirm_subscene(&pending.selected,
            &session, characters.observed_generation, modes.selection_filter(&editor)); state.scene_id = Some(pending.scene_id); state.selected = Some(pending.selected); }
            editor.status = "已添加完整子场景；内部内容通过源场景编辑".into(); }
        Err(error) => { editor.status = error; }
    }
}

pub(super) fn publish(session: Res<EditorAuthoringSession>, mut preview: ResMut<SceneInstanceAuthorPreview>,
    transform: Option<Res<crate::viewport::subscenes::SubsceneTransformState>>) {
    if session.draft.is_none() && preview.scene_id.is_none() && preview.instances.is_empty() { return; }
    let (scene_id, references) = session.draft.as_ref().map_or((None, Arc::from([])),
        |draft| (Some(draft.scene_id.clone()), transform.as_ref().and_then(|state| state.preview(&session)).unwrap_or(&draft.manifest.subscenes).clone()));
    let terrains = session.draft.as_ref().and_then(|draft| draft.manifest.terrains.clone());
    let same_terrains = match (&preview.terrains, &terrains) { (None, None) => true,
        (Some(before), Some(after)) => Arc::ptr_eq(before, after), _ => false };
    if preview.scene_id != scene_id || !Arc::ptr_eq(&preview.instances, &references) || !same_terrains {
        preview.scene_id = scene_id; preview.instances = references; preview.terrains = terrains; preview.revision = preview.revision.wrapping_add(1);
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync(mut commands: Commands, editor: Res<SceneEditorState>, session: Res<EditorAuthoringSession>,
    mut state: ResMut<SubsceneEditorState>, active: Res<ActiveNumericInput>, scrub: Res<ActiveNumericInputScrub>,
    mut values: MessageWriter<NumericInputSetValue>,
    runtime: Option<Res<feathrise_scene::SubsceneRuntimeStatus>>,
    transform: Option<Res<crate::viewport::subscenes::SubsceneTransformState>>,
    mut summaries: Query<&mut Text, (With<Summary>, Without<RowText>)>,
    mut row_texts: Query<(&RowText, &mut Text), Without<Summary>>,
    mut rows: Query<(&Row, &mut Node, Option<&mut EditorButtonClick>), (Without<ListViewport>, Without<ListContent>)>,
    fields: Query<(Entity, &Field, &NumericInputValue, Has<NumericInputDisabled>)>,
    mut lists: (Query<(&mut Node, &mut ScrollPosition), (With<ListViewport>, Without<ListContent>)>,
        Query<&mut Node, (With<ListContent>, Without<ListViewport>)>),
    mut branches: Query<&mut Node, (With<SceneBranch>, Without<Row>, Without<ListViewport>, Without<ListContent>)>,
    identities: (Local<Option<(Option<feathrise_scene_content::SceneId>, u64, usize)>>,
        Local<Option<(Option<feathrise_scene_content::SceneId>, Option<SceneInstanceId>, usize)>>)) {
    let (mut presented_scope, mut list_identity) = identities;
    let scene_id = session.draft.as_ref().map(|draft| draft.scene_id.clone());
    if state.scene_id != scene_id { state.scene_id = scene_id; state.selected = None; state.list_start = 0; }
    let references = transform.as_ref().and_then(|state| state.preview(&session)).map(|value| value.as_ref()).or_else(|| session.draft.as_ref().map(|draft| draft.manifest.subscenes.as_ref())).unwrap_or(&[]);
    let identity = (state.scene_id.clone(), state.selected.clone(), references.len());
    let reset = list_identity.as_ref().is_none_or(|old| old.0 != identity.0);
    // 只有正式选择变化才定位；滚动查看其他行不会被旧选中项拉回。
    let reveal = (list_identity.as_ref() != Some(&identity)).then(|| state.selected.as_ref()
        .and_then(|id| references.iter().position(|record| &record.id == id))).flatten();
    if let (Ok((mut viewport, mut scroll)), Ok(mut content)) = (lists.0.single_mut(), lists.1.single_mut()) {
        if list_identity.as_ref() != Some(&identity) || scroll.is_changed() {
            // 稳定帧不重写布局，滚动与结构变化只更新固定可见窗口。
            let start = crate::ui::list_layout::fit_scene_object_list(
                references.len(), reset, reveal, &mut viewport, &mut content, &mut scroll);
            if state.list_start != start { state.list_start = start; }
            *list_identity = Some(identity);
        }
    }
    let selected = state.selected.as_ref().and_then(|id| references.iter().find(|record| &record.id == id));
    // 热更新失败也须在工作台可见；初次进图加载页之外不能只把错误留在后台资源。
    let loading = if state.pending.is_some() { " · 检查引用中".to_owned() }
        else if let Some(runtime) = runtime.as_deref().filter(|runtime| runtime.root_scene_id == state.scene_id) {
            if let Some(error) = &runtime.last_error { format!(" · {error}") }
            else if !references.is_empty() && (!runtime.ready || runtime.prepared < runtime.total || runtime.total < references.len()) {
                format!(" · 加载 {}/{}{}", runtime.prepared, runtime.total.max(references.len()),
                    runtime.loading_detail.as_deref().map(|detail| format!(" · {detail}")).unwrap_or_default())
            } else { String::new() }
        } else { String::new() };
    for mut node in &mut branches {
        let display = if !references.is_empty() || !loading.is_empty() { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
    }
    for mut text in &mut summaries {
        let value = format!("{} 个子场景{}", references.len(), loading); if text.0 != value { text.0 = value; }
    }
    let scope = (state.scene_id.clone(), session.revision, state.list_start);
    let scope_changed = presented_scope.as_ref() != Some(&scope);
    *presented_scope = Some(scope);
    for (row, mut node, click) in &mut rows {
        if scope_changed && let Some(mut click) = click { click.invalidate(); }
        let top = px((state.list_start + row.0) as f32 * crate::ui::list_layout::SCENE_OBJECT_ROW_PITCH);
        if node.top != top { node.top = top; }
        let display = if references.get(state.list_start + row.0).is_some() { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
    }
    for (row, mut text) in &mut row_texts {
        let value = references.get(state.list_start + row.0).map(|record| record.label.clone()).unwrap_or_default(); // 图标说明类型，完整源图身份保留在所选属性中。
        if text.0 != value { text.0 = value; }
    }
    let usable = enabled(&editor, &session, &state) && selected.is_some();
    for (entity, field, current, disabled) in &fields {
        if usable && disabled { commands.entity(entity).remove::<NumericInputDisabled>(); }
        if !usable && !disabled { commands.entity(entity).insert(NumericInputDisabled); }
        if active.entity == Some(entity) || scrub.entity == Some(entity) { continue; }
        let value = selected.map_or(0.0, |record| if field.0 < 3 { record.transform.translation[field.0] } else { record.transform.yaw_degrees });
        if current.0.as_f32() != Some(value) { values.write(NumericInputSetValue { entity, value: NumericValue::F32(value), cancel_edit: true }); }
    }
}

#[cfg(test)]
mod continuous_list_tests {
    use super::*;

    #[test]
    fn subscene_list_reuses_rows_for_later_references_and_clamps_after_removal() {
        let mut app = App::new();
        let mut draft = crate::authoring::test_heightfield_draft(vec![0; 121].into());
        draft.manifest.subscenes = (0..40).map(|index| SceneInstancePlacement {
            id: SceneInstanceId::parse(format!("instance-{index}")).unwrap(),
            source_scene_id: feathrise_scene_content::SceneId::parse("source").unwrap(),
            label: format!("子场景{index}"), transform: default(),
        }).collect::<Vec<_>>().into();
        app.insert_resource(EditorAuthoringSession { draft: Some(draft), ..default() })
            .insert_resource(SceneEditorState { open: true, ..default() })
            .init_resource::<SubsceneEditorState>()
            .init_resource::<ActiveNumericInput>()
            .init_resource::<ActiveNumericInputScrub>()
            .add_message::<NumericInputSetValue>()
            .add_systems(Update, sync);
        let viewport = app.world_mut().spawn((ListViewport, Node::default(), ScrollPosition::default())).id();
        app.world_mut().spawn((ListContent, Node::default()));
        let summary = app.world_mut().spawn((Summary, Text::default())).id();
        for slot in 0..crate::ui::list_layout::SCENE_OBJECT_LIST_ROWS {
            app.world_mut().spawn((Row(slot), Node::default()));
            app.world_mut().spawn((RowText(slot), Text::default()));
        }
        app.update();
        app.world_mut().get_mut::<ScrollPosition>(viewport).unwrap().y = 27.0 * 24.0;
        app.update();
        assert_eq!(app.world().resource::<SubsceneEditorState>().list_start, 23);
        assert!(!app.world().get::<Text>(summary).unwrap().contains("页"));
        let world = app.world_mut();
        assert!(world.query::<(&RowText, &Text)>().iter(world)
            .any(|(row, text)| row.0 == 2 && text.contains("子场景25")));
        // 视口选中最后一个引用会定位；普通滚动不改变引用或产生作者历史。
        world.resource_mut::<SubsceneEditorState>().selected = Some(SceneInstanceId::parse("instance-39").unwrap());
        app.update();
        let start = app.world().resource::<SubsceneEditorState>().list_start;
        assert!(start <= 39 && start + crate::ui::list_layout::SCENE_OBJECT_LIST_ROWS > 39);
        app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap().manifest.subscenes = Arc::from([]);
        app.update();
        assert_eq!(app.world().get::<ScrollPosition>(viewport).unwrap().y, 0.0);
        assert_eq!(app.world().get::<Node>(viewport).unwrap().display, Display::None);
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    }
}
