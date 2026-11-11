//! 将植被业务字段与官方输入焦点同步；活动字段保持实体、选区与IME组合态。
use super::super::fields::field_value; // 强类型草稿读取。
use super::super::types::{PresetEditorFieldText, PresetField, VegetationPresetEditorState}; // 本域字段身份。
use crate::resources::SceneEditorState; // F12生命周期。
use crate::ui::text_input::EditorTextInput;
use bevy::input_focus::{FocusCause, InputFocus}; // 唯一官方焦点。
use bevy::prelude::*; // 资源及实体查询。
use bevy::text::{EditableText, EditableTextFilter, TextCursorStyle}; // 官方文字编辑及字段过滤。 // 公共输入法仲裁标记。

/// 用真实文本控件构造所有名称、路径和自由备注。
pub(in super::super) fn text_bundle(value: &str, field: PresetField) -> impl Bundle {
    // 构造时确定字段语义。
    let multiline = field == PresetField::Notes; // 只有备注允许换行。
    (
        // 官方组件直接渲染文本，无隐藏代理。
        EditorTextInput, // 标识公共输入法拥有者。
        EditableText {
            // 同时负责Unicode、选区、预编辑和剪贴板。
            max_characters: Some(if multiline {
                8_192
            } else if field == PresetField::Label {
                96
            } else {
                2_048
            }), // 维持原输入预算。
            allow_newlines: multiline,  // Enter在备注里换行。
            ..EditableText::new(value)  // 初始内容来自当前草稿。
        }, // 不再用Text追加拼音和假光标。
        EditableTextFilter::new(move |c| {
            !c.is_control() || (multiline && matches!(c, '\n' | '\t'))
        }), // 备注保留换行和制表符，路径保持单行。
        TextCursorStyle::default(), // 官方绘制光标和选区。
    ) // 字体和Node继续由原布局提供。
} // 文本规则统一供单行和多行构造复用。

/// 按钮运行后确定焦点；取消时恢复草稿，正常打字不覆盖官方编辑状态。
pub(in super::super) fn sync_text_focus(
    // 本域不接管其它文本编辑器的焦点。
    state: Res<VegetationPresetEditorState>, // 按钮完成后的业务字段。
    editor: Res<SceneEditorState>,             // 关闭状态用于释放焦点。
    browser: Option<Res<crate::resources::SceneBrowserState>>, // 文件夹模态拥有焦点优先权。
    numeric: Option<Res<feathrise_editor_ui::ActiveNumericInput>>, // 数值精确编辑不被底层名称抢焦点。
    mut focus: ResMut<InputFocus>,                                 // 官方键盘目标。
    mut texts: Query<(Entity, &PresetEditorFieldText, &mut EditableText)>, // 有限可见字段。
) {
    // 不增删实体，不反复重置光标。
    if editor.new_scene_dialog_open
        || browser
            .as_ref()
            .is_some_and(|browser| browser.dialog.is_some())
        || numeric
            .as_ref()
            .is_some_and(|numeric| numeric.entity.is_some())
    {
        return;
    } // 模态或数值会话由公共焦点管理，保留底层草稿。
    for (entity, marker, mut editable) in &mut texts {
        // 本域固定活动分区。
        if editor.open && state.active_field == Some(marker.0) {
            // 业务已明确进入此字段。
            if focus.get() != Some(entity) {
                focus.set(entity, FocusCause::Pressed);
            } // 只在首次聚焦时切换。
        } else {
            // 已提交或取消的字段不保留悬挂焦点。
            if focus.get() == Some(entity) {
                focus.clear();
            } // 不清除其他域焦点。
            if let Some(draft) = state.active_draft() {
                // 关闭标签后等待实体回收。
                let value = field_value(&draft.descriptor, marker.0).unwrap_or_default(); // 已提交草稿仍是非活动字段的事实来源。
                if editable.value().to_string() != value {
                    // 稳定帧不触发编辑器重建。
                    let maximum = editable.max_characters; // 保留名称/路径/备注预算。
                    let multiline = editable.allow_newlines; // 保留备注的Enter语义。
                    *editable = EditableText {
                        max_characters: maximum,
                        allow_newlines: multiline,
                        ..EditableText::new(value)
                    }; // 撤销同时丢弃在途粘贴，避免稍后覆盖。
                } // 已取消字符不会回流草稿。
            } // 草稿不存在时不猜新目标。
        } // 当前字段同步完成。
    } // 窗口IME由公共协调器处理。
} // 不负责文本编辑本身。

#[cfg(test)] // 无窗口回归验证真实业务缓冲与官方组件之间的边界。
mod tests {
    // 测试不访问剪贴板或修改作者数据。
    use super::super::super::types::{PresetDraft, PresetDraftMode};
    use super::*; // 直接执行正式焦点同步系统。 // 构造真实活动草稿。

    #[test] // 官方编辑系统验证多行和IME事务，而非模拟字符串追加。
    fn official_notes_keep_newlines_and_exclude_uncommitted_composition() {
        // 不启动窗口，不读写剪贴板。
        use bevy::text::{FontCx, LayoutCx, TextEdit, apply_text_edits}; // 直接运行官方编辑逻辑。
        let mut app = App::new(); // 纯文本不需要GPU。
        app.init_resource::<FontCx>()
            .init_resource::<LayoutCx>()
            .init_resource::<bevy::clipboard::Clipboard>()
            .add_systems(Update, apply_text_edits); // 仅初始化后端，不进行剪贴板读写。
        let entity = app
            .world_mut()
            .spawn(text_bundle("第一行", PresetField::Notes))
            .id(); // 使用备注真实字段规则。
        app.world_mut()
            .get_mut::<EditableText>(entity)
            .unwrap()
            .queue_edit(TextEdit::Insert("\n第二行".into())); // 正式多行插入。
        app.update(); // 应用官方过滤和编辑。
        assert_eq!(
            app.world()
                .get::<EditableText>(entity)
                .unwrap()
                .value()
                .to_string(),
            "第一行\n第二行"
        ); // 中文段落完整保留。
        app.world_mut()
            .get_mut::<EditableText>(entity)
            .unwrap()
            .queue_edit(TextEdit::ImeSetCompose {
                value: "zhong".into(),
                cursor: None,
            }); // 官方候选事务。
        app.update(); // 组合态由官方维护。
        assert!(
            app.world()
                .get::<EditableText>(entity)
                .unwrap()
                .is_composing()
        ); // 候选仍在组合中。
        assert_eq!(
            app.world()
                .get::<EditableText>(entity)
                .unwrap()
                .value()
                .to_string(),
            "第一行\n第二行"
        ); // 拼音不进入业务镜像。
        app.world_mut()
            .get_mut::<EditableText>(entity)
            .unwrap()
            .queue_edit(TextEdit::ImeCommit {
                value: "中".into()
            }); // 确认后才采用中文。
        app.update(); // 官方提交结束组合。
        assert_eq!(
            app.world()
                .get::<EditableText>(entity)
                .unwrap()
                .value()
                .to_string(),
            "第一行\n第二行中"
        ); // 不重复字符或额外换行。
    } // 官方多行和IME回归完成。

    #[test] // Esc取消恢复草稿，而活动输入不能被旧草稿反向覆盖。
    fn active_official_buffer_survives_sync_and_cancel_restores_draft() {
        // 同一实体贯穿整个编辑会话。
        let catalog: feathrise_scene_content::VegetationPresetCatalogAsset =
            serde_json::from_str(include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/vegetation_presets.vegetation_presets.json"
            )))
            .unwrap(); // 复用正式目录夹具。
        let descriptor = catalog.presets.into_iter().next().unwrap(); // 只取一个完整合法预设。
        let original = descriptor.label.clone(); // 取消后的期望值。
        let mut state = VegetationPresetEditorState::default(); // 测试独立会话。
        state.open_draft(PresetDraft::new(PresetDraftMode::Create, descriptor)); // 建立真实活动标签。
        state.active_field = Some(PresetField::Label); // 模拟已进入名称编辑。
        let mut app = App::new(); // 不启动窗口及渲染。
        app.insert_resource(state)
            .insert_resource(SceneEditorState {
                open: true,
                ..default()
            })
            .init_resource::<InputFocus>()
            .add_systems(Update, sync_text_focus); // 安装正式同步逻辑。
        let entity = app
            .world_mut()
            .spawn((
                PresetEditorFieldText(PresetField::Label),
                text_bundle("中文待提交", PresetField::Label),
            ))
            .id(); // 真实可见文本使用相同Bundle。
        app.update(); // 激活焦点不会覆盖输入。
        assert_eq!(app.world().resource::<InputFocus>().get(), Some(entity)); // 官方焦点指向真实字段。
        assert_eq!(
            app.world()
                .get::<EditableText>(entity)
                .unwrap()
                .value()
                .to_string(),
            "中文待提交"
        ); // 草稿旧值不能覆盖正在编辑的值。
        app.world_mut()
            .resource_mut::<VegetationPresetEditorState>()
            .active_field = None; // 模拟Esc取消。
        app.update(); // 取消只恢复字段本身。
        assert_eq!(app.world().resource::<InputFocus>().get(), None); // 没有悬挂键盘目标。
        assert_eq!(
            app.world()
                .get::<EditableText>(entity)
                .unwrap()
                .value()
                .to_string(),
            original
        ); // 未提交内容不进入草稿。
    } // 焦点与取消回归结束。
} // 所有回归仅操作内存。
