//! 「特性」下拉只编辑一份临时勾选。整组应用走原预览配置事务，清单读取留给后台。

use super::{components::*, messages::RequestBuildFeatures, resources::{BuildProfileMenuState, FeatureMenu}};
use crate::native_play::{ProjectBuildFeatures, project_feature_catalog};
use bevy::{
    input::mouse::{MouseScrollUnit, MouseWheel},
    prelude::*,
    tasks::IoTaskPool,
    ui::{FocusPolicy, InteractionDisabled, RelativeCursorPosition},
    ui_widgets::{ControlOrientation, Scrollbar, ScrollbarThumb},
};
use feathrise_editor_ui::{
    ButtonColors, EditorButtonClick, InteractionButton, UiInteraction, colors as c,
    localization::EditorLocale,
};

const ROW_HEIGHT: f32 = 26.;

pub(super) fn mount(group: &mut ChildSpawnerCommands, font: &Handle<Font>) {
    group.spawn(Node { height: px(26), flex_shrink: 0., ..default() }).with_children(|anchor| {
        anchor.spawn((button(), FeatureToggle, Name::new("特性"))).with_child(
            (FeatureToggleLabel, text(font, "特性 ▼")),
        );
        // 面板总高含边框和底栏，超出窗口一半时只压缩中间的列表区。
        anchor.spawn((FeaturePanel, Name::new("特性面板"), GlobalZIndex(4201), FocusPolicy::Block,
            UiInteraction::None, RelativeCursorPosition::default(),
            Node { display: Display::None, position_type: PositionType::Absolute, right: px(0), top: px(28),
                width: px(280), max_height: Val::Vh(50.), flex_direction: FlexDirection::Column, row_gap: px(4),
                padding: UiRect::all(px(4)), border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(3)), ..default() },
            BackgroundColor(c::PANEL_BG_STRONG), BorderColor::all(c::COPPER_BORDER_WEAK),
        )).with_children(|menu| {
            menu.spawn((FeatureStatusLabel, text(font, ""), Node { display: Display::None, flex_shrink: 0., ..default() }));
            menu.spawn(Node { min_height: px(0), column_gap: px(4), ..default() }).with_children(|region| {
                let list = region.spawn((FeatureList, RelativeCursorPosition::default(), ScrollPosition::default(), UiInteraction::None,
                    Node { flex_grow: 1., min_width: px(0), min_height: px(0), overflow: Overflow::scroll_y(), ..default() },
                )).with_child((
                    FeatureListContent(font.clone()),
                    Node { width: percent(100), flex_shrink: 0., ..default() },
                )).id();
                // 原生拖动与滚轮作用在同一个列表 ScrollPosition 上。
                region.spawn((FeatureScrollbar, Name::new("特性滚动条"),
                    Scrollbar::new(list, ControlOrientation::Vertical, 24.),
                    RelativeCursorPosition::default(), FocusPolicy::Block, UiInteraction::None,
                    Node { width: px(8), flex_shrink: 0., display: Display::None,
                        border_radius: BorderRadius::all(px(4)), ..default() },
                    BackgroundColor(c::PANEL_BG_SECTION),
                )).with_child((
                    ScrollbarThumb { border_radius: BorderRadius::all(px(4)), ..default() },
                    BackgroundColor(c::COPPER_BORDER),
                ));
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
            align_items: AlignItems::Center, justify_content: JustifyContent::SpaceBetween, column_gap: px(6),
            border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(3)), ..default() },
        BackgroundColor(c::BUTTON_DEV_SMALL_NORMAL), BorderColor::all(c::COPPER_BORDER_WEAK),
    )
}

fn text(font: &Handle<Font>, value: &str) -> impl Bundle {
    (Text::new(value), TextFont { font: font.clone().into(), font_size: 12.0.into(), ..default() },
        TextColor(c::TEXT_PRIMARY), TextLayout::no_wrap(), Pickable::IGNORE)
}

fn spawn_row(content: &mut ChildSpawnerCommands, font: &Handle<Font>, slot: usize) {
    content.spawn((
        button(), FeatureRow { slot, index: None, revision: 0, checked: false, inherited: false },
        InteractionDisabled, Name::new("特性选项"),
    )).with_children(|row| {
        row.spawn((FeatureRowLabel, text(font, ""), Node { min_width: px(0), overflow: Overflow::clip_x(), ..default() }));
        row.spawn((FeatureDefaultBadge, text(font, ""), Node { flex_shrink: 0., ..default() }));
    });
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
                Ok(options) => {
                    // default 单独置顶，其余特性仍按名称排序，供已保存选择做二分查找。
                    let first = usize::from(options.first().is_some_and(|option| option.name == "default"));
                    menu.draft.retain(|name| options[first..].binary_search_by(|option| option.name.cmp(name)).is_ok());
                }
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
        let Some(option) = row.index.and_then(|index| menu.options().get(index)) else { continue; };
        if option.name == "default" {
            menu.draft_default = !menu.draft_default;
            continue;
        }
        // 默认组成员随组生效。个人手动选择单独保留，仅在可单独选择时修改。
        if menu.draft_default && option.in_default { continue; }
        let name = option.name.clone();
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

pub(super) fn controls(
    mut commands: Commands,
    state: Res<BuildProfileMenuState>,
    menu: Res<FeatureMenu>,
    selected: Res<ProjectBuildFeatures>,
    locale: Option<Res<EditorLocale>>,
    mut headings: Query<(&mut Text, &mut Node, Has<FeatureToggleLabel>), Or<(With<FeatureToggleLabel>, With<FeatureStatusLabel>)>>,
    apply: Query<(Entity, Has<InteractionDisabled>), With<ApplyFeaturesButton>>,
) {
    let locale = locale.as_deref().copied().unwrap_or_default();
    let status = match &menu.catalog {
        None => Some("toolbar.launch.features.loading"),
        Some(Err(_)) => Some("toolbar.launch.features.unavailable"),
        Some(Ok(options)) if options.is_empty() => Some("toolbar.launch.features.empty"),
        Some(Ok(_)) => None,
    };
    for (mut label, mut node, caption) in &mut headings {
        if caption {
            let title = locale.text("toolbar.launch.features");
            let value = if selected.names().is_empty() { format!("{title} ▼") }
                else { format!("{title} ({}) ▼", selected.names().len()) };
            if label.as_str() != value { **label = value; }
        } else {
            let display = if status.is_some() { Display::Flex } else { Display::None };
            if node.display != display { node.display = display; }
            let value = status.map_or("", |key| locale.text(key));
            if label.as_str() != value { **label = value.into(); }
        }
    }
    if !state.features_open() { return; }

    let can_apply = matches!(menu.catalog.as_ref(), Some(Ok(_))) && menu.changed(&selected);
    for (entity, disabled) in &apply {
        if can_apply == disabled {
            if can_apply { commands.entity(entity).remove::<InteractionDisabled>(); }
            else { commands.entity(entity).insert(InteractionDisabled); }
        }
    }
}

pub(super) fn present(
    mut commands: Commands,
    state: Res<BuildProfileMenuState>,
    menu: Res<FeatureMenu>,
    locale: Option<Res<EditorLocale>>,
    mut panes: Query<(&RelativeCursorPosition, &ComputedNode, &mut ScrollPosition), With<FeatureList>>,
    mut tracks: Query<(&RelativeCursorPosition, &mut Node), With<FeatureScrollbar>>,
    mut content: Query<(Entity, &FeatureListContent, &mut Node, Option<&Children>), Without<FeatureScrollbar>>,
    mut rows: Query<(Entity, &Children, &mut FeatureRow, &mut Node, &mut EditorButtonClick, &mut UiInteraction,
        &mut BorderColor, Has<InteractionDisabled>), (Without<FeatureScrollbar>, Without<FeatureListContent>)>,
    mut labels: Query<(&mut Text, &mut TextColor), Or<(With<FeatureRowLabel>, With<FeatureDefaultBadge>)>>,
    mut wheels: MessageReader<MouseWheel>,
    mut revision: Local<Option<u64>>,
) {
    let delta: f32 = wheels.read().map(|wheel| match wheel.unit {
        MouseScrollUnit::Line => wheel.y * ROW_HEIGHT * 2.,
        MouseScrollUnit::Pixel => wheel.y,
    }).sum();
    if !state.features_open() { return; }
    let Ok((cursor, computed, mut position)) = panes.single_mut() else { return; };
    let Ok((content_id, font, mut content_node, children)) = content.single_mut() else { return; };
    let options = menu.options();
    let total = options.len() as f32 * ROW_HEIGHT;
    if content_node.height != px(total) { content_node.height = px(total); }

    let inverse_scale = computed.inverse_scale_factor();
    let visible = computed.content_box().height() * inverse_scale;
    let maximum = (total - visible).max(0.);
    let overflow = visible > 0. && maximum > inverse_scale * 0.5;
    let mut hovered = cursor.cursor_over();
    for (cursor, mut track) in &mut tracks {
        hovered |= overflow && cursor.cursor_over();
        let display = if overflow { Display::Flex } else { Display::None };
        if track.display != display { track.display = display; }
    }
    let scroll = if *revision != Some(menu.revision) || !overflow { 0. }
        else { (position.y - if hovered { delta } else { 0. }).clamp(0., maximum) };
    if position.y != scroll { position.y = scroll; }
    *revision = Some(menu.revision);

    // 按实际可见高度扩充行池，滚到半行时多留一行，不随清单总量创建控件。
    let visible_rows = if visible > 0. { ((visible / ROW_HEIGHT).ceil() as usize + 1).min(options.len()) } else { 0 };
    let row_count = children.map_or(0, |children| children.len());
    if visible_rows > row_count {
        commands.entity(content_id).with_children(|parent| {
            for slot in row_count..visible_rows { spawn_row(parent, &font.0, slot); }
        });
    }
    let first = (scroll / ROW_HEIGHT).floor() as usize;
    let locale_changed = locale.as_ref().is_some_and(|locale| locale.is_changed());
    let locale = locale.as_deref().copied().unwrap_or_default();
    for (entity, children, mut row, mut node, mut click, mut interaction, mut border, disabled) in &mut rows {
        let index = first + row.slot;
        let option = options.get(index).filter(|_| row.slot < visible_rows);
        let bound = option.map(|_| index);
        let inherited = option.is_some_and(|option| option.in_default && menu.draft_default);
        let checked = inherited || option.is_some_and(|option| {
            if option.name == "default" { menu.draft_default } else { menu.draft.binary_search(&option.name).is_ok() }
        });
        let rebound = row.index != bound || row.revision != menu.revision;
        let should_disable = option.is_none() || inherited;
        if !rebound && checked == row.checked && inherited == row.inherited && disabled == should_disable && !locale_changed { continue; }
        if rebound || inherited != row.inherited {
            // 滚动换行或改为默认组接管后，释放旧按下不能触发另一个选择。
            click.invalidate();
            *interaction = UiInteraction::None;
        }
        if rebound {
            row.index = bound;
            row.revision = menu.revision;
            node.position_type = PositionType::Absolute;
            node.width = percent(100);
            node.top = px(index as f32 * ROW_HEIGHT);
            node.display = if bound.is_some() { Display::Flex } else { Display::None };
        }
        if should_disable != disabled {
            if should_disable { commands.entity(entity).insert(InteractionDisabled); }
            else { commands.entity(entity).remove::<InteractionDisabled>(); }
        }
        row.checked = checked;
        row.inherited = inherited;
        *border = BorderColor::all(if checked { c::COPPER_BORDER } else { c::COPPER_BORDER_WEAK });
        let Some(option) = option else { continue; };
        if let Ok([(mut name, _), (mut badge, mut color)]) = labels.get_many_mut([children[0], children[1]]) {
            **name = format!("{} {}", if checked { "✓" } else { "□" }, option.name);
            let value = if inherited { locale.text("toolbar.launch.features.inherited") }
                else if option.in_default { locale.text("toolbar.launch.features.default_member") }
                else { "" };
            **badge = value.into();
            color.0 = if inherited { c::TEXT_ACCENT } else { c::TEXT_MUTED };
        }
    }
}
