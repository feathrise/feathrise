//! GLB浏览器可见行池、滚动范围、展开选择与样式同步。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::EditorButtonClick;
use bevy::prelude::*; // ECS系统参数、UI组件和字体。
use crate::ui::theme as colors; // 固定行池复用全屏编辑器水墨铜边配色。

use crate::authoring::EditorAuthoringSession;
use crate::resources::{
    EditorViewportSelectionState, SceneEditorNumericScrubMouseGuard, SceneEditorState,
}; // 场景树点击同时结束查看工具中的植被整层选择。
use crate::ui::continuous_list::pool_capacity;
use crate::ui::list_layout::{
    SCENE_OBJECT_DEFAULT_VIEWPORT_HEIGHT, SCENE_OBJECT_ROW_PITCH, fit_scene_object_list_in_height,
};
use crate::ui::scene_inspector::InspectorTree;
use crate::ui::scene_sidebar::{TreeRowToggle, tree_toggle_button, tree_toggle_icon};
use crate::viewport::placement_move::PlacementMoveState;
use crate::viewport::SelectedPlacementViewportState;

use super::helpers::selection_matches; // 选择身份不受热更索引漂移影响。
use super::{
    EditorGltfBrowserEntryKind, EditorGltfBrowserState, PlacementTreeExpander, PlacementTreeToggle, SceneEditorGltfBrowserList,
    SceneEditorGltfBrowserListContent, SceneEditorGltfBrowserRow, SceneEditorGltfBrowserRowText,
    SceneEditorGltfBrowserStatusText,
}; // 模块私有组件与公开资源。

#[cfg(test)]
#[path = "input_tests.rs"]
mod input_tests;

#[derive(Component)]
pub(crate) struct GltfRowCaption;

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
                SceneEditorGltfBrowserRow { slot, entry_index: None, entries_revision: 0 },
                Node {
                    display: Display::None,
                    position_type: PositionType::Absolute, // 内容层布局保留完整树，行按实际索引定位。
                    width: percent(100),
                    min_width: px(0),
                    height: px(feathrise_editor_ui::metrics::ROW_HEIGHT),
                    flex_shrink: 0.0,
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(px(5)),
                    column_gap: px(2),
                    border_radius: BorderRadius::all(px(2)),
                    overflow: Overflow::clip_x(),
                    ..default()
                },
                BackgroundColor(colors::PANEL_BG),
                BorderColor::all(colors::COPPER_BORDER_WEAK),
            )).id();
            // 箭头有自己的释放脉冲，名称行仍负责单击选择和双击定位。
            let expander = commands.spawn((
                ChildOf(row),
                PlacementTreeExpander::default(),
                InteractionButton,
                bevy::ui::InteractionDisabled,
                Visibility::Hidden,
                Node {
                    width: px(18), height: px(21), flex_shrink: 0.0,
                    align_items: AlignItems::Center, justify_content: JustifyContent::Center,
                    border_radius: BorderRadius::all(px(3)), ..default()
                },
                BackgroundColor(Color::NONE),
            )).id();
            commands.spawn_scene(tree_toggle_icon()).insert(ChildOf(expander));
            // 单独裁切图标和文字区，为三个行内按钮保留固定宽度。
            commands.spawn((ChildOf(row), GltfRowCaption, Node {
                flex_grow: 1.0, flex_basis: px(0), min_width: px(0),
                align_items: AlignItems::Center, column_gap: px(5),
                overflow: Overflow::clip_x(), ..default()
            }, Pickable::IGNORE)).with_children(|caption| {
                caption.spawn((
                    crate::components::SceneEditorGltfBrowserRowGlyph,
                    ImageNode::default(),
                    Node { width: px(16), height: px(16), flex_shrink: 0.0, ..default() },
                    Pickable::IGNORE,
                ));
                caption.spawn((
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
            });
            for kind in TreeRowToggle::ALL {
                let button = commands.spawn_scene(tree_toggle_button(kind))
                    .insert((ChildOf(row), PlacementTreeToggle::new(kind), bevy::ui::InteractionDisabled)).id();
                commands.spawn_scene(tree_toggle_icon()).insert(ChildOf(button));
            }
        }
    }
}

/// 名称只选择，箭头只改变当前分支，沿用同一鼠标所有权。
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
    if !editor.open || numeric_scrub_mouse.blocked()
        || movement.as_ref().is_some_and(|state| state.owns_mouse())
        || sidebar.as_ref().is_some_and(|state| !state.scene_tree_visible())
        || layout.as_ref().is_some_and(|state| !state.content_shown())
        || workspace.as_ref().is_some_and(|state| state.active_tab != crate::ui::vegetation_preset_editor::PresetWorkspaceTab::Scene) {
        return; // 系统持续推进Changed tick；隐藏或所有权帧保持选择且不延迟点击。
    }
    if let Some((_, parent, expander)) = expanders.iter().find(|(click, ..)| click.is_clicked()) {
        if let Ok((_, row)) = rows.get(parent.parent())
            && let Some(index) = expander.target_index(&state, &session, row)
        {
            state.toggle_tree_branch(index);
        }
        return; // 只处理箭头自己的脉冲，也不让旧箭头手势改派到名称行。
    }
    let Some(entry_index) = rows.iter().find_map(|(interaction, row)| {
        (interaction.is_changed() && interaction.is_clicked() && row.entries_revision == state.published_entries_revision)
            .then_some(row.entry_index)
            .flatten()
    }) else { return; }; // 点击只采用上一呈现阶段发布的树行身份。
    if state.signature_build.is_some() || state.entries_build.is_some() {
        state.status = "场景树正在更新，请完成后再选择或展开".to_owned();
        return; // 旧活动行保持完整，但其索引在新结构发布前不得继续接受输入。
    }
    let Some(entry) = state.entries.get(entry_index).cloned() else {
        return; // 热更错帧后的旧行索引不猜测目标。
    };
    if let Some(sidebar) = sidebar.as_mut() {
        sidebar.reveal_scene_properties(); // 左树选择后立即显示全高属性，保留导航和工具记忆。
    }
    viewport_selection.clear_vegetation_selection(); // 明确选择场景对象时隐藏旧植被覆盖。
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
        EditorGltfBrowserEntryKind::Group { .. } => unreachable!("目录已由上方消费"),
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
    state: Res<EditorGltfBrowserState>, // 公开扁平条目与当前选择。
    multi_selection: Option<Res<crate::viewport::multi_select::PlacementSelectionSet>>,
    asset_server: Option<Res<AssetServer>>,
    images: Option<Res<Assets<Image>>>,
    tree: Query<&ComputedNode, With<InspectorTree>>,
    mut presented_scope: Local<Option<(Option<feathrise_scene_content::SceneId>, u64, usize, f32)>>,
    mut glyphs: Query<&mut ImageNode, With<crate::components::SceneEditorGltfBrowserRowGlyph>>,
    captions: Query<&Children, With<GltfRowCaption>>,
    mut rows: Query<
        (
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
    let scene_changed = presented_scope.as_ref().is_none_or(|(scene, _, _, _)| scene != &editor.selected_scene_id);
    let height = scene_tree_viewport_height(&tree)
        .or_else(|| presented_scope.as_ref().map(|(_, _, _, height)| *height))
        .unwrap_or(SCENE_OBJECT_DEFAULT_VIEWPORT_HEIGHT);
    let scope = (editor.selected_scene_id.clone(), state.published_entries_revision, state.entries.len(), height);
    let scope_changed = presented_scope.as_ref() != Some(&scope);
    *presented_scope = Some(scope);
    let mut start = 0;
    if let (Ok((mut viewport, mut scroll)), Ok(mut content)) = (layouts.0.single_mut(), layouts.1.single_mut()) {
        if scope_changed || viewport.is_added() || scroll.is_changed() {
            // 换场景回到顶部，展开、折叠及热更新只钳制既有滚动位置。
            let mut position = (*scroll).clone();
            fit_scene_object_list_in_height(state.entries.len(), height, scene_changed, None,
                &mut viewport, &mut content, &mut position);
            if scroll.0 != position.0 {
                *scroll = position;
            }
        }
        start = ((scroll.y / SCENE_OBJECT_ROW_PITCH).floor() as usize).saturating_sub(1);
    }
    for (interaction, mut click, mut row, mut node, mut background, mut border, children) in &mut rows {
        let entry_index = start.saturating_add(row.slot);
        let binding = state.entries.get(entry_index).map(|_| entry_index);
        if scene_changed || row.entries_revision != state.published_entries_revision || row.entry_index != binding {
            if let Some(click) = click.as_deref_mut() {
                click.invalidate();
            }
            row.entry_index = binding;
            row.entries_revision = state.published_entries_revision;
        }
        let Some(entry) = state.entries.get(entry_index) else {
            node.display = Display::None; // 实体池保留但不参与布局。
            continue;
        };
        node.display = Display::Flex; // 当前条目原地复用槽位。
        node.top = px(entry_index as f32 * SCENE_OBJECT_ROW_PITCH);
        node.padding.left = px(5.0 + entry.depth.min(8) as f32 * 18.0); // 与场景树共用层级步长。
        let selected = match &entry.kind {
            EditorGltfBrowserEntryKind::Group { name, .. } => state.selected_group.as_ref() == Some(name),
            EditorGltfBrowserEntryKind::Placement { placement_id, .. } => state
                .selected_placement_id
                .as_deref()
                .is_some_and(|selected| selected == placement_id)
                || multi_selection.as_ref().is_some_and(|selection| selection.contains(placement_id)), // 多选根行共用同一集合高亮。
            EditorGltfBrowserEntryKind::Mesh | EditorGltfBrowserEntryKind::MissingWaterBinding => {
                selection_matches(state.selection.as_ref(), entry.selection.as_ref())
            } // 子行仍只高亮精确Blender网格，避免整棵树同时变色。
        };
        background.0 = match (*interaction, selected) {
            (UiInteraction::Pressed, _) => colors::SELECT_BG_PRESSED, // 按下始终给即时强反馈。
            (UiInteraction::Hovered, _) => colors::SELECT_BG_HOVER,   // 可点击行明确区别静态文字。
            (UiInteraction::None, true) => colors::SELECT_BG,         // 当前网格保持暗金选中底。
            (UiInteraction::None, false) => colors::PANEL_BG,
        };
        *border = BorderColor::all(if selected || *interaction != UiInteraction::None {
            colors::COPPER_BORDER // hover与选择强化完整铜边。
        } else {
            colors::COPPER_BORDER_WEAK // 静止普通行保留弱分隔。
        });
        let (prefix, base_color) = match &entry.kind {
            EditorGltfBrowserEntryKind::Group { .. } => ("", colors::TEXT_PRIMARY),
            EditorGltfBrowserEntryKind::Placement { .. } => ("", colors::TEXT_ACCENT),
            EditorGltfBrowserEntryKind::Mesh => {
                if entry
                    .selection
                    .as_ref()
                    .is_some_and(|selection| selection.valid)
                {
                    ("", colors::TEXT_PRIMARY)
                } else {
                    ("!", colors::TEXT_MUTED) // 缺名或重复ID明确作为诊断行。
                }
            }
            EditorGltfBrowserEntryKind::MissingWaterBinding => {
                ("×", colors::DANGER_TEXT) // 当前GLB缺失的作者绑定使用明确危险色且仍可点击清除。
            }
        };
        let Some(caption) = children.iter().find_map(|entity| captions.get(entity).ok()) else { continue; };
        for child in caption.iter() {
            if images.is_some() && let Some(server) = asset_server.as_ref() && let Ok(mut image) = glyphs.get_mut(child) {
                let icon = if matches!(entry.kind, EditorGltfBrowserEntryKind::Group { .. }) { "tree_folder" } else { "tree_mesh" };
                let handle = server.load(format!("icons/editor/{icon}.png"));
                if image.image != handle { image.image = handle; }
                if image.color != base_color { image.color = base_color; }
            }
            if let Ok((mut text, mut color)) = row_texts.get_mut(child) {
                **text = if prefix.is_empty() { entry.display_name.clone() } else { format!("{prefix} {}", entry.display_name) };
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
