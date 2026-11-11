//! 中央模型复用场景四球和附件租约，只改变唯一离屏预览相机。

use bevy::{ecs::system::SystemParam, prelude::*, ui::RelativeCursorPosition};

use crate::{
    SceneEditorSystemSet,
    resources::SceneEditorState,
    ui::vegetation_preset_editor::VegetationPresetEditorState,
    viewport::shading::{
        AssetPreviewShadingState, ShadingClicks, ShadingHost, ShadingToolbarRoot,
        ViewShadingRoot, spawn_toolbar,
    },
};

use super::{AssetPreviewWorkspace, types::*};

pub(super) fn install(app: &mut App) {
    app.init_resource::<AssetPreviewShadingState>()
        .add_systems(Update, setup.in_set(SceneEditorSystemSet::Tasks))
        .add_systems(
            Update,
            close_menu
                .after(feathrise_editor_ui::EditorUiSystemSet::NumericInput)
                .after(super::AssetTabMenuInputSet)
                .before(super::workspace::actions)
                .before(crate::ui::actions::handle_editor_shortcuts)
                .in_set(SceneEditorSystemSet::Input),
        )
        .add_systems(Update, (sync_toolbar, sync_camera).in_set(SceneEditorSystemSet::Presentation));
}

fn setup(
    mut commands: Commands,
    assets: Res<AssetServer>,
    canvases: Query<Entity, Added<AssetPreviewCanvas>>,
) {
    let Ok(canvas) = canvases.single() else {
        return;
    };
    let font = feathrise_editor_core::editor_font(&assets, crate::ui::scene::FONT_PATH);
    commands.entity(canvas).with_children(|parent| {
        spawn_toolbar(parent, &font, ShadingHost::Asset);
    });
}

/// 画布随宿主原位迁移，必须同时核对中央标签、来源和渲染资格。
pub(super) fn expanded_model(
    preview: &AssetPreviewState,
    media: &AssetPreviewWorkspace,
    workspace: &VegetationPresetEditorState,
) -> bool {
    preview.render_visible
        && preview.external_preview.is_none()
        && matches!(preview.content, AssetPreviewContent::Model { phase: AssetPreviewPhase::Ready, .. })
        && media.active(workspace.active_tab)
        && media.expanded.as_ref().is_some_and(|entry|
            preview.selected_path.as_deref() == Some(entry.path.as_str()))
}

fn sync_toolbar(
    preview: Res<AssetPreviewState>,
    media: Res<AssetPreviewWorkspace>,
    workspace: Res<VegetationPresetEditorState>,
    mut roots: Query<(&ShadingHost, &mut Node), With<ShadingToolbarRoot>>,
) {
    let display = if expanded_model(&preview, &media, &workspace) {
        Display::Flex
    } else {
        Display::None
    };
    for (host, mut node) in &mut roots {
        if *host == ShadingHost::Asset && node.display != display {
            node.display = display;
        }
    }
}

fn sync_camera(world: &mut World) {
    let preview = world.resource::<AssetPreviewState>();
    let Some(camera) = preview.camera.filter(|entity| world.get::<AssetPreviewCamera>(*entity).is_some()) else {
        return;
    };
    let view = expanded_model(
        preview,
        world.resource::<AssetPreviewWorkspace>(),
        world.resource::<VegetationPresetEditorState>(),
    ).then(|| world.resource::<AssetPreviewShadingState>().0.view());
    // 背景选项由共用深度后处理执行，天空及灯光仍由预览自己的环境系统拥有。
    feathrise_scene::author_view::apply_shading(world, camera, view);
}

fn close_menu(
    preview: Res<AssetPreviewState>,
    media: Res<AssetPreviewWorkspace>,
    workspace: Res<VegetationPresetEditorState>,
    editor: Res<SceneEditorState>,
    mut controls: ResMut<AssetPreviewShadingState>,
    mut keyboard: Option<ResMut<ButtonInput<KeyCode>>>,
) {
    if !controls.0.menu_open() {
        return;
    }
    if !editor.open || editor.close_requested || !expanded_model(&preview, &media, &workspace) {
        controls.0.close_menu();
    } else if let Some(keys) = keyboard.as_deref_mut() {
        controls.0.dismiss(keys);
    }
}

/// 由正式导航在模态、可见性和拖动门之后转发同一完成点击。
#[derive(SystemParam)]
pub(super) struct Input<'w, 's> {
    controls: Option<ResMut<'w, AssetPreviewShadingState>>,
    clicks: ShadingClicks<'w, 's>,
    pub(super) pointer: Pointer<'w, 's>,
}

/// 新的画布手势要给可见着色栏和弹出的菜单让出鼠标。
#[derive(SystemParam)]
pub(super) struct Pointer<'w, 's> {
    roots: Query<
        'w, 's,
        (&'static ShadingHost, &'static Node, &'static RelativeCursorPosition),
        With<ViewShadingRoot>,
    >,
}

impl Pointer<'_, '_> {
    pub(super) fn cursor_over(&self) -> bool {
        self.roots.iter().any(|(host, node, cursor)|
            *host == ShadingHost::Asset && node.display != Display::None && cursor.cursor_over())
    }
}

impl Input<'_, '_> {
    pub(super) fn apply(&mut self) -> bool {
        let (Some(controls), Some(action)) = (self.controls.as_mut(), self.clicks.action(ShadingHost::Asset)) else {
            return false;
        };
        controls.0.apply(action);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::core_pipeline::prepass::{DepthPrepass, NormalPrepass};
    use crate::{
        ui::vegetation_preset_editor::PresetWorkspaceTab,
        viewport::shading::types::{ShadingAction, ShadingMode},
    };
    use feathrise_scene::author_view::ShadingView;

    #[test]
    fn expanded_model_shading_releases_only_its_preview_camera_on_tab_change() {
        let mut app = App::new();
        app.init_resource::<AssetPreviewShadingState>()
            .init_resource::<VegetationPresetEditorState>()
            .add_systems(Update, (sync_toolbar, sync_camera).chain());
        let world = app.world_mut();
        world.resource_mut::<VegetationPresetEditorState>().active_tab = PresetWorkspaceTab::AssetPreview;
        let map_view = ShadingView::mesh_triangles();
        let map = world.spawn((Camera3d::default(), map_view)).id();
        let preview = world.spawn((
            AssetPreviewCamera, Camera3d::default(), Msaa::Sample4, DepthPrepass,
            AmbientLight { brightness: 17.0, ..default() },
        )).id();
        let root = world.spawn_empty().id();
        let toolbar = world.spawn((ShadingHost::Asset, ShadingToolbarRoot, Node::default())).id();
        let path = "角色.glb";
        world.insert_resource(AssetPreviewWorkspace {
            expanded: Some(ExpandedAssetPreview {
                id: 1, path: path.into(), return_tab: PresetWorkspaceTab::Scene,
                view: default(), target: AssetPreviewModelTarget::Scene0,
                audio_position: 0.0, restore_cursor: 0,
            }),
            ..default()
        });
        world.insert_resource(AssetPreviewState {
            camera: Some(preview),
            content: AssetPreviewContent::Model {
                handle: Handle::default(), root, phase: AssetPreviewPhase::Ready,
            },
            render_visible: true,
            selected_path: Some(path.into()),
            ..default()
        });
        app.update();
        let world = app.world_mut();
        assert_eq!(world.get::<Node>(toolbar).unwrap().display, Display::Flex);
        assert_eq!(world.get::<ShadingView>(preview).unwrap().options.x, 3.0);
        world.resource_mut::<AssetPreviewShadingState>().0.apply(ShadingAction::Mode(ShadingMode::Solid));
        app.update();
        let world = app.world_mut();
        assert_eq!(world.get::<Msaa>(preview), Some(&Msaa::Off));
        assert!(world.get::<NormalPrepass>(preview).is_some());
        world.resource_mut::<AssetPreviewShadingState>().0.apply(ShadingAction::Mode(ShadingMode::MaterialPreview));
        app.update();
        let world = app.world_mut();
        assert_eq!(world.get::<AmbientLight>(preview).unwrap().brightness, 800.0);
        world.resource_mut::<VegetationPresetEditorState>().active_tab = PresetWorkspaceTab::Scene;
        app.update();
        let world = app.world();
        assert_eq!(world.get::<Node>(toolbar).unwrap().display, Display::None);
        assert!(world.get::<ShadingView>(preview).is_none());
        assert_eq!(world.get::<Msaa>(preview), Some(&Msaa::Sample4));
        assert!(world.get::<DepthPrepass>(preview).is_some());
        assert!(world.get::<NormalPrepass>(preview).is_none());
        assert_eq!(world.get::<AmbientLight>(preview).unwrap().brightness, 17.0);
        assert!(world.get::<ShadingView>(map) == Some(&map_view));
    }
}
