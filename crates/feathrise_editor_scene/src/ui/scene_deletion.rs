//! 场景目录选中项的删除保护门、二次确认和当前图安全卸载编排。

use feathrise_editor_ui::EditorButtonClick;
use bevy::prelude::*; // 按钮交互、当前场景与资源查询。
use feathrise_scene_content::{ActiveDynamicScene, SceneCatalogAsset, SceneContentRegistry}; // 当前场景身份与正式目录入口。
#[cfg(test)]
use feathrise_scene_content::SceneId;
use feathrise_scene_core::state::ScenePhase; // 专用场景和统一动态场景需要还原真实SceneId。

use crate::authoring::{DraftSaveTask, scene_deletion_request}; // 复用正式保存忙碌门与受限删除请求构造。
use crate::components::SceneEditorActionButton; // 删除按钮使用稳定结构化动作marker。
use crate::scene_identity::active_gameplay_scene_id; // 删除选中项与当前加载场景必须通过真实SceneId比较。
use crate::resources::{
    SceneBrowserState, SceneCreationTask, SceneDeletionTask, SceneEditorNumericScrubMouseGuard,
    SceneEditorState, SceneFolderPersistence,
}; // 创建、删除、鼠标所有权和编辑会话不能并发改写目录。
use crate::types::{EditorAction, SceneBrowserItem, SceneDeletionMode, SceneDeletionPlan}; // 确认后冻结目标、方式和安全当前图。

/// 前台拒绝只附加需处理回执；确认已冻结时始终描述原对象，不跟随后来高亮项。
fn record_deletion_warning(
    editor: &mut SceneEditorState,
    request: Option<&crate::types::SceneDeletionRequest>,
) {
    let target = if let Some(request) = request {
        crate::types::EditorStatusTarget::new(request.scene_id.as_str(), &request.display_name)
    } else {
        let id = editor
            .scene_list_selection_id
            .as_ref()
            .map_or("", |id| id.as_str());
        crate::types::EditorStatusTarget::new(id, id)
    };
    editor.record_operation_result(
        crate::types::EditorStatusOperation::SceneDelete,
        &target,
        crate::types::EditorStatusKind::Warning,
    );
}

/// 检查当前会话是否正在执行会改写目录或作者文件的互斥事务。
fn mutation_is_busy(
    editor: &SceneEditorState,    // 已登记但尚未冻结快照的保存也是目录写入意图。
    creation: &SceneCreationTask, // 新建场景最后会原子改写同一catalog。
    save: &DraftSaveTask,       // 在途保存仍可能写当前私有目录。
    folder_persistence: &SceneFolderPersistence, // 归类保存与删除归属改写使用同一文件。
) -> bool {
    editor.save_requested // 等待植被编译的保存请求尚未出现Task，仍不能与删除交叉。
        || creation.task.is_some() // 已经进入IO线程的新建事务。
        || creation.pending_after_save.is_some() // 保存后创建仍持有目录写入意图。
        || save.task.is_some() // 当前草稿保存尚未完整采用。
        || folder_persistence.busy() // 删除必须从最新已落盘归类基线准备联合事务。
}

/// 打开目录选中场景的二次确认框；单击行本身不要求先进入该场景。
#[allow(clippy::too_many_arguments)] // 显式资源参数让目录、场景和互斥任务门均可独立审查。
fn open_deletion_confirmation(
    editor: &mut SceneEditorState,    // 写入状态与关闭可能残留的新建弹框。
    deletion: &mut SceneDeletionTask, // 保存冻结确认快照。
    creation: &SceneCreationTask,     // 防止与目录创建事务并发。
    save: &DraftSaveTask,           // 防止与作者保存事务并发。
    folder_persistence: &SceneFolderPersistence, // 防止与归类保存事务并发。
    registry: Option<&SceneContentRegistry>, // 获取当前热加载目录句柄。
    catalogs: &Assets<SceneCatalogAsset>, // 获取作者顺序、名称和精确私有文件夹。
) {
    if editor.dirty {
        editor.status = "当前场景有未保存修改；请先保存或撤销后再删除".to_owned(); // 用户确认的禁止规则。
        record_deletion_warning(editor, None);
        return;
    }
    if mutation_is_busy(editor, creation, save, folder_persistence) {
        editor.status = "场景创建或保存事务尚未完成，暂时不能删除场景".to_owned(); // 防止并发改写catalog或私有目录。
        record_deletion_warning(editor, None);
        return;
    }
    if deletion.busy() {
        editor.status = "已有场景删除确认或事务正在执行".to_owned(); // single-flight不允许新目标覆盖旧目标。
        record_deletion_warning(editor, deletion.confirmation.as_ref());
        return;
    }
    if editor.pending_scene_transition.is_some() || editor.scene_transition_in_progress {
        editor.status = "场景切换尚未完成，请进入目标场景后再删除".to_owned(); // 不删除仍可能被LoadingScene使用的目录。
        record_deletion_warning(editor, None);
        return;
    }
    let Some(selected_scene_id) = editor.scene_list_selection_id.as_ref() else {
        editor.status = "尚未选择可删除场景".to_owned(); // 空选择不猜测任意目录。
        record_deletion_warning(editor, None);
        return;
    };
    let Some(registry) = registry else {
        editor.status = "场景目录注册表尚未就绪".to_owned(); // 没有权威目录句柄时不构造路径。
        record_deletion_warning(editor, None);
        return;
    };
    let Some(catalog) = catalogs.get(&registry.catalog) else {
        editor.status = "场景目录仍在加载，暂时不能删除".to_owned(); // 等待AssetServer完整资产。
        record_deletion_warning(editor, None);
        return;
    };
    let Some(entry) = catalog
        .scenes
        .iter()
        .find(|entry| &entry.id == selected_scene_id)
    else {
        editor.status = format!("场景{selected_scene_id}已不在正式目录中"); // 外部热更后的旧选择不能继续。
        record_deletion_warning(editor, None);
        return;
    };
    let request = match scene_deletion_request(entry) {
        Ok(request) => request, // 仅编辑器场景的精确私有目录能够进入确认框。
        Err(error) => {
            editor.status = error; // HN和自定义正式目录直接显示保护原因。
            record_deletion_warning(editor, None);
            return;
        }
    };
    editor.new_scene_dialog_open = false; // 两种模态作者事务不能同时显示。
    editor.status = format!(
        "等待确认：为场景“{}” [{}]选择直接删除或放入回收站",
        request.display_name, request.scene_id
    ); // 底栏和弹框共同显示冻结目标。
    deletion.confirmation = Some(request); // 仅此写入决定确认框显隐。
}

/// 确认后只登记后台回收计划；当前图须先关闭或切走，其他场景在原稳定现场回收。
#[allow(clippy::too_many_arguments)] // 再次核对全部身份与互斥门，防止弹框打开后状态发生变化。
fn confirm_deletion(
    editor: &mut SceneEditorState,               // 核对草稿与活动场景身份，不隐式切换工程场景。
    deletion: &mut SceneDeletionTask,            // 确认目标转为后台回收计划。
    creation: &SceneCreationTask,                // 确认期间新建事务可能开始，必须再次检查。
    save: &DraftSaveTask,                      // 确认期间保存可能开始，必须再次检查。
    folder_persistence: &SceneFolderPersistence, // 确认期间归类保存可能开始，必须再次检查。
    scene: ScenePhase,                          // 取得目标与当前真实玩法图的关系。
    active_dynamic: &ActiveDynamicScene,         // 动态场景真实身份再次核对。
    idle_menu: bool,                           // 关闭最后一个标签后，稳定空工作台也是合法的回收起点。
    registry: Option<&SceneContentRegistry>,     // 确认当前工程的权威目录句柄。
    catalogs: &Assets<SceneCatalogAsset>,        // 复核待删除目标仍在同一目录资产中。
    mode: SceneDeletionMode,                     // 明确点击的确认方式随计划冻结。
) {
    let Some(request) = deletion.confirmation.clone() else {
        return; // 没有冻结确认目标时忽略迟到按钮事件。
    };
    if editor.dirty {
        editor.status = "场景在确认期间产生了未保存修改，删除已阻止".to_owned(); // 不丢任何作者变更。
        record_deletion_warning(editor, Some(&request));
        deletion.confirmation = None; // 状态变化后必须重新发起并审阅目标。
        return;
    }
    if mutation_is_busy(editor, creation, save, folder_persistence) || deletion.task.is_some() {
        editor.status = "场景创建、保存或删除事务尚未完成".to_owned(); // 文件事务保持互斥。
        record_deletion_warning(editor, Some(&request));
        return;
    }
    if editor.pending_scene_transition.is_some() || editor.scene_transition_in_progress {
        editor.status = "场景切换在确认期间开始，删除已取消".to_owned(); // 不与另一个进入请求共享场景身份。
        record_deletion_warning(editor, Some(&request));
        deletion.confirmation = None; // 丢弃旧快照要求重新操作。
        return;
    }
    if editor.scene_list_selection_id.as_ref() != Some(&request.scene_id) {
        editor.status = "场景目录选中项在确认期间发生变化，删除已取消".to_owned(); // 按钮永远作用于用户眼前高亮的项。
        record_deletion_warning(editor, Some(&request));
        deletion.confirmation = None; // 旧弹框快照不能跟随新高亮偷换目标。
        return;
    }
    let current_scene_id = active_gameplay_scene_id(scene, active_dynamic);
    if current_scene_id.is_none() && !idle_menu {
        editor.status = "当前世界仍在加载或交接，请稍后重试删除".to_owned(); // 无玩法身份不等于安全空世界。
        record_deletion_warning(editor, Some(&request));
        deletion.confirmation = None; // 场景身份失效后要求重新选择。
        return;
    }
    if editor.selected_scene_id != current_scene_id {
        editor.status = "当前作者草稿与运行场景身份不一致，删除已阻止".to_owned(); // 不在身份半切换状态启动文件事务。
        record_deletion_warning(editor, Some(&request));
        deletion.confirmation = None; // 等编辑器重新对齐后再操作。
        return;
    }
    let Some(registry) = registry else {
        editor.status = "场景目录注册表在确认期间失效".to_owned(); // 无权威句柄时不冻结启动计划。
        record_deletion_warning(editor, Some(&request));
        return;
    };
    let Some(catalog) = catalogs.get(&registry.catalog) else {
        editor.status = "场景目录在确认期间重新加载，请稍后重试".to_owned(); // 不使用丢失的目录快照。
        record_deletion_warning(editor, Some(&request));
        return;
    };
    let Some(entry) = catalog.scenes.iter().find(|entry| entry.id == request.scene_id) else {
        editor.status = format!("场景{}已不在正式目录中", request.scene_id); // 热更后的旧弹框不保留删除权。
        record_deletion_warning(editor, Some(&request));
        deletion.confirmation = None; // 目标消失后结束模态阶段。
        return;
    };
    let latest_request = match scene_deletion_request(entry) {
        Ok(latest_request) => latest_request, // 重新核对场景前缀与精确私有目录。
        Err(error) => {
            editor.status = error; // 目录热更为受保护形状后立即拒绝。
            record_deletion_warning(editor, Some(&request));
            deletion.confirmation = None; // 需要作者重新审阅目标。
            return;
        }
    };
    if latest_request != request {
        editor.status = "场景名称或私有目录在确认期间发生变化，请重新操作".to_owned(); // 二次确认必须对应同一份可见数据。
        record_deletion_warning(editor, Some(&request));
        deletion.confirmation = None; // 不让旧弹框授权新目录值。
        return;
    }
    let deleting_current = current_scene_id.as_ref() == Some(&request.scene_id); // 只有目标正被世界使用时才需要先走LoadingScene。
    if deleting_current {
        editor.status = "请先关闭该场景标签或切换到其他场景，再执行删除".to_owned();
        record_deletion_warning(editor, Some(&request));
        return; // 与现有标签占用保护一致，不再依赖固定托底场景。
    }
    let safe_active_scene_id = current_scene_id.clone(); // 已退出的目标在当前稳定世界或空工作台回收。
    deletion.confirmation = None; // 确认框立即隐藏并停止接收重复点击。
    deletion.pending_start = Some(SceneDeletionPlan {
        request: request.clone(), // 目标身份与目录快照保持不变。
        mode,
        safe_active_scene_id: safe_active_scene_id.clone(), // Tasks阶段只在该场景真正运行时启动IO。
    });
    {
        editor.status = format!(
            "已确认{}场景“{}” [{}]；正在等待后台安全校验",
            mode.label(),
            request.display_name, // 回显作者可读名称。
            request.scene_id        // 同名场景通过稳定ID区分。
        ); // 按钮帧只登记计划，文件IO留给Tasks阶段。
    }
}

/// 消费删除相关静态按钮；普通工具按钮系统只负责样式和显式跳过这些动作。
#[allow(clippy::too_many_arguments)] // 删除确认必须同时看到目录、场景和互斥事务证据。
pub(crate) fn handle_scene_deletion_buttons(
    buttons: Query<
        (
            &EditorButtonClick,
            &SceneEditorActionButton,
            Option<&super::scene_sidebar::SceneSidebarActionBinding>,
        ),
        Changed<EditorButtonClick>,
    >, // 侧栏删除不能采用旧显示目标的按下。
    numeric_scrub_mouse: Res<SceneEditorNumericScrubMouseGuard>, // 数值拖动和弦结束前丢弃删除按钮边沿。
    current_scene: Res<State<ScenePhase>>,                      // 判断当前真正玩法场景。
    active_dynamic: Res<ActiveDynamicScene>,                     // DynamicSceneRoot的实际SceneId。
    registry: Option<Res<SceneContentRegistry>>,                 // 当前正式目录注册表。
    catalogs: Res<Assets<SceneCatalogAsset>>,                    // 目录显示名与私有文件夹。
    mut editor: ResMut<SceneEditorState>,                        // 状态、脏标记和切换场景请求。
    mut deletion: ResMut<SceneDeletionTask>,                     // 确认、切换场景等待与任务槽。
    creation: Res<SceneCreationTask>,                            // 与新建事务互斥。
    save: Res<DraftSaveTask>,                                  // 与作者保存事务互斥。
    browser: Res<SceneBrowserState>, // 上下文“删除”只有高亮场景时才进入回收流程。
    folder_persistence: Res<SceneFolderPersistence>, // 删除与归类single-flight互斥。
    mut sidebar_context: super::scene_sidebar::SceneSidebarInputContext,
    documents: Option<Res<super::scene_documents::SceneDocuments>>,
    world: crate::resources::SceneEditorWorldSession,
) {
    if numeric_scrub_mouse.blocked() {
        return; // 系统仍每帧运行以推进Changed tick，但所有权帧不得执行或延迟删除动作。
    }
    for (interaction, button, binding) in &buttons {
        if !interaction.is_clicked() {
            continue; // 悬停和松开只由公共按钮样式系统处理。
        }
        if matches!(
            button.0,
            EditorAction::DeleteScene | EditorAction::ConfirmDeleteScene | EditorAction::ConfirmDeleteScenePermanently
        ) && editor
            .scene_list_selection_id
            .as_ref()
            .is_some_and(|map| documents.as_ref().is_some_and(|docs| docs.contains(map)))
        {
            editor.status = "请先关闭该场景标签，再从目录删除场景".into();
            deletion.confirmation = None;
            continue; // 后台标签的草稿和引用租约仍拥有目录，不能只按当前图dirty删除它。
        }
        match button.0 {
            EditorAction::DeleteScene => {
                if browser.selection_count() > 1 || browser.selection_pending {
                    continue; // 混合选集只支持批量整理，绝不隐式删除其中一个焦点场景。
                }
                if binding.is_some_and(|binding| {
                    !editor.open
                        || !sidebar_context.visible()
                        || !binding.matches(browser.selected_item.as_ref())
                }) {
                    continue; // 与文件夹原入口保持同一显示身份门；不改变二次确认或回收事务。
                }
                if !matches!(browser.selected_item.as_ref(), Some(SceneBrowserItem::Scene(_))) {
                    continue; // 文件夹删除由scene_browser的空目录确认流程独占处理。
                }
                if binding.is_some()
                    && let Some(SceneBrowserItem::Scene(scene_id)) = browser.selected_item.as_ref()
                {
                    editor.scene_list_selection_id = Some(scene_id.clone()); // 通过显示身份门后同步原回收入口，不能沿用另一个场景的兼容选择。
                }
                open_deletion_confirmation(
                    &mut editor,         // 写入确认状态。
                    &mut deletion,       // 冻结目标快照。
                    &creation,           // 检查创建互斥。
                    &save,               // 检查保存互斥。
                    &folder_persistence, // 检查归类写入互斥。
                    registry.as_deref(), // 读取权威句柄表。
                    &catalogs,           // 读取完整目录资产。
                );
                if binding.is_some()
                    && let Some(feedback) = sidebar_context.feedback.as_deref_mut()
                {
                    feedback.record(&editor, &browser, registry.as_deref(), deletion.busy());
                }
            }
            EditorAction::ConfirmDeleteScene | EditorAction::ConfirmDeleteScenePermanently => confirm_deletion(
                &mut editor,          // 登记回退切换场景。
                &mut deletion,        // 把确认转成等待事务。
                &creation,            // 再次检查创建互斥。
                &save,                // 再次检查保存互斥。
                &folder_persistence,  // 再次检查归类写入互斥。
                *current_scene.get(), // 再次核对当前场景。
                &active_dynamic,      // 再次核对动态SceneId。
                world.idle_menu(),   // 空工作台必须同时满足无加载和无驻留交接。
                registry.as_deref(),  // 确认工程目录句柄存在。
                &catalogs,            // 确认待删除目标的目录入口仍有效。
                if button.0 == EditorAction::ConfirmDeleteScenePermanently {
                    SceneDeletionMode::Permanent
                } else {
                    SceneDeletionMode::Recycle
                },
            ),
            EditorAction::CancelDeleteScene => {
                if let Some(request) = deletion.confirmation.take() {
                    editor.status = format!("已取消删除场景{}", request.scene_id); // 取消没有任何文件副作用。
                }
            }
            _ => {} // 其余按钮由现有作者系统处理。
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap; // 最小注册表不需要加载任何私有场景句柄。

    use feathrise_scene_content::{SCENE_SCHEMA_VERSION, SceneCatalogEntry}; // 使用正式目录格式验证删除目标来源。

    use super::*; // 纯身份测试不启动窗口、AssetServer或文件事务。

    #[test]
    fn save_intent_blocks_deletion_before_the_io_task_exists() {
        let editor = SceneEditorState {
            save_requested: true, // 模拟保存正等待植被后台编译，尚未创建DraftSaveTask。
            ..default()
        };
        assert!(mutation_is_busy(
            &editor,
            &SceneCreationTask::default(),
            &DraftSaveTask::default(),
            &SceneFolderPersistence::default(),
        )); // 删除不能越过已登记的catalog写入意图。
    }

    #[test]
    fn delete_button_freezes_the_directory_selection_instead_of_the_current_scene() {
        let current = SceneId::parse("HN-01").expect("测试当前SceneId合法");
        let target = SceneId::parse("SCENE-target").expect("测试目标SceneId合法");
        let target_entry = SceneCatalogEntry {
            id: target.clone(),                    // 目录高亮指向可删除编辑器场景。
            display_name: "待删除场景".to_owned(), // 确认框必须显示该名称。
            folder: format!("scenes/{target}"),       // 满足精确私有目录保护规则。
            published: true,                       // 是否发布不改变场景私有目录的删除语义。
            placements_glb: false,
            surface_path: None,                    // 测试不加载私有内容。
            vegetation_path: None,                 // 测试不加载私有内容。
            water_path: None,                      // 测试不加载私有内容。
            characters_path: None,                 // 旧目录保持角色来源缺席。
            boundary_path: None,                   // 测试不加载私有内容。
        };
        let mut catalogs = Assets::<SceneCatalogAsset>::default();
        let catalog_handle = catalogs.add(SceneCatalogAsset {
            schema_version: SCENE_SCHEMA_VERSION, // 使用当前公共目录版本。
            scenes: vec![target_entry],           // 只需让高亮目标存在于权威目录。
        });
        let registry = SceneContentRegistry {
            catalog: catalog_handle, // 确认门从正式注册表句柄取得目录。
            revision: 1,             // 模拟已经原子发布的目录修订。
            scenes: BTreeMap::new(),   // 打开弹框不要求目标私有资产已经加载。
            last_error: None,        // 当前目录没有校验失败。
        };
        let mut editor = SceneEditorState {
            selected_scene_id: Some(current), // 实际仍停留并编辑受保护的HN-01。
            scene_list_selection_id: Some(target.clone()), // 目录行独立高亮另一个场景。
            ..default()
        };
        let mut deletion = SceneDeletionTask::default();

        open_deletion_confirmation(
            &mut editor,
            &mut deletion,
            &SceneCreationTask::default(),
            &DraftSaveTask::default(),
            &SceneFolderPersistence::default(),
            Some(&registry),
            &catalogs,
        );

        assert_eq!(
            deletion
                .confirmation
                .as_ref()
                .map(|request| &request.scene_id),
            Some(&target)
        ); // 删除目标来自目录高亮，而不是当前HN-01身份。
        assert_eq!(
            editor.selected_scene_id.as_ref().map(SceneId::as_str),
            Some("HN-01")
        ); // 打开确认框不会切换场景。
    }

    #[test]
    fn closed_last_scene_can_be_confirmed_in_an_idle_workspace_but_not_during_loading() {
        let target = SceneId::parse("SCENE-closed-last-tab").unwrap();
        let mut catalogs = Assets::<SceneCatalogAsset>::default();
        let catalog = catalogs.add(SceneCatalogAsset {
            schema_version: SCENE_SCHEMA_VERSION,
            scenes: vec![SceneCatalogEntry {
                id: target.clone(), display_name: "555".into(), folder: format!("scenes/{target}"), published: true,
                placements_glb: false,
                surface_path: None, vegetation_path: None, water_path: None, characters_path: None, boundary_path: None,
            }],
        });
        let registry = SceneContentRegistry { catalog, ..default() };
        for mode in [SceneDeletionMode::Recycle, SceneDeletionMode::Permanent] {
            for (scene, idle, allowed) in [(ScenePhase::Idle, true, true),
                (ScenePhase::Idle, false, false), (ScenePhase::Loading, false, false)] {
                let mut editor = SceneEditorState { open: true, scene_list_selection_id: Some(target.clone()), ..default() };
                let mut deletion = SceneDeletionTask::default();
                open_deletion_confirmation(&mut editor, &mut deletion, &SceneCreationTask::default(),
                    &DraftSaveTask::default(), &SceneFolderPersistence::default(), Some(&registry), &catalogs);
                assert!(deletion.confirmation.is_some());
                confirm_deletion(&mut editor, &mut deletion, &SceneCreationTask::default(),
                    &DraftSaveTask::default(), &SceneFolderPersistence::default(), scene, &ActiveDynamicScene::default(),
                    idle, Some(&registry), &catalogs, mode);
                assert_eq!(deletion.pending_start.is_some(), allowed, "{scene:?}: {}", editor.status);
                if let Some(plan) = &deletion.pending_start {
                    assert_eq!(plan.request.scene_id, target);
                    assert_eq!(plan.safe_active_scene_id, None);
                    assert_eq!(plan.mode, mode, "确认方式必须与等待计划一起冻结");
                }
                assert!(editor.pending_scene_transition.is_none(), "删除已关闭的场景不应加载HN-01或重新打开目标");
            }
        }
    }
}
