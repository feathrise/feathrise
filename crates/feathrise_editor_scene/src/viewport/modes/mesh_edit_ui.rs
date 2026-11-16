//! 对象与点/边/面编辑的原生视口工具栏；按钮只发布编辑意图，几何事务由网格编辑域统一处理。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use bevy::prelude::*;
use bevy::ui::{FocusPolicy, RelativeCursorPosition};
use feathrise_editor_ui::EditorButtonClick;

use crate::authoring::mesh_edit::{MeshEditAction, MeshEditState, MeshElementKind, SubdivisionCorner};
use crate::resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState};
use crate::ui::theme as colors;

#[derive(Component)]
pub(crate) struct MeshEditToolbarRoot;

#[derive(Component, Clone, Copy)]
pub(super) struct MeshEditButton(MeshEditAction);

#[derive(Component)]
pub(super) struct MeshEditToggleCaption;

#[derive(Component)]
pub(super) struct MeshSelectionCaption;

#[derive(Component)]
pub(super) struct MeshEditOnly;

#[derive(Component)]
pub(super) struct MeshMoveOnly;

#[derive(Resource, Default)]
pub(super) struct MeshAdvancedMenuState {
    open: bool,
}

#[derive(Component)]
pub(super) struct MeshAdvancedToggle;

#[derive(Component)]
pub(super) struct MeshAdvancedMenuRoot;

#[derive(Component)]
pub(super) struct MeshSubdivisionControls;

#[derive(Component)]
pub(super) struct MeshSubdivisionValue;

pub(super) fn spawn(parent: &mut ChildSpawnerCommands, font: &Handle<Font>) {
    parent.spawn((
        MeshEditToolbarRoot, UiInteraction::default(), RelativeCursorPosition::default(), FocusPolicy::Block,
        Node {
            position_type: PositionType::Absolute, left: px(50), right: px(8), top: px(8),
            max_width: px(112), flex_wrap: FlexWrap::Wrap, column_gap: px(3), row_gap: px(3),
            padding: UiRect::all(px(2)), border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(2)),
            ..default()
        },
        BackgroundColor(colors::PANEL_BG_STRONG), BorderColor::all(colors::COPPER_BORDER_WEAK), ZIndex(26),
    )).with_children(|row| {
        button(row, font, "网格编辑 Tab", MeshEditAction::Toggle, 106.0, false, false);
        for (label, action, width) in [
            ("点 1", MeshEditAction::Kind(MeshElementKind::Vertex), 44.0),
            ("边 2", MeshEditAction::Kind(MeshElementKind::Edge), 44.0),
            ("面 3", MeshEditAction::Kind(MeshElementKind::Face), 44.0),
            ("全选 A", MeshEditAction::SelectAll, 60.0),
            ("框选 B", MeshEditAction::BoxSelect, 60.0),
            ("挤出 E", MeshEditAction::Extrude, 60.0),
            ("确认", MeshEditAction::Confirm, 44.0),
            ("取消 Esc", MeshEditAction::Cancel, 72.0),
        ] {
            button(row, font, label, action, width, true,
                matches!(action, MeshEditAction::Confirm | MeshEditAction::Cancel));
        }
        row.spawn((
            InteractionButton, MeshAdvancedToggle, FocusPolicy::Block,
            Node { width: px(60), height: px(26), display: Display::None, flex_shrink: 0.0,
                border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(2)),
                align_items: AlignItems::Center, justify_content: JustifyContent::Center, ..default() },
            BackgroundColor(colors::SLOT_BG), BorderColor::all(colors::COPPER_BORDER_WEAK),
        )).with_child((
            Text::new("更多"), TextFont { font: font.clone().into(), font_size: 12.0.into(), ..default() },
            TextColor(colors::TEXT_PRIMARY), TextLayout::no_wrap(), FocusPolicy::Pass, Pickable::IGNORE,
        ));
        row.spawn((
            MeshSelectionCaption, Text::new("已选 0 点"),
            TextFont { font: font.clone().into(), font_size: 12.0.into(), ..default() },
            TextColor(colors::TEXT_MUTED), TextLayout::no_wrap(), Pickable::IGNORE,
            Node { display: Display::None, height: px(26), align_self: AlignSelf::Center, flex_shrink: 0.0, margin: UiRect::horizontal(px(3)), ..default() },
        )); // 多选反馈只读取固定元素位集，不遍历场景或重建按钮。
        row.spawn((
            MeshAdvancedMenuRoot,
            Node { display: Display::None, width: percent(100), flex_wrap: FlexWrap::Wrap,
                column_gap: px(3), row_gap: px(3), ..default() },
        )).with_children(|menu| {
            for (label, action) in [
                ("内插面", MeshEditAction::Inset), ("倒角", MeshEditAction::Bevel),
                ("环切", MeshEditAction::LoopCut), ("切割", MeshEditAction::Knife),
                ("细分", MeshEditAction::Subdivide),
            ] { button(menu, font, label, action, 68.0, true, false); }
        }); // 高级入口仍在原工具栏背景内折行，避免单行拉长或浮层溢出窄视口。
        subdivision_controls(row, font);
    });
}

fn subdivision_controls(row: &mut ChildSpawnerCommands, font: &Handle<Font>) {
    let parameter_row = Node {
        align_items: AlignItems::Center,
        flex_wrap: FlexWrap::Wrap,
        column_gap: px(3),
        row_gap: px(3),
        ..default()
    };
    let parameter_font = TextFont { font: font.clone().into(), font_size: 12.0.into(), ..default() };
    // 参数区只属于细分预览；所有按钮都走原点击入口，和确认、取消共用同一次事务。
    row.spawn((
        MeshSubdivisionControls,
        Node { display: Display::None, width: percent(100), flex_direction: FlexDirection::Column,
            row_gap: px(3), ..default() },
    )).with_children(|settings| {
        settings.spawn(parameter_row.clone()).with_children(|cuts| {
            cuts.spawn((
                feathrise_editor_ui::localization::editor_label("切割次数"), parameter_font.clone(),
                TextColor(colors::TEXT_MUTED), TextLayout::no_wrap(), Pickable::IGNORE,
            ));
            button(cuts, font, "−", MeshEditAction::SubdivisionCutsDelta(-1), 26.0, false, false);
            cuts.spawn((
                MeshSubdivisionValue, Text::new("1"), parameter_font.clone(),
                TextColor(colors::TEXT_PRIMARY), TextLayout::justify(Justify::Center),
                Node { width: px(28), flex_shrink: 0.0, ..default() }, Pickable::IGNORE,
            ));
            button(cuts, font, "+", MeshEditAction::SubdivisionCutsDelta(1), 26.0, false, false);
            button(cuts, font, "创建 N 边面", MeshEditAction::ToggleSubdivisionNgons, 116.0, false, false);
        });
        settings.spawn(parameter_row).with_children(|corners| {
            corners.spawn((
                feathrise_editor_ui::localization::editor_label("四边面角点"), parameter_font,
                TextColor(colors::TEXT_MUTED), TextLayout::no_wrap(), Pickable::IGNORE,
            ));
            for (label, corner, width) in [
                ("直切", SubdivisionCorner::StraightCut, 74.0),
                ("内点", SubdivisionCorner::InnerVertex, 68.0),
                ("沿路径", SubdivisionCorner::Path, 55.0),
                ("扇形", SubdivisionCorner::Fan, 52.0),
            ] {
                button(corners, font, label, MeshEditAction::SubdivisionCorner(corner), width, false, false);
            }
        });
    });
}

fn button(parent: &mut ChildSpawnerCommands, font: &Handle<Font>, label: &str,
    action: MeshEditAction, width: f32, editing_only: bool, moving_only: bool) {
    let mut button = parent.spawn((
        InteractionButton, MeshEditButton(action), FocusPolicy::Block,
        Node {
            width: px(width), height: px(26), flex_shrink: 0.0,
            display: if editing_only { Display::None } else { Display::Flex },
            border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(2)),
            align_items: AlignItems::Center, justify_content: JustifyContent::Center, ..default()
        },
        BackgroundColor(colors::SLOT_BG), BorderColor::all(colors::COPPER_BORDER_WEAK),
    ));
    if editing_only { button.insert(MeshEditOnly); }
    if moving_only { button.insert(MeshMoveOnly); }
    if matches!(action, MeshEditAction::Subdivide) {
        button.insert(crate::ui::tooltips::EditorTooltipTarget(crate::ui::tooltips::TooltipAction::MeshSubdivide));
    }
    button.with_children(|parent| {
        let mut caption = parent.spawn((
            feathrise_editor_ui::localization::editor_label(label), TextFont { font: font.clone().into(), font_size: 12.0.into(), ..default() },
            TextColor(colors::TEXT_PRIMARY), TextLayout::no_wrap(), FocusPolicy::Pass, Pickable::IGNORE,
        ));
        if matches!(action, MeshEditAction::Toggle) { caption.insert(MeshEditToggleCaption); }
    });
}

fn extrude_available(state: &MeshEditState) -> bool {
    state.active() && !state.moving() && state.kind == MeshElementKind::Face && state.selected_count() == 1
} // UI仅开放当前单面入口；拓扑容量、有效面和几何校验仍由核心在受理时统一复核。

fn action_available(state: &MeshEditState, action: MeshEditAction) -> bool {
    if matches!(action, MeshEditAction::Toggle | MeshEditAction::Cancel | MeshEditAction::Confirm) { return true; }
    if matches!(action, MeshEditAction::SubdivisionCutsDelta(_) | MeshEditAction::SubdivisionCorner(_) | MeshEditAction::ToggleSubdivisionNgons) {
        return state.subdivision_options().is_some();
    }
    if !state.active() || state.moving() || state.boxing() { return false; }
    match action {
        MeshEditAction::Extrude | MeshEditAction::Inset => extrude_available(state),
        MeshEditAction::Bevel => state.kind == MeshElementKind::Edge && state.selected_count() > 0,
        MeshEditAction::LoopCut => state.kind == MeshElementKind::Edge && state.selected_count() == 1,
        MeshEditAction::Knife => state.selected_count() > 0,
        MeshEditAction::Subdivide => state.selected_count() > 0,
        _ => true,
    }
}

// 库测试与显式验收共用同一组真实视口控件查询。
#[cfg(any(test, feature = "validation"))]
pub(crate) fn validation_mesh_button(world: &mut World, action: Option<MeshEditAction>) -> Option<Entity> {
    let entity = world
        .query::<(Entity, Option<&MeshEditButton>, Has<MeshAdvancedToggle>, &ComputedNode, &InheritedVisibility)>()
        .iter(world)
        .find_map(|(entity, button, more, node, visible)| {
            let matches = match action {
                Some(action) => button.is_some_and(|button| button.0 == action),
                None => more,
            };
            (matches && visible.get() && node.size().min_element() > 0.0).then_some(entity)
        })?;
    let mut nodes = bevy::ecs::system::SystemState::<Query<(&Node, Option<&ChildOf>)>>::new(world);
    super::ui::controls_visible(entity, &nodes.get(world).ok()?).then_some(entity)
}

/// 完成点击与几何输入之间保持显式排序；按下、隐藏旧事件及模态后的迟到请求不会执行。
pub(super) fn input(
    editor: Res<SceneEditorState>, guard: Res<SceneEditorNumericScrubMouseGuard>,
    mut state: Option<ResMut<MeshEditState>>,
    buttons: Query<(Entity, &MeshEditButton, &EditorButtonClick), Changed<EditorButtonClick>>,
    nodes: Query<(&Node, Option<&ChildOf>)>,
    toggles: Query<(Entity, &EditorButtonClick), (With<MeshAdvancedToggle>, Changed<EditorButtonClick>)>,
    mut menu: Option<ResMut<MeshAdvancedMenuState>>,
    mut keyboard: Option<ResMut<ButtonInput<KeyCode>>>,
    workspace: Option<Res<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>>,
) {
    let Some(state) = state.as_deref_mut() else { return; };
    if !state.active() || state.moving() || state.boxing() || !editor.open || editor.close_requested {
        if let Some(menu) = menu.as_deref_mut() { menu.open = false; }
    }
    if menu.as_ref().is_some_and(|menu| menu.open)
        && keyboard.as_ref().is_some_and(|keyboard| keyboard.just_pressed(KeyCode::Escape)) {
        if let Some(menu) = menu.as_deref_mut() { menu.open = false; }
        if let Some(keyboard) = keyboard.as_deref_mut() { keyboard.clear_just_pressed(KeyCode::Escape); }
        return; // Esc先收起高级区，不能同时清空元素选择。
    }
    if !editor.open || editor.close_requested || state.request.is_some()
        || workspace.as_ref().is_some_and(|workspace| workspace.active_tab != crate::ui::vegetation_preset_editor::PresetWorkspaceTab::Scene)
        || (guard.blocked() && !state.moving() && !state.boxing())
    { return; }
    if state.active() && !state.moving() && !state.boxing()
        && toggles.iter().any(|(entity, click)| click.is_clicked() && super::ui::controls_visible(entity, &nodes))
    {
        if let Some(menu) = menu.as_deref_mut() { menu.open = !menu.open; }
        return;
    }
    for (entity, button, click) in &buttons {
        if !action_available(state, button.0) { continue; } // 忙碌或元素类型不支持的迟到点击不能替换当前事务。
        if click.is_clicked() && super::ui::controls_visible(entity, &nodes) {
            state.request = Some(button.0);
            if let Some(menu) = menu.as_deref_mut() { menu.open = false; }
            break; // 元素和竖栏共用一个请求槽，保留本帧已冻结的首个意图。
        }
    }
}

pub(super) fn style(
    state: Option<Res<MeshEditState>>,
    menu: Option<Res<MeshAdvancedMenuState>>,
    locale: Option<Res<feathrise_editor_ui::localization::EditorLocale>>,
    viewports: Query<&ComputedNode, With<crate::components::SceneEditorViewport>>,
    mut roots: Query<&mut Node, (With<MeshEditToolbarRoot>, Without<MeshEditButton>, Without<MeshSelectionCaption>)>,
    mut buttons: Query<(&MeshEditButton, &UiInteraction, &mut Node, &mut BackgroundColor, Has<MeshEditOnly>, Has<MeshMoveOnly>), (Without<MeshEditToolbarRoot>, Without<MeshSelectionCaption>)>,
    mut captions: Query<(&mut Text, Has<MeshSubdivisionValue>),
        (Or<(With<MeshEditToggleCaption>, With<MeshSubdivisionValue>)>, Without<MeshSelectionCaption>)>,
    mut selection: Query<(&mut Text, &mut Node), (With<MeshSelectionCaption>, Without<MeshEditButton>, Without<MeshEditToolbarRoot>)>,
    mut advanced: Query<(&mut Node, Has<MeshAdvancedToggle>, Has<MeshSubdivisionControls>, Option<&UiInteraction>, Option<&mut BackgroundColor>),
        (Or<(With<MeshAdvancedMenuRoot>, With<MeshAdvancedToggle>, With<MeshSubdivisionControls>)>,
            Without<MeshEditToolbarRoot>, Without<MeshEditButton>, Without<MeshSelectionCaption>)>,
) {
    let locale = locale.as_deref().copied().unwrap_or_default();
    let active = state.as_ref().is_some_and(|state| state.active());
    let moving = state.as_ref().is_some_and(|state| state.moving());
    let busy = moving || state.as_ref().is_some_and(|state| state.boxing());
    let subdivision = state.as_ref().and_then(|state| state.subdivision_options());
    let menu_open = active && !busy && menu.as_ref().is_some_and(|menu| menu.open);
    let viewport_width=viewports.single().ok().map(|node|node.size().x*node.inverse_scale_factor());
    for (mut node, toggle, subdivision_controls, interaction, background) in &mut advanced {
        if !toggle {
            // 窄视口的右下方向球占独立观察区；高级按钮只在左侧折行，不与该区域争夺命中。
            let width=viewport_width.filter(|width|*width<610.0)
                .map_or(Val::Auto,|width|px((width-150.0).max(68.0)));
            if node.max_width!=width {node.max_width=width;}
        }
        let show = if subdivision_controls { subdivision.is_some() } else { active && !busy && (toggle || menu_open) };
        let display = if show { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
        if let Some(mut background) = background {
            let color = match (interaction.copied().unwrap_or_default(), menu_open) {
                (UiInteraction::Pressed, _) => colors::SELECT_BG_PRESSED,
                (UiInteraction::Hovered, _) => colors::SELECT_BG_HOVER,
                (UiInteraction::None, true) => colors::SELECT_BG,
                _ => colors::SLOT_BG,
            };
            if background.0 != color { background.0 = color; }
        }
    }
    for mut root in &mut roots {
        let left = px(50.0); // 元素变换沿用左竖栏，框选/挤出等操作始终在其旁侧折行。
        let width = px(if active { 410.0 } else { 112.0 });
        if root.left != left { root.left = left; }
        if root.max_width != width { root.max_width = width; }
    }
    for (mut caption, subdivision_value) in &mut captions {
        if subdivision_value {
            if let Some(options) = subdivision {
                let text = options.cuts.to_string();
                if caption.as_str() != text { **caption = text; }
            }
        } else {
            let text = locale.editor_label(if active { "返回对象 Tab" } else { "网格编辑 Tab" });
            if caption.as_str() != text { **caption = text.into(); }
        }
    }
    for (mut text, mut node) in &mut selection {
        let display = if active { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
        if let Some(state) = state.as_ref() {
            let caption = locale.format("mesh.edit.selection", &[
                ("count", state.selected_count().to_string()), ("kind", locale.editor_label(state.kind.label()).into()),
            ]);
            if text.as_str() != caption { **text = caption; }
        }
    }
    for (button, interaction, mut node, mut background, editing_only, moving_only) in &mut buttons {
        let hidden = editing_only && (!active || if moving_only { !busy } else { busy })
            || state.as_ref().is_some_and(|state| !action_available(state, button.0));
        let display = if hidden { Display::None } else { Display::Flex };
        if node.display != display { node.display = display; }
        let selected = state.as_ref().is_some_and(|state| active && match button.0 {
            MeshEditAction::Toggle => true,
            MeshEditAction::Kind(kind) => state.kind == kind,
            MeshEditAction::SubdivisionCorner(corner) => subdivision.is_some_and(|options| options.corner == corner),
            MeshEditAction::ToggleSubdivisionNgons => subdivision.is_some_and(|options| options.create_ngons),
            _ => false,
        });
        let color = match (*interaction, selected) {
            (UiInteraction::Pressed, _) => colors::SELECT_BG_PRESSED,
            (UiInteraction::Hovered, _) => colors::SELECT_BG_HOVER,
            (UiInteraction::None, true) => colors::SELECT_BG,
            (UiInteraction::None, false) => colors::SLOT_BG,
        };
        if background.0 != color { background.0 = color; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advanced_capabilities_and_escape_keep_the_selected_elements() {
        let mut app=App::new();
        app.insert_resource(SceneEditorState {open:true,..default()})
            .init_resource::<SceneEditorNumericScrubMouseGuard>().init_resource::<MeshEditState>()
            .init_resource::<MeshAdvancedMenuState>().init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update,input);
        {
            let mut state=app.world_mut().resource_mut::<MeshEditState>();
            state.activate_for_test();
            state.select_for_test(MeshElementKind::Vertex,1);
            assert!(!action_available(&state,MeshEditAction::Extrude));
            assert!(action_available(&state,MeshEditAction::BoxSelect));
            state.select_for_test(MeshElementKind::Face,3);
            assert!(!action_available(&state,MeshEditAction::Extrude));
            state.select_for_test(MeshElementKind::Face,1);
            assert!(action_available(&state,MeshEditAction::Extrude));
            assert!(action_available(&state,MeshEditAction::Inset));
            state.select_for_test(MeshElementKind::Edge,3);
            assert!(action_available(&state,MeshEditAction::Bevel));
            assert!(!action_available(&state,MeshEditAction::LoopCut));
            state.select_for_test(MeshElementKind::Edge,1);
            assert!(action_available(&state,MeshEditAction::LoopCut));
        }
        let button=app.world_mut().spawn((Node::default(),MeshEditButton(MeshEditAction::BoxSelect),EditorButtonClick::triggered())).id();
        app.update();
        assert!(matches!(app.world().resource::<MeshEditState>().request,Some(MeshEditAction::BoxSelect)));
        app.world_mut().resource_mut::<MeshEditState>().request=None;
        app.world_mut().entity_mut(button).insert(EditorButtonClick::default());
        app.world_mut().resource_mut::<MeshAdvancedMenuState>().open=true;
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);app.update();
        assert!(!app.world().resource::<MeshAdvancedMenuState>().open);
        assert_eq!(app.world().resource::<MeshEditState>().selected_count(),1);
        assert!(!app.world().resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::Escape));
        assert!(app.world().resource::<MeshEditState>().request.is_none());
    }

    #[test]
    fn toolbar_consumes_completed_clicks_and_discards_hidden_or_guarded_events() {
        let mut app = App::new();
        app.insert_resource(SceneEditorState { open: true, ..default() })
            .init_resource::<SceneEditorNumericScrubMouseGuard>()
            .init_resource::<MeshEditState>()
            .add_systems(Update, input);
        let button = app.world_mut().spawn((
            Node::default(), MeshEditButton(MeshEditAction::Toggle),
            UiInteraction::Pressed, EditorButtonClick::default(),
        )).id();
        app.update();
        assert!(app.world().resource::<MeshEditState>().request.is_none(), "按下不能提前切换模式");
        app.world_mut().entity_mut(button).insert(EditorButtonClick::triggered());
        app.update();
        assert!(matches!(app.world().resource::<MeshEditState>().request, Some(MeshEditAction::Toggle)));
        app.world_mut().resource_mut::<MeshEditState>().request = None;
        app.world_mut().get_mut::<Node>(button).unwrap().display = Display::None;
        app.world_mut().entity_mut(button).insert(EditorButtonClick::triggered());
        app.update();
        assert!(app.world().resource::<MeshEditState>().request.is_none());
        app.world_mut().get_mut::<Node>(button).unwrap().display = Display::Flex;
        app.world_mut().resource_mut::<SceneEditorNumericScrubMouseGuard>().advance(true, true, false);
        app.world_mut().entity_mut(button).insert(EditorButtonClick::triggered());
        app.update();
        assert!(app.world().resource::<MeshEditState>().request.is_none());
        app.world_mut().resource_mut::<SceneEditorNumericScrubMouseGuard>().advance(false, false, false);
        app.update();
        assert!(app.world().resource::<MeshEditState>().request.is_none(), "释放后不能重放旧完成点击");
    }
}
