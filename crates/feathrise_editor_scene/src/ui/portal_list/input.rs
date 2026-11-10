//! 传送双列表的目标身份收口、点击选择和当前源点删除输入。

use feathrise_editor_ui::EditorButtonClick;
use bevy::prelude::*; // Changed交互、资产集合与资源系统参数。
use feathrise_scene_content::{
    SceneContentRegistry, SceneId, SceneManifestAsset, ScenePortalRecord, ScenePortalTarget, ScenePortalsAsset,
}; // 目标场景清单、传送图和稳定引用使用公共作者格式。

use crate::authoring::{EditorAuthoringSession, EditorCommand}; // 删除也必须进入当前作者会话统一撤销栈。
use crate::components::{
    SceneEditorActionButton, SceneEditorCurrentPortalRow, SceneEditorTargetPortalRow,
}; // 两类动态行只通过结构化marker提交输入。
use crate::resources::{SceneBrowserState, SceneEditorNumericScrubMouseGuard, SceneEditorState}; // 场景树目标、统一鼠标门和选择会话态。
use crate::types::{EditorAction, EditorTool}; // 只消费传送工具、名称和删除动作。
use crate::viewport::SelectedPortalViewportState; // 当前列表单击标记、双击定位共享稳定点击身份。

use super::helpers::{
    PortalDisplayContext, TargetPortalDocuments, current_portal_row_text, selected_target_scene,
    target_exists, target_portal_documents, default_spawn_row_text, target_portal_row_text,
}; // 输入系统复用纯解析和用户可读文案。

/// 只按稳定ID从当前场景数组移出一项，并保留原下标供精确撤销。
fn take_portal_by_id(
    portals: &mut Vec<ScenePortalRecord>,
    portal_id: &str,
) -> Option<(usize, ScenePortalRecord)> {
    let index = portals.iter().position(|portal| portal.id == portal_id)?;
    Some((index, portals.remove(index))) // 标签允许重名，删除绝不按显示文字猜测。
}

/// 目标收口只比较与传送选择有关的小型身份，视口光标每帧变化不会触发完整列表扫描。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PortalSelectionSignature {
    target_scene_id: Option<SceneId>, // 场景树高亮场景决定唯一目标场景。
    portal_revision: u64,         // 当前草稿传送结构换代后重新校验稳定ID。
    catalog_revision: u64,        // 目录热更新可能移除目标场景或替换句柄。
    selected_current_portal_id: Option<String>, // 当前源点删除或撤销后需要清理旧高亮。
    portal_target: Option<ScenePortalTarget>, // 具体目标端点变化后只校验该一项。
}

/// 构造传送选择专用的常数大小签名。
fn selection_signature(
    editor: &SceneEditorState,          // 读取当前两类选择，不复制无关编辑器字段。
    browser: &SceneBrowserState,        // 读取场景树当前高亮项。
    session: &EditorAuthoringSession, // 读取传送专用修订。
    registry: Option<&SceneContentRegistry>, // 读取目录公开修订。
) -> PortalSelectionSignature {
    PortalSelectionSignature {
        target_scene_id: selected_target_scene(browser).cloned(), // 根或目录选择稳定映射为None。
        portal_revision: session.portal_revision, // 地形、水域和相机变化不会污染该签名。
        catalog_revision: registry.map_or(0, |registry| registry.revision), // 目录未就绪使用零代次。
        selected_current_portal_id: editor.selected_current_portal_id.clone(), // 仅复制一个稳定ID。
        portal_target: editor.portal_target.clone(), // 仅复制场景ID和可选传送点ID。
    }
}

/// 场景树、草稿或目标资产变化时清理已经失效的当前源点与目标落点。
pub(crate) fn reconcile_portal_selection(
    mut editor: ResMut<SceneEditorState>, // 只在真实失效或目标场景变化时写两类选择。
    browser: Res<SceneBrowserState>,      // 场景树高亮不等于当前已进入场景。
    session: Res<EditorAuthoringSession>, // 当前草稿是当前列表唯一权威源。
    registry: Option<Res<SceneContentRegistry>>, // 目标场景必须仍位于正式目录。
    manifests: Res<Assets<SceneManifestAsset>>, // 默认出生点有效性依赖清单Ready。
    portal_assets: Res<Assets<ScenePortalsAsset>>, // 具体目标ID有效性依赖目标传送图Ready。
    mut last_signature: Local<Option<PortalSelectionSignature>>, // 稳定帧短路完整ID查找。
) {
    if !editor.open {
        *last_signature = None; // 再次打开时按当时完整热加载状态重新校验一次。
        return;
    }
    let signature = selection_signature(&editor, &browser, &session, registry.as_deref()); // 只构造常数大小身份。
    let asset_changed = manifests.is_changed() || portal_assets.is_changed(); // 目标文档加载和热更需要绕过签名早退。
    if last_signature.as_ref() == Some(&signature) && !asset_changed {
        return; // 稳定帧不扫描当前或目标场景的完整传送数组。
    }

    let target_scene_id = selected_target_scene(&browser).cloned(); // 后续写editor时不继续借用browser内部值。
    if let Some(selected_id) = editor.selected_current_portal_id.as_deref() {
        let current_exists = session.draft.as_ref().is_some_and(|draft| {
            draft
                .portals
                .portals
                .iter()
                .any(|portal| portal.id == selected_id)
        }); // 只在传送修订或选择变化时线性验证一项。
        if !current_exists {
            editor.selected_current_portal_id = None; // 撤销或切换场景后不保留悬空当前行高亮。
        }
    }

    let Some(target_scene_id) = target_scene_id.as_ref() else {
        if editor.portal_target.take().is_some() && editor.tool == EditorTool::Portal {
            editor.status = "目标场景选择已清除；请先在场景页高亮一个场景".to_owned(); // 根、目录或空选择都有明确恢复步骤。
        }
        *last_signature = Some(selection_signature(
            &editor,             // 保存清理后的最终选择身份。
            &browser,            // 场景树当前仍没有目标场景。
            &session,            // 当前草稿修订保持不变。
            registry.as_deref(), // 目录修订继续参与下一次比较。
        ));
        return;
    };

    let documents = target_portal_documents(
        target_scene_id,       // 只解析场景树当前高亮的一个场景。
        &session,            // 同图目标优先使用未保存草稿。
        registry.as_deref(), // 其他场景通过正式目录句柄读取。
        &manifests,          // 默认出生点来自清单。
        &portal_assets,      // 具体端点来自目标传送图。
    );
    let target_invalid = editor.portal_target.as_ref().is_some_and(|target| {
        target.scene_id != *target_scene_id
            || matches!(
                &documents,
                TargetPortalDocuments::Ready { .. } | TargetPortalDocuments::Missing
            ) && !target_exists(target, &documents) // Loading期间保留选择，Ready或Missing才作确定性收口。
    });
    if target_invalid {
        editor.portal_target = None; // 热更新删除目标点后禁止继续放置运行时必失败引用。
        if editor.tool == EditorTool::Portal {
            editor.status = "目标场景的原到达点已失效，请重新选择".to_owned(); // 作者可直接从刷新后的列表恢复，不暴露SceneId。
        }
    }
    *last_signature = Some(selection_signature(
        &editor,             // 记录可能被清理后的最终目标。
        &browser,            // 目标场景保持同一稳定ID。
        &session,            // 传送修订保持当前值。
        registry.as_deref(), // 目录修订保持当前值。
    ));
}

/// 点击当前场景行只选择已有源点；点击目标场景行才设置新传送点的到达端点。
#[allow(clippy::too_many_arguments)] // 两套完整列表共享鼠标门，但分别验证草稿与目标场景资产。
pub(crate) fn handle_portal_list_row_clicks(
    navigation: super::navigation::PortalSidebarNavigation,
    current_rows: Query<(&EditorButtonClick, &SceneEditorCurrentPortalRow), Changed<EditorButtonClick>>, // 当前源点松开点击。
    target_rows: Query<(&EditorButtonClick, &SceneEditorTargetPortalRow), Changed<EditorButtonClick>>, // 目标落点松开点击。
    numeric_scrub_mouse: Res<SceneEditorNumericScrubMouseGuard>, // 三键数值和弦及release帧独占鼠标。
    mut editor: ResMut<SceneEditorState>,                        // 写当前高亮、目标引用和底栏提示。
    browser: Res<SceneBrowserState>, // 目标行必须仍属于场景树当前高亮场景。
    session: Res<EditorAuthoringSession>, // 当前行和同图目标从未保存草稿二次解析。
    registry: Option<Res<SceneContentRegistry>>, // 其他目标场景通过正式目录句柄校验。
    manifests: Res<Assets<SceneManifestAsset>>, // 默认出生点点击要求清单完整Ready。
    portal_assets: Res<Assets<ScenePortalsAsset>>, // 具体目标点击要求传送点仍存在。
    real_time: Res<Time<Real>>,    // F12暂停玩法虚拟时间时双击窗口仍正常推进。
    mut selected_portal_viewport: ResMut<SelectedPortalViewportState>, // 普通单击不移动相机，双击只登记请求。
    mut candidates: Option<ResMut<crate::viewport::LayerCandidateState>>,
    selection_context: (Option<Res<crate::game_authoring::scene_interaction::SceneInteractionState>>, Option<Res<crate::viewport::modes::ViewportModeState>>),
) {
    if numeric_scrub_mouse.blocked() || !navigation.visible(&editor) {
        return; // 和弦使用过的松开点击就地丢弃，不在解锁后补选旧行。
    }
    if session.saving {
        return; // 保存快照在途时不改变作者选择语义或放置准备状态。
    }
    let display_context = PortalDisplayContext::new(&session, registry.as_deref(), &manifests); // 点击反馈与完整列表复用同一“前往{场景名}”解析。
    for (interaction, row) in &current_rows {
        if !interaction.is_clicked() {
            continue; // 只消费真实松开点击。
        }
        let Some(portal_id) = row.portal_id.as_deref() else {
            continue; // Presentation尚未绑定的动态行不得提交旧ID。
        };
        let Some(draft) = session.draft.as_ref() else {
            editor.status = "当前场景传送点列表已变化，请重新选择".to_owned();
            continue;
        };
        let Some((index, portal)) = draft
            .portals
            .portals
            .iter()
            .enumerate()
            .find(|(_, portal)| portal.id == portal_id)
        else {
            editor.status = "当前场景传送点列表已变化，请重新选择".to_owned(); // 撤销或热更边界不猜测同名项。
            continue;
        };
        editor.selected_current_portal_id = Some(portal.id.clone()); // 当前列表只高亮已有源点。
        if let Some(candidates) = candidates.as_mut() {
            candidates.confirm_tree_author(crate::viewport::LayerTarget::Portal(portal.id.clone()), Some(index),
                &session, selection_context.0.as_ref().map_or(0, |state| state.observed_generation),
                selection_context.1.as_ref().map_or_else(Default::default, |state| state.selection_filter(&editor)));
        } // 原表单在属性中切源点，也同步完整正式身份；不因空下标把表单归回工具页。
        let portal_label = display_context.portal_label(portal); // 源点名称不再采用旧“传送点_N”快照。
        let target_scene_identity = display_context.target_scene_identity_label(&portal.target); // 点击反馈保留场景名称与编号。
        let target_arrival = display_context.current_target_arrival_label(portal, &portal_assets); // 点击反馈使用实际安全到达坐标。
        if selected_portal_viewport.register_portal_click(
            &draft.scene_id,
            &portal.id,
            real_time.elapsed_secs_f64(),
        ) {
            editor.status = format!("正在定位传送点：{portal_label}"); // Cursor阶段同帧覆盖为带坐标的完成提示。
        } else {
            editor.status = format!(
                "已选择当前源点：{}",
                current_portal_row_text(&portal_label, &target_scene_identity, &target_arrival), // 完整关系留在底栏点击详情，列表只显示短名。
            );
        }
    }

    for (interaction, row) in &target_rows {
        if !interaction.is_clicked() {
            continue; // 每个松开点击只提交一次目标选择。
        }
        selected_portal_viewport.clear_click_sequence(); // 目标落点选择不能与上一当前源点单击组成双击。
        let Some(target) = row.target.as_ref() else {
            continue; // 尚未绑定完成的动态行不继承旧目标身份。
        };
        let Some(target_scene_id) = selected_target_scene(&browser) else {
            editor.status = "请先在场景页高亮目标场景".to_owned(); // 目标场景来源保持唯一且可见。
            continue;
        };
        if target.scene_id != *target_scene_id {
            editor.status = "目标场景列表正在刷新，请重新点击到达点".to_owned(); // 场景树切换同帧拒绝迟到旧行。
            continue;
        }
        let documents = target_portal_documents(
            target_scene_id,       // 重新解析当前场景树目标。
            &session,            // 同图未保存传送点仍可成为目标。
            registry.as_deref(), // 其他场景读取正式强句柄。
            &manifests,          // 默认出生点必须来自Ready清单。
            &portal_assets,      // 具体传送点必须仍在Ready文档。
        );
        if !target_exists(target, &documents) {
            editor.status = "目标场景的该到达点尚未就绪或已失效".to_owned(); // 不发布半套跨文件引用，也不把MAP-*当名称。
            continue;
        }
        let TargetPortalDocuments::Ready { manifest, portals } = &documents else {
            continue; // target_exists已保证这里只接受Ready文档，防御未来有效性规则修改。
        };
        let endpoint_text = target.portal_id.as_deref()
            .and_then(|id| portals.portals.iter().find(|portal| portal.id == id))
            .map_or_else(|| default_spawn_row_text(manifest.default_spawn), |portal| target_portal_row_text(
                &display_context.portal_label(portal), &display_context.target_scene_identity_label(&portal.target),
                &display_context.current_target_arrival_label(portal, &portal_assets)));
        // 短列表只保留名称；完整端点关系与安全到达坐标仍可在底栏点击反馈中核对。
        editor.portal_target = Some(target.clone()); // 新放置传送点从此读取明确场景和端点。
        editor.status = format!(
            "已选择目标落点：{} / {}；现在可在中央场景点击放置",
            manifest.display_name, // 当前选择的目标场景名称足够建立上下文。
            endpoint_text,         // 默认点或作者自定义区域名称均保持紧凑。
        );
    }
}

/// 删除当前场景列表中明确高亮的源点；目标场景落点列表始终保持只读。
pub(crate) fn handle_portal_delete_button(
    mut commands: Commands,
    navigation: super::navigation::PortalSidebarNavigation,
    buttons: Query<(&EditorButtonClick, &SceneEditorActionButton), Changed<EditorButtonClick>>, // 只消费删除按钮松开点击。
    numeric_scrub_mouse: Res<SceneEditorNumericScrubMouseGuard>, // 数值和弦及release帧独占编辑器鼠标。
    mut editor: ResMut<SceneEditorState>,                        // 写选择、dirty与状态提示。
    mut session: ResMut<EditorAuthoringSession>, // 修改当前草稿、预览所有权和统一撤销栈。
) {
    for (interaction, button) in &buttons {
        if !interaction.is_clicked() || button.0 != EditorAction::PortalDeleteSelected {
            continue; // 其他静态按钮由各自功能域处理。
        }
        if numeric_scrub_mouse.blocked() {
            return; // 被数值交互使用过的边沿不延迟补删。
        }
        if !navigation.visible(&editor) {
            return; // 隐藏操作行即使残留迟到Interaction也不得修改作者数据。
        }
        if session.saving {
            editor.status = "场景保存进行中，暂不能删除传送点".to_owned();
            return;
        }
        let Some(portal_id) = editor.selected_current_portal_id.clone() else {
            editor.status = "请先在当前场景传送点列表选择要删除的源点".to_owned();
            return;
        };
        let removed = session
            .draft
            .as_mut()
            .and_then(|draft| take_portal_by_id(&mut draft.portals.portals, &portal_id)); // 草稿借用在处理预览表前结束，避免跨作者字段保持可变借用。
        let Some((index, record)) = removed else {
            editor.selected_current_portal_id = None;
            editor.status = "所选传送点已不存在，列表已刷新".to_owned();
            return;
        };
        let had_author_preview = session
            .portal_previews
            .remove(&record.id)
            .is_some_and(|entity| {
                commands.entity(entity).try_despawn(); // 未保存新增点立即从作者视口消失。
                true
            });
        session.undo.push(EditorCommand::PortalRemoved {
            index,                  // 撤销时恢复原列表顺序而非追加到末尾。
            record: record.clone(), // 小型JSON记录由命令完整拥有。
            had_author_preview,     // 只有原未保存点需要在撤销时重建预览。
        });
        session.redo.clear(); // 新删除操作终止旧重做分支。
        editor.selected_current_portal_id = None; // 删除后不把高亮误指向列表中的下一项。
        session.mark_portals_changed(&mut editor); // 刷新当前/同图目标列表并进入统一保存门。
        editor.status = "已删除所选传送点；可撤销，保存后更新正式传送".to_owned(); // 旧流水名称与内部ID都不再作为删除反馈。
        return; // 单次按下只形成一条删除命令，防御重复按钮实体覆盖成功提示。
    }
}

#[cfg(test)]
mod tests {
    use super::*; // 回归当前列表稳定ID删除语义。

    fn portal(id: &str, label: &str) -> ScenePortalRecord {
        ScenePortalRecord {
            terrain_binding: None,
            arrival_terrain_binding: None,
            id: id.to_owned(),
            label: label.to_owned(),
            label_customized: true, // 删除测试中的同名值都是显式作者名称。
            position: [0.0, 0.0, 0.0],
            arrival: [1.0, 0.0, 0.0],
            target: ScenePortalTarget::default_spawn(
                SceneId::parse("MAP-target").expect("测试场景ID有效"),
            ),
            color_srgba: [1.0, 0.5, 0.1, 1.0],
            arrow_degrees: 0.0,
        }
    }

    #[test]
    fn delete_uses_stable_id_and_preserves_original_index() {
        let mut portals = vec![
            portal("portal-a", "同名传送点"),
            portal("portal-b", "同名传送点"),
            portal("portal-c", "另一个传送点"),
        ];

        let (index, removed) =
            take_portal_by_id(&mut portals, "portal-b").expect("稳定ID应命中第二项");

        assert_eq!(index, 1);
        assert_eq!(removed.id, "portal-b");
        assert_eq!(
            portals
                .iter()
                .map(|portal| portal.id.as_str())
                .collect::<Vec<_>>(),
            vec!["portal-a", "portal-c"]
        ); // 同名标签不会让系统误删第一项。
        portals.insert(index, removed);
        assert_eq!(
            portals
                .iter()
                .map(|portal| portal.id.as_str())
                .collect::<Vec<_>>(),
            vec!["portal-a", "portal-b", "portal-c"]
        ); // 撤销所需下标能精确恢复原顺序。
    }
}
