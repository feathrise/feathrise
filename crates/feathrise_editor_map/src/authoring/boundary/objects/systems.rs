//! 空气墙对象操作与固定行回显；所有写入按稳定ID进入统一撤销栈。
use super::super::{helpers::selected_path_index, path_history::commit_path_change};
use super::{PanelState, components::*}; // 固定面板状态。
use crate::authoring::{BoundaryEditState, EditorAuthoringSession, next_author_id}; // 单一草稿与拖点状态。
use crate::resources::MapEditorState; // 编辑器状态。
use crate::types::EditorTool; // 墙编辑工具。
use crate::ui::theme as colors;
use crate::viewport::content_selection::{LayerCandidateState, LayerTarget}; // 与视口选择共用身份。
use bevy::input_focus::InputFocus; // 使用官方输入焦点。
use bevy::prelude::*; // 系统与交互。
use bevy::text::EditableText; // 中文输入。
use feathrise_map_content::{
    MAP_SCHEMA_VERSION, MAX_MAP_BOUNDARY_PATHS, MapBoundariesAsset, MapBoundaryPath,
    MapBoundaryRole,
}; // 新版显式墙类型。

/// 共用不随地图重开复位的身份源；新建边沿无需遍历整张地图的墙对象。
fn new_wall_id(session: &mut EditorAuthoringSession) -> String {
    next_author_id(session, "wall")
}
#[allow(clippy::too_many_arguments)]
pub(super) fn actions(
    buttons: Query<(&feathrise_editor_ui::EditorButtonClick, Option<&Action>, Option<&WallRow>), Changed<feathrise_editor_ui::EditorButtonClick>>, // 普通动作及对象行只在按钮内松开执行。
    mut names: Query<(Entity, &Interaction, &EditableText), With<WallName>>, // 官方文本值。
    mouse: Option<Res<ButtonInput<MouseButton>>>, // 点击字段外释放本面板焦点。
    mut focus: ResMut<InputFocus>,
    mut panel: ResMut<PanelState>, // 页码与输入身份。
    mut session: ResMut<EditorAuthoringSession>,
    mut editor: ResMut<MapEditorState>, // 当前草稿。
    mut edit: ResMut<BoundaryEditState>,
    mut candidates: ResMut<LayerCandidateState>, // 清除旧拖动与候选。
    selection_context: (Option<Res<crate::game_authoring::map_interaction::MapInteractionState>>, Option<Res<crate::viewport::modes::ViewportModeState>>, Option<Res<crate::ui::sidebars::MapEditorSidebarState>>),
) {
    if !editor.open {
        return;
    } // 隐藏编辑器不消费操作。
    let generation = selection_context.0.as_ref().map_or(0, |state| state.observed_generation);
    let filter = selection_context.1.as_ref().map_or_else(Default::default, |state| state.selection_filter(&editor));
    let in_properties = selection_context.2.as_ref().is_some_and(|sidebar| sidebar.right == crate::ui::sidebars::EditorRightSidebarTab::Scene);
    if mouse
        .as_ref()
        .is_some_and(|mouse| mouse.just_pressed(MouseButton::Left))
        && names
            .iter()
            .all(|(_, interaction, _)| *interaction != Interaction::Pressed)
        && focus.get().is_some_and(|entity| names.contains(entity))
    {
        focus.clear();
    } // 地图点击不被残留名称焦点吞掉。
    for (entity, interaction, _) in &mut names {
        // 一个输入框。
        if *interaction == Interaction::Pressed {
            focus.set(entity, bevy::input_focus::FocusCause::Pressed);
        } // 官方输入系统拥有键盘。
    }
    for (interaction, action, row) in &buttons {
        if !interaction.is_clicked() {
            continue;
        } // 悬停只由展示系统换色。
        if action.is_none() && row.is_none() {
            continue;
        } // 名称字段与其他UI不是墙对象动作。
        if edit.dragging() {
            editor.status = "请先松开控制点完成本次拖动".into();
            continue;
        } // 不丢弃未完成的拖动事务。
        if focus.get().is_some_and(|entity| names.contains(entity)) {
            focus.clear();
        } // 操作后键盘归还视口。
        if let Some(row) = row {
            let index = panel.page * 8 + row.0;
            let target = session.draft.as_ref().and_then(|draft| {
                draft
                    .boundaries
                    .as_ref()
                    .and_then(|doc| doc.paths.get(index))
                    .map(|path| (draft.map_id.clone(), path.id.clone()))
            }); // 固定索引O1。
            if let Some(target) = target {
                candidates.confirm_tree_author(LayerTarget::Boundary(target.1.clone()), Some(index), &session,
                    generation, filter);
                // 属性表单内选择另一墙仍发布完整身份，原字段根不因旧空下标被移回工具页。
                session.selected_boundary_path = Some(target); // 编辑目标独立于列表页码。
                edit.clear_drag();
                edit.selected_point = None; // 选另一墙不沿用旧点。
                if !in_properties {
                    editor.tool = EditorTool::Boundary; // 工具页选择沿用准备绘制；属性页选择不切顶部模块。
                }
            }
            continue;
        }
        let Some(action) = action else {
            continue;
        };
        let count = session
            .draft
            .as_ref()
            .and_then(|draft| draft.boundaries.as_ref())
            .map_or(0, |doc| doc.paths.len()); // 页码不遍历文档。
        match action {
            Action::Previous => {
                panel.page = panel.page.saturating_sub(1);
                continue;
            }
            Action::Next => {
                panel.page = (panel.page + 1).min(count.saturating_sub(1) / 8);
                continue;
            }
            _ => {}
        }
        if session.saving {
            editor.status = "保存期间暂不修改空气墙".into();
            continue;
        } // 保存快照不可同时改变。
        if matches!(action, Action::CreateInternal | Action::CreateOuter) {
            let Some(draft) = session.draft.as_ref() else {
                continue;
            };
            if !draft.manifest.is_data_driven() {
                editor.status = "当前地图不支持数据驱动空气墙".into();
                continue;
            }
            let outer = matches!(action, Action::CreateOuter); // 显式用途不依赖路径顺序。
            if outer
                && draft.boundaries.as_ref().is_some_and(|doc| {
                    doc.paths
                        .iter()
                        .any(|path| path.role == MapBoundaryRole::Outer)
                })
            {
                editor.status = "地图已有外框；请选中编辑，内部封路使用内部墙".into();
                continue;
            }
            if count >= MAX_MAP_BOUNDARY_PATHS {
                editor.status = "空气墙数量已达文件安全上限".into();
                continue;
            }
            let map = draft.map_id.clone(); // 在可变借用前冻结身份与高度。
            let bottom = draft.manifest.terrain.base_height - 10.0; // 默认覆盖地形下方。
            let top =
                draft.manifest.terrain.base_height + draft.manifest.terrain.height_scale + 50.0; // 默认覆盖地图高度。
            let id = new_wall_id(&mut session); // 地图修订与序列共同生成稳定ID。
            let path = MapBoundaryPath {
                id: id.clone(),
                label: if outer {
                    "地图外框"
                } else {
                    "内部空气墙"
                }
                .into(),
                role: if outer {
                    MapBoundaryRole::Outer
                } else {
                    MapBoundaryRole::Internal
                },
                enabled: true,
                visible: true,
                closed: false,
                points: vec![],
                bottom_y: bottom,
                top_y: top,
                thickness: 2.0,
            }; // 新墙由作者逐点绘制。
            if let Some(draft) = session.draft.as_mut() {
                if draft.boundaries.is_none() {
                    draft.boundary_catalog_registration_required = true;
                }
                draft.boundaries.get_or_insert_with(|| MapBoundariesAsset {
                    schema_version: MAP_SCHEMA_VERSION,
                    map_id: map.clone(),
                    paths: vec![],
                });
            } // 空地图直接使用新结构。
            match commit_path_change(
                &mut session,
                &mut editor,
                count,
                None,
                Some(path),
                "已新建空气墙；在编辑模式逐点点击地图",
            ) {
                Ok(()) => {
                    session.selected_boundary_path = Some((map, id.clone()));
                    candidates.confirm_tree_author(LayerTarget::Boundary(id), Some(count), &session, generation, filter);
                    panel.page = count / 8;
                    if !in_properties { editor.tool = EditorTool::Boundary; }
                    edit.clear_drag();
                    edit.selected_point = None;
                }
                Err(error) => editor.status = error,
            }
            continue;
        }
        let Some(index) = selected_path_index(&session) else {
            editor.status = "请先选择空气墙".into();
            continue;
        };
        let Some((map, before)) = session.draft.as_ref().and_then(|draft| {
            draft
                .boundaries
                .as_ref()
                .and_then(|doc| doc.paths.get(index))
                .map(|path| (draft.map_id.clone(), path.clone()))
        }) else {
            continue;
        }; // 只复制当前对象。
        let mut after = before.clone(); // 参数修改不持有全图快照。
        let result = match action {
            Action::Copy => {
                if count >= MAX_MAP_BOUNDARY_PATHS {
                    editor.status = "空气墙数量已达文件安全上限".into();
                    continue;
                }
                after.id = new_wall_id(&mut session);
                after.label = format!("{} 副本", before.label);
                after.role = MapBoundaryRole::Internal; // 外框副本是独立内部围挡。
                let id = after.id.clone();
                let result = commit_path_change(
                    &mut session,
                    &mut editor,
                    count,
                    None,
                    Some(after),
                    "已复制为空气墙独立对象",
                );
                if result.is_ok() {
                    session.selected_boundary_path = Some((map, id.clone()));
                    candidates.confirm_tree_author(LayerTarget::Boundary(id), Some(count), &session, generation, filter);
                    panel.page = count / 8;
                } // 显示新副本。
                result
            }
            Action::Delete => {
                let result = commit_path_change(
                    &mut session,
                    &mut editor,
                    index,
                    Some(before),
                    None,
                    "已删除空气墙，可撤销恢复",
                );
                if result.is_ok() {
                    session.selected_boundary_path = session.draft.as_ref().and_then(|draft| {
                        draft
                            .boundaries
                            .as_ref()
                            .and_then(|doc| {
                                doc.paths.get(index.min(doc.paths.len().saturating_sub(1)))
                            })
                            .map(|path| (draft.map_id.clone(), path.id.clone()))
                    });
                    if let Some((_, id)) = session.selected_boundary_path.as_ref() {
                        let selected_index = session.draft.as_ref().and_then(|draft| draft.boundaries.as_ref())
                            .map(|doc| index.min(doc.paths.len().saturating_sub(1)));
                        candidates.confirm_tree_author(LayerTarget::Boundary(id.clone()), selected_index, &session, generation, filter);
                    } else { candidates.clear_for_scene_tree(); }
                } // 删除后选择相邻对象。
                result
            }
            Action::Enabled => {
                after.enabled = !after.enabled;
                commit_path_change(
                    &mut session,
                    &mut editor,
                    index,
                    Some(before),
                    Some(after),
                    "已切换所选墙碰撞",
                )
            }
            Action::Visible => {
                after.visible = !after.visible;
                commit_path_change(
                    &mut session,
                    &mut editor,
                    index,
                    Some(before),
                    Some(after),
                    "已切换辅助线显示；碰撞设置不受影响",
                )
            }
            Action::Rename => {
                if panel.name_target.as_ref() != Some(&(map, before.id.clone())) {
                    editor.status = "名称编辑目标已变化，请重新输入".into();
                    continue;
                }
                let Some((_, _, input)) = names.iter().next() else {
                    continue;
                };
                let value: String = input
                    .value()
                    .to_string()
                    .trim()
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(64)
                    .collect(); // 收口空白与控制字符。
                if value.is_empty() {
                    editor.status = "空气墙名称不能为空".into();
                    continue;
                }
                after.label = value;
                commit_path_change(
                    &mut session,
                    &mut editor,
                    index,
                    Some(before),
                    Some(after),
                    "已更新空气墙名称",
                )
            }
            _ => continue,
        };
        if let Err(error) = result {
            editor.status = error;
        } // 统一回显身份冲突。
        edit.clear_drag();
        edit.selected_point = None; // 对象动作结束旧控制点事务。
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync(
    editor: Res<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    mut panel: ResMut<PanelState>, // 仅读草稿，不逐帧复制。
    mut focus: ResMut<InputFocus>,
    mut names: Query<
        (Entity, &mut EditableText, Option<&mut BorderColor>),
        (With<WallName>, Without<WallRow>, Without<Action>),
    >, // 保留官方输入实体及编辑中的文本。
    mut rows: Query<(
        Entity,
        &WallRow,
        &Interaction,
        &mut Node,
        &mut BackgroundColor,
        &mut BorderColor,
    )>, // 固定八行。
    mut labels: Query<(&WallLabel, &mut Text), Without<WallTitle>>, // 与标题查询互斥。
    mut titles: Query<&mut Text, With<WallTitle>>,                  // 数量与页码。
    mut buttons: Query<(&Interaction, &mut BackgroundColor), (With<Action>, Without<WallRow>)>, // 独立颜色写者。
    mut controls: Query<(&WallControls, &mut Node), Without<WallRow>>,
) {
    if !editor.open {
        return;
    }
    let draft = session.draft.as_ref();
    let document = draft.and_then(|draft| draft.boundaries.as_ref()); // O1引用。
    let target = session.selected_boundary_path.clone(); // 列表和视口共享稳定选择。
    let target_changed = panel.name_target != target;
    if target_changed || session.is_changed() {
        let selected = target.as_ref().and_then(|(map, id)| {
            draft
                .filter(|draft| draft.map_id == *map)
                .and_then(|_| document)
                .and_then(|doc| {
                    doc.paths
                        .iter()
                        .enumerate()
                        .find(|(_, path)| path.id == *id)
                })
        }); // 仅在变更边沿解析稳定ID，显隐回显不逐帧扫描整图墙体。
        if target_changed {
            if let Some((index, _)) = selected {
                let page = index / 8;
                if panel.page != page {
                    panel.page = page;
                }
            }
        } // 视口选到其他页的墙时显示其所在页；手动翻页不强制跳回。
        let selection_valid = selected.is_some();
        if panel.selection_valid != selection_valid {
            panel.selection_valid = selection_valid;
        }
        if target_changed
            || names
                .iter()
                .all(|(entity, _, _)| focus.get() != Some(entity))
        {
            let label = selected.map_or("", |(_, path)| path.label.as_str());
            for (_, mut input, _) in &mut names {
                if target_changed || input.value().to_string() != label {
                    crate::ui::text_input::replace_single_line_value(&mut input, label, 64);
                }
            }
        } // 名称仍显式提交，不因悬停或样式更新制造历史。
        if target_changed {
            panel.name_target = target;
        }
    }
    if !panel.selection_valid && focus.get().is_some_and(|entity| names.contains(entity)) {
        focus.clear(); // 删除或切图收起字段时，不保留不可见名称框的键盘焦点。
    }
    let count = document.map_or(0, |doc| doc.paths.len());
    let page = panel.page.min(count.saturating_sub(1) / 8);
    if panel.page != page {
        panel.page = page;
    } // 删除末页后的夹取。
    for mut title in &mut titles {
        let value = if draft.is_none() {
            "空气墙 · 等待作者数据".into()
        } else if count == 0 {
            "暂无空气墙，先新建内部墙或外框。".into()
        } else if count <= 8 {
            format!("空气墙 · {count} 道")
        } else {
            format!(
                "空气墙 · {count} 道 · {}/{} 页",
                panel.page + 1,
                count.div_ceil(8)
            )
        };
        if **title != value {
            **title = value;
        }
    } // 小字符串不会遍历墙。
    for (controls, mut node) in &mut controls {
        let visible = match controls {
            WallControls::Pagination => count > 8,
            WallControls::Selection => panel.selection_valid,
        };
        let display = if visible {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    for (entity, row, interaction, mut node, mut background, mut border) in &mut rows {
        let path = document.and_then(|doc| doc.paths.get(panel.page * 8 + row.0)); // 仅访问当前页。
        let display = if path.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        let selected = path.is_some_and(|path| {
            session
                .selected_boundary_path
                .as_ref()
                .is_some_and(|(map, id)| {
                    draft.is_some_and(|draft| *map == draft.map_id) && *id == path.id
                })
        }); // 稳定ID高亮。
        let color = if selected {
            colors::SELECT_BG
        } else if *interaction != Interaction::None {
            colors::SELECT_BG_HOVER
        } else {
            colors::SLOT_BG
        }; // 单一系统负责悬停。
        if background.0 != color {
            background.0 = color;
        }
        let outline = BorderColor::all(if selected {
            colors::COPPER_BORDER
        } else {
            colors::COPPER_BORDER_WEAK
        });
        if *border != outline {
            *border = outline;
        }
        if let Some(path) = path {
            for (label, mut text) in &mut labels {
                if label.0 == entity {
                    let value = format!(
                        "{} · {} · {}点\n碰撞{} / 辅助线{}",
                        path.label,
                        if path.role == MapBoundaryRole::Outer {
                            "外框"
                        } else if path.closed {
                            "围挡"
                        } else {
                            "折线"
                        },
                        path.points.len(),
                        if path.enabled { "开" } else { "关" },
                        if path.visible { "开" } else { "关" }
                    );
                    if **text != value {
                        **text = value;
                    }
                }
            }
        }
    }
    for (interaction, mut color) in &mut buttons {
        let background = match interaction {
            Interaction::None => colors::BUTTON_DEV_SMALL_NORMAL,
            Interaction::Hovered => colors::BUTTON_DEV_SMALL_HOVERED,
            Interaction::Pressed => colors::BUTTON_DEV_SMALL_PRESSED,
        };
        if color.0 != background {
            color.0 = background;
        }
    } // 不与全局Button样式竞争。
    for (entity, _, border) in &mut names {
        if let Some(mut border) = border {
            let outline = BorderColor::all(if focus.get() == Some(entity) {
                colors::INPUT_ACTIVE_BORDER
            } else {
                colors::INPUT_BORDER
            });
            if *border != outline {
                *border = outline;
            }
        }
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests; // 真实面板回归。
