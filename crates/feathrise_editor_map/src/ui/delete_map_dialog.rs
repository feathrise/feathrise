//! 地图直接删除或放入项目回收站的模态确认框与目标回显。

use feathrise_editor_ui::metrics as ui_metrics;
use bevy::prelude::*; // BSN场景、UI布局、文本和资源查询。
use bevy::text::FontSourceTemplate; // 确认内容使用项目中文字体。
use bevy::ui::{FocusPolicy, RelativeCursorPosition}; // 模态命中与两个独立滚动区。
use crate::ui::theme as colors; // 复用编辑器暗墨、铜边和米白文字色。

use crate::components::{MapDeletionDialog, MapDeletionDialogText}; // 显隐和目标文字使用稳定marker。
use crate::resources::{MapDeletionTask, MapEditorState}; // 确认快照是弹框唯一权威状态。
use crate::types::EditorAction; // 确认和取消继续使用结构化按钮动作。

use super::management_dialogs::{DialogKind, components::*, scene as modal};
use super::scene::FONT_PATH;

/// 构造阻止点透的删除确认遮罩；真正文件事务只由确认动作启动。
pub(super) fn delete_map_dialog() -> impl Scene {
    bsn! {
        MapDeletionDialog
        Interaction::default()
        Node {
            display: Display::None, position_type: PositionType::Absolute,
            left: px(0), top: px(0), width: percent(100), height: percent(100),
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            padding: UiRect::all(px(8)),
        }
        GlobalZIndex(440)
        BackgroundColor(colors::OVERLAY_BG)
        Children [(
            ManagementPanel(DialogKind::MapDeletion)
            template_value(modal::panel_node())
            BackgroundColor(colors::PANEL_BG_STRONG)
            BorderColor::all(colors::COPPER_BORDER_WEAK)
            Children [
                (modal::heading("删除地图")),
                (
                    ManagementScroll(DialogKind::MapDeletion)
                    ScrollPosition::default() RelativeCursorPosition::default()
                    template_value(modal::body_node())
                    Children [
                        (modal::caption("删除目标 · 请核对身份")),
                        (
                            MapDeletionDialogText Text("等待删除目标") feathrise_editor_ui::localization::label_scene("等待删除目标")
                            TextLayout::linebreak(bevy::text::LineBreak::AnyCharacter)
                            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                            TextColor(colors::TEXT_PRIMARY)
                            Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
                        ),
                        (modal::caption("删除后果与保护")),
                        (modal::caption("直接删除：永久删除地图文件，不可恢复。放入回收站：将文件移到工程的 assets/maps/.trash/，可人工恢复。")),
                        (modal::caption("两种方式都会移除地图目录入口；文件夹已不存在时只清理记录。其他地图仍引用它时拒绝删除，必须先关闭该地图标签并等待场景卸载。")),
                    ]
                ),
                (
                    ManagementScroll(DialogKind::MapDeletion) ManagementFeedback(DialogKind::MapDeletion)
                    ScrollPosition::default() RelativeCursorPosition::default()
                    template_value(modal::feedback_node())
                    BackgroundColor(colors::PANEL_BG_SECTION)
                    Children [(modal::caption("待确认 · 尚未启动删除；确认后仍须通过引用与文件校验。"))]
                ),
                (
                    ManagementFooter(DialogKind::MapDeletion)
                    template_value(modal::footer_node())
                    Children [
                        (modal::button("取消", EditorAction::CancelDeleteMap)),
                        (modal::button("直接删除", EditorAction::ConfirmDeleteMapPermanently)),
                        (modal::button("放入回收站", EditorAction::ConfirmDeleteMap)),
                    ]
                ),
            ]
        )]
    }
}

/// BSN不能直接模板化枚举组件，因此在模态根首次生成时补入阻止点透策略。
pub(crate) fn configure_delete_map_dialog_focus(
    mut commands: Commands, // 只在新增模态根上插入一次组件。
    dialogs: Query<Entity, Added<MapDeletionDialog>>, // 静态场景生命周期内通常只有一个实体。
) {
    for entity in &dialogs {
        commands.entity(entity).insert(FocusPolicy::Pass); // 弹框初始隐藏；显隐系统只在确认目标存在时启用阻止点透。
    }
}

/// 同步模态根的布局与命中策略；隐藏时主动释放全屏命中层，避免布局传播前的一帧残留遮罩。
fn apply_delete_dialog_root_state(
    visible: bool,                  // 只有存在冻结删除请求时才允许模态根捕获鼠标。
    node: &mut Node,                // `Display`控制弹框及全部子节点布局。
    interaction: &mut Interaction,  // 隐藏时清除根节点遗留的按压或悬停状态。
    focus_policy: &mut FocusPolicy, // `Pass`保证不可见根绝不拦截目录按钮。
) {
    node.display = if visible {
        Display::Flex // 显示完整确认框。
    } else {
        Display::None // 没有确认目标时不参与布局。
    };
    *focus_policy = if visible {
        FocusPolicy::Block // 可见遮罩阻止点击穿透到地图目录和视口。
    } else {
        FocusPolicy::Pass // 隐藏当帧立即释放命中，即使ComputedNode尚未完成下一轮布局。
    };
    if !visible {
        *interaction = Interaction::None; // 下一次打开从干净交互状态开始。
    }
}

/// 仅在编辑器或删除状态变化时原地更新模态显隐与冻结目标文字。
pub(crate) fn sync_delete_map_dialog(
    editor: Res<MapEditorState>,    // F12隐藏时确认框也必须隐藏。
    deletion: Res<MapDeletionTask>, // confirmation决定是否存在待确认目标。
    mut dialogs: Query<
        (Entity, &mut Node, &mut Interaction, &mut FocusPolicy), // 根实体同时控制布局、交互与阻止点透策略。
        With<MapDeletionDialog>,                                 // 只处理唯一删除确认遮罩。
    >,
    children: Query<&Children>, // 隐藏时只遍历确认框固定小层级，清除两个按钮的遗留交互。
    mut descendant_interactions: Query<&mut Interaction, Without<MapDeletionDialog>>, // 与根查询保持结构性互斥。
    mut texts: Query<&mut Text, With<MapDeletionDialogText>>, // 唯一目标文字只改String。
) {
    if !(editor.is_changed() || deletion.is_changed()) {
        return; // 稳定帧不重复写UI组件。
    }
    let visible = editor.open && deletion.confirmation.is_some(); // 只有F12内的未确认请求显示模态层。
    for (entity, mut node, mut interaction, mut focus_policy) in &mut dialogs {
        apply_delete_dialog_root_state(
            visible,           // 同一冻结请求同时决定渲染与命中。
            &mut node,         // 原地更新布局，不重建BSN实体。
            &mut interaction,  // 取消后释放根交互状态。
            &mut focus_policy, // 隐藏态必须透传，显示态必须阻止点透。
        );
        if !visible {
            for descendant in children.iter_descendants(entity) {
                if let Ok(mut descendant_interaction) = descendant_interactions.get_mut(descendant)
                {
                    *descendant_interaction = Interaction::None; // 取消、确认按钮不会把旧Pressed状态带到下一次弹框。
                }
            }
        }
    }
    let Some(request) = deletion.confirmation.as_ref() else {
        return; // 隐藏状态无需改写占位文字。
    };
    let message = format!(
        "地图：{}\nMapId：{}\n资产目录：assets/{}",
        request.display_name, request.map_id, request.folder
    ); // 名称与稳定ID共同确认，避免同名地图误删。
    for mut text in &mut texts {
        if text.0 != message {
            text.0 = message.clone(); // 仅冻结目标真正改变时更新文本。
        }
    }
}

#[cfg(test)]
mod tests {
    use feathrise_map_content::MapId; // 构造合法编辑器地图身份。

    use crate::types::MapDeletionRequest; // 模拟打开、取消和再次打开同一确认流程。

    use super::*; // 直接运行显隐系统，覆盖真实资源变化顺序。

    fn deletion_request() -> MapDeletionRequest {
        MapDeletionRequest {
            map_id: MapId::parse("MAP-delete-dialog-test").expect("测试MapId合法"), // 使用受保护规则允许的动态地图身份。
            display_name: "待删除地图".to_owned(), // 弹框文字使用稳定作者名称。
            folder: "maps/MAP-delete-dialog-test".to_owned(), // 私有目录与MapId严格一致。
        }
    }

    #[test]
    fn cancel_releases_hidden_overlay_and_allows_the_dialog_to_open_again() {
        let mut app = App::new(); // 纯ECS测试不启动窗口或渲染插件。
        app.init_resource::<MapEditorState>(); // 显隐同时受F12打开状态控制。
        app.init_resource::<MapDeletionTask>(); // confirmation是唯一弹框权威状态。
        app.add_systems(Update, sync_delete_map_dialog); // 运行与正式插件相同的增量同步系统。
        let root = app
            .world_mut()
            .spawn((
                MapDeletionDialog, // 标记唯一全屏遮罩根。
                Node::default(),   // 初始布局由同步系统改为Flex。
                Interaction::None, // 初次打开没有旧按压状态。
                FocusPolicy::Pass, // 隐藏初始态不阻止底层命中。
            ))
            .id();
        let cancel = app
            .world_mut()
            .spawn((Interaction::None,)) // 模拟确认框内的取消按钮。
            .id();
        app.world_mut().entity_mut(root).add_child(cancel); // 建立真实模态父子关系供隐藏清理遍历。
        app.world_mut().resource_mut::<MapEditorState>().open = true; // 模拟F12已经打开。
        app.world_mut()
            .resource_mut::<MapDeletionTask>()
            .confirmation = Some(deletion_request()); // 第一次点击“删除地图”。

        app.update(); // 显示弹框并启用全屏阻止点透。

        let root_entity = app.world().entity(root); // 读取第一次打开后的根状态。
        assert_eq!(
            root_entity.get::<Node>().expect("根Node存在").display,
            Display::Flex
        ); // 第一次确认框可见。
        assert_eq!(root_entity.get::<FocusPolicy>(), Some(&FocusPolicy::Block)); // 可见时必须阻止底层点击。
        app.world_mut()
            .entity_mut(root)
            .insert(Interaction::Pressed); // 模拟点击取消时根命中状态仍未释放。
        app.world_mut()
            .entity_mut(cancel)
            .insert(Interaction::Pressed); // 模拟取消按钮在隐藏前仍处于Pressed。
        app.world_mut()
            .resource_mut::<MapDeletionTask>()
            .confirmation = None; // 取消动作清空冻结删除请求。

        app.update(); // 同帧显隐采用后必须完整释放不可见遮罩。

        let root_entity = app.world().entity(root); // 读取取消后的根状态。
        assert_eq!(
            root_entity.get::<Node>().expect("根Node存在").display,
            Display::None
        ); // 取消后弹框不参与布局。
        assert_eq!(root_entity.get::<FocusPolicy>(), Some(&FocusPolicy::Pass)); // 隐藏根不得再拦截“进入/删除地图”。
        assert_eq!(root_entity.get::<Interaction>(), Some(&Interaction::None)); // 根交互已经清空。
        assert_eq!(
            app.world().entity(cancel).get::<Interaction>(),
            Some(&Interaction::None),
        ); // 子按钮也不保留会污染下一次打开的Pressed。
        app.world_mut()
            .resource_mut::<MapDeletionTask>()
            .confirmation = Some(deletion_request()); // 第二次点击“删除地图”。

        app.update(); // 再次打开必须恢复完整模态语义。

        let root_entity = app.world().entity(root); // 读取第二次打开后的根状态。
        assert_eq!(
            root_entity.get::<Node>().expect("根Node存在").display,
            Display::Flex
        ); // 第二次确认框仍能显示。
        assert_eq!(root_entity.get::<FocusPolicy>(), Some(&FocusPolicy::Block)); // 再次显示后恢复阻止点透。
    }
}
