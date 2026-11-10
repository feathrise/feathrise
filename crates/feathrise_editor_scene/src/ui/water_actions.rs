//! F12静水体、河流样条与瀑布按钮的选择和一帧请求分派。

use crate::authoring::{EditorAuthoringSession, WaterBrushStroke, WaterNetworkEditState}; // 水域选择统一读取保存、静水笔划和拖点互斥门。
use crate::resources::SceneEditorState; // 工具、选择、状态和一帧水网请求。
use crate::types::{EditorAction, EditorTool, WaterLevelInputTarget, WaterNetworkAction}; // 静态UI动作映射到作者域动作。
use feathrise_scene_content::SceneTerrainKind; // 取水位提示仍区分Clipmap真实三角查询。

/// 判断按钮是否会切换、增删当前静水体、河流或瀑布身份。
fn water_feature_switch_action(action: EditorAction) -> bool {
    matches!(
        action,
        EditorAction::WaterAddLayer
            | EditorAction::WaterDuplicateLayer
            | EditorAction::RiverAdd
            | EditorAction::WaterfallAdd
            | EditorAction::WaterListPreviousPage
            | EditorAction::WaterListNextPage
            | EditorAction::WaterDeleteSelected
    ) // 控制点、连接和水位动作不改变当前水域对象身份。
}

/// 不支持数据驱动样条水的地形可以展示旧对象，但所有修改按钮必须禁用。
pub(super) fn spline_edit_action(action: EditorAction, tool: EditorTool) -> bool {
    matches!(
        action,
        EditorAction::RiverAdd
            | EditorAction::WaterfallAdd
            | EditorAction::WaterPointDelete
            | EditorAction::WaterSegmentKindToggle
            | EditorAction::WaterSourceNext
            | EditorAction::WaterTargetNext
    ) || (action == EditorAction::WaterDeleteSelected
        && matches!(tool, EditorTool::RiverSpline | EditorTool::Waterfall))
}

/// 所有水域对象切换入口共享同一跨帧事务保护；返回true表示本次输入已明确拒绝。
pub(crate) fn reject_water_feature_switch(
    editor: &mut SceneEditorState,
    session: &EditorAuthoringSession,
    stroke: &WaterBrushStroke,
    network_edit: &WaterNetworkEditState,
) -> bool {
    if stroke.active {
        editor.status = "当前静水笔划正在收尾；松开鼠标后再切换水域对象".to_owned();
        return true; // 笔划冻结的稳定层ID必须先完成待补段和撤销收口。
    }
    if network_edit.dragging() {
        editor.status = "请先松开正在拖动的水网控制点，再切换水域对象".to_owned();
        return true; // 拖点完整快照不能在中途改换河流或瀑布身份。
    }
    if session.saving {
        editor.status = "场景保存期间暂不切换水域对象".to_owned();
        return true; // 保存冻结的作者快照与左栏焦点保持同一对象代次。
    }
    false // 没有跨帧事务持有水域对象身份，可以继续分派。
}

/// 分派一项水域按钮；返回true表示动作已完整识别，通用总match不得再次处理。
pub(super) fn handle_water_action(
    action: EditorAction,
    editor: &mut SceneEditorState,
    session: &EditorAuthoringSession,
    stroke: &WaterBrushStroke,
    network_edit: &WaterNetworkEditState,
) -> bool {
    if water_feature_switch_action(action)
        && reject_water_feature_switch(editor, session, stroke, network_edit)
    {
        return true; // 已识别的水域按钮必须原地消费，不能落入通用动作分支。
    }
    let edits_spline = spline_edit_action(action, editor.tool);
    if edits_spline
        && session
            .draft
            .as_ref()
            .is_some_and(|draft| !draft.terrain_manifest().terrain.kind.supports_spline_water())
    {
        editor.water_network_action_requested = None;
        editor.status = "当前地形类型不支持河流、急流与瀑布编辑".to_owned();
        return true; // 业务层拒绝，不能只依赖按钮显示状态。
    }
    let network_action = match action {
        EditorAction::WaterAddLayer | EditorAction::WaterDuplicateLayer => {
            return true; // 专用静水层系统原子修改描述、Arc遮罩和撤销快照。
        }
        EditorAction::WaterListPreviousPage | EditorAction::WaterListNextPage => {
            return true; // 专用固定行池系统按稳定ID直选目标页首项，选择本身不标脏。
        }
        EditorAction::WaterLevelFromCursor => {
            editor.water_level_from_cursor_requested = session.draft.as_ref().and_then(|draft| {
                let layer = draft
                    .water
                    .as_ref()?
                    .layers
                    .get(editor.selected_water_layer)?;
                Some(WaterLevelInputTarget {
                    scene_id: draft.scene_id.clone(),
                    layer_id: layer.id.clone(),
                    authoring_revision: session.revision,
                })
            }); // Clipmap可跨侧栏到视口等待，但不能在等待期间改换场景、层或作者修订。
            if editor.water_level_from_cursor_requested.is_none() {
                editor.status = "当前没有可取得地形水位的湖泊/池塘".to_owned();
            } else if session
                .draft
                .as_ref()
                .is_some_and(|draft| draft.terrain_manifest().terrain.kind == SceneTerrainKind::SparseClipmap)
            {
                editor.tool = EditorTool::Water;
                editor.status =
                    "请把光标移到Geometry Clipmap真实地形上；高度就绪后自动取得水位".to_owned();
            }
            return true;
        }
        EditorAction::RiverAdd => WaterNetworkAction::RiverAdd,
        EditorAction::WaterfallAdd => WaterNetworkAction::WaterfallAdd,
        EditorAction::WaterDeleteSelected => match editor.tool {
            EditorTool::Water => return true, // 静水删除交给同帧专用图层系统维护R8遮罩和端点断连。
            EditorTool::RiverSpline => WaterNetworkAction::RiverDelete, // 当前高亮河流复用完整水网删除快照。
            EditorTool::Waterfall => WaterNetworkAction::WaterfallDelete, // 当前高亮瀑布复用既有连接清理与撤销事务。
            _ => {
                editor.status = "请先从水域对象列表选择要删除的静水体、河流或瀑布".to_owned(); // 非水域工具没有合法高亮目标。
                return true; // 通用删除已被明确消费，不落入其他编辑器动作。
            }
        },
        EditorAction::WaterPointPrevious => WaterNetworkAction::PointPrevious,
        EditorAction::WaterPointDelete => WaterNetworkAction::PointDelete,
        EditorAction::WaterSegmentKindToggle => WaterNetworkAction::ToggleSegmentKind,
        EditorAction::WaterSourceNext => WaterNetworkAction::SourceNext,
        EditorAction::WaterTargetNext => WaterNetworkAction::TargetNext,
        _ => return false,
    };
    editor.tool = match action {
        EditorAction::RiverAdd => EditorTool::RiverSpline,
        EditorAction::WaterfallAdd => EditorTool::Waterfall,
        _ => editor.tool, // 控制点与连接属性始终作用于当前河流或瀑布工具。
    };
    editor.water_network_action_requested = Some(network_action);
    true
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use feathrise_scene_content::{SceneId, SceneTerrainKind}; // 构造完整拖点身份和地形能力回归。

    use super::*; // 直接验证水域对象动作不会绕过统一保护门。

    /// 断言一组结构与列表动作在给定保护状态下完全不改变当前选择。
    fn assert_switches_are_blocked(
        session: &EditorAuthoringSession,
        stroke: &WaterBrushStroke,
        network_edit: &WaterNetworkEditState,
        expected_reason: &str,
    ) {
        for action in [
            EditorAction::WaterAddLayer,
            EditorAction::WaterDuplicateLayer,
            EditorAction::RiverAdd,
            EditorAction::WaterfallAdd,
            EditorAction::WaterListPreviousPage,
            EditorAction::WaterListNextPage,
            EditorAction::WaterDeleteSelected,
        ] {
            let mut editor = SceneEditorState::default(); // 每个动作从同一无水域工具状态独立开始。
            editor.selected_water_layer = 3; // 非零索引可捕获受保护动作误切换。
            editor.selected_water_river = 4; // 河流身份在结构动作被拒绝时保持不变。
            editor.selected_waterfall = 5; // 瀑布身份同样保持原值。
            assert!(handle_water_action(
                action,
                &mut editor,
                session,
                stroke,
                network_edit,
            )); // 水域动作仍应被领域分派器识别并消费。
            assert_eq!(editor.tool, EditorTool::Inspect); // 工具类别没有在受保护期间变化。
            assert_eq!(editor.selected_water_layer, 3); // 静水身份保持稳定。
            assert_eq!(editor.selected_water_river, 4); // 河流身份保持稳定。
            assert_eq!(editor.selected_waterfall, 5); // 瀑布身份保持稳定。
            assert!(editor.water_network_action_requested.is_none()); // 不向后续作者系统泄漏迟到请求。
            assert!(editor.status.contains(expected_reason)); // 即使结构与分页动作本身不改索引，也必须证明保护门先执行。
        }
    }

    #[test]
    fn water_feature_actions_are_blocked_during_still_water_stroke() {
        let session = EditorAuthoringSession::default(); // 空草稿足以证明保护先于数组读取执行。
        let mut stroke = WaterBrushStroke::default(); // 构造一个正在收尾的稳定ID笔划。
        stroke.active = true; // 结构按钮必须与对象列表使用同一个活动门。
        let network_edit = WaterNetworkEditState::default(); // 没有拖点时单独验证笔划原因。
        assert_switches_are_blocked(&session, &stroke, &network_edit, "笔划"); // 全部身份动作均不得切换。
    }

    #[test]
    fn water_feature_actions_are_blocked_while_saving() {
        let mut session = EditorAuthoringSession::default(); // 保存门不依赖草稿是否已经填充。
        session.saving = true; // 模拟后台文件事务已经冻结作者快照。
        let stroke = WaterBrushStroke::default(); // 没有笔划时仍必须由保存门单独拒绝。
        let network_edit = WaterNetworkEditState::default(); // 没有拖点时单独验证保存原因。
        assert_switches_are_blocked(&session, &stroke, &network_edit, "保存"); // 操作入口不能先切工具再被下游拒绝。
    }

    #[test]
    fn water_feature_actions_are_blocked_during_network_drag() {
        let session = EditorAuthoringSession::default(); // 拖点保护在读取作者数组之前执行。
        let stroke = WaterBrushStroke::default(); // 没有静水笔划时单独验证拖点原因。
        let mut network_edit = WaterNetworkEditState::default(); // 三个稳定身份同时存在才代表真实拖点。
        network_edit.scene_id = Some(SceneId::parse("MAP-water-switch-test").expect("测试场景ID有效"));
        network_edit.feature_id = Some("river-a".to_owned()); // 冻结当前河流身份。
        network_edit.point_id = Some("point-a".to_owned()); // 冻结当前控制点身份。
        assert_switches_are_blocked(&session, &stroke, &network_edit, "控制点"); // 列表和结构按钮都必须等待松手。
    }

    #[test]
    fn clipmap_routes_spline_mutations_and_keeps_static_water_actions() {
        let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
        draft.manifest.terrain.kind = SceneTerrainKind::SparseClipmap;
        let session = EditorAuthoringSession {
            draft: Some(draft),
            ..Default::default()
        };
        let stroke = WaterBrushStroke::default();
        let network_edit = WaterNetworkEditState::default();
        for (action, tool) in [
            (EditorAction::RiverAdd, EditorTool::Inspect),
            (EditorAction::WaterfallAdd, EditorTool::Inspect),
            (EditorAction::WaterPointDelete, EditorTool::RiverSpline),
            (
                EditorAction::WaterSegmentKindToggle,
                EditorTool::RiverSpline,
            ),
            (EditorAction::WaterSourceNext, EditorTool::RiverSpline),
            (EditorAction::WaterTargetNext, EditorTool::RiverSpline),
            (EditorAction::WaterDeleteSelected, EditorTool::Waterfall),
        ] {
            let mut editor = SceneEditorState {
                tool,
                ..Default::default()
            };
            assert!(spline_edit_action(action, tool));
            assert!(handle_water_action(
                action,
                &mut editor,
                &session,
                &stroke,
                &network_edit,
            ));
            assert!(editor.water_network_action_requested.is_some()); // Clipmap与GLB共用水网作者事务。
            assert!(!editor.status.contains("不支持"));
        }
        assert!(!spline_edit_action(
            EditorAction::WaterAddLayer,
            EditorTool::Water,
        ));
        assert!(!spline_edit_action(
            EditorAction::WaterLevelFromCursor,
            EditorTool::Water,
        ));
    }

    #[test]
    fn clipmap_water_level_pick_freezes_scene_layer_and_author_revision() {
        let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
        draft.manifest.terrain.kind = SceneTerrainKind::SparseClipmap;
        draft.water = Some(crate::authoring::water_asset_with_layer_for_tests(
            &draft.scene_id,
            &draft.folder,
        ));
        let layer_id = draft.water.as_ref().unwrap().layers[0].id.clone();
        let scene_id = draft.scene_id.clone();
        let session = EditorAuthoringSession {
            draft: Some(draft),
            revision: 27,
            ..Default::default()
        };
        let mut editor = SceneEditorState::default();
        let stroke = WaterBrushStroke::default();
        let network_edit = WaterNetworkEditState::default();

        assert!(handle_water_action(
            EditorAction::WaterLevelFromCursor,
            &mut editor,
            &session,
            &stroke,
            &network_edit,
        ));

        assert_eq!(editor.tool, EditorTool::Water);
        assert_eq!(
            editor.water_level_from_cursor_requested,
            Some(WaterLevelInputTarget {
                scene_id,
                layer_id,
                authoring_revision: 27,
            })
        );
        assert!(editor.status.contains("真实地形"));
    }

    #[test]
    fn delete_selected_routes_to_the_current_water_feature_kind() {
        let session = EditorAuthoringSession::default(); // 动作路由只需要当前工具，不读取或修改作者草稿。
        let stroke = WaterBrushStroke::default(); // 没有静水笔划时允许执行删除动作。
        let network_edit = WaterNetworkEditState::default(); // 没有控制点拖动时允许执行删除动作。
        let mut editor = SceneEditorState::default(); // 依次模拟列表三类高亮和无高亮状态。
        editor.tool = EditorTool::Water; // 静水行高亮由静水工具和类内索引共同表达。
        assert!(handle_water_action(
            EditorAction::WaterDeleteSelected, // 使用列表下方唯一删除动作。
            &mut editor,                       // 读取当前高亮类别并发布对应请求。
            &session,                          // 跨帧保护读取当前作者会话。
            &stroke,                           // 静水笔划未占用对象身份。
            &network_edit,                     // 水网拖点未占用对象身份。
        )); // 通用删除必须被水域分派器消费。
        assert!(editor.water_network_action_requested.is_none()); // 静水删除留给图层系统处理R8遮罩。
        editor.tool = EditorTool::RiverSpline; // 切换为当前高亮河流。
        assert!(handle_water_action(
            EditorAction::WaterDeleteSelected, // 仍使用同一个列表级删除按钮。
            &mut editor,                       // 当前工具已表达河流高亮。
            &session,                          // 作者会话保持可编辑状态。
            &stroke,                           // 没有活动静水笔划。
            &network_edit,                     // 没有活动控制点拖动。
        )); // 同一按钮再次按当前类别分派。
        assert_eq!(
            editor.water_network_action_requested.take(), // 读取并清空一帧水网请求。
            Some(WaterNetworkAction::RiverDelete),        // 河流进入原有水网删除事务。
        ); // 通用按钮不得误路由到静水或瀑布。
        editor.tool = EditorTool::Waterfall; // 切换为当前高亮瀑布。
        assert!(handle_water_action(
            EditorAction::WaterDeleteSelected, // 仍使用同一个列表级删除按钮。
            &mut editor,                       // 当前工具已表达瀑布高亮。
            &session,                          // 作者会话保持可编辑状态。
            &stroke,                           // 没有活动静水笔划。
            &network_edit,                     // 没有活动控制点拖动。
        )); // 不需要第二个瀑布删除按钮。
        assert_eq!(
            editor.water_network_action_requested.take(), // 读取并清空一帧水网请求。
            Some(WaterNetworkAction::WaterfallDelete),    // 瀑布进入原有水网删除事务。
        ); // 通用按钮不得误路由到静水或河流。
        editor.tool = EditorTool::Inspect; // 非水域工具代表列表没有当前高亮对象。
        assert!(handle_water_action(
            EditorAction::WaterDeleteSelected, // 无高亮时仍是已识别的水域动作。
            &mut editor,                       // 状态提示写回同一编辑器资源。
            &session,                          // 作者会话保持可编辑状态。
            &stroke,                           // 没有活动静水笔划。
            &network_edit,                     // 没有活动控制点拖动。
        )); // 无目标也必须明确消费按钮。
        assert!(editor.status.contains("请先从水域对象列表选择")); // 用户得到可执行的选择提示而不是误删旧索引。
    }
}
