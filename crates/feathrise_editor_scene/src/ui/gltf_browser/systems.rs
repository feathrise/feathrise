//! GLB浏览器固定行池、滚动范围、展开选择与样式同步。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::EditorButtonClick;
use bevy::prelude::*; // ECS系统参数、UI组件和字体。
use bevy::time::Real; // Scene0网格双击定位使用F12暂停期间仍推进的真实时间。
use crate::ui::theme as colors; // 固定行池复用全屏编辑器水墨铜边配色。

use crate::authoring::EditorAuthoringSession; // 点击冻结当前草稿场景身份，跨场景同名实例不能组成双击。
use crate::resources::{
    EditorViewportSelectionState, SceneEditorNumericScrubMouseGuard, SceneEditorState,
}; // 场景树点击同时结束查看工具中的植被整层选择。
use crate::ui::list_layout::{SCENE_OBJECT_LIST_ROWS, SCENE_OBJECT_ROW_PITCH, fit_scene_object_list};
use crate::viewport::placement_move::PlacementMoveState;
use crate::viewport::SelectedPlacementViewportState; // 有效网格按下登记中央高亮与双击定位请求。

use super::entries::request_visible_entries_rebuild; // 点击只请求固定预算行构建。
use super::helpers::selection_matches; // 选择身份不受热更索引漂移影响。
use super::{
    EditorGltfBrowserEntryKind, EditorGltfBrowserState, SceneEditorGltfBrowserList,
    SceneEditorGltfBrowserListContent, SceneEditorGltfBrowserRow, SceneEditorGltfBrowserRowText,
    SceneEditorGltfBrowserStatusText,
}; // 模块私有组件与公开资源。

#[cfg(test)]
#[path = "input_tests.rs"]
mod input_tests;

/// 在BSN内容层下只创建一次固定可见行池。
pub(crate) fn spawn_gltf_browser_rows(
    mut commands: Commands, // 容器首次出现时挂载固定子实体。
    lists: Query<Entity, Added<SceneEditorGltfBrowserListContent>>,
    asset_server: Res<AssetServer>, // 统一加载项目中文字体。
) {
    let font = feathrise_editor_core::editor_font(&asset_server, "fonts/open_source/noto_sans/cjk/NotoSansCJKsc-VF.otf"); // 与编辑器其他动态行共享字体路径。
    for container in &lists {
        commands.entity(container).with_children(|parent| {
            for slot in 0..SCENE_OBJECT_LIST_ROWS {
                parent
                    .spawn((
                        InteractionButton, // 原生Interaction负责左键点击和hover。
                        SceneEditorGltfBrowserRow {
                            slot,              // 固定物理槽位永不改变。
                            entry_index: None,
                            entries_revision: 0,
                        },
                        Node {
                            display: Display::None, // 无条目槽默认隐藏。
                            position_type: PositionType::Absolute, // 内容层高度代表完整树，行按实际树索引定位。
                            width: percent(100),    // 整行接收点击。
                            min_width: px(0),
                            height: px(feathrise_editor_ui::metrics::ROW_HEIGHT),
                            flex_shrink: 0.0, // 固定行高避免文本抖动。
                            align_items: AlignItems::Center, // 单行垂直居中。
                            padding: UiRect::horizontal(px(5)), // 深度会由同步系统只改左padding。
                            column_gap: px(5),
                            border_radius: BorderRadius::all(px(2)),
                            overflow: Overflow::clip_x(), // 长Blender路径不会撑宽右栏。
                            ..default()
                        },
                        BackgroundColor(colors::PANEL_BG), // 普通树行与导航面板连成一体。
                        BorderColor::all(colors::COPPER_BORDER_WEAK), // 默认弱铜边。
                    ))
                    .with_children(|parent| {
                        parent.spawn((crate::components::SceneEditorGltfBrowserRowGlyph, ImageNode::default(), Node { width: px(16), height: px(16), flex_shrink: 0.0, ..default() }, Pickable::IGNORE));
                        parent.spawn((
                        SceneEditorGltfBrowserRowText, // 固定唯一文本子节点。
                        Text::new(""),               // 由可见窗口覆盖。
                        TextFont {
                            font: font.clone().into(), // 强字体句柄随UI实体持有。
                            font_size: feathrise_editor_ui::metrics::FONT_CONTROL.into(),
                            ..default()
                        },
                        TextColor(colors::TEXT_PRIMARY), // 行类型和有效性随后细分。
                        TextLayout::no_wrap(),
                        Node {
                            width: percent(100),
                            min_width: px(0),
                            ..default()
                        },
                        bevy::ui::FocusPolicy::Pass,
                        ));
                    });
            }
        });
    }
}

/// 左键点场景树选中实例或网格，沿用当前视口工具。
pub(crate) fn handle_gltf_browser_input(
    mut editor: ResMut<SceneEditorState>, // 双击登记后在底栏回显正在等待当前活动Scene0。
    numeric_scrub_mouse: Res<SceneEditorNumericScrubMouseGuard>, // 数值拖动和弦期间丢弃点击。
    real_time: Res<Time<Real>>,         // 玩法虚拟时间暂停时仍识别正常双击节奏。
    session: Res<EditorAuthoringSession>, // 当前草稿提供不可跨场景复用的SceneId。
    rows: Query<(&EditorButtonClick, &SceneEditorGltfBrowserRow), Changed<EditorButtonClick>>, // 固定行点击。
    movement: Option<Res<PlacementMoveState>>,
    mut state: ResMut<EditorGltfBrowserState>, // 只修改树的折叠和选择。
    mut selected_viewport: ResMut<SelectedPlacementViewportState>, // 单击只高亮，双击只登记相机请求。
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
    let Some(entry_index) = rows.iter().find_map(|(interaction, row)| {
        (interaction.is_clicked() && row.entries_revision == state.published_entries_revision)
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
    if let EditorGltfBrowserEntryKind::Group { name, .. } = &entry.kind {
        state.selected_group = Some(name.clone());
        state.selected_placement_id = None; state.selection = None;
        if !state.collapsed_groups.remove(name) { state.collapsed_groups.insert(name.clone()); }
        if let Some(selection) = multi_selection.as_mut() { selection.clear(); }
        candidates.clear_for_scene_tree(); selected_viewport.clear_click_sequence();
        request_visible_entries_rebuild(&mut state);
        return; // 目录点击管理目录和折叠态；整组选中由独立按钮完成，不触发模型展开。
    }
    state.selected_group = None;
    viewport_selection.clear_vegetation_selection(); // 明确选择场景对象时隐藏旧植被覆盖。
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
        EditorGltfBrowserEntryKind::Placement {
            placement_id,
            expandable,
            ..
        } => {
            selected_viewport.clear_click_sequence(); // 展开/折叠实例根不属于可定位网格双击。
            let Some(asset_path) = state.placement_asset_paths.get(&placement_id).cloned() else {
                state.status = "该对象ID重复或已过期，不能稳定展开".to_owned();
                return; // 重复ID根只供诊断，绝不猜测应加载哪条记录。
            };
            state.selected_placement_id = Some(placement_id.clone()); // 根行同时成为五类完整资产操作目标。
            candidates.clear_for_scene_tree(); // 即使重复点同一实例，也取消旧候选悬停及迟到查询。
            state.selection = None; // 根选择不冒充具体Blender网格，网格水属性随之收起。
            if !expandable {
                let builtin = state.placement_record_indices.get(&placement_id)
                    .and_then(|index| state.signature.get(*index))
                    .is_some_and(|signature| signature.asset_target.is_builtin());
                state.status = if builtin {
                    format!("已选网格 · 立方体 · {placement_id}")
                } else {
                    format!("已选场景对象 · {placement_id}")
                };
                return; // 保留整项变换、复制和删除选择，但绝不请求同文件完整场景。
            }
            if state.expanded_placements.remove(&placement_id) {
                if let Some(count) = state.asset_demand_counts.get_mut(&asset_path) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        state.asset_demand_counts.remove(&asset_path);
                    }
                }
            } else {
                state.expanded_placements.insert(placement_id); // 默认折叠；首次点击才产生Scene0需求。
                let count = state
                    .asset_demand_counts
                    .entry(asset_path.clone())
                    .or_default();
                *count = count.saturating_add(1); // 同一路径多个实例共享一个强句柄。
            }
            state.asset_demand_changes.insert(asset_path); // Tasks阶段每帧最多采用固定数量路径变化。
            request_visible_entries_rebuild(&mut state); // 箭头与子树完成后原子替换，不同步扫描8192根。
        }
        EditorGltfBrowserEntryKind::Mesh | EditorGltfBrowserEntryKind::MissingWaterBinding => {
            candidates.clear_for_scene_tree(); // 同一个网格再次点击也应切回精确网格查看。
            let selection = entry.selection; // 先取得完整稳定身份，再同时发布浏览选择和视口请求。
            state.selected_placement_id = selection
                .as_ref()
                .map(|selection| selection.placement_id.clone()); // 任一网格行同样选中其所属完整实例。
            let valid_selection = selection
                .as_ref()
                .filter(|selection| selection.valid && !selection.orphaned_water_binding); // 失效或歧义行只供诊断，不伪造世界目标。
            if let (Some(draft), Some(selection)) = (session.draft.as_ref(), valid_selection) {
                if selected_viewport.register_mesh_click(
                    &draft.scene_id,                // 同名摆放ID在不同场景中完全隔离。
                    selection,                    // 节点路径和网格名共同构成稳定目标。
                    real_time.elapsed_secs_f64(), // 不受F12暂停虚拟时间影响。
                ) {
                    editor.status = format!(
                        "正在定位场景网格：{} / {} / {}；等待当前活动实例",
                        selection.placement_id,
                        selection.node_path.join("/"),
                        selection.mesh_name,
                    ); // Cursor阶段取得完整primitive范围后覆盖最终定位结果。
                }
            } else {
                selected_viewport.clear_click_sequence(); // 无效或没有当前草稿时终止旧双击请求。
            }
            state.selection = selection; // 无效引用也保留，由valid明确要求后续动作拒绝。
        }
    }
}

/// 固定行池按滚动窗口原地更新文本、缩进和选中样式。
pub(crate) fn sync_gltf_browser_rows(
    editor: Res<SceneEditorState>,        // 编辑器打开时才刷新可见行。
    state: Res<EditorGltfBrowserState>, // 公开扁平条目与当前选择。
    multi_selection: Option<Res<crate::viewport::multi_select::PlacementSelectionSet>>,
    asset_server: Option<Res<AssetServer>>,
    images: Option<Res<Assets<Image>>>,
    mut presented_scope: Local<Option<(Option<feathrise_scene_content::SceneId>, u64, usize)>>,
    mut presented_search: Local<String>,
    mut glyphs: Query<&mut ImageNode, With<crate::components::SceneEditorGltfBrowserRowGlyph>>,
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
    let scene_changed = presented_scope.as_ref().is_none_or(|(scene, _, _)| scene != &editor.selected_scene_id);
    let scope = (editor.selected_scene_id.clone(), state.published_entries_revision, state.entries.len());
    let scope_changed = presented_scope.as_ref() != Some(&scope);
    *presented_scope = Some(scope);
    let search_changed = *presented_search != state.object_search;
    if search_changed {
        presented_search.clone_from(&state.object_search);
    }
    let mut start = 0;
    if let (Ok((mut viewport, mut scroll)), Ok(mut content)) = (layouts.0.single_mut(), layouts.1.single_mut()) {
        if scope_changed || search_changed || viewport.is_added() || scroll.is_changed() {
            // 换场景或改搜索回到顶部，展开、折叠及热更新只钳制既有滚动位置。
            let mut position = (*scroll).clone();
            fit_scene_object_list(state.entries.len(), scene_changed || search_changed, None,
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
            EditorGltfBrowserEntryKind::Placement {
                collapsed,
                expandable,
                ..
            } => {
                (
                    if !expandable {
                        "" // 单primitive根的类型由图标说明。
                    } else if *collapsed {
                        "▸"
                    } else {
                        "▾"
                    },
                    colors::TEXT_ACCENT,
                ) // Scene0根显示折叠态，直接primitive使用固定实体标记。
            }
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
        for child in children.iter() {
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
