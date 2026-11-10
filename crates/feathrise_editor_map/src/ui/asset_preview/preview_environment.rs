//! 中央模型借用渲染器的大气和网格，只向独立预览相机写附件。

use bevy::{
    camera::{Hdr, Projection},
    light::{Atmosphere, atmosphere::ScatteringMedium},
    pbr::AtmosphereSettings,
    prelude::*,
};
use feathrise_atmosphere::{WorldCelestialLight, WorldCelestialLightKind, WorldTimeSystemSet};
use feathrise_scene::author_view::{GridView, ShadingView};

use super::{
    AssetPreviewNavigation, AssetPreviewState, AssetPreviewWorkspace, preview_shading::expanded_model,
    types::{AssetPreviewCamera, AssetPreviewLight},
};
use crate::{MapEditorSystemSet, ui::vegetation_preset_editor::VegetationPresetEditorState};

#[derive(Component)]
struct PreviewSky {
    introduced_hdr: bool,
}

pub(super) fn install(app: &mut App) {
    app.add_systems(
        Update,
        sync.after(WorldTimeSystemSet::ApplyLighting)
            .after(MapEditorSystemSet::Presentation),
    );
}

fn grid_view(projection: &Projection, rotation: Quat, step: f32) -> GridView {
    let normal = rotation * Vec3::Z;
    if matches!(projection, Projection::Orthographic(_)) && normal.y.abs() < 0.000_1 {
        // 正侧视网格随人体的正面方位，过源坐标原点，Y 轴保持竖直。
        GridView {
            origin_stride: Vec3::ZERO.extend(step),
            plane: normal.extend(0.0),
            axis_u: (rotation * Vec3::X).extend(0.0),
            axis_v: Vec4::Y,
        }
    } else {
        GridView::horizontal_world(step)
    }
}

fn sync(
    mut commands: Commands,
    preview: Res<AssetPreviewState>,
    media: Res<AssetPreviewWorkspace>,
    workspace: Res<VegetationPresetEditorState>,
    navigation: Res<AssetPreviewNavigation>,
    cameras: Query<
        (Entity, &Camera, &Projection, &Transform, Option<&GridView>, Option<&ShadingView>, Option<&PreviewSky>,
            Has<AtmosphereSettings>, Has<Hdr>),
        With<AssetPreviewCamera>,
    >,
    atmospheres: Query<(), With<Atmosphere>>,
    mut media_assets: Option<ResMut<Assets<ScatteringMedium>>>,
    world_lights: Query<(&WorldCelestialLight, &DirectionalLight, &Transform), Without<AssetPreviewLight>>,
    mut preview_light: Query<(&mut DirectionalLight, &mut Transform), (With<AssetPreviewLight>, Without<AssetPreviewCamera>)>,
) {
    let Ok((entity, camera, projection, transform, old_grid, shading, sky, has_settings, has_hdr)) =
        cameras.single()
    else {
        return;
    };
    let expanded = camera.is_active && expanded_model(&preview, &media, &workspace);
    let grid = (expanded && navigation.grid_visible()).then(|| {
        grid_view(projection, transform.rotation, navigation.snapping().translation_meters)
    });
    if old_grid.copied() != grid {
        if let Some(grid) = grid {
            commands.entity(entity).insert(grid);
        } else {
            commands.entity(entity).remove::<GridView>();
        }
    }

    let perspective = expanded && matches!(projection, Projection::Perspective(_));
    // 线框与关闭场景背景沿用地图的中性底色，避免大气和着色后处理竞争背景。
    let wants_sky = perspective
        && shading.is_none_or(|view| view.options.x >= 0.5 && view.options.w > 0.5);
    if wants_sky && !has_settings {
        if atmospheres.is_empty() {
            let Some(assets) = &mut media_assets else { return };
            // 与地图共用一个地球大气，独立工程首次需要时登记固定大小的散射资产。
            commands.spawn((
                Name::new("WorldDynamicAtmosphere"),
                Atmosphere::earth(assets.add(ScatteringMedium::earth(256, 256))),
            ));
        }
        commands.entity(entity).insert((
            AtmosphereSettings::default(),
            PreviewSky { introduced_hdr: !has_hdr },
        ));
    } else if !wants_sky && let Some(sky) = sky {
        let mut camera = commands.entity(entity);
        camera.remove::<(AtmosphereSettings, PreviewSky)>();
        if sky.introduced_hdr {
            camera.remove::<Hdr>();
        }
    }
    if let Ok((mut light, mut transform)) = preview_light.single_mut() {
        // 仅抄取正式太阳的光照，预览相机不注册为地图天气的活动相机。
        let sun = perspective.then(|| world_lights.iter().find(|(celestial, _, _)| {
            celestial.kind() == WorldCelestialLightKind::Sun
        })).flatten();
        let (color, illuminance, rotation) = sun.map_or(
            (Color::WHITE, 13_000.0, Quat::from_euler(EulerRot::YXZ, -0.75, -0.85, 0.0)),
            |(_, light, transform)| (light.color, light.illuminance, transform.rotation),
        );
        if light.color != color || light.illuminance != illuminance {
            light.color = color;
            light.illuminance = illuminance;
        }
        if transform.rotation != rotation {
            transform.rotation = rotation;
        }
    }
}
