//! GLB浏览器可见行池、滚动范围、展开选择与样式同步。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::EditorButtonClick;
use bevy::prelude::*; // ECS系统参数、UI组件和字体。
use crate::ui::theme as colors; // 固定行池复用全屏编辑器水墨铜边配色。

use crate::authoring::{EditorAuthoringSession, EditorSceneDraft};
use crate::resources::{
    EditorViewportSelectionState, SceneEditorNumericScrubMouseGuard, SceneEditorState,
}; // 场景树点击同时结束查看工具中的植被整层选择。
use crate::ui::continuous_list::pool_capacity;
use crate::ui::asset_icons;
use crate::ui::list_layout::{
    SCENE_OBJECT_DEFAULT_VIEWPORT_HEIGHT, SCENE_OBJECT_ROW_PITCH, fit_scene_object_list_in_height,
};
use crate::ui::scene_inspector::InspectorTree;
use crate::ui::scene_sidebar::{
    TREE_BRANCH_GAP, TREE_CAPTION_GAP, TreeRowToggle, tree_object_branch_button, tree_parent_guide_x, tree_row_colors, tree_row_left,
    tree_toggle_button, tree_toggle_icon,
};
use crate::viewport::placement_move::PlacementMoveState;
use crate::viewport::SelectedPlacementViewportState;

use super::{helpers::selection_matches, row_controls::BranchTarget}; // 选择身份不受热更索引漂移影响。
use super::{
    EditorGltfBrowserEntryKind, EditorGltfBrowserState, MainSceneGlbRow, MainSceneSelection, PlacementTreeDrag, PlacementTreeExpander, PlacementTreeToggle, SceneEditorGltfBrowserList,
    SceneEditorGltfBrowserListContent, SceneEditorGltfBrowserRow, SceneEditorGltfBrowserRowText,
    SceneEditorGltfBrowserStatusText, SceneTreeRootRow,
}; // 模块私有组件与公开资源。

#[cfg(test)]
#[path = "input_tests.rs"]
mod input_tests;

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct GltfRowCaption;

#[derive(Component)]
pub(crate) struct GltfRowGuide;

pub(super) fn main_glb_draft<'a>(
    editor: &SceneEditorState, session: &'a EditorAuthoringSession,
) -> Option<&'a EditorSceneDraft> {
    // 只展示当前场景实际使用的 GLB，切换过程中不借用上一场景的动画目录。
    session.draft.as_ref().filter(|draft| {
        !editor.scene_transition_in_progress && editor.selected_scene_id.as_ref() == Some(&draft.scene_id)
            && draft.placement_glb_hash.is_some()
    })
}

/// 以左栏整个滚动视口为上限，保留由外层滚动露出摆放树的空间。
fn scene_tree_viewport_height(tree: &Query<&ComputedNode, With<InspectorTree>>) -> Option<f32> {
    let node = tree.single().ok()?;
    let height = node.size().y * node.inverse_scale_factor();
    (height.is_finite() && height > 0.0).then_some(height)
}

/// 可见行池只随视口尺寸调整，展开模型继续复用现有行实体。
pub(crate) fn spawn_gltf_browser_rows(
    mut commands: Commands,
    lists: Query<(Entity, Option<&Children>), With<SceneEditorGltfBrowserListContent>>,
    tree: Query<&ComputedNode, With<InspectorTree>>,
    asset_server: Res<AssetServer>, // 统一加载项目中文字体。
) {
    let height = scene_tree_viewport_height(&tree);
    let capacity = pool_capacity(height.unwrap_or(SCENE_OBJECT_DEFAULT_VIEWPORT_HEIGHT), 1, SCENE_OBJECT_ROW_PITCH);
    for (container, children) in &lists {
        let existing = children.map_or(0, |children| children.len());
        if height.is_none() && existing > 0 { continue; } // 隐藏页尚未测量，保留原行池。
        if let Some(children) = children {
            for row in children.iter().skip(capacity) {
                commands.entity(row).despawn();
            }
        }
        if existing >= capacity { continue; }
        let font = feathrise_editor_core::editor_font(&asset_server, "fonts/open_source/noto_sans/cjk/NotoSansCJKsc-VF.otf");
        for slot in existing..capacity {
            let row = commands.spawn((
                ChildOf(container),
                InteractionButton,
                bevy::ui::RelativeCursorPosition::default(),
                SceneEditorGltfBrowserRow { slot, entry_index: None, entries_revision: 0, main_glb: None },
                Node {
                    display: Display::None,
                    position_type: PositionType::Absolute, // 内容层布局保留完整树，行按实际索引定位。
                    width: percent(100),
                    min_width: px(0),
                    height: px(feathrise_editor_ui::metrics::ROW_HEIGHT),
                    flex_shrink: 0.0,
                    align_items: AlignItems::Center,
                    padding: UiRect { left: tree_row_left(0), right: px(5), ..UiRect::ZERO },
                    column_gap: px(TREE_BRANCH_GAP),
                    border_radius: BorderRadius::all(px(2)),
                    overflow: Overflow::clip_x(),
                    ..default()
                },
                BackgroundColor(colors::PANEL_BG),
                BorderColor::all(colors::COPPER_BORDER_WEAK),
            )).id();
            // 引导线画在父级缩进内，箭头与类型图标共用原行，保留名称的命中区。
            commands.spawn((ChildOf(row), GltfRowGuide, Pickable::IGNORE, Visibility::Hidden,
                Node { position_type: PositionType::Absolute, left: px(0), top: px(0),
                    width: px(1), height: px(SCENE_OBJECT_ROW_PITCH), ..default() },
                UiTransform::default(), BackgroundColor(colors::COPPER_BORDER_WEAK)));
            commands.spawn_scene(tree_object_branch_button()).insert((
                ChildOf(row), PlacementTreeExpander::default(),
                bevy::ui::InteractionDisabled, Visibility::Hidden, Pickable::IGNORE,
            ));
            // 单独裁切图标和文字区，为三个行内按钮保留固定宽度。
            let caption = commands.spawn((ChildOf(row), GltfRowCaption, Node {
                flex_grow: 1.0, flex_basis: px(0), min_width: px(0),
                align_items: AlignItems::Center, column_gap: px(TREE_CAPTION_GAP),
                overflow: Overflow::clip_x(), ..default()
            }, Pickable::IGNORE)).id();
            commands.spawn_scene(super::scene::gltf_row_icon()).insert(ChildOf(caption));
            commands.spawn((
                ChildOf(caption),
                SceneEditorGltfBrowserRowText,
                Text::new(""),
                TextFont {
                    font: font.clone().into(),
                    font_size: feathrise_editor_ui::metrics::FONT_CONTROL.into(),
                    ..default()
                },
                TextColor(colors::TEXT_PRIMARY),
                TextLayout::no_wrap(),
                Node { min_width: px(0), ..default() },
                Pickable::IGNORE,
            ));
            for kind in TreeRowToggle::ALL {
                let button = commands.spawn_scene(tree_toggle_button(kind))
                    .insert((ChildOf(row), PlacementTreeToggle::new(kind), bevy::ui::InteractionDisabled)).id();
                commands.spawn_scene(tree_toggle_icon()).insert(ChildOf(button));
            }
        }
    }
}

/// 分支行由箭头和类型图标开合，名称区由整行负责选择。
pub(crate) fn handle_gltf_browser_input(
    editor: Res<SceneEditorState>,
    numeric_scrub_mouse: Res<SceneEditorNumericScrubMouseGuard>, // 数值拖动和弦期间丢弃点击。
    session: Res<EditorAuthoringSession>, // 当前草稿提供不可跨场景复用的SceneId。
    rows: Query<(Ref<EditorButtonClick>, &SceneEditorGltfBrowserRow)>,
    expanders: Query<(&EditorButtonClick, &ChildOf, &PlacementTreeExpander), Changed<EditorButtonClick>>,
    movement: Option<Res<PlacementMoveState>>,
    mut state: ResMut<EditorGltfBrowserState>, // 只修改树的折叠和选择。
    mut selected_viewport: ResMut<SelectedPlacementViewportState>,
    mut viewport_selection: ResMut<EditorViewportSelectionState>, // 实际条目点击与植被整层保持单选语义。
    mut candidates: ResMut<crate::viewport::content_selection::LayerCandidateState>, // 显式场景树点击退出视口整实例选择。
    keyboard: Option<Res<ButtonInput<KeyCode>>>,
    mut multi_selection: Option<ResMut<crate::viewport::multi_select::PlacementSelectionSet>>,
    mut sidebar: Option<ResMut<crate::ui::sidebars::SceneEditorSidebarState>>,
    layout: Option<Res<crate::resources::SceneEditorLayoutState>>,
    workspace: Option<Res<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>>,
) {
    if let Some((scene, MainSceneSelection::Animation(id))) = &state.selected_main_row
        && session.draft.as_ref().is_some_and(|draft| &draft.scene_id == scene && draft.motion.clip_name_for(id).is_none())
    {
        state.selected_main_row = None; // 原片段删除后清除高亮，保留摆放和网格选择。
    }
    if !editor.open || numeric_scrub_mouse.blocked()
        || movement.as_ref().is_some_and(|state| state.owns_mouse())
        || sidebar.as_ref().is_some_and(|state| !state.scene_tree_visible())
        || layout.as_ref().is_some_and(|state| !state.content_shown())
        || workspace.as_ref().is_some_and(|state| state.active_tab != crate::ui::vegetation_preset_editor::PresetWorkspaceTab::Scene) {
        return; // 系统持续推进Changed tick；隐藏或所有权帧保持选择且不延迟点击。
    }
    if let Some((_, parent, expander)) = expanders.iter().find(|(click, ..)| click.is_clicked()) {
        if let Ok((_, row)) = rows.get(parent.parent())
            && let Some(target) = expander.target(&editor, &state, &session, row)
        {
            match target {
                BranchTarget::Entry(index) => state.toggle_tree_branch(index),
                BranchTarget::MainGlb => {
                    let scene = editor.selected_scene_id.as_ref();
                    state.collapsed_main_glb = if state.collapsed_main_glb.as_ref() == scene { None } else { scene.cloned() };
                }
            }
        }
        return; // 箭头释放不会改派给名称行。
    }
    let Some(row) = rows.iter().find_map(|(interaction, row)| {
        (interaction.is_changed() && interaction.is_clicked() && row.entries_revision == state.published_entries_revision)
            .then_some(row)
    }) else { return; }; // 点击只采用上一呈现阶段发布的树行身份。
    if row.main_glb == Some(MainSceneGlbRow::File) {
        let Some(draft) = main_glb_draft(&editor, &session) else { return; };
        state.clear_viewport_selection();
        // 文件行只选中主 GLB，摆放或地形的旧目标不得继续接收属性操作。
        state.selected_main_row = Some((draft.scene_id.clone(), MainSceneSelection::File));
        viewport_selection.clear_vegetation_selection();
        candidates.clear_for_scene_tree();
        selected_viewport.clear_click_sequence();
        if let Some(selection) = multi_selection.as_mut() { selection.clear(); }
        if let Some(sidebar) = sidebar.as_mut() { sidebar.reveal_scene_properties(); }
        return;
    }
    if let Some(MainSceneGlbRow::Animation { index, id }) = &row.main_glb {
        if let Some(draft) = main_glb_draft(&editor, &session)
            && draft.motion.clip_at(*index).is_some_and(|(current, _)| current == id)
        {
            // 动画行选中后可经原菜单改名。保留视口姿势目标，双击仍由原时间轴入口处理。
            state.selected_main_row = Some((draft.scene_id.clone(), MainSceneSelection::Animation(id.clone())));
            state.selected_group = None;
            if let Some(sidebar) = sidebar.as_mut() { sidebar.reveal_scene_properties(); }
        }
        return;
    }
    if row.main_glb.is_some() { return; } // 只读动画和空提示没有可选作者对象。
    let Some(entry_index) = row.entry_index else { return; };
    if state.signature_build.is_some() || state.entries_build.is_some() {
        state.status = "场景树正在更新，请完成后再选择或展开".to_owned();
        return; // 旧活动行保持完整，但其索引在新结构发布前不得继续接受输入。
    }
    let Some(entry) = state.entries.get(entry_index).cloned() else {
        return; // 热更错帧后的旧行索引不猜测目标。
    };
    if matches!(entry.kind, EditorGltfBrowserEntryKind::SourceAnimation { .. }) {
        return; // 源文件片段由自己的动画标签处理。
    }
    if let Some(sidebar) = sidebar.as_mut() {
        sidebar.reveal_scene_properties(); // 左树选择后立即显示全高属性，保留导航和工具记忆。
    }
    viewport_selection.clear_vegetation_selection(); // 明确选择场景对象时隐藏旧植被覆盖。
    state.selected_main_row = None;
    if let EditorGltfBrowserEntryKind::Group { name, .. } = &entry.kind {
        state.selected_group = Some(name.clone());
        state.selected_placement_id = None; state.selection = None;
        if let Some(selection) = multi_selection.as_mut() { selection.clear(); }
        candidates.clear_for_scene_tree(); selected_viewport.clear_click_sequence();
        return; // 组行直接选中该组，保留分支与子模型展开偏好。
    }
    state.selected_group = None;
    if let Some(selection) = multi_selection.as_mut() {
        let additive = keyboard.as_ref().is_some_and(|keys| keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]));
        let id = match &entry.kind {
            EditorGltfBrowserEntryKind::Placement { placement_id, .. } => Some(placement_id.as_str()),
            _ => entry.selection.as_ref().map(|selection| selection.placement_id.as_str()),
        };
        if let Some((index, record)) = id.and_then(|id| state.placement_record_index(id, session.placement_structure_revision))
            .and_then(|index| session.draft.as_ref()?.placements.placements.get(index).map(|record| (index, record))) {
            if record.building_preset_id.is_none() && record.editor.selectable() {
                let position = match crate::authoring::placement_pose::placement_record_world_position(session.draft.as_ref().unwrap(), record) {
                    Ok(position) => position,
                    Err(error) => {
                        selection.clear();
                        crate::viewport::multi_select::publish_primary(selection, &mut state, &mut candidates);
                        selected_viewport.clear_click_sequence();
                        state.status = error;
                        return; // 失败来源不能通过场景树保留可操作的旧主选择或局部锚点。
                    }
                };
                let member = crate::viewport::multi_select::SelectedMember { id: record.id.clone(), index, position };
                if let Err(error) = selection.click(member, additive, &session) { state.status = error; return; }
                if additive {
                    candidates.freeze_selection();
                    crate::viewport::multi_select::publish_primary(selection, &mut state, &mut candidates);
                    selected_viewport.clear_click_sequence();
                    state.status = format!("已选择{}个场景对象", selection.members.len()); return;
                }
            } else if additive && !selection.members.is_empty() { state.status = "建筑继续使用单栋或院落选择，不能加入普通资产多选".into(); return; }
            else { selection.clear(); }
        } else if !additive { selection.clear(); }
    }
    match entry.kind {
        EditorGltfBrowserEntryKind::Group { .. } | EditorGltfBrowserEntryKind::SourceAnimation { .. } =>
            unreachable!("目录与动画已由上方消费"),
        EditorGltfBrowserEntryKind::Placement { placement_id, .. } => {
            selected_viewport.clear_click_sequence(); // 明确选择新的树行时取消旧定位请求。
            if !state.placement_asset_paths.contains_key(&placement_id) {
                state.status = "该对象ID重复或已过期，不能稳定选择".to_owned();
                return; // 重复ID根只供诊断。
            }
            state.selected_placement_id = Some(placement_id.clone()); // 根行同时成为五类完整资产操作目标。
            candidates.clear_for_scene_tree(); // 即使重复点同一实例，也取消旧候选悬停及迟到查询。
            state.selection = None; // 根选择不冒充具体Blender网格，网格水属性随之收起。
            let builtin = state.placement_record_indices.get(&placement_id)
                .and_then(|index| state.signature.get(*index))
                .is_some_and(|signature| signature.asset_target.is_builtin());
            state.status = if builtin {
                format!("已选网格 · 立方体 · {placement_id}")
            } else {
                format!("已选场景对象 · {placement_id}")
            };
        }
        EditorGltfBrowserEntryKind::Mesh | EditorGltfBrowserEntryKind::MissingWaterBinding => {
            candidates.clear_for_scene_tree(); // 同一个网格再次点击也应切回精确网格查看。
            let selection = entry.selection; // 共用聚焦入口只接手已确认的双击，普通树行点击只选择。
            state.selected_placement_id = selection
                .as_ref()
                .map(|selection| selection.placement_id.clone()); // 任一网格行同样选中其所属完整实例。
            selected_viewport.clear_click_sequence();
            state.selection = selection; // 无效引用也保留，由valid明确要求后续动作拒绝。
        }
    }
}

/// 可见行池按滚动窗口原地更新文本、缩进和选中样式。
pub(crate) fn sync_gltf_browser_rows(
    editor: Res<SceneEditorState>,        // 编辑器打开时才刷新可见行。
    session: Res<EditorAuthoringSession>,
    state: Res<EditorGltfBrowserState>, // 公开扁平条目与当前选择。
    view: (Option<Res<PlacementTreeDrag>>, Option<Res<feathrise_editor_ui::localization::EditorLocale>>),
    multi_selection: Option<Res<crate::viewport::multi_select::PlacementSelectionSet>>,
    asset_server: Option<Res<AssetServer>>,
    images: Option<Res<Assets<Image>>>,
    tree: Query<&ComputedNode, With<InspectorTree>>,
    mut presented_scope: Local<Option<(Option<feathrise_scene_content::SceneId>, u64, usize, f32)>>,
    mut glyphs: Query<&mut ImageNode, With<crate::components::SceneEditorGltfBrowserRowGlyph>>,
    mut guides: Query<(&mut UiTransform, &mut Visibility), With<GltfRowGuide>>,
    captions: Query<&Children, With<GltfRowCaption>>,
    mut rows: Query<
        (
            Entity,
            &UiInteraction,                 // 当前hover或按压反馈。
            Option<&mut EditorButtonClick>,
            &mut SceneEditorGltfBrowserRow, // 写回当前窗口绑定的索引。
            &mut Node,                    // 隐藏空槽并调整Blender层级缩进。
            &mut BackgroundColor,         // 选择和hover底色。
            &mut BorderColor,             // 选择和hover铜边。
            &Children,                    // 唯一文本子实体。
        ),
        (
            With<SceneEditorGltfBrowserRow>,
            Without<SceneTreeRootRow>, // 主 GLB 根行固定，不参与滚动槽位换绑。
            Without<SceneEditorGltfBrowserList>,
            Without<SceneEditorGltfBrowserListContent>,
        ),
    >,
    mut row_texts: Query<(&mut Text, &mut TextColor), With<SceneEditorGltfBrowserRowText>>, // 行文字和有效性颜色。
    mut status_texts: Query<
        (&mut Text, &mut Node),
        (
            With<SceneEditorGltfBrowserStatusText>,
            Without<SceneEditorGltfBrowserRowText>,
            Without<SceneEditorGltfBrowserRow>,
            Without<SceneEditorGltfBrowserList>,
            Without<SceneEditorGltfBrowserListContent>,
        ),
    >,
    mut layouts: (
        Query<
            (&mut Node, &mut ScrollPosition),
            (
                With<SceneEditorGltfBrowserList>,
                Without<SceneEditorGltfBrowserListContent>,
                Without<SceneEditorGltfBrowserRow>,
            ),
        >,
        Query<
            &mut Node,
            (
                With<SceneEditorGltfBrowserListContent>,
                Without<SceneEditorGltfBrowserList>,
                Without<SceneEditorGltfBrowserRow>,
            ),
        >,
    ),
) {
    if !editor.open {
        // 工程预览临时隐藏编辑器时保留树位置，真正关闭才清除会话滚动身份。
        if !state.initialized {
            *presented_scope = None;
        }
        return; // 隐藏UI无需重复写文本和样式。
    }
    let main_glb = main_glb_draft(&editor, &session);
    let motion = main_glb.map(|draft| &draft.motion);
    let selected_animation = main_glb.and_then(|draft| {
        let id = state.selected_scene_animation(&draft.scene_id)?;
        draft.motion.clip_name_for(id).map(|_| id)
    });
    let empty_hint = usize::from(state.main_glb_entry_count == 0);
    // 主文件在独立根行，滚动列表从动画和内嵌网格开始，再列引用与文件夹。
    let main_rows = motion.map_or(0, |motion| (motion.clip_count() + motion.foreign_clip_count()).max(empty_hint));
    let total_rows = main_rows.saturating_add(state.entries.len());
    let (drag, locale) = view;
    let locale = locale.as_deref().copied().unwrap_or_default();
    let scene_changed = presented_scope.as_ref().is_none_or(|(scene, _, _, _)| scene != &editor.selected_scene_id);
    let height = scene_tree_viewport_height(&tree)
        .or_else(|| presented_scope.as_ref().map(|(_, _, _, height)| *height))
        .unwrap_or(SCENE_OBJECT_DEFAULT_VIEWPORT_HEIGHT);
    let scope = (editor.selected_scene_id.clone(), state.published_entries_revision, total_rows, height);
    let scope_changed = presented_scope.as_ref() != Some(&scope);
    *presented_scope = Some(scope);
    let mut start = 0;
    if let (Ok((mut viewport, mut scroll)), Ok(mut content)) = (layouts.0.single_mut(), layouts.1.single_mut()) {
        if scope_changed || viewport.is_added() || scroll.is_changed() {
            // 换场景回到顶部，展开、折叠及热更新只钳制既有滚动位置。
            let mut position = (*scroll).clone();
            fit_scene_object_list_in_height(total_rows, height, scene_changed, None,
                &mut viewport, &mut content, &mut position);
            if scroll.0 != position.0 {
                *scroll = position;
            }
        }
        start = ((scroll.y / SCENE_OBJECT_ROW_PITCH).floor() as usize).saturating_sub(1);
    }
    for (entity, interaction, mut click, mut row, mut node, mut background, mut border, children) in &mut rows {
        let tree_index = start.saturating_add(row.slot);
        let animation = motion.and_then(|motion| motion.clip_at(tree_index));
        let read_only = motion.and_then(|motion| {
            let index = tree_index.checked_sub(motion.clip_count())?;
            Some((index, motion.foreign_clip_at(index)?))
        });
        let empty = tree_index < main_rows && animation.is_none() && read_only.is_none();
        // 只读当前可见槽，ID 仅在换绑时复制。
        let same_main_row = match (&row.main_glb, animation, read_only, empty) {
            (Some(MainSceneGlbRow::Animation { index, id: previous }), Some((id, _)), _, _) =>
                *index == tree_index && previous == id,
            (Some(MainSceneGlbRow::ReadOnlyAnimation(previous)), _, Some((index, _)), _) =>
                *previous == index,
            (Some(MainSceneGlbRow::Empty), None, None, true) | (None, None, None, false) => true,
            _ => false,
        };
        let binding = tree_index.checked_sub(main_rows)
            .filter(|&index| index < state.entries.len());
        let rebound = scene_changed || row.entries_revision != state.published_entries_revision
            || row.entry_index != binding || !same_main_row;
        if rebound {
            if let Some(click) = click.as_deref_mut() {
                click.invalidate();
            }
            row.entry_index = binding;
            row.main_glb = animation.map(|(id, _)| MainSceneGlbRow::Animation { index: tree_index, id: id.to_owned() })
                .or_else(|| read_only.map(|(index, _)| MainSceneGlbRow::ReadOnlyAnimation(index)))
                .or_else(|| empty.then_some(MainSceneGlbRow::Empty));
            row.entries_revision = state.published_entries_revision;
        }
        let placement = binding.and_then(|index| state.entries.get(index));
        let (depth, name, prefix, base_color, icon, selected) = if let Some((id, name)) = animation {
            (0, name, "", colors::TEXT_PRIMARY, "icons/editor/tree_file.png", selected_animation == Some(id))
        } else if let Some((_, name)) = read_only {
            (0, name, "", colors::TEXT_MUTED, "icons/editor/tree_file.png", false)
        } else if empty {
            (0, locale.text("label.main-scene-no-animations"), "", colors::TEXT_MUTED, "icons/editor/tree_file.png", false)
        } else if let Some(entry) = placement {
            let selected = selected_animation.is_none() && match &entry.kind {
                EditorGltfBrowserEntryKind::Group { name, .. } => state.selected_group.as_ref() == Some(name),
                EditorGltfBrowserEntryKind::Placement { placement_id, .. } =>
                    state.selection.is_none() && (state.selected_placement_id.as_ref() == Some(placement_id)
                        || multi_selection.as_ref().is_some_and(|selection| selection.contains(placement_id))),
                EditorGltfBrowserEntryKind::Mesh | EditorGltfBrowserEntryKind::MissingWaterBinding =>
                    selection_matches(state.selection.as_ref(), entry.selection.as_ref()),
                EditorGltfBrowserEntryKind::SourceAnimation { .. } => false,
            };
            let (prefix, base_color, icon) = match &entry.kind {
                EditorGltfBrowserEntryKind::Group { collapsed, .. } => ("", colors::TEXT_PRIMARY,
                    if *collapsed { "icons/editor/tree_folder.png" } else { "icons/editor/tree_root.png" }),
                EditorGltfBrowserEntryKind::Placement { glb, .. } => ("", colors::TEXT_ACCENT,
                    if *glb { asset_icons::GLB } else { "icons/editor/tree_mesh.png" }),
                EditorGltfBrowserEntryKind::Mesh if entry.selection.as_ref().is_some_and(|mesh| mesh.is_source_folder()) =>
                    ("", colors::TEXT_PRIMARY, "icons/editor/tree_folder.png"),
                EditorGltfBrowserEntryKind::Mesh => {
                    let (prefix, color) = if entry.selection.as_ref().is_some_and(|selection| selection.valid) {
                        ("", colors::TEXT_PRIMARY)
                    } else { ("!", colors::TEXT_MUTED) };
                    (prefix, color, "icons/editor/tree_mesh.png")
                }
                EditorGltfBrowserEntryKind::MissingWaterBinding => ("×", colors::DANGER_TEXT, "icons/editor/tree_mesh.png"),
                EditorGltfBrowserEntryKind::SourceAnimation { .. } => ("", colors::TEXT_PRIMARY, "icons/editor/tree_file.png"),
            };
            // 文件夹和 GLB 共享真实的可见层级，直属网格和动画也占下一档。
            (entry.depth, entry.display_name.as_str(), prefix, base_color, icon, selected)
        } else {
            node.display = Display::None; // 实体池保留但不参与布局。
            continue;
        };
        node.display = Display::Flex; // 当前条目原地复用槽位。
        node.top = px(tree_index as f32 * SCENE_OBJECT_ROW_PITCH);
        let depth = depth.min(8);
        node.padding.left = tree_row_left(depth);
        for child in children.iter() {
            if let Ok((mut transform, mut visibility)) = guides.get_mut(child) {
                visibility.set_if_neq(if depth > 0 { Visibility::Inherited } else { Visibility::Hidden });
                let translation = Val2::px(tree_parent_guide_x(depth), 0.0);
                if transform.translation != translation { transform.translation = translation; }
            }
        }
        let interaction = if empty || read_only.is_some() || drag.as_ref().is_some_and(|drag| drag.owns_mouse()) {
            UiInteraction::None
        } else { *interaction };
        // 行池同帧滚动换绑时，不把上一行的有效落点高亮借给新条目。
        let drop_target = !rebound && drag.as_ref().is_some_and(|drag| drag.highlights(entity));
        let (fill, outline) = tree_row_colors(selected, drop_target, interaction);
        node.border = if drop_target { UiRect::all(px(1)) } else { UiRect::ZERO };
        background.set_if_neq(BackgroundColor(fill));
        border.set_if_neq(outline);
        let Some(caption) = children.iter().find_map(|entity| captions.get(entity).ok()) else { continue; };
        for child in caption.iter() {
            if images.is_some() && let Some(server) = asset_server.as_ref()
                && let Ok(mut image) = glyphs.get_mut(child)
            {
                let handle = server.load(icon);
                if image.image != handle { image.image = handle; }
                let tint = asset_icons::tint(icon, base_color);
                if image.color != tint { image.color = tint; }
            }
            if let Ok((mut text, mut color)) = row_texts.get_mut(child) {
                if prefix.is_empty() && text.as_str() != name {
                    name.clone_into(&mut **text);
                } else if !prefix.is_empty() {
                    **text = format!("{prefix} {name}");
                }
                color.0 = if selected {
                    colors::TEXT_ACCENT // 选中态优先于行类型色。
                } else {
                    base_color // 根、有效网格和无效网格各自可辨。
                };
            }
        }
    }

    let selection_hint = state.selection.as_ref().map_or_else(
        || {
            state
                .selected_placement_id
                .as_ref()
                .map_or(String::new(), |placement_id| {
                    format!(" · 已选对象 {placement_id}")
                })
        },
        |selection| {
            if selection.orphaned_water_binding {
                " · 失效水绑定（可点“取消水体”清除）".to_owned() // 删除或改名目标不能继续改参数，但必须可恢复作者数据。
            } else if selection.valid {
                format!(
                    " · 已选 {} / {}",
                    selection.placement_id, selection.mesh_name
                ) // 回显可供后续动作采用的身份。
            } else {
                " · 当前选择不可引用（请在Blender显式命名节点与网格）".to_owned() // 无效选择不静默丢失。
            }
        },
    );
    for (mut text, mut node) in &mut status_texts {
        let visible = !state.initialized || !state.draft_available || state.source_placement_count > 0
            || state.truncated || state.loading_asset_count > 0 || state.failed_asset_count > 0;
        let display = if visible { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
        **text = format!("{}{selection_hint}", state.status);
    }
}
