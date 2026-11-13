//! 类型菜单和动画展开共用一个锚定浮层；动作点击只接受当前目录与原片段身份。

mod scene;

use super::{filter::ProjectTypeFilterButton, input::BrowserInputGate, ProjectBrowserState, ProjectBrowserView};
use crate::{
    asset_catalog::{AssetTypeFilter, EditorAssetCatalog, project_files::EditorProjectFileCatalog},
    components::AssetRowBinding,
    resources::SceneEditorState,
    ui::{asset_preview::PickProjectAnimationClip, rows::AssetViewport},
};
use bevy::{input::mouse::MouseWheel, prelude::*, ui::RelativeCursorPosition};
use feathrise_editor_ui::{EditorButtonClick, UiInteraction};
use std::path::PathBuf;

pub(super) use scene::{bind_expanders, mount, show};
pub(crate) use scene::spawn_expander;

#[derive(Component)]
pub(crate) struct ProjectTypeFilterOption(pub(crate) AssetTypeFilter);

#[derive(Component, Default)]
pub(crate) struct ProjectAnimationExpand(pub(crate) Option<AssetRowBinding>);

#[derive(Component)]
pub(crate) struct ProjectAnimationClip {
    slot: usize,
    pub(crate) clip_index: Option<usize>,
}

#[derive(Component)]
pub(super) struct Choices;

struct FilterScope {
    view: ProjectBrowserView,
    root: PathBuf,
    context: Option<u64>,
}

struct ClipSource {
    revision: u64,
    viewport_revision: u64,
    entry_index: usize,
    path: String,
}

impl ClipSource {
    fn current(&self, browser: &ProjectBrowserState, catalog: &EditorAssetCatalog) -> bool {
        browser.assets_visible() && browser.type_filter == AssetTypeFilter::Animation
            && !catalog.is_loading() && catalog.adopted_type_filter == AssetTypeFilter::Animation
            && self.revision == catalog.applied_revision && self.viewport_revision == catalog.viewport_revision
            && catalog.entries.get(self.entry_index).is_some_and(|entry| entry.asset_path == self.path)
    }
}

enum PopupKind {
    Filter(FilterScope),
    Clips(ClipSource),
}

struct OpenPopup {
    anchor: Entity,
    kind: PopupKind,
    scroll: f32,
    first: usize,
    visible: usize,
}

impl OpenPopup {
    fn new(anchor: Entity, kind: PopupKind) -> Self {
        Self { anchor, kind, scroll: 0.0, first: 0, visible: 0 }
    }
}

#[derive(Resource, Default)]
pub(crate) struct ProjectBrowserPopup {
    open: Option<OpenPopup>,
    picked: Option<(ClipSource, usize)>,
    captured: bool,
}

impl ProjectBrowserPopup {
    pub(crate) fn owns_mouse(&self) -> bool {
        self.open.is_some() || self.captured
    }
}

pub(super) fn control(
    gate: BrowserInputGate,
    editor: Res<SceneEditorState>,
    mut browser: ResMut<ProjectBrowserState>,
    mut files: ResMut<EditorProjectFileCatalog>,
    mut catalog: ResMut<EditorAssetCatalog>,
    mut popup: ResMut<ProjectBrowserPopup>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut wheels: MessageReader<MouseWheel>,
    viewport: AssetViewport,
    triggers: Query<(Entity, &EditorButtonClick), (With<ProjectTypeFilterButton>, Changed<EditorButtonClick>)>,
    expanders: Query<(Entity, &ProjectAnimationExpand, &EditorButtonClick), Changed<EditorButtonClick>>,
    choices: Query<(&Node, &UiInteraction, &RelativeCursorPosition, Ref<EditorButtonClick>,
        Option<&ProjectTypeFilterOption>, Option<&ProjectAnimationClip>),
        Or<(With<ProjectTypeFilterOption>, With<ProjectAnimationClip>)>>,
    panel: Query<&RelativeCursorPosition, With<Choices>>,
) {
    let delta = wheels.read().map(|event| super::super::systems::editor_scroll_delta(event.unit, event.y)).sum::<f32>();
    let was_open = popup.open.is_some();
    if popup.captured != was_open { popup.captured = was_open; }
    if !gate.context_available(&editor) || keys.just_pressed(KeyCode::F12) {
        if was_open { popup.open = None; }
        if keys.just_pressed(KeyCode::F12) { popup.captured = false; }
        return;
    }
    if let Some(open) = popup.open.as_ref() {
        let length = match &open.kind {
            PopupKind::Filter(scope) if browser.view == scope.view
                && files.source_context() == scope.context && files.project_root() == Some(scope.root.as_path()) => super::filter::FILTERS.len(),
            PopupKind::Clips(source) if source.current(&browser, &catalog) && viewport.is_current(&catalog) =>
                catalog.animation_clips(&source.path).len(),
            _ => 0,
        };
        if length == 0 || keys.just_pressed(KeyCode::Escape)
            || mouse.just_pressed(MouseButton::Right) || mouse.just_pressed(MouseButton::Middle)
        {
            popup.open = None;
            return;
        }
        let mut hits_choice = false;
        for (node, interaction, cursor, click, filter, clip) in &choices {
            if node.display == Display::None { continue; }
            hits_choice |= cursor.cursor_over() && *interaction == UiInteraction::Pressed;
            if !click.is_changed() || !click.is_clicked() { continue; }
            match (&open.kind, filter, clip) {
                (PopupKind::Filter(_), Some(filter), _) => {
                    browser.set_type_filter(filter.0, &mut files, &mut catalog);
                    popup.open = None;
                    return;
                }
                (PopupKind::Clips(source), _, Some(clip)) => {
                    let index = catalog.animation_clips(&source.path).get(open.first + clip.slot)
                        .map(|clip| clip.index).filter(|index| Some(*index) == clip.clip_index);
                    if let Some(index) = index
                        && let Some(OpenPopup { kind: PopupKind::Clips(source), .. }) = popup.open.take()
                    {
                        // 先关闭遮罩，下一静默帧再交给原预览输入门。
                        popup.picked = Some((source, index));
                    }
                    return;
                }
                _ => {}
            }
        }
        if mouse.just_pressed(MouseButton::Left) && !hits_choice {
            popup.open = None;
        } else if delta.is_finite() && delta != 0.0 && panel.iter().any(RelativeCursorPosition::cursor_over)
            && let Some(open) = popup.open.as_mut()
        {
            let last = length.saturating_sub(open.visible) as f32;
            open.scroll = (open.scroll + delta / scene::OPTION_PITCH).clamp(0.0, last);
        }
        return;
    }
    if !gate.available(&editor) { return; }
    if let Some((anchor, _)) = triggers.iter().find(|(_, click)| click.is_clicked())
        && let Some(root) = files.project_root()
    {
        popup.open = Some(OpenPopup::new(anchor, PopupKind::Filter(FilterScope {
            view: browser.view, root: root.to_path_buf(), context: files.source_context(),
        })));
        return;
    }
    for (anchor, button, click) in &expanders {
        let Some(binding) = button.0.filter(|_| click.is_clicked()) else { continue; };
        let Some(entry_index) = binding.index else { continue; };
        let Some(entry) = catalog.entries.get(entry_index) else { continue; };
        let source = ClipSource { revision: binding.revision, viewport_revision: binding.viewport_revision,
            entry_index, path: entry.asset_path.clone() };
        if source.current(&browser, &catalog) && viewport.is_current(&catalog)
            && !catalog.animation_clips(&source.path).is_empty()
        {
            popup.open = Some(OpenPopup::new(anchor, PopupKind::Clips(source)));
            break;
        }
    }
}

pub(super) fn publish_clip(
    gate: BrowserInputGate,
    editor: Res<SceneEditorState>,
    browser: Res<ProjectBrowserState>,
    catalog: Res<EditorAssetCatalog>,
    viewport: AssetViewport,
    mut popup: ResMut<ProjectBrowserPopup>,
    mut requests: Option<ResMut<Messages<PickProjectAnimationClip>>>,
) {
    let Some((source, _)) = popup.picked.as_ref() else { return; };
    if !gate.context_available(&editor) || !source.current(&browser, &catalog) || !viewport.is_current(&catalog) {
        popup.picked = None;
        return;
    }
    if popup.owns_mouse() || !gate.available(&editor) { return; }
    if let Some((source, clip_index)) = popup.picked.take()
        && let Some(requests) = requests.as_mut()
    {
        requests.write(PickProjectAnimationClip { asset_path: source.path, clip_index, target: None });
    }
}
