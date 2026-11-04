//! 地图名称、文件夹名称与传送点名称绑定官方文本实体；仅在会话切换时重置编辑器。

use super::text_input::EditorTextInput; // 会话切换保留已完成样式同步的官方编辑器。
use crate::components::{
    MapEditorPortalNameInputText, MapFolderDialogNameText, NewMapNameText, NewMapNotesText,
}; // 名称与多行备注分别绑定官方输入实体。
use crate::resources::{MapBrowserState, MapEditorState}; // 已有业务缓冲与模态状态。
use crate::types::{MapFolderDialogMode, NewMapInputField}; // 静态字段实际可编辑条件。
use super::portal_list::PortalSidebarNavigation; // 名称绑定不能被Select中的Inspect收起。
use bevy::input_focus::{FocusCause, InputFocus}; // 与姓名框共享唯一官方焦点。
use bevy::prelude::*; // 固定三个静态输入实体与业务资源。
use bevy::text::EditableText; // 唯一文字、光标、选区与预编辑来源。 // 复用官方字段配置及全局释放身份。
use feathrise_editor_ui::{NumericInputSetValue, NumericInputValue}; // 模态名称接管前结束残留的公共数值精确输入。

type StaticInputs<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static mut EditableText,
        Option<&'static NewMapNameText>,
        Option<&'static NewMapNotesText>,
        Option<&'static MapFolderDialogNameText>,
        Option<&'static MapEditorPortalNameInputText>,
    ),
>; // 只对三个标记实体绑定业务值。

pub(crate) fn read_values(
    // 本帧按钮与表单确认之前读取官方已提交文本。
    focus: Res<InputFocus>, // 官方点击可从数值字段转回名称。
    navigation: PortalSidebarNavigation,
    binding: Res<crate::resources::EditorStaticTextInputState>, // 仅已初始化会话可以写回业务值。
    inputs: Query<(
        Entity,
        &EditableText,
        Option<&NewMapNameText>,
        Option<&NewMapNotesText>,
        Option<&MapFolderDialogNameText>,
        Option<&MapEditorPortalNameInputText>,
    )>, // 同帧切焦点时仍从旧业务会话读回末字。
    mut editor: ResMut<MapEditorState>, // 地图与传送名称保持既有事务入口。
    mut browser: ResMut<MapBrowserState>, // 文件夹名称保留原验证和保存事务。
) {
    // 隐藏域不能把残留文本写到新业务会话。
    if !editor.open {
        return;
    } // 主菜单输入不属于本绑定器。
    for (entity, text, map, notes, folder, portal) in &inputs {
        // 固定三个业务输入域，不随地图内容规模增长。
        let initialized = binding
            .previous
            .as_ref()
            .is_some_and(|(bound, _)| *bound == entity); // 防止首帧空控件覆盖名称初值。
        if initialized
            && map.is_some()
            && editor.new_map_dialog_open
            && editor.new_map_input_field == NewMapInputField::Name
        {
            // 仅真实名称焦点接收文字。
            let value = text.value().to_string(); // value排除未确认的拼音。
            if editor.new_map_name != value {
                editor.new_map_name = value;
            } // 稳定帧不标记业务资源变化。
        } else if initialized
            && notes.is_some()
            && editor.new_map_dialog_open
            && editor.new_map_input_field == NewMapInputField::Notes
        {
            let value = text.value().to_string(); // 已提交多行文字不包含未确认的IME候选。
            if editor.new_map_notes != value {
                editor.new_map_notes = value; // 新建或编辑只在点击确认时写入清单。
            }
        } else if initialized
            && folder.is_some()
            && let Some(dialog) = browser.dialog.as_ref()
            && dialog.mode != MapFolderDialogMode::Delete
        {
            // 删除确认不编辑名称。
            let value = text.value().to_string(); // 只有已确认Unicode进入归类模型。
            if dialog.name != value {
                browser.dialog.as_mut().unwrap().name = value;
            } // 保留48字符校验与持久化入口。
        } else if initialized
            && portal.is_some()
            && navigation.visible(&editor)
            && editor.portal_name_edit.active
            && editor.portal_name_edit.portal_id == editor.selected_current_portal_id
        {
            // 冻结ID不得串写其他传送点。
            let value = text.value().to_string(); // 拼音与选区只留在官方编辑器。
            if editor.portal_name_edit.value != value {
                editor.portal_name_edit.value = value;
            } // 名称仅进输入事务，应用时才写撤销栈。
        } else if map.is_some() && editor.new_map_dialog_open && focus.get() == Some(entity) {
            // 点击真实文字区可从数值字段回到名称。
            editor.new_map_input_field = NewMapInputField::Name; // 本帧末由绑定器恢复名称，不读取尚未初始化的旧控件值。
        } else if notes.is_some() && editor.new_map_dialog_open && focus.get() == Some(entity) {
            editor.new_map_input_field = NewMapInputField::Notes; // 鼠标点击备注直接取得多行IME焦点。
        } // 数字输入仍由专业控件处理。
    } // 完成全部仍有效会话的读取，包括刚失焦的最后一个字。
} // 完成官方文本到业务缓冲的单向同步。

pub(crate) fn sync_focus(
    // 输入按钮与模态收口后只在身份变化时初始化控件。
    editor: Res<MapEditorState>,   // 三个静态字段共享编辑器可见边界。
    navigation: PortalSidebarNavigation,
    browser: Res<MapBrowserState>, // 文件夹模态优先于底层表单。
    workspace: Res<super::vegetation_preset_editor::VegetationPresetEditorState>, // 传送名称不能抢走中央预设标签的焦点。
    numeric: Res<feathrise_editor_ui::ActiveNumericInput>, // 数字精确输入不能继续编辑隐藏中文框。
    numeric_values: Query<&NumericInputValue>,             // 取消旧精确输入时保留其已生效数值。
    mut numeric_requests: MessageWriter<NumericInputSetValue>, // 通过公共控件入口释放编辑态，不直接改其内部缓冲。
    mut focus: ResMut<InputFocus>,                             // 控制原生官方输入焦点。
    mut inputs: StaticInputs,                                  // 三个可见官方输入实体。
    editor_inputs: Query<(), With<EditorTextInput>>,           // 关闭F12时仅清理本编辑器焦点。
    mut binding: ResMut<crate::resources::EditorStaticTextInputState>, // 固定一个活跃静态字段身份，避免逐帧重置光标。
) {
    // 所有名称初始化都发生在业务已冻结目标后。
    if editor.open
        && !editor.close_requested
        && browser.dialog.is_none()
        && editor.new_map_dialog_open
        && matches!(
            editor.new_map_input_field,
            NewMapInputField::Name | NewMapInputField::Notes
        )
        && let Some(entity) = numeric.entity
        && let Ok(value) = numeric_values.get(entity)
    {
        // 新建地图名称是上层模态的明确目标；旧数值框若仍占键盘，下一帧先让公共控件安全释放它。
        numeric_requests.write(NumericInputSetValue {
            entity,
            value: value.0,
            cancel_edit: true,
        }); // 点击“新建地图”或名称框后不再因残留数值焦点永远没有光标。
    }
    let mut target = None; // 默认无静态文本焦点。
    if editor.open && !editor.close_requested && numeric.entity.is_none() {
        // 数值与关闭具有最高优先级。
        for (entity, _, map, notes, folder, portal) in &mut inputs {
            // 输入实体数固定，不扫描地图内容。
            let binding = if let Some(dialog) = browser.dialog.as_ref() {
                // 文件夹模态屏蔽底层名称框。
                (folder.is_some() && dialog.mode != MapFolderDialogMode::Delete).then(|| {
                    (
                        format!("folder:{:?}:{:?}", dialog.mode, dialog.target_folder_id),
                        dialog.name.clone(),
                    )
                }) // 冻结操作和目标身份。
            } else if editor.new_map_dialog_open {
                // 地图资料弹框仅名称和备注能取得官方文本焦点。
                if map.is_some() && editor.new_map_input_field == NewMapInputField::Name {
                    Some((
                        format!(
                            "map:name:{:?}",
                            editor.new_map_edit_target.as_ref().map(|target| &target.id)
                        ),
                        editor.new_map_name.clone(),
                    )) // 切图或新建模式重开必须刷新文本。
                } else if notes.is_some() && editor.new_map_input_field == NewMapInputField::Notes {
                    Some((
                        format!(
                            "map:notes:{:?}",
                            editor.new_map_edit_target.as_ref().map(|target| &target.id)
                        ),
                        editor.new_map_notes.clone(),
                    )) // 备注与名称绝不能互相覆盖。
                } else {
                    None // 禁用的尺寸控件不创建文本会话。
                }
            } else {
                // 普通地图页只允许当前所选传送名称。
                (portal.is_some()
                    && workspace.active_tab
                        == super::vegetation_preset_editor::PresetWorkspaceTab::Map
                    && navigation.visible(&editor)
                    && editor.portal_name_edit.active
                    && editor.portal_name_edit.portal_id == editor.selected_current_portal_id)
                    .then(|| {
                        (
                            format!("portal:{:?}", editor.portal_name_edit.portal_id),
                            editor.portal_name_edit.value.clone(),
                        )
                    }) // 稳定ID变化必须重置输入。
            }; // 无效字段不取得焦点。
            if let Some((key, value)) = binding {
                target = Some((entity, key, value));
                break;
            } // 唯一静态字段终止固定枚举。
        } // 所有普通预设焦点由各领域绑定。
    } // 隐藏时不恢复任何静态会话。
    if let Some((entity, key, value)) = target {
        // 新会话从业务值初始化，持续输入原位保存。
        if binding.previous.as_ref() != Some(&(entity, key.clone())) {
            // 只在实体或业务身份变化时重置。
            if let Ok((_, mut text, _, _, _, _)) = inputs.get_mut(entity) {
                reset_session_text(&mut text, &value); // 原位重置业务文本，保留字体与布局代次。
            } // 同步原字符上限。
            binding.previous = Some((entity, key)); // 保存当前会话供下一帧复用。
        } // 鼠标定位和中文组合期间不重建EditableText。
        if focus.get() != Some(entity) {
            focus.set(entity, FocusCause::Navigated);
        } // 打开模态即可输入。
    } else {
        // 关闭、切数字或切目标时释放旧静态焦点。
        if let Some((entity, _)) = binding.previous.take() {
            // 仅取消本绑定器拥有的输入编辑。
            if let Ok((_, mut text, _, _, _, _)) = inputs.get_mut(entity) {
                let value = text.value().to_string(); // 失焦保留已确认文字显示，只丢弃候选与待处理状态。
                reset_session_text(&mut text, &value); // 失焦同样不能丢掉已同步的字体、行高和换行设置。
            } // 清理未提交预编辑和待处理字符。
            if focus.get() == Some(entity) {
                focus.clear();
            } // 不覆盖其他预设新取得的焦点。
        } // 业务草稿仍由原事务持有。
        if (!editor.open
            || editor.close_requested
            || numeric.entity.is_some()
            || browser.dialog.is_some()
            || editor.new_map_dialog_open)
            && focus
                .get()
                .is_some_and(|entity| editor_inputs.contains(entity))
        {
            focus.clear();
        } // 无文字模态与数值字段必须释放底层预设焦点。
    } // 完成静态输入焦点生命周期。
} // 官方插件随后同步候选位置与窗口开关。

/// Bevy 仅在样式组件变化时同步字体；重建 EditableText 会丢失样式且复用旧布局代次，导致文字和光标不可见。
fn reset_session_text(text: &mut EditableText, value: &str) {
    super::text_input::replace_single_line_value(text, value, 48);
} // 字体、行高、单行排版、缩放和布局代次均沿用真实控件。
