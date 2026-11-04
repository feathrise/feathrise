//! 场景树下的作者层候选卡片：多层直接显示，单层隐藏，保留悬停预高亮。

use super::layers::LayerCandidateState;

use crate::viewport::modes::{ViewportMode, ViewportModeState}; // 编辑模式隐藏候选交互。
use bevy::{
    picking::Pickable,
    prelude::*,
    ui::{FocusPolicy, RelativeCursorPosition},
}; // 原生UI与文字穿透。
use crate::ui::theme as colors; // 同一水墨铜木色板。 // 不在UI写地图文档。

#[derive(Component)] // 只重建这个最多十一按钮的小型浮层。
pub(crate) struct LayerCandidatePopup;
#[derive(Component, Clone, Default)] // 场景树下的BSN静态挂载位。
pub(crate) struct LayerCandidateSlot;
#[derive(Component, Clone, Copy)] // 行下标绑定当前完整候选版本。
pub(crate) enum CandidateAction {
    Select(usize),
    Previous,
    Next,
} // 页面导航不切换地图。

fn button(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    label: String,
    action: CandidateAction,
) {
    // 统一尺寸与可见中文字布局。
    parent
        .spawn((
            Button, // 候选行保留独立高亮，同时接入普通按钮的合法释放协议。
            feathrise_editor_ui::EditorButtonClick::default(), // 独立无窗口夹具也具有相同的动作契约。
            action,
            Node {
                width: percent(100),
                min_height: px(28),
                padding: UiRect::axes(px(8), px(4)),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(colors::SLOT_BG),
            BorderColor::all(colors::COPPER_BORDER),
            FocusPolicy::Block,
        )) // 每行完整阻挡地图点击。
        .with_child((
            Text::new(label),
            TextFont {
                font: font.clone().into(),
                font_size: 12.0.into(),
                ..default()
            },
            TextColor(colors::TEXT_PRIMARY),
            Node {
                width: percent(100),
                min_width: px(0),
                flex_shrink: 0.0,
                ..default()
            },
            Pickable::IGNORE,
            FocusPolicy::Pass,
        )); // 标签明确宽度，避免再次出现空白字。
}

pub(crate) fn sync_layer_candidate_ui(
    mut commands: Commands,
    assets: Res<AssetServer>,
    modes: Res<ViewportModeState>, // 变换期间收起候选卡片，回到选择后恢复原列表。
    mesh_edit: Option<Res<crate::authoring::mesh_edit::MeshEditState>>,
    mut slots: Query<(Entity, &mut Node), With<LayerCandidateSlot>>, // 场景页固定挂载位。
    popups: Query<Entity, With<LayerCandidatePopup>>,                // 仅回收旧卡片。
    mut state: ResMut<LayerCandidateState>,                          // 用结构版本避免悬停重建。
) {
    let Ok((slot, mut node)) = slots.single_mut() else {
        return; // 右栏未建立时等待重试。
    };
    let display = if modes.mode == ViewportMode::Select && state.candidates.len() > 1
        && !mesh_edit.as_ref().is_some_and(|state| state.active()) {
        Display::Flex
    } else {
        Display::None
    };
    if node.display != display {
        node.display = display;
    } // 模式切换只改一个固定挂载节点，不清空正式目标或重建候选。
    if state.applied_ui_revision == state.ui_revision {
        return;
    }
    for popup in &popups {
        commands.entity(popup).despawn(); // 子行随旧卡片回收。
    }
    state.applied_ui_revision = state.ui_revision; // 完整结构已排队。
    if state.candidates.len() < 2 {
        return; // 零层和单层不创建卡片。
    }
    let font: Handle<Font> = feathrise_editor_core::editor_font(&assets, "fonts/open_source/noto_sans/cjk/NotoSansCJKsc-VF.otf"); // 共用中文字体。
    commands.entity(slot).with_children(|parent| {
        parent
            .spawn((
                LayerCandidatePopup, // 当前列表根节点。
                Node {
                    width: percent(100),                     // 跟随右栏宽度。
                    min_width: px(0),                        // 长名称允许换行。
                    flex_shrink: 0.0,                        // 不压扁列表。
                    flex_direction: FlexDirection::Column,   // 纵向排列候选。
                    row_gap: px(2),                          // 紧凑行间距。
                    padding: UiRect::all(px(4)),             // 卡片内部留白。
                    border: UiRect::all(px(1)),              // 独立卡片边框。
                    border_radius: BorderRadius::all(px(5)), // 统一圆角。
                    ..default()                              // 使用侧栏正常文档流。
                },
                BackgroundColor(colors::OVERLAY_BG), // 暗墨底。
                BorderColor::all(colors::COPPER_BORDER), // 铜色边框。
                RelativeCursorPosition::default(),   // 内边距同样消费鼠标。
                FocusPolicy::Block,                  // 阻止交互穿透。
            ))
            .with_children(|card| {
                let start = state.page * 8; // 固定八行窗口保持大地图预算。
                for (index, candidate) in state.candidates.iter().enumerate().skip(start).take(8) {
                    button(
                        card,
                        &font,
                        candidate.label.clone(),
                        CandidateAction::Select(index),
                    ); // 直接显示候选，不设数量及展开按钮。
                }
                if state.page > 0 {
                    button(card, &font, "上一页".to_owned(), CandidateAction::Previous); // 访问前页。
                }
                if start + 8 < state.candidates.len() {
                    button(card, &font, "下一页".to_owned(), CandidateAction::Next); // 访问后页。
                }
            });
    });
}

pub(crate) fn layer_candidate_interactions(
    modes: Res<ViewportModeState>,
    mesh_edit: Option<Res<crate::authoring::mesh_edit::MeshEditState>>,
    editor: Option<Res<crate::resources::MapEditorState>>, // 独立候选测试可省略，只有选择模式允许候选交互。
    session: Option<Res<crate::authoring::EditorAuthoringSession>>, // 停放来源不能借当前槽的同名作者层预高亮。
    guard: Option<Res<crate::resources::MapEditorNumericScrubMouseGuard>>, // 手柄/数值手势及 release 帧不能在候选卡片上补触发。
    mut state: ResMut<LayerCandidateState>,
    popups: Query<&RelativeCursorPosition, With<LayerCandidatePopup>>, // 整个菜单范围阻挡视口。
    mut buttons: Query<(
        &Interaction,
        Ref<feathrise_editor_ui::EditorButtonClick>,
        &CandidateAction,
        &mut BackgroundColor,
    )>,
) {
    // 唯一hover和按钮颜色写入者。
    if guard.as_ref().is_some_and(|guard| guard.blocked())
        || mesh_edit.as_ref().is_some_and(|state| state.active())
        || !editor.as_ref().map_or(modes.mode == ViewportMode::Select, |editor| modes.selection_allowed(editor)) {
        state.pointer_over_ui = false;
        state.hover = None;
        state.hover_author_index = None;
        return;
    } // 模式切换不残留不可见点击门。
    let mut hover = None; // 离开候选后恢复正式选中覆盖。
    let mut hover_index = None; // 与hover同时采用。
    let mut over = popups.iter().any(|cursor| cursor.cursor_over()); // padding/行缝同样不穿透。
    for (interaction, change, action, mut background) in &mut buttons {
        // 至多十一行固定成本。
        over |= *interaction != Interaction::None; // UI标题、翻页也不得穿透。
        let selected = matches!(action, CandidateAction::Select(index) if *index == state.index); // 选中行持续高亮。
        let next = if *interaction != Interaction::None || selected {
            colors::BUTTON_SETTINGS_HOVERED
        } else {
            colors::SLOT_BG
        }; // hover与选中态在唯一入口决定颜色。
        if background.0 != next {
            background.0 = next; // 颜色实际变化才写入，稳定悬停不触发多余更新。
        }
        if *interaction != Interaction::None {
            // hover预高亮不正式选中。
            if let CandidateAction::Select(index) = *action {
                if let Some(candidate) = state.candidates.get(index).filter(|candidate|
                    session.as_ref().is_none_or(|session| session.draft.as_ref().is_some_and(|draft|
                        state.candidate_canvas_matches(draft, &candidate.target)))) {
                    hover = Some(match &candidate.target {
                        super::layers::LayerTarget::RootWater(kind, id) => super::layers::LayerTarget::Water(*kind, id.clone()),
                        target => target.clone(),
                    }); // 活动根水继续使用原覆盖协议；停放源只给行反馈，正式点击才恢复画布。
                    hover_index = candidate.author_index;
                }
            } // 直接读取可见行，避免扫描全列表。
        }
        if !change.is_clicked() || !change.is_changed() {
            continue;
        } // 悬停保留预览，普通选择只消费按钮内松开的一次点击。
        state.ticket = state.ticket.wrapping_add(1); // UI操作淘汰旧地图点击结果，不允许迟到选择抢回。
        state.request = None; // 未启动的旧点击同时撤销。
        state.waiting_canvas = None; // 明确选择候选后，旧网格载入不能重放先前点击。
        state.task_identity = None;
        match *action {
            // 每个按钮只改变会话态。
            CandidateAction::Select(index) => {
                // 候选行点击进入统一正式选层出口。
                if let Some(candidate) = state.candidates.get(index).cloned() {
                    state.apply_candidate_index = Some(index);
                    state.apply = Some(candidate.target);
                } // 不关闭菜单，便于继续比较。
            }
            CandidateAction::Previous => {
                state.page = state.page.saturating_sub(1);
                state.ui_revision = state.ui_revision.wrapping_add(1);
            } // 固定窗口向前。
            CandidateAction::Next => {
                state.page = (state.page + 1).min(state.candidates.len().saturating_sub(1) / 8);
                state.ui_revision = state.ui_revision.wrapping_add(1);
            } // 固定窗口向后。
        }
    }
    state.pointer_over_ui = over; // 旧内容射线系统读取同一帧输入门。
    state.hover = hover; // 离开行返回None，而非改写正式选层。
    state.hover_author_index = hover_index; // 覆盖可常数校验稳定身份。
}
