//! 插件加载错误右下角通知：有界摘要、同内容去重，关闭不丢失管理器中的真实错误。
use super::{management, resources::NativeTools};
use bevy::{prelude::*, ui::FocusPolicy};
use feathrise_editor_map::workbench::WorkbenchHeader;
use feathrise_editor_ui::{EditorButtonClick, colors as c};
use sha2::{Digest, Sha256};

// 发现最多合并两个256项来源、16个加载失败及两个16项计划诊断；1024覆盖生产上界。
// 摘要上限与集合身份无关：每帧至多16KiB正文和16条记录，完整bytes分帧参与SHA。
const MAX_ERRORS: usize = 1024;
const SCAN_BYTES_PER_FRAME: usize = 16 * 1024;
const SCAN_ENTRIES_PER_FRAME: usize = 16;
const MAX_SUMMARY_BYTES: usize = 300;

#[derive(PartialEq, Eq)]
enum ErrorFingerprint {
    Complete(Vec<[u8; 32]>),
    // 越过明确的来源预算时报告未能完整分析，不以截断前缀冒充完整集合。
    Overflow(super::resources::DiagnosticIdentity),
}

struct DiagnosticScan {
    identity: super::resources::DiagnosticIdentity,
    previous_visible: bool,
    index: usize,
    offset: usize,
    hasher: Sha256,
    digests: Vec<[u8; 32]>,
    summary: String,
    selected_error: Option<String>,
}

impl DiagnosticScan {
    fn new(native: &NativeTools, previous_visible: bool) -> Self {
        let first = native.errors.first().or_else(|| native.management_errors.first()).map(String::as_str).unwrap_or_default();
        let summary = if first.is_empty() { String::new() } else {
            format!("{}{}  完整信息请查看插件管理器。", bounded(first, MAX_SUMMARY_BYTES),
                if first.len() > MAX_SUMMARY_BYTES { "…" } else { "" })
        };
        // ID最多128字节，只从固定小前缀定位；长错误正文不在创建扫描时同步遍历。
        let prefix = bounded(first, 132);
        let selected_error = prefix.split_once('：').or_else(|| prefix.split_once(':'))
            .and_then(|(id, _)| native.load_states.contains_key(id).then(|| id.to_owned()));
        Self { identity: native.diagnostics, previous_visible, index: 0, offset: 0,
            hasher: Sha256::new(), digests: Vec::new(), summary, selected_error }
    }

    fn step(&mut self, native: &NativeTools) -> bool {
        let mut bytes = SCAN_BYTES_PER_FRAME;
        let mut entries = SCAN_ENTRIES_PER_FRAME;
        while entries != 0 {
            let error = if self.index < native.errors.len() {
                native.errors.get(self.index)
            } else {
                native.management_errors.get(self.index - native.errors.len())
            };
            let Some(error) = error else { return true; };
            let available = error.len() - self.offset;
            if available != 0 && bytes == 0 { return false; }
            let take = available.min(bytes);
            self.hasher.update(&error.as_bytes()[self.offset..self.offset + take]);
            self.offset += take;
            bytes -= take;
            if self.offset != error.len() { return false; }
            let hash = std::mem::take(&mut self.hasher).finalize();
            let mut digest = [0; 32];
            digest.copy_from_slice(hash.as_ref());
            self.digests.push(digest);
            self.index += 1;
            self.offset = 0;
            entries -= 1;
        }
        self.index == native.errors.len() + native.management_errors.len()
    }
}

#[derive(Resource, Default)]
pub(crate) struct NotificationState {
    pub(crate) visible: bool,
    pub(crate) summary: String,
    pub(crate) count: usize,
    pub(crate) selected_error: Option<String>,
    source_revision: Option<super::resources::DiagnosticIdentity>,
    fingerprint: Option<ErrorFingerprint>,
    scan: Option<DiagnosticScan>,
    select_after_open: bool,
    #[cfg(test)]
    scanned_batches: u64,
}

#[derive(Component)]
pub(crate) struct PluginErrorNotification;
#[derive(Component)]
pub(crate) struct PluginErrorNotificationLayer;
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NotificationAction { Dismiss, Inspect }
#[derive(Component)]
struct Summary;
#[derive(Component)]
struct Title;

pub(crate) fn install(app: &mut App) {
    app.init_resource::<NotificationState>()
        .add_systems(Update, (mount, sync, control, present).chain().after(super::systems::poll));
}

fn text(font: &Handle<Font>, value: &str, size: f32, color: Color) -> impl Bundle {
    (Text::new(value), TextFont { font: font.clone().into(), font_size: size.into(), ..default() },
        TextColor(color), TextLayout { linebreak: bevy::text::LineBreak::AnyCharacter, ..default() },
        Pickable::IGNORE)
}

fn action(parent: &mut ChildSpawnerCommands, font: &Handle<Font>, value: NotificationAction, title: &str) {
    let style = if value == NotificationAction::Dismiss {
        feathrise_editor_ui::dialog_close_button_style()
    } else {
        feathrise_editor_ui::dialog_button_style(false)
    };
    parent.spawn((Button, value, style.node, style.colors, BackgroundColor(style.colors.normal),
        BorderColor::all(c::DIALOG_BORDER)))
        .with_child(text(font, title, style.font_size, style.text_color));
}

fn mount(mut commands: Commands, headers: Query<Entity, Added<WorkbenchHeader>>, assets: Res<AssetServer>) {
    let font = feathrise_editor_core::editor_font(&assets, feathrise_editor_core::FONT_PATH);
    for header in &headers {
        commands.entity(header).with_children(|root| {
            // 全屏层不参与拦截；只有右下角卡片阻挡底下控件，层级低于所有模态对话框。
            root.spawn((PluginErrorNotificationLayer, GlobalZIndex(4300), Pickable::IGNORE, FocusPolicy::Pass,
                Node { display: Display::None, position_type: PositionType::Absolute, top: px(0), left: px(0),
                    width: Val::Vw(100.), height: Val::Vh(100.), padding: UiRect::all(px(12)),
                    justify_content: JustifyContent::End, align_items: AlignItems::End, ..default() },
            )).with_children(|layer| {
                layer.spawn((PluginErrorNotification, FocusPolicy::Block, Interaction::None,
                    Node { width: px(380), max_width: percent(100), min_width: px(0),
                        flex_direction: FlexDirection::Column, row_gap: px(8), padding: UiRect::all(px(12)),
                        ..feathrise_editor_ui::dialog_panel_node() },
                    BackgroundColor(c::DIALOG_BG), BorderColor::all(c::DIALOG_BORDER),
                )).with_children(|card| {
                    card.spawn(Node { align_items: AlignItems::Center, column_gap: px(8), ..default() })
                        .with_children(|header| {
                            header.spawn((Title, text(&font, "插件加载出现问题", 13., c::TEXT_PRIMARY),
                                Node { flex_grow: 1., min_width: px(0), ..default() }));
                            action(header, &font, NotificationAction::Dismiss, "×");
                        });
                    card.spawn((Summary, text(&font, "", 12., c::TEXT_MUTED),
                        Node { max_height: px(96), overflow: Overflow::clip(), ..default() }));
                    card.spawn(Node { justify_content: JustifyContent::End, ..default() })
                        .with_children(|actions| action(actions, &font, NotificationAction::Inspect, "查看插件"));
                });
            });
        });
    }
}

fn bounded(value: &str, budget: usize) -> &str {
    let mut end = value.len().min(budget);
    while !value.is_char_boundary(end) { end -= 1; }
    &value[..end]
}

fn sync(world: &mut World) {
    let mut invalidate = false;
    world.resource_scope(|world, mut state: Mut<NotificationState>| {
        let Some(native) = world.get_resource::<NativeTools>() else {
            invalidate = state.visible || state.select_after_open;
            state.visible = false;
            state.select_after_open = false;
            state.scan = None;
            state.source_revision = None;
            state.fingerprint = None;
            return;
        };
        let revision = native.diagnostics;
        if state.source_revision != Some(revision) {
            let rebuilt = state.source_revision.is_some_and(|previous| previous.instance != revision.instance);
            let previous_visible = state.scan.as_ref().map_or(state.visible, |scan| scan.previous_visible);
            state.source_revision = Some(revision);
            state.select_after_open = false;
            state.selected_error = None;
            state.visible = false;
            state.scan = None;
            if rebuilt { state.fingerprint = None; }
            invalidate = true;
            #[cfg(test)]
            { state.scanned_batches += 1; }
            let total = native.errors.len().saturating_add(native.management_errors.len());
            if total > MAX_ERRORS {
                state.count = total;
                state.summary = format!("插件诊断超出{MAX_ERRORS}条分析预算，未能完整分析；完整记录保留在插件管理器。");
                state.fingerprint = Some(ErrorFingerprint::Overflow(revision));
                state.visible = true;
                return;
            }
            state.scan = Some(DiagnosticScan::new(native, previous_visible));
        }
        // 普通业务快照在没有扫描时只比较诊断二元组；不读盘、不遍历正文。
        let Some(mut scan) = state.scan.take() else { return; };
        if !scan.step(native) {
            state.scan = Some(scan);
            return;
        }
        if scan.identity != native.diagnostics { return; }
        // 最多1024个32字节摘要；待扫描和已采用集合各≤32KiB，完整正文从不复制。
        scan.digests.sort_unstable();
        scan.digests.dedup();
        let count = scan.digests.len();
        let fingerprint = ErrorFingerprint::Complete(scan.digests);
        let changed = state.fingerprint.as_ref() != Some(&fingerprint);
        state.fingerprint = Some(fingerprint);
        state.count = count;
        state.summary = scan.summary;
        state.selected_error = scan.selected_error;
        state.visible = count != 0 && (changed || scan.previous_visible);
    });
    if !invalidate { return; }
    // 换代立即隐藏旧卡片并撤销旧手势；只有完整集合仍属于当前身份时才原子展示。
    for (mut click, mut interaction) in world.query_filtered::<(&mut EditorButtonClick, &mut Interaction),
        With<NotificationAction>>().iter_mut(world) {
        click.invalidate();
        *interaction = Interaction::None;
    }
}

fn blocked(world: &World) -> bool {
    world.get_resource::<management::PluginManagerState>().is_some_and(|state| state.captures_input())
        || world.get_resource::<crate::native_project_close::NativeProjectCloseState>().is_some_and(|state| state.captures_input())
        || world.get_resource::<crate::project_log::ProjectLogState>().is_some_and(|state| state.captures_input())
        || world.get_resource::<crate::workspace_ui::AboutState>().is_some_and(|state| state.captures_input())
        || world.get_resource::<crate::code_iteration::diagnostics_ui::DiagnosticsOverlayState>().is_some_and(|state| state.captures_input())
        || world.get_resource::<feathrise_editor_map::workbench::WorkbenchHostPageState>().is_some_and(|state| state.settings_open)
        || crate::project_export::ui::captures_input(world)
}

fn control(world: &mut World) {
    let accepts = world.resource::<NotificationState>().visible && !blocked(world);
    let mut selected = None;
    for (action, mut click) in world.query::<(&NotificationAction, &mut EditorButtonClick)>().iter_mut(world) {
        if click.is_clicked() {
            if accepts { selected.get_or_insert(*action); }
            click.invalidate();
        }
    }
    match selected {
        Some(NotificationAction::Dismiss) => world.resource_mut::<NotificationState>().visible = false,
        Some(NotificationAction::Inspect) => {
            management::open_from_workspace(world);
            let mut state = world.resource_mut::<NotificationState>();
            state.visible = false;
            state.select_after_open = true;
        }
        None => {}
    }
    if !world.resource::<NotificationState>().select_after_open { return; }
    let target = world.resource::<NotificationState>().selected_error.clone();
    let Some(mut manager) = world.get_resource_mut::<management::PluginManagerState>() else {
        world.resource_mut::<NotificationState>().select_after_open = false;
        return;
    };
    if !manager.open {
        // 用户已经关闭管理器，不能在下一次手动打开时重放旧通知的定位请求。
        world.resource_mut::<NotificationState>().select_after_open = false;
        return;
    }
    if manager.busy { return; }
    if manager.has_project() && manager.scope != management::types::PluginScope::Project {
        // 加载失败属于当前工程的实际来源总览；不能误选同ID但健康的全局副本。
        // 范围切换会真实启动后台刷新，旧范围的行清空后再等待新目录回执。
        if let Err(error) = manager.switch_scope(management::types::PluginScope::Project) {
            manager.status = error;
            manager.revision = manager.revision.wrapping_add(1);
            world.resource_mut::<NotificationState>().select_after_open = false;
        }
        return;
    }
    if manager.rows.is_empty() { return; }
    let id = target.filter(|id| manager.rows.iter().any(|row| &row.id == id))
        .or_else(|| manager.rows.iter().find(|row| row.error.is_some()).map(|row| row.id.clone()));
    if id.is_some() && manager.selected != id {
        manager.selected = id;
        manager.revision = manager.revision.wrapping_add(1);
    }
    world.resource_mut::<NotificationState>().select_after_open = false;
}

fn present(world: &mut World) {
    let modal = blocked(world);
    world.resource_scope(|world, state: Mut<NotificationState>| {
        let display = if state.visible && !modal { Display::Flex } else { Display::None };
        for mut node in world.query_filtered::<&mut Node, With<PluginErrorNotificationLayer>>().iter_mut(world) {
            if node.display != display { node.display = display; }
        }
        for mut text in world.query_filtered::<&mut Text, With<Summary>>().iter_mut(world) {
            if text.as_str() != state.summary { **text = state.summary.clone(); }
        }
        let title = format!("插件加载出现问题（{}项）", state.count);
        for mut text in world.query_filtered::<&mut Text, With<Title>>().iter_mut(world) {
            if text.as_str() != title { **text = title.clone(); }
        }
    });
}

#[cfg(test)]
#[path = "notifications_tests.rs"]
mod tests;
