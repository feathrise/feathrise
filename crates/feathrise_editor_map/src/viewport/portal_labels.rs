//! 让正式与未保存传送标签在F12独立离屏相机中保持正面、可读并服从距离裁剪。

use bevy::prelude::*; // Camera、Transform、Visibility和ECS查询。
use feathrise_billboard_text::{BillboardTextHandle, BillboardTextLink}; // 从标签宿主定位已烘焙的世界文字实体。
use feathrise_portal::PortalLabel; // 正式传送点独立文字锚点身份。

use crate::components::{EditorPortalPreview, MapEditorCamera}; // 未保存预览根和唯一F12相机。
use crate::resources::MapEditorState; // 关闭F12后立即归还玩法相机的Billboard控制权。

/// F12打开时，用最终编辑相机覆盖传送标签的朝向、距离缩放和可见性。
pub(crate) fn sync_editor_portal_labels(
    editor: Res<MapEditorState>, // 只在独立编辑视口实际打开时接管呈现。
    editor_camera: Query<(&Camera, &Transform), With<MapEditorCamera>>, // 唯一离屏相机提供本帧最终姿态。
    label_owners: Query<
        &BillboardTextHandle, // 请求宿主回写的稳定Billboard实体身份。
        Or<(With<EditorPortalPreview>, With<PortalLabel>)>, // 同时覆盖未保存预览和已采用正式传送标签。
    >,
    mut billboards: Query<
        (&BillboardTextLink, &mut Transform, &mut Visibility), // 复用公共Billboard的距离规格与烘焙状态。
        Without<MapEditorCamera>, // 与上方相机Transform查询显式互斥，避免Bevy B0001冲突。
    >,
) {
    if !editor.open {
        return; // F12关闭时公共Billboard系统继续以玩法主相机为唯一权威。
    }
    let Ok((camera, camera_transform)) = editor_camera.single() else {
        return; // 独立相机缺失或歧义时保持公共系统上一份安全结果。
    };
    if !camera.is_active {
        return; // RTT尚未激活时不能把隐藏标签错误转为可见。
    }
    for handle in &label_owners {
        let Ok((link, mut transform, mut visibility)) = billboards.get_mut(handle.0) else {
            continue; // 新请求仍在Commands或固定烘焙队列中时等待下一帧。
        };
        if !link.is_ready() {
            continue; // 未完整烘焙的纹理必须沿用公共系统的Hidden状态。
        }
        let distance = camera_transform.translation.distance(transform.translation); // 以F12相机而非后台玩法相机判断标签距离。
        if distance > link.max_distance {
            *visibility = Visibility::Hidden; // 超出标签自身正式规格时继续隐藏。
            continue; // 远距隐藏无需更新旋转和缩放。
        }
        let distance_scale = if link.distance_scaling {
            (distance / link.ref_distance).min(1.0) // 近距离按公共规格缩小，远处不超过作者尺寸。
        } else {
            1.0 // 传送标签默认保持固定世界尺寸，确保目标文字稳定可读。
        };
        *visibility = Visibility::Visible; // 已烘焙且在F12有效距离内时恢复标签。
        transform.scale = Vec3::splat(distance_scale * link.scale); // 同步公共Billboard最终尺寸语义。
        transform.rotation = camera_transform.rotation; // 独立RTT相机旋转变化后同帧保持文字正面朝向。
    }
}
