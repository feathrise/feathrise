//! 汇总 F12 全部自定义文本焦点，并在每帧末统一决定主窗口中文输入法状态。

use bevy::prelude::*; // 系统参数、主窗口查询和资源读取。
use bevy::window::PrimaryWindow; // F12 当前只编辑唯一主窗口。
use feathrise_editor_ui::ActiveNumericInput; // 专业数值精确输入优先于遗留文本焦点。

use super::text_input::EditorTextInput;
use crate::resources::SceneEditorState; // F12可见与关闭边界。
use bevy::input_focus::InputFocus; // 官方文本输入的唯一焦点。 // 只认真实可见官方编辑器字段。

/// 在全部输入与 Presentation 系统之后覆盖中间写入，消除多个表单争抢 `Window::ime_enabled` 的顺序依赖。
#[allow(clippy::too_many_arguments)] // 唯一仲裁器必须读取每个自定义文本域，才能避免任何域自行猜测其他域焦点。
pub(crate) fn sync_scene_editor_ime(
    editor: Res<SceneEditorState>,      // 编辑器关闭时不争抢姓名框所有权。
    numeric: Res<ActiveNumericInput>, // 专业数字输入优先于文本焦点。
    focus: Res<InputFocus>,           // 以官方真实焦点代替多份业务字段猜测。
    inputs: Query<(), (With<EditorTextInput>, With<bevy::text::EditableText>)>, // 只有迁移后的正式文本框可请求IME。
    mut windows: Query<&mut Window, With<PrimaryWindow>>, // 主窗口最终状态写入点。
) {
    // 所有业务焦点完成后再决定窗口输入法状态。
    if !editor.open {
        return;
    } // 主菜单仍由已验证姓名框控制。
    let enabled = !editor.close_requested
        && editor.scene_switch_confirmation.is_none() // 三选一切换场景模态不接受文本；背后表单不能继续拥有中文候选窗口。
        && numeric.entity.is_none()
        && focus.get().is_some_and(|entity| inputs.contains(entity)); // 真实焦点拥有原生候选窗口。
    super::actions::set_ime_enabled(&mut windows, enabled); // 不再依赖遗留预编辑字符串。
}
#[cfg(test)] // 真实官方焦点和非文本模态回归，无需Windows窗口。
mod tests {
    // 最小ECS应用运行正式绑定与仲裁系统。
    use super::*; // 复用生产IME仲裁。
    use crate::components::{SceneFolderDialogNameText, NewSceneNameText}; // 两种真实静态输入框身份。
    use crate::resources::SceneBrowserState; // 文件夹模态状态。
    use crate::types::{SceneFolderDialogMode, SceneFolderDialogState}; // 明确创建与删除业务模式。
    use crate::ui::vegetation_preset_editor::VegetationPresetEditorState; // 模拟中央工作区稳定身份。
    use bevy::input_focus::FocusCause;
    use bevy::text::EditableText; // 测试正式Unicode控件，不创建手写候选缓冲。 // 用户导航触发业务焦点。

    fn app() -> (App, Entity) {
        // 公共无窗口夹具。
        let mut app = App::new(); // 只运行固定输入绑定系统。
        app.insert_resource(SceneEditorState {
            open: true,
            ..default()
        }); // 测试F12打开时的所有权。
        app.init_resource::<SceneBrowserState>();
        app.init_resource::<crate::resources::EditorStaticTextInputState>(); // 正式静态字段绑定身份。 // 初始没有模态。
        app.init_resource::<VegetationPresetEditorState>(); // 默认场景中央标签。
        app.init_resource::<ActiveNumericInput>(); // 数值输入初始空闲。
        app.add_message::<feathrise_editor_ui::NumericInputSetValue>(); // 静态名称可请求公共控件释放残留数值焦点。
        app.init_resource::<InputFocus>(); // 官方唯一输入焦点。
        let window = app
            .world_mut()
            .spawn((
                Window {
                    ime_enabled: true,
                    ..default()
                },
                PrimaryWindow,
            ))
            .id(); // 原生状态模拟遗留true。
        app.add_systems(
            Update,
            (
                crate::ui::static_text_input::sync_focus,
                sync_scene_editor_ime,
            )
                .chain(),
        ); // 与生产相同先绑定后仲裁。
        (app, window) // 返回窗口供断言最终状态。
    } // 夹具不依赖图形驱动。

    #[test] // 打开中文模态应取得真实官方焦点与窗口IME。
    fn official_folder_dialog_focus_enables_ime_and_keeps_unicode() {
        let (mut app, window) = app(); // 准备未聚焦编辑器。
        let input = app
            .world_mut()
            .spawn((
                EditableText::default(),
                EditorTextInput,
                SceneFolderDialogNameText,
            ))
            .id(); // 对应可见BSN名称框。
        app.world_mut().resource_mut::<SceneBrowserState>().dialog = Some(SceneFolderDialogState {
            mode: SceneFolderDialogMode::Create,
            target_folder_id: None,
            name: "江南".to_owned(),
        }); // 名称从业务状态初始化一次。
        app.update(); // 正式系统取得控件焦点。
        assert_eq!(app.world().resource::<InputFocus>().get(), Some(input)); // 中文焦点对应真实输入实体。
        assert_eq!(
            app.world()
                .get::<EditableText>(input)
                .unwrap()
                .value()
                .to_string(),
            "江南"
        ); // 初始化保留完整Unicode。
        assert!(app.world().get::<Window>(window).unwrap().ime_enabled); // IME不再靠模拟光标判定。
        app.world_mut()
            .get_mut::<EditableText>(input)
            .unwrap()
            .editor
            .set_text("江南水乡"); // 模拟官方组件已经应用的后续输入。
        app.update(); // 稳定身份不得重置输入。
        assert_eq!(
            app.world()
                .get::<EditableText>(input)
                .unwrap()
                .value()
                .to_string(),
            "江南水乡"
        ); // 不用旧业务字符串覆盖新字符。
    } // 中文稳定焦点回归完成。

    #[test] // 删除模态不能让底层预设继续收字。
    fn official_non_text_modal_releases_underlying_preset_focus() {
        let (mut app, window) = app(); // 初始遗留IME开启。
        let underlying = app
            .world_mut()
            .spawn((EditableText::new("旧预设"), EditorTextInput))
            .id(); // 模拟中央预设字段。
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(underlying, FocusCause::Navigated); // 底层字段原先聚焦。
        app.world_mut().resource_mut::<SceneBrowserState>().dialog = Some(SceneFolderDialogState {
            mode: SceneFolderDialogMode::Delete,
            target_folder_id: None,
            name: "目录".to_owned(),
        }); // 非文本模态接管界面。
        app.update(); // 必须清除任何底层官方焦点。
        assert_eq!(app.world().resource::<InputFocus>().get(), None); // 不再有隐藏编辑器收字。
        assert!(!app.world().get::<Window>(window).unwrap().ime_enabled); // 原生IME同步关闭。
    } // 非文本模态所有权回归完成。

    #[test] // 关闭F12只清除自己的焦点，主菜单姓名框保持所有权。
    fn official_editor_close_preserves_main_menu_input_focus() {
        let (mut app, _) = app(); // 默认F12状态。
        let name = app.world_mut().spawn(EditableText::new("沈照雪")).id(); // 姓名框没有EditorTextInput标记。
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(name, FocusCause::Navigated); // 主菜单已接管焦点。
        app.world_mut().resource_mut::<SceneEditorState>().open = false; // F12后台仍存在。
        app.update(); // 后台系统不能覆盖姓名输入。
        assert_eq!(app.world().resource::<InputFocus>().get(), Some(name)); // 已验证的姓名方案保持不变。
    } // 跨UI输入所有权回归完成。

    #[test] // 备注转名称点击不依赖父Button事件冒泡。
    fn official_click_name_restores_field_from_notes_input() {
        let (mut app, _) = app(); // 复用真实焦点夹具。
        app.add_systems(PreUpdate, crate::ui::static_text_input::read_values); // 模拟业务阶段之前同步。
        let name = app
            .world_mut()
            .spawn((EditableText::default(), EditorTextInput, NewSceneNameText))
            .id(); // 空输入依然可以直接点击。
        {
            let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
            editor.new_scene_dialog_open = true;
            editor.new_scene_name = "洛阳".to_owned();
            editor.new_scene_input_field = crate::types::NewSceneInputField::Notes;
        } // 原先正在编辑备注。
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(name, FocusCause::Navigated); // 官方点击直接取得文字实体。
        app.update(); // 切字段后恢复原业务名称。
        assert_eq!(
            app.world()
                .get::<EditableText>(name)
                .unwrap()
                .value()
                .to_string(),
            "洛阳"
        ); // 不拿空旧控件覆盖名称。
    } // 名称与备注互切回归完成。
}
