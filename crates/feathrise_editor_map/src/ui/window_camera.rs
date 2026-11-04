//! F12外壳的窗口相机生命周期；根节点保持固定UI目标，不跟随驻留地图默认相机切换。

use crate::components::{MapEditorRoot, MapEditorUiCamera};
use crate::resources::MapEditorState;
use bevy::camera::{RenderTarget, visibility::RenderLayers};
use bevy::ecs::{entity_disabling::Disabled, query::Allow};
use bevy::prelude::*;

/// 在相机更新和UI布局之前采用窗口目标，所有地图、冷载和暖恢复共用同一份窗口尺寸及DPI。
pub(crate) fn install(app: &mut App) {
    app.add_systems(
        PostUpdate,
        sync_editor_ui_camera
            .before(bevy::camera::CameraUpdateSystems)
            .before(bevy::ui::UiSystems::Prepare),
    );
}

fn sync_editor_ui_camera(
    mut commands: Commands,
    editor: Res<MapEditorState>,
    mut cameras: Query<
        (Entity, &mut Camera, Has<Disabled>),
        (With<MapEditorUiCamera>, Allow<Disabled>),
    >,
    roots: Query<(Entity, Option<&UiTargetCamera>), With<MapEditorRoot>>,
) {
    if roots.is_empty() {
        return;
    }
    let camera_entity = if let Ok((entity, mut camera, disabled)) = cameras.single_mut() {
        let was_active = camera.is_active;
        if camera.is_active != editor.open {
            camera.is_active = editor.open;
        }
        if editor.open && disabled {
            commands.entity(entity).remove::<Disabled>();
        } else if !editor.open && !disabled && !was_active {
            // 先让标准提取看见一帧is_active=false，清除RenderApp的旧窗口视图；
            // 同帧Disabled会让提取查询跳过相机，残留视图便持续覆盖玩法画面。
            // 下一帧再撤下默认UI资格，既复用原相机，也不把菜单长期绑定到关闭的F12相机。
            commands.entity(entity).insert(Disabled);
        }
        entity
    } else {
        if !editor.open {
            return;
        }
        commands
            .spawn((
                Name::new("地图编辑器窗口UI相机"),
                MapEditorUiCamera,
                Camera2d,
                Camera {
                    order: 600,
                    clear_color: ClearColorConfig::None,
                    ..default()
                },
                RenderTarget::default(), // 固定主窗口；不采用任何地图RTT、视口裁剪或按图资源快照。
                RenderLayers::none(),    // 只提交本相机的UI，不重复绘制世界中的Sprite或Mesh2d。
            ))
            .id()
    };
    for (root, target) in &roots {
        if target.is_none_or(|target| target.entity() != camera_entity) {
            commands.entity(root).insert(UiTargetCamera(camera_entity));
        }
    }
}

#[cfg(test)]
#[path = "window_camera_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "window_camera_gpu_tests.rs"]
mod gpu_tests;
