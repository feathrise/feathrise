//! 工程视口复用原显示控件及GPU实现；信息徽标保留可恢复底板，XYZ仅显示真实控件轮廓。
use crate::{
    components::{
        SceneEditorCamera, SceneEditorSelectedPlacementBadge,
        SceneEditorSelectedPortalBadge, SceneEditorSelectedWaterBadge,
    },
    standalone::embedded_snapshot::EmbeddedSnapshotState,
    viewport::multi_select::MultiSelectionBadge,
};
use bevy::prelude::*;
use feathrise_scene::author_view::{GridView, ShadingView};

#[derive(Component)]
pub(super) struct PanelBackgroundLease(Option<Color>);

pub(super) fn sync(
    mut commands: Commands,
    embedded: Option<Res<EmbeddedSnapshotState>>,
    axes: Query<
        (
            Entity,
            Option<&BackgroundColor>,
            Option<&PanelBackgroundLease>,
        ),
        Or<(
            With<MultiSelectionBadge>,
            With<SceneEditorSelectedPlacementBadge>,
            With<SceneEditorSelectedPortalBadge>,
            With<SceneEditorSelectedWaterBadge>,
        )>,
    >,
) {
    let enabled = embedded.as_ref().is_some_and(|state| state.enabled);
    // XYZ通过原生圆环/斜线轮廓露出作者控件，根容器必须继续透明；不能给它补整块底板。
    for (entity, background, lease) in &axes {
        if enabled && lease.is_none() {
            commands.entity(entity).insert((
                PanelBackgroundLease(background.map(|color| color.0)),
                BackgroundColor(crate::ui::theme::PANEL_BG_STRONG),
            ));
        } else if !enabled && let Some(lease) = lease {
            match lease.0 {
                Some(color) => {
                    commands.entity(entity).insert(BackgroundColor(color));
                }
                None => {
                    commands.entity(entity).remove::<BackgroundColor>();
                }
            }
            commands.entity(entity).remove::<PanelBackgroundLease>();
        }
    }
}

/// 固定小型显示投影；作者库不依赖进程协议，宿主只转换纯值。
pub struct EmbeddedRenderView {
    pub shading: [f32; 4],
    pub wire: [f32; 4],
    pub grid: Option<[[f32; 4]; 4]>,
    pub terrain_flat: bool,
    pub terrain_grid: bool,
    pub terrain_backfaces: bool,
    pub physics_debug: bool,
}

pub fn capture(world: &mut World) -> Option<EmbeddedRenderView> {
    let (shading, grid) = world
        .query_filtered::<(&ShadingView, Option<&GridView>), With<SceneEditorCamera>>()
        .single(world)
        .ok()
        .map(|(shading, grid)| (*shading, grid.copied()))?;
    Some(EmbeddedRenderView {
        shading: shading.options.to_array(),
        wire: shading.wire.to_array(),
        grid: grid.map(|grid| {
            [
                grid.origin_stride.to_array(),
                grid.plane.to_array(),
                grid.axis_u.to_array(),
                grid.axis_v.to_array(),
            ]
        }),
        terrain_flat: world
            .get_resource::<feathrise_scene::TerrainShadingPreview>()
            .is_some_and(|view| view.flat),
        terrain_grid: world
            .get_resource::<feathrise_scene::TerrainGridPreview>()
            .is_some_and(|view| view.visible),
        terrain_backfaces: world
            .get_resource::<feathrise_scene::TerrainBackfacesPreview>()
            .is_some_and(|view| view.enabled),
        physics_debug: world
            .get_resource::<feathrise_editor_core::EditorSettings>()
            .is_some_and(|settings| settings.scene.show_physics_debug),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn embedded_render_controls_keep_xyz_transparent_and_restore_other_panel_backgrounds() {
        use crate::viewport::snapping::{SnapAction, SnapButton};
        use crate::components::SceneEditorOrientationAxisRoot;
        let mut app = App::new();
        app.init_resource::<EmbeddedSnapshotState>()
            .add_systems(Update, sync);
        let grid = app.world_mut().spawn(SnapButton(SnapAction::Grid)).id();
        let axis = app.world_mut().spawn(SceneEditorOrientationAxisRoot).id();
        let original = Color::srgba(0.1, 0.2, 0.3, 0.4);
        let badge = app.world_mut().spawn((SceneEditorSelectedPlacementBadge, BackgroundColor(original))).id();
        app.world_mut()
            .resource_mut::<EmbeddedSnapshotState>()
            .enabled = true;
        app.update();
        assert!(
            app.world()
                .get::<bevy::ui::InteractionDisabled>(grid)
                .is_none()
        );
        assert!(app.world().get::<BackgroundColor>(axis).is_none());
        assert!(app.world().get::<PanelBackgroundLease>(axis).is_none());
        assert_eq!(app.world().get::<BackgroundColor>(badge).unwrap().0, crate::ui::theme::PANEL_BG_STRONG);
        app.world_mut()
            .resource_mut::<EmbeddedSnapshotState>()
            .enabled = false;
        app.update();
        assert!(app.world().get::<BackgroundColor>(axis).is_none());
        assert_eq!(app.world().get::<BackgroundColor>(badge).unwrap().0, original);
    }
}
