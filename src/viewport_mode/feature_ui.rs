//! 「特性」下拉只编辑一份临时勾选。整组应用走原预览配置事务，清单读取留给后台。

use super::{components::*, messages::RequestBuildFeatures, resources::{BuildProfileMenuState, FeatureMenu}};
use crate::native_play::{ProjectBuildFeatures, project_feature_catalog};
use bevy::{
    input::mouse::{MouseScrollUnit, MouseWheel},
    prelude::*,
    tasks::IoTaskPool,
    ui::{FocusPolicy, InteractionDisabled, RelativeCursorPosition},
};
use feathrise_editor_ui::{
    ButtonColors, EditorButtonClick, InteractionButton, UiInteraction, colors as c,
    localization::EditorLocale,
};

const ROW_HEIGHT: f32 = 26.;
const VISIBLE_ROWS: usize = 8;

pub(super) fn mount(group: &mut ChildSpawnerCommands, font: &Handle<Font>) {
    group.spawn(Node { height: px(26), flex_shrink: 0., ..default() }).with_children(|anchor| {
        anchor.spawn((button(), FeatureToggle, Name::new("特性"))).with_child(
            (FeatureToggleLabel, text(font, "特性 ▼")),
        );
        // 固定可见窗口，超出部分通过滚动访问；为大量特性也只创建可见行和一行过扫。
        anchor.spawn((FeaturePanel, GlobalZIndex(4201), FocusPolicy::Block, UiInteraction::None, RelativeCursorPosition::default(),
            Node { display: Display::None, position_type: PositionType::Absolute, right: px(0), top: px(28),
                width: px(224), flex_direction: FlexDirection::Column, row_gap: px(4),
                padding: UiRect::all(px(4)), border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(3)), ..default() },
            BackgroundColor(c::PANEL_BG_STRONG), BorderColor::all(c::COPPER_BORDER_WEAK),
        )).with_children(|menu| {
            menu.spawn((FeatureStatusLabel, text(font, "")));
            menu.spawn((FeatureList, RelativeCursorPosition::default(), ScrollPosition::default(), UiInteraction::None,
                Node { width: percent(100), max_height: Val::Vh(40.), overflow: Overflow::scroll_y(), ..default() },
            )).with_children(|pane| {
                pane.spawn((FeatureListContent, Node { width: percent(100), ..default() })).with_children(|content| {
                    for slot in 0..=VISIBLE_ROWS {
                        content.spawn((
                            button(), FeatureRow { slot, index: None, revision: 0, checked: false },
                            InteractionDisabled, Name::new("特性选项"),
                        )).with_child((FeatureRowLabel, text(font, "")));
                    }
                });
            });
            menu.spawn((button(), ApplyFeaturesButton, InteractionDisabled, Name::new("应用特性"))).with_child((
                feathrise_editor_ui::localization::LocalizedText::with_args("toolbar.launch.features.apply", Vec::new()),
                text(font, "应用"),
            ));
        });
    });
}

fn button() -> impl Bundle {
    (
        InteractionButton,
        ButtonColors::new(c::BUTTON_DEV_SMALL_NORMAL, c::BUTTON_DEV_SMALL_HOVERED, c::BUTTON_DEV_SMALL_PRESSED),
        Node { height: px(ROW_HEIGHT), flex_shrink: 0., padding: UiRect::horizontal(px(7)),
            align_items: AlignItems::Center, border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(3)), ..default() },
        BackgroundColor(c::BUTTON_DEV_SMALL_NORMAL), BorderColor::all(c::COPPER_BORDER_WEAK),
    )
}

fn text(font: &Handle<Font>, value: &str) -> impl Bundle {
    (Text::new(value), TextFont { font: font.clone().into(), font_size: 12.0.into(), ..default() },
        TextColor(c::TEXT_PRIMARY), TextLayout::no_wrap(), Pickable::IGNORE)
}

pub(super) fn input(
    mut state: ResMut<BuildProfileMenuState>,
    menu: Option<ResMut<FeatureMenu>>,
    selected: Res<ProjectBuildFeatures>,
    options: Query<(&FeatureRow, &EditorButtonClick), Changed<EditorButtonClick>>,
    apply: Query<&EditorButtonClick, (With<ApplyFeaturesButton>, Changed<EditorButtonClick>)>,
    mut requests: MessageWriter<RequestBuildFeatures>,
    mut feedback: MessageWriter<feathrise_editor_scene::standalone::SavedPlayFeedback>,
) {
    let Some(mut menu) = menu else { return; };
    if !state.features_open() {
        if menu.was_open { menu.was_open = false; }
        return;
    }
    if !menu.was_open {
        menu.was_open = true;
        menu.revision = menu.revision.wrapping_add(1);
        menu.draft = selected.names().to_vec();
        menu.draft_default = selected.default_enabled();
        menu.catalog = None;
        // 关闭后重开要读新清单；在途任务仍占同一个槽，旧结果完成后再发本次读取。
        menu.discard_read = menu.loading.is_some();
    }
    if let Some(result) = menu.loading.as_ref().and_then(|loading| match loading.try_recv() {
        Ok(result) => Some(result),
        Err(async_channel::TryRecvError::Empty) => None,
        Err(async_channel::TryRecvError::Closed) => Some(Err("特性读取任务未返回".into())),
    }) {
        menu.loading = None;
        if !menu.discard_read {
            match &result {
                Ok(names) => menu.draft.retain(|name| names.binary_search(name).is_ok()),
                Err(error) => { feedback.write(feathrise_editor_scene::standalone::SavedPlayFeedback(error.clone())); }
            }
            menu.catalog = Some(result);
        }
    }
    if menu.catalog.is_none() && menu.loading.is_none() {
        let (send, recv) = async_channel::bounded(1);
        let path = menu.manifest.clone();
        menu.loading = Some(recv);
        menu.discard_read = false;
        IoTaskPool::get().spawn(async move {
            let _ = send.try_send(project_feature_catalog(&path));
        }).detach();
    }
    if !matches!(menu.catalog.as_ref(), Some(Ok(_))) { return; }
    for (row, click) in &options {
        if !click.is_clicked() || row.revision != menu.revision { continue; }
        let Some(name) = row.index.and_then(|index| menu.names().get(index)).cloned() else { continue; };
        if name == "default" {
            menu.draft_default = !menu.draft_default;
            continue;
        }
        match menu.draft.binary_search(&name) {
            Ok(index) => { menu.draft.remove(index); }
            Err(index) => menu.draft.insert(index, name),
        }
    }
    if apply.iter().any(EditorButtonClick::is_clicked) && menu.changed(&selected) {
        let draft = ProjectBuildFeatures::new(menu.draft_default, std::mem::take(&mut menu.draft));
        requests.write(RequestBuildFeatures(draft));
        state.open = false;
        menu.was_open = false;
    }
}

pub(super) fn present(
    mut commands: Commands,
    state: Res<BuildProfileMenuState>,
    menu: Res<FeatureMenu>,
    selected: Res<ProjectBuildFeatures>,
    locale: Option<Res<EditorLocale>>,
    mut headings: Query<(&mut Text, Has<FeatureToggleLabel>), Or<(With<FeatureToggleLabel>, With<FeatureStatusLabel>)>>,
    mut apply: Query<(Entity, Has<InteractionDisabled>), With<ApplyFeaturesButton>>,
    mut panes: Query<(&RelativeCursorPosition, &ComputedNode, &mut Node, &mut ScrollPosition),
        (With<FeatureList>, Without<FeatureListContent>, Without<FeatureRow>)>,
    mut content: Query<&mut Node, (With<FeatureListContent>, Without<FeatureRow>)>,
    mut rows: Query<(Entity, &Children, &mut FeatureRow, &mut Node, &mut EditorButtonClick, &mut UiInteraction,
        &mut BorderColor, Has<InteractionDisabled>)>,
    mut labels: Query<&mut Text, (With<FeatureRowLabel>, Without<FeatureToggleLabel>, Without<FeatureStatusLabel>)>,
    mut wheels: MessageReader<MouseWheel>,
    mut revision: Local<Option<u64>>,
) {
    let delta: f32 = wheels.read().map(|wheel| match wheel.unit {
        MouseScrollUnit::Line => wheel.y * ROW_HEIGHT * 2.,
        MouseScrollUnit::Pixel => wheel.y,
    }).sum();
    let locale = locale.as_deref().copied().unwrap_or_default();
    let names = menu.names();
    let status = match &menu.catalog {
        None => "toolbar.launch.features.loading",
        Some(Err(_)) => "toolbar.launch.features.unavailable",
        Some(Ok(names)) if names.is_empty() => "toolbar.launch.features.empty",
        Some(Ok(names)) if names.len() > VISIBLE_ROWS => "toolbar.launch.features.scroll",
        Some(Ok(_)) => "toolbar.launch.features.saved",
    };
    for (mut label, caption) in &mut headings {
        if caption {
            let title = locale.text("toolbar.launch.features");
            let value = if selected.names().is_empty() { format!("{title} ▼") }
                else { format!("{title} ({}) ▼", selected.names().len()) };
            if label.as_str() != value { **label = value; }
        } else {
            let value = locale.text(status);
            if label.as_str() != value { **label = value.into(); }
        }
    }
    if !state.features_open() { return; }

    let can_apply = matches!(menu.catalog.as_ref(), Some(Ok(_))) && menu.changed(&selected);
    for (entity, disabled) in &mut apply {
        if can_apply == disabled {
            if can_apply { commands.entity(entity).remove::<InteractionDisabled>(); }
            else { commands.entity(entity).insert(InteractionDisabled); }
        }
    }

    let mut first = 0;
    let total = names.len() as f32 * ROW_HEIGHT;
    for (cursor, computed, mut node, mut position) in &mut panes {
        let height = px(total.min(VISIBLE_ROWS as f32 * ROW_HEIGHT));
        if node.height != height { node.height = height; }
        let visible_height = computed.content_box().height() * computed.inverse_scale_factor();
        let maximum = (total - visible_height).max(0.);
        let scroll = if *revision != Some(menu.revision) { 0. }
            else { (position.y - if cursor.cursor_over() { delta } else { 0. }).clamp(0., maximum) };
        if position.y != scroll { position.y = scroll; }
        first = (scroll / ROW_HEIGHT).floor() as usize;
    }
    *revision = Some(menu.revision);
    for mut node in &mut content {
        if node.height != px(total) { node.height = px(total); }
    }
    for (entity, children, mut row, mut node, mut click, mut interaction, mut border, disabled) in &mut rows {
        let index = first + row.slot;
        let name = names.get(index);
        let bound = name.map(|_| index);
        let checked = name.is_some_and(|name| {
            if name == "default" { menu.draft_default } else { menu.draft.binary_search(name).is_ok() }
        });
        let rebound = row.index != bound || row.revision != menu.revision;
        if !rebound && checked == row.checked { continue; }
        if rebound {
            // 行池滚动后换了特性，按下旧行再释放不能勾选新位置。
            row.index = bound;
            row.revision = menu.revision;
            click.invalidate();
            *interaction = UiInteraction::None;
            node.position_type = PositionType::Absolute;
            node.width = percent(100);
            node.top = px(index as f32 * ROW_HEIGHT);
            node.display = if name.is_some() { Display::Flex } else { Display::None };
            if name.is_some() == disabled {
                if disabled { commands.entity(entity).remove::<InteractionDisabled>(); }
                else { commands.entity(entity).insert(InteractionDisabled); }
            }
        }
        row.checked = checked;
        *border = BorderColor::all(if checked { c::COPPER_BORDER } else { c::COPPER_BORDER_WEAK });
        if let Some(name) = name && let Some(label) = children.first() && let Ok(mut text) = labels.get_mut(*label) {
            **text = format!("{} {name}", if checked { "✓" } else { "□" });
        }
    }
}
