//! 中央 GLB 视图工具条。复用场景控件外形和原有预览相机输入门，独立保存文件的观察与吸附偏好。

use std::borrow::Cow;

use bevy::{ecs::system::SystemParam, prelude::*, ui::InteractionDisabled};
use feathrise_editor_core::{EditorSnapMode, EditorSnapSettings};
use feathrise_editor_ui::{EditorButtonClick, UiInteraction, localization::EditorLocale, metrics};

use crate::{
    SceneEditorSystemSet,
    resources::SceneEditorState,
    types::EditorView,
    ui::{
        theme as colors, vegetation_preset_editor::VegetationPresetEditorState, viewport_toolbar,
    },
};

use super::{
    AssetPreviewNavigation, AssetPreviewWorkspace, DocumentState, rigging::RiggingState, types::*,
    view_math::AssetPreviewView,
};

#[derive(Component, Clone, Copy, Default)]
struct ViewToolbar;

#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Action {
    Direction(EditorView),
    #[default]
    Orthographic,
    FocusSelected,
    FrameAll,
    Grid,
    Snap,
    SnapMode,
    SnapStep,
}

impl Action {
    fn apply(
        self,
        view: &mut AssetPreviewView,
        bounds: AssetPreviewModelBounds,
        front_yaw: f32,
        selected: Option<Vec3>,
    ) {
        match self {
            Self::Direction(direction) => view.set_direction(direction, front_yaw),
            Self::Orthographic => view.orthographic = !view.orthographic,
            Self::FocusSelected => view.frame_model(bounds.minimum, bounds.maximum, selected),
            Self::FrameAll => view.frame_model(bounds.minimum, bounds.maximum, None),
            Self::Grid => view.snapping.show_grid = !view.snapping.show_grid,
            Self::Snap => view.snapping.enabled = !view.snapping.enabled,
            Self::SnapMode => {
                view.snapping.mode = match view.snapping.mode {
                    EditorSnapMode::WorldGrid => EditorSnapMode::Relative,
                    EditorSnapMode::Relative => EditorSnapMode::WorldGrid,
                };
            }
            Self::SnapStep => {
                const STEPS: [f32; 10] = [0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0];
                view.snapping.translation_meters = STEPS
                    .into_iter()
                    .find(|step| *step > view.snapping.translation_meters + 0.000_001)
                    .unwrap_or(STEPS[0]);
            }
        }
    }

    fn selected(self, view: AssetPreviewView, direction: EditorView) -> bool {
        match self {
            Self::Direction(candidate) => candidate == direction,
            Self::Orthographic => view.orthographic,
            Self::Grid => view.snapping.show_grid,
            Self::Snap => view.snapping.enabled,
            _ => false,
        }
    }

    fn caption(self, locale: EditorLocale, snap: EditorSnapSettings) -> Cow<'static, str> {
        let key = match self {
            Self::Direction(EditorView::Perspective) => "label.free",
            Self::Direction(EditorView::Top) => "label.top",
            Self::Direction(EditorView::Front) => "label.front",
            Self::Direction(EditorView::Side) => "label.side",
            Self::Orthographic => "label.orthographic",
            Self::FocusSelected => "label.frame-selection",
            Self::FrameAll => "label.frame-all",
            Self::Grid => "label.grid",
            Self::Snap if snap.enabled => "viewport.snap.on",
            Self::Snap => "viewport.snap.off",
            Self::SnapMode => return locale.editor_label(snap.mode.label()).into(),
            Self::SnapStep => {
                return locale
                    .format(
                        "viewport.snap.step",
                        &[("meters", snap.translation_meters.to_string())],
                    )
                    .into();
            }
        };
        locale.text(key).into()
    }
}

fn button(label: &str, action: Action, font_size: f32) -> impl Scene {
    bsn! {
        @viewport_toolbar::control_surface()
        ~{action}
        Children [@viewport_toolbar::control_text(label, font_size)]
    }
}

pub(super) fn scene() -> impl Scene {
    let controls = bsn! {
        Children [
                @viewport_toolbar::control_group()
                Children [
                    @button("自由", Action::Direction(EditorView::Perspective), metrics::FONT_CONTROL)--
                    @button("顶视", Action::Direction(EditorView::Top), metrics::FONT_CONTROL)--
                    @button("前视", Action::Direction(EditorView::Front), metrics::FONT_CONTROL)--
                    @button("侧视", Action::Direction(EditorView::Side), metrics::FONT_CONTROL)--
                    @button("正交", Action::Orthographic, metrics::FONT_CONTROL)--
                    @button("聚焦所选", Action::FocusSelected, metrics::FONT_CONTROL)--
                    @button("查看全部", Action::FrameAll, metrics::FONT_CONTROL)
                ]
            --
                @viewport_toolbar::control_group()
                Children [
                    @button("网格", Action::Grid, 11.0)--
                    @button("吸附关", Action::Snap, 11.0)--
                    @button("网格点", Action::SnapMode, 11.0)--
                    @button("0.1米", Action::SnapStep, 11.0)
                ]
            --
            @super::weights::parameter_toolbar() // 与地图笔刷一致，参数放在视图与网格右侧。
        ]
    };
    bsn! { ViewToolbar @viewport_toolbar::toolbar_layout(Display::None, controls) }
}

#[derive(SystemParam)]
pub(super) struct Input<'w, 's> {
    toolbar: Query<'w, 's, &'static Node, With<ViewToolbar>>,
    buttons:
        Query<'w, 's, (&'static Action, &'static EditorButtonClick), Changed<EditorButtonClick>>,
    documents: Option<Res<'w, DocumentState>>,
}

impl Input<'_, '_> {
    /// 只从已采用的中央预览帧接收释放动作，使用同一导航状态和鼠标所有权。
    pub(super) fn apply(
        &self,
        preview: &AssetPreviewState,
        rigging: Option<&RiggingState>,
        view: &mut AssetPreviewView,
    ) -> bool {
        let Some(bounds) = preview.model_bounds.filter(|_| ready(preview)) else {
            return false;
        };
        if !self
            .toolbar
            .single()
            .is_ok_and(|node| node.display == Display::Flex)
        {
            return false;
        }
        let Some((action, _)) = self.buttons.iter().find(|(_, click)| click.is_clicked()) else {
            return false;
        };
        let (front_yaw, selected) =
            rigging
                .zip(self.documents.as_deref())
                .map_or((0.0, None), |(rigging, documents)| {
                    let root = documents.asset_root();
                    (
                        rigging.front_yaw(root, preview),
                        rigging.selected_landmark(root, preview),
                    )
                });
        action.apply(view, bounds, front_yaw, selected);
        true
    }
}

fn ready(preview: &AssetPreviewState) -> bool {
    preview.render_visible
        && preview.external_preview.is_none()
        && matches!(
            preview.content,
            AssetPreviewContent::Model {
                phase: AssetPreviewPhase::Ready,
                ..
            }
        )
}

#[allow(clippy::too_many_arguments)] // 仅同步一个已存在的行和固定十一枚按钮。
fn sync(
    mut commands: Commands,
    editor: Res<SceneEditorState>,
    workspace: Res<VegetationPresetEditorState>,
    media: Res<AssetPreviewWorkspace>,
    preview: Res<AssetPreviewState>,
    navigation: Res<AssetPreviewNavigation>,
    rigging: Option<Res<RiggingState>>,
    documents: Option<Res<DocumentState>>,
    locale: Option<Res<EditorLocale>>,
    mut toolbar: Query<(Entity, &mut Node), With<ViewToolbar>>,
    mut buttons: Query<(
        Entity,
        &Action,
        &UiInteraction,
        Has<InteractionDisabled>,
        &Children,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
    mut captions: Query<(&mut Text, &mut TextColor)>,
    mut previous_labels: Local<Option<(Entity, EditorLocale, EditorSnapSettings)>>,
) {
    let Ok((entity, mut toolbar)) = toolbar.single_mut() else {
        return;
    };
    let path = media.expanded.as_ref().map(|entry| entry.path.as_str());
    let visible = editor.open
        && media.active(workspace.active_tab)
        && path
            .and_then(|path| path.rsplit_once('.'))
            .is_some_and(|(_, extension)| {
                extension.eq_ignore_ascii_case("glb") || extension.eq_ignore_ascii_case("gltf")
            });
    let display = if visible {
        Display::Flex
    } else {
        Display::None
    };
    if toolbar.display != display {
        toolbar.display = display; // 加载期间预留相同的工具条高度。
    }
    if !visible {
        return;
    }
    let enabled = ready(&preview)
        && !media.restore_view
        && preview.selected_path.as_deref() == path
        && preview.model_bounds.is_some();
    let view = navigation.view;
    let front_yaw = rigging
        .as_deref()
        .zip(documents.as_deref())
        .map_or(0.0, |(rigging, documents)| {
            rigging.front_yaw(documents.asset_root(), &preview)
        });
    let direction = view.direction(front_yaw);
    let locale = locale.as_deref().copied().unwrap_or_default();
    let labels = Some((entity, locale, view.snapping));
    let refresh_labels = *previous_labels != labels;
    *previous_labels = labels;
    for (entity, action, interaction, disabled, children, mut background, mut border) in
        &mut buttons
    {
        if enabled == disabled {
            if enabled {
                commands.entity(entity).remove::<InteractionDisabled>();
            } else {
                commands.entity(entity).insert(InteractionDisabled);
            }
        }
        let selected = enabled && action.selected(view, direction);
        let color = if !enabled {
            colors::SLOT_BG_DISABLED
        } else {
            match (*interaction, selected) {
                (UiInteraction::Pressed, _) => colors::SELECT_BG_PRESSED,
                (UiInteraction::Hovered, _) => colors::BUTTON_DEV_SMALL_HOVERED,
                (_, true) => colors::SELECT_BG,
                _ => colors::BUTTON_DEV_SMALL_NORMAL,
            }
        };
        background.set_if_neq(BackgroundColor(color));
        border.set_if_neq(BorderColor::all(if selected {
            colors::COPPER_BORDER
        } else {
            colors::COPPER_BORDER_WEAK
        }));
        for child in children.iter() {
            if let Ok((mut text, mut color)) = captions.get_mut(child) {
                color.set_if_neq(TextColor(if enabled {
                    colors::TEXT_PRIMARY
                } else {
                    colors::TEXT_MUTED
                }));
                if refresh_labels {
                    let wanted = action.caption(locale, view.snapping);
                    if text.as_str() != wanted.as_ref() {
                        **text = wanted.into_owned();
                    }
                }
            }
        }
    }
}

pub(super) fn install(app: &mut App) {
    app.add_systems(Update, sync.in_set(SceneEditorSystemSet::Presentation));
}
