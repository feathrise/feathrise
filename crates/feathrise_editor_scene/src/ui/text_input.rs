//! 官方文本输入的公共身份与快捷键保护；文字编辑完全交给 Bevy EditableText。

use bevy::prelude::*; // 注册系统与读取主窗口输入消息。
use bevy::window::Ime; // 只观察事件种类，绝不复制或提交输入法文本。

pub(crate) fn install(app: &mut App) {
    // 独立编辑器测试同样具备公共输入资源。
    app.init_resource::<crate::resources::EditorStaticTextInputState>() // 静态字段首次初始化身份。
        .init_resource::<EditorTextInputGuard>() // 每帧保护默认关闭。
        .init_resource::<bevy::input_focus::InputFocus>() // 与主菜单共享官方唯一焦点。
        .add_message::<Ime>() // 与窗口插件重复注册安全，不创建额外输入源。
        .add_message::<feathrise_editor_ui::NumericInputSetValue>() // 独立F12安装时仍可请求公共数值框释放旧焦点。
        .add_systems(
            PreUpdate,
            collect_ime_guard.after(bevy::input::InputSystems),
        ) // 读取窗口本帧已收集的组合消息。
        .add_systems(
            Update,
            (
                bevy::text::apply_text_edits.run_if(editor_text_runtime_ready),
                super::static_text_input::read_values,
            )
                .chain()
                .before(crate::SceneEditorSystemSet::Input),
        ) // 先用官方系统应用同帧队列，再让业务读取；避免最后一个字符或粘贴因保存/切页丢失。
        .add_systems(
            Update,
            super::static_text_input::sync_focus
                .after(crate::SceneEditorSystemSet::Presentation)
                .before(super::ime::sync_scene_editor_ime),
        ) // 业务焦点收口后再更新窗口输入法。
        .add_systems(
            Update,
            style_fields
                .after(crate::SceneEditorSystemSet::Presentation)
                .before(super::static_text_input::sync_focus),
        ); // 新建字段同帧补齐统一官方光标与选区。

    #[cfg(debug_assertions)]
    app.add_message::<bevy::input::keyboard::KeyboardInput>()
        .add_systems(Last, diagnose_new_scene_name_input); // 实机复现时只记录事件数量与焦点，不记录场景名称或输入内容。
} // 完成公共调度注册。

#[cfg(debug_assertions)]
#[allow(clippy::too_many_arguments)] // 同一条短日志对齐焦点、窗口事件、控件排版和业务写回。
fn diagnose_new_scene_name_input(
    editor: Res<crate::resources::SceneEditorState>,
    focus: Res<bevy::input_focus::InputFocus>,
    numeric: Res<feathrise_editor_ui::ActiveNumericInput>,
    names: Query<
        (
            Entity,
            &bevy::text::EditableText,
            &bevy::ui::ComputedNode,
            &bevy::text::TextLayoutInfo,
        ),
        With<crate::components::NewSceneNameText>,
    >,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut keyboard: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mut ime: MessageReader<Ime>,
    mut previous: Local<
        Option<(
            Option<Entity>,
            crate::types::NewSceneInputField,
            Option<Entity>,
            usize,
            usize,
            bool,
            bool,
        )>,
    >,
) {
    let mut pressed = 0_u32;
    let mut with_text = 0_u32;
    for event in keyboard.read() {
        if event.state.is_pressed() {
            pressed += 1;
            with_text += u32::from(event.text.as_ref().is_some_and(|text| !text.is_empty()));
        }
    }
    let mut preedit = 0_u32;
    let mut commit = 0_u32;
    for event in ime.read() {
        preedit += u32::from(matches!(event, Ime::Preedit { .. }));
        commit += u32::from(matches!(event, Ime::Commit { .. }));
    }
    if !editor.open || !editor.new_scene_dialog_open {
        *previous = None;
        return;
    }
    let name = names.single().ok();
    let value_chars = name.as_ref().map_or(0, |(_, text, _, _)| {
        text.value().to_string().chars().count()
    });
    let caret_height = name
        .as_ref()
        .and_then(|(_, _, _, layout)| layout.cursor.map(|(_, rect)| rect.height()))
        .unwrap_or(0.0);
    let name_focused = name
        .as_ref()
        .is_some_and(|(entity, _, _, _)| focus.get() == Some(*entity));
    let window_focused = windows.single().is_ok_and(|window| window.focused);
    let signature = (
        focus.get(),
        editor.new_scene_input_field,
        numeric.entity,
        value_chars,
        editor.new_scene_name.chars().count(),
        window_focused,
        caret_height > 0.0,
    );
    if previous.as_ref() == Some(&signature) && pressed == 0 && preedit == 0 && commit == 0 {
        return;
    }
    *previous = Some(signature);
    info!(
        "[NEW_SCENE_INPUT] focus_name={} field={:?} keys={}/{} ime={}/{} text_chars={} model_chars={} numeric={} node_h={:.1} caret_h={:.1} window_focus={}",
        name_focused,
        editor.new_scene_input_field,
        pressed,
        with_text,
        preedit,
        commit,
        value_chars,
        editor.new_scene_name.chars().count(),
        numeric.entity.is_some(),
        name.as_ref().map_or(0.0, |(_, _, node, _)| node.size().y),
        caret_height,
        window_focused,
    ); // 若值已经增长而界面仍空，排查渲染；若 keys=0，排查 Winit/窗口焦点。
}

pub(crate) fn style_fields(
    mut commands: Commands,
    fields: Query<Entity, (Added<EditorTextInput>, With<bevy::text::EditableText>)>,
) {
    // 仅处理新建字段，稳定帧不扫描或重写界面。
    use crate::ui::theme as colors; // 沿用主菜单已验证的水墨铜金视觉令牌。
    for entity in &fields {
        // 新建表单字段数量由当前活动分区决定。
        commands.entity(entity).insert(bevy::text::TextCursorStyle {
            // 官方控件需要显式光标样式才能统一绘制选区。
            color: colors::TEXT_ACCENT, // 铜金光标保持现有姓名框视觉。
            selection_color: colors::SELECT_BG_HOVER, // 聚焦选区沿用原界面选择色。
            unfocused_selection_color: colors::SELECT_BG, // 失焦选区使用弱选择底色。
            selection_radius: 0.0,
            selected_text_color: Some(colors::TEXT_PRIMARY), // 选中文字仍使用米白主文字色。
        }); // 不改字体、文字内容或业务状态。
    } // 每个实体只在首次加入时设置一次。
} // 官方布局继续负责真实光标尺寸与滚动。

fn editor_text_runtime_ready(
    editor: Res<crate::resources::SceneEditorState>,
    fonts: Option<Res<bevy::text::FontCx>>,
    layout: Option<Res<bevy::text::LayoutCx>>,
    clipboard: Option<Res<bevy::clipboard::Clipboard>>,
) -> bool {
    // 纯业务测试没有完整字体运行时，不能执行布局驱动。
    editor.open && fonts.is_some() && layout.is_some() && clipboard.is_some() // 正式F12仅把官方编辑应用提前到业务输入前，PostUpdate仍负责正常绘制与候选位置。
} // 不创建第二套编辑算法，也不影响F12关闭时的主菜单输入。

pub(crate) use crate::components::EditorTextInput; // 保留各字段共享的组件导入路径。
pub(crate) use crate::resources::EditorTextInputGuard; // 资源声明集中在资源层。

pub(crate) fn collect_ime_guard(
    // 在 Update 业务系统前记录本帧是否有候选事件。
    mut events: MessageReader<Ime>, // 独立只读消息游标不影响官方输入插件。
    mut guard: ResMut<EditorTextInputGuard>, // 每帧重新计算，候选结束后自然恢复快捷键。
    focus: Res<bevy::input_focus::InputFocus>, // 候选跨帧停留期间也必须保护保存按钮。
    inputs: Query<&bevy::text::EditableText>, // 查询当前官方预编辑状态，不保存候选副本。
) {
    // 官方插件仍独占预编辑、提交、选区与剪贴板处理。
    guard.block_shortcuts = events.read().any(|event| {
        // 本帧存在任意组合事件就保护业务快捷键。
        matches!(
            event,
            Ime::Preedit { .. } | Ime::Commit { .. } | Ime::Disabled { .. }
        ) // 包括取消与提交的末帧。
    }); // 输入法启用通知本身不阻止普通操作。
    events.clear(); // any 短路后仍推进全部消息，不能把旧事件带到下一帧。
    guard.block_shortcuts |= focus
        .get()
        .and_then(|entity| inputs.get(entity).ok())
        .is_some_and(|text| text.is_composing()); // 没有新IME事件时仍保护正在选择的候选。
} // 完成输入法与表单快捷键边界。

pub(crate) use feathrise_editor_ui::single_line_input;

/// 同实体回显名称必须沿用底层编辑器；重建EditableText会丢失只在样式变化时同步的字体与布局。
pub(crate) fn replace_single_line_value(text: &mut bevy::text::EditableText, value: &str, max: usize) {
    text.clear(); // 清理旧待处理字符、粘贴与组合状态，不替换已经配置字形样式的编辑器。
    text.editor.set_text(value);
    text.max_characters = Some(max);
    text.allow_newlines = false;
    text.queue_edit(bevy::text::TextEdit::TextEnd(false)); // 原位更新布局代次并将新选择的光标移到末尾。
}

#[cfg(test)] // 验证生产调度中的队列应用、中文过滤和业务读取顺序。
mod tests {
    // 不启动游戏、不访问系统剪贴板内容。
    use super::*; // 使用正式安装入口和字段配置。
    use crate::components::NewSceneNameText; // 正式场景名称输入实体。
    use crate::resources::{SceneBrowserState, SceneEditorState}; // 场景名称与弹框数据。
    use bevy::text::{EditableText, EditableTextFilter, FontCx, LayoutCx, TextEdit}; // 测试真实官方编辑队列。

    #[test] // 最后一个字与业务确认同帧时不能读取上一帧旧值。
    fn official_queued_text_and_completed_paste_reach_business_before_input() {
        let mut app = App::new(); // 无窗口ECS文本测试。
        app.insert_resource(SceneEditorState {
            open: true,
            new_scene_dialog_open: true,
            new_scene_name: "开封".to_owned(),
            ..default()
        }); // 模拟已打开新场景表单。
        app.init_resource::<SceneBrowserState>()
            .init_resource::<feathrise_editor_ui::ActiveNumericInput>()
            .init_resource::<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>(); // 固定业务绑定资源。
        app.init_resource::<FontCx>()
            .init_resource::<LayoutCx>()
            .init_resource::<bevy::clipboard::Clipboard>(); // 只准备官方编辑后端，不读写剪贴板。
        install(&mut app); // 必须使用生产系统链验证先应用、后读取。
        let input = app
            .world_mut()
            .spawn((
                single_line_input("", 48),
                EditorTextInput,
                NewSceneNameText,
                EditableTextFilter::new(|c| !c.is_control()),
            ))
            .id(); // 正式单行过滤与长度配置。
        app.update(); // 首帧建立可见字段会话并恢复开封初值。
        app.world_mut()
            .get_mut::<EditableText>(input)
            .unwrap()
            .queue_edit(TextEdit::Insert("城郊".into())); // 同帧普通Unicode末字。
        app.update(); // 官方先处理队列，业务随后读取完整值。
        assert_eq!(
            app.world().resource::<SceneEditorState>().new_scene_name,
            "开封城郊"
        ); // 不遗漏最后一个字，也不让控制字符进入名称。
        app.world_mut()
            .get_mut::<EditableText>(input)
            .unwrap()
            .pending_paste = Some(bevy::clipboard::ClipboardRead::Ready(Ok("河道".to_owned()))); // 注入已完成读取，避免接触用户真实剪贴板。
        app.update(); // 官方正常消费完成的粘贴票据。
        assert_eq!(
            app.world().resource::<SceneEditorState>().new_scene_name,
            "开封城郊河道"
        ); // 粘贴与普通文字同样在业务提交前可见。
    } // 正式调度与粘贴顺序回归完成。
} // 所有测试均不创建原生窗口。
