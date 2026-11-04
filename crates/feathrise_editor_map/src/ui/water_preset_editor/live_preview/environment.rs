//! 水体样品相机复用当前编辑器天空、IBL、环境光、雾和天体方向光。
//!
//! 【功能设计】不复制地图实体，也不创建固定假灯；样品相机只看专用河面层，当前世界
//! 太阳/月亮临时增加该层，天空与相机局部环境组件则从中央编辑相机同步。

use bevy::camera::{Hdr, visibility::RenderLayers}; // 当前天体方向光临时加入样品专层，并镜像动态天空HDR资格。
use bevy::light::AmbientLight; // 相机级最终环境光按当前时刻同步。
use bevy::pbr::AtmosphereSettings; // 动态天空复用同一地球大气和相机质量设置。
use bevy::prelude::*; // Commands、Camera、DistanceFog与Query。
use feathrise_atmosphere::{AuthoredSkyboxRequest, WorldCelestialLight}; // 正式天空加载请求和当前太阳/月亮身份。
use feathrise_editor_core::constants::render_layers::MAP_EDITOR_WATER_PRESET_PREVIEW_LAYER; // 世界实体与样品光照的唯一交集层。

use super::WaterPresetLivePreviewCamera; // 目标始终是唯一独立样品相机。
use crate::components::MapEditorCamera; // 中央编辑相机是当前作者天空和环境的权威来源。

/// 按样品相机活动状态增加或移除方向光专层，并保留光源原有全部层。
pub(super) fn preview_light_layers(current: &RenderLayers, active: bool) -> RenderLayers {
    if active {
        current.clone().with(MAP_EDITOR_WATER_PRESET_PREVIEW_LAYER) // 当前太阳/月亮同帧照亮样品河面。
    } else {
        current
            .clone()
            .without(MAP_EDITOR_WATER_PRESET_PREVIEW_LAYER) // 隐藏预览时不保留额外阴影视图资格。
    }
}

/// 在正式天空状态机运行前，把中央编辑相机的作者请求镜像到样品相机。
pub(in crate::ui::water_preset_editor) fn sync_live_preview_environment_request(
    mut commands: Commands, // 请求真实变化时才写入目标相机。
    sources: Query<
        &AuthoredSkyboxRequest,
        (With<MapEditorCamera>, Without<WaterPresetLivePreviewCamera>),
    >, // 中央编辑相机已解析当前地图草稿的完整天空与IBL设置。
    targets: Query<
        (Entity, Option<&AuthoredSkyboxRequest>),
        (With<WaterPresetLivePreviewCamera>, Without<MapEditorCamera>),
    >, // 样品相机独立参与同一个正式异步天空加载器。
) {
    let (Ok(source), Ok((target, current))) = (sources.single(), targets.single()) else {
        return; // 任一相机尚未建立时不创建临时替身。
    };
    if current != Some(source) {
        commands.entity(target).insert(source.clone()); // 同修订请求共享资产句柄缓存并独立原子采用。
    }
}

/// 判断两份相机清屏配置是否视觉相同；类型本身未实现PartialEq。
fn clear_color_matches(left: ClearColorConfig, right: ClearColorConfig) -> bool {
    match (left, right) {
        (ClearColorConfig::Default, ClearColorConfig::Default)
        | (ClearColorConfig::None, ClearColorConfig::None) => true, // 相同内建模式无需重复写Camera。
        (ClearColorConfig::Custom(left), ClearColorConfig::Custom(right)) => left == right, // 自定义色按完整RGBA比较。
        _ => false, // 不同模式必须立即跟随中央相机。
    }
}

/// 判断最终环境光是否已与中央相机一致，避免暂停稳定帧重复写组件。
fn ambient_matches(left: &AmbientLight, right: &AmbientLight) -> bool {
    left.color == right.color
        && left.brightness == right.brightness
        && left.affects_lightmapped_meshes == right.affects_lightmapped_meshes // 三个公开字段共同构成最终PBR环境光。
}

/// 世界光照与天气完成本帧最终值后，同步相机局部环境并切换当前太阳/月亮样品层。
#[allow(clippy::type_complexity)]
pub(in crate::ui::water_preset_editor) fn sync_live_preview_lighting(
    mut commands: Commands, // 可选组件首次出现或被正式源移除时通过命令同步结构。
    sources: Query<
        (
            &Camera,                         // 当前中央视口最终清屏配置。
            Option<Ref<AmbientLight>>,       // 当前时刻和天气后的最终环境光。
            Option<Ref<AtmosphereSettings>>, // 动态天空模式的相机质量与资格。
            Option<Ref<DistanceFog>>,        // 当前天气和能见度派生出的最终距离雾。
            Has<Hdr>,                        // 动态大气要求HDR，天空盒模式则跟随中央相机精确归还。
        ),
        (With<MapEditorCamera>, Without<WaterPresetLivePreviewCamera>),
    >, // 只读取中央作者相机，不依赖玩法窗口是否被F12遮住。
    mut targets: Query<
        (
            Entity,                          // Commands结构变更的精确目标。
            &mut Camera,                     // 直接同步清屏色且保留独立RTT和活动状态。
            Option<&mut AmbientLight>,       // 已存在时原位写入，避免逐帧命令替换。
            Option<&mut AtmosphereSettings>, // 动态/天空盒切换时精确增删。
            Option<&mut DistanceFog>,        // 天气变化时复用正式最终雾参数。
            Has<Hdr>,                        // 防止移除大气后残留HDR资格与LDR样品RTT格式不一致。
        ),
        (With<WaterPresetLivePreviewCamera>, Without<MapEditorCamera>),
    >, // 样品相机仍只看河面专层，不会因此显示地图实体。
    mut celestial_lights: Query<&mut RenderLayers, With<WorldCelestialLight>>, // 当前场景太阳、月亮或固定兼容灯。
) {
    let Ok((source_camera, source_ambient, source_atmosphere, source_fog, source_hdr)) =
        sources.single()
    else {
        return; // 中央相机未就绪时保留上一份完整样品环境。
    };
    let Ok((
        target_entity,
        mut target_camera,
        target_ambient,
        target_atmosphere,
        target_fog,
        target_hdr,
    )) = targets.single_mut()
    else {
        return; // 样品相机尚未创建时没有同步目标。
    };
    let active = target_camera.is_active; // 只有右栏场景页可见时才让天体光进入样品层。
    if !clear_color_matches(target_camera.clear_color, source_camera.clear_color) {
        target_camera.clear_color = source_camera.clear_color; // 动态大气或天空资源等待时也复用相同背景基色。
    }

    match (source_ambient, target_ambient) {
        (Some(source), Some(mut target)) if !ambient_matches(&source, &target) => {
            *target = (*source).clone(); // 当前昼夜和天气环境光同帧更新。
        }
        (Some(source), None) => {
            commands.entity(target_entity).insert((*source).clone()); // 首次建立相机级环境光覆盖。
        }
        (None, Some(_)) => {
            commands.entity(target_entity).remove::<AmbientLight>(); // 正式源归还全局环境光时样品同步归还。
        }
        _ => {} // 完整相同或双方均缺失时保持稳定。
    }
    match (source_atmosphere, target_atmosphere) {
        (Some(source), Some(mut target)) if source.is_changed() => {
            *target = (*source).clone(); // 动态天空质量设置真实变化时原位同步。
        }
        (Some(source), None) => {
            commands.entity(target_entity).insert((*source).clone()); // 动态模式复用唯一地球大气实体，不创建第二套天空世界。
        }
        (None, Some(_)) => {
            commands
                .entity(target_entity)
                .remove::<AtmosphereSettings>(); // 天空盒取得所有权时关闭样品动态大气。
        }
        _ => {} // 稳定模式不反复写相机质量组件。
    }
    match (source_fog, target_fog) {
        (Some(source), Some(mut target)) if source.is_changed() => {
            *target = (*source).clone(); // 当前天气能见度与色温同帧更新。
        }
        (Some(source), None) => {
            commands.entity(target_entity).insert((*source).clone()); // 首次复制正式距离雾。
        }
        (None, Some(_)) => {
            commands.entity(target_entity).remove::<DistanceFog>(); // 正式天气关闭后不留预览残余。
        }
        _ => {} // 稳定天气不重复替换组件。
    }
    if source_hdr != target_hdr {
        if source_hdr {
            commands.entity(target_entity).insert(Hdr); // 动态大气与中央作者视口使用同一HDR主通道资格。
        } else {
            commands.entity(target_entity).remove::<Hdr>(); // 天空盒/LDR模式不保留上帧动态大气的HDR残留。
        }
    }

    for mut layers in &mut celestial_lights {
        let next = preview_light_layers(&layers, active); // 保留玩法、角色和地图原有全部层。
        if *layers != next {
            *layers = next; // 只在预览启停边沿改变方向光可见层集合。
        }
    }
}
