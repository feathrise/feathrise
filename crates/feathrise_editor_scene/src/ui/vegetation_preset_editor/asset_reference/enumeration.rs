//! 真实GLTF子资产固定预算枚举；候选保存数字身份，不复制网格、材质或节点树。

use super::super::types::{AssetReferenceRequest, ReferenceKind, SubAssetChoice};
use bevy::gltf::{Gltf, GltfMesh, GltfNode}; // 实际加载器发布的元数据。
use bevy::prelude::*; // 只读Assets与节点变换。 // 单一请求的游标和POD候选。

pub(super) const WORK_PER_FRAME: usize = 64; // 节点、网格、primitive、材质合用同一个小预算。

/// 返回true表示全部候选已完整枚举；false不能暴露部分候选供选择。
pub(super) fn collect_choices(
    request: &mut AssetReferenceRequest, // 唯一在途请求，不启动并行遍历。
    gltf: &Gltf,                         // 根已经连同依赖完整加载。
    meshes: &Assets<GltfMesh>,           // 只读取当前一个网格的元信息。
    nodes: &Assets<GltfNode>,            // Blade沿用先应用作者变换的约束。
    default_material_exists: bool,       // 默认材质只在加载器真正生成后才可选。
) -> Result<bool, String> {
    for _ in 0..WORK_PER_FRAME {
        if request.kind == ReferenceKind::BladeMesh && request.cursor.node < gltf.nodes.len() {
            let handle = &gltf.nodes[request.cursor.node]; // 一次只检查一个节点。
            let node = nodes
                .get(handle)
                .ok_or_else(|| "GLTF节点在枚举期间失效，请重新选择".to_owned())?; // 热更不能得到混合快照。
            if !identity_transform(node.transform) {
                return Err(
                    "此GLB含未应用的节点变换；Blade网格请先在Blender应用变换后再选择".to_owned(),
                ); // 不绕过既有Blade作者约束。
            }
            request.cursor.node += 1; // 后续帧从下一项继续。
            continue;
        }
        if request.kind == ReferenceKind::Material {
            if request.cursor.material < gltf.materials.len() {
                request
                    .choices
                    .push(SubAssetChoice::Material(request.cursor.material)); // GLTF材质数组与MaterialN严格对应。
                request.cursor.material += 1; // 不扫描完整名称表。
                continue;
            }
            if !default_material_exists || request.cursor.default_material {
                return Ok(true); // 没有真实默认材质或已确认使用时，无需继续查找。
            }
            // Bevy为每个GLTF都可能生成默认/std；只有primitive真正省略材质时才增加这个选项。
        }
        let Some(handle) = gltf.meshes.get(request.cursor.mesh) else {
            return Ok(true); // 已枚举完全部网格。
        };
        let mesh = meshes
            .get(handle)
            .ok_or_else(|| "GLTF网格在枚举期间失效，请重新选择".to_owned())?; // 只接受真实GltfMesh。
        if let Some(primitive) = mesh.primitives.get(request.cursor.primitive) {
            if request.kind == ReferenceKind::Material {
                if primitive.material.is_none() {
                    request.choices.push(SubAssetChoice::DefaultMaterial); // 一个真实省略材质的primitive足以证明默认项可选。
                    request.cursor.default_material = true; // 只追加一次，不再扫描剩余primitive。
                    return Ok(true); // 声明材质此前已完整枚举，候选现在完整。
                }
            } else {
                request.choices.push(SubAssetChoice::Mesh {
                    mesh: primitive.parent_mesh_index, // 使用资产事实，不依赖当前数组下标猜测标签。
                    primitive: primitive.index,        // 保留真正的primitive编号。
                });
            }
            request.cursor.primitive += 1; // 下一预算单元再处理下一个primitive。
        } else {
            request.cursor.mesh += 1; // 空网格同样消耗一个预算单元。
            request.cursor.primitive = 0; // 从下一网格首primitive继续。
        }
    }
    Ok(false) // 本帧预算耗尽，主线程不等待后续工作。
}

/// 与原Blade导入规则相同的浮点容差，禁止单字段导入悄悄丢掉作者节点变换。
fn identity_transform(transform: Transform) -> bool {
    transform.translation.length() <= 0.001 // 叶片根位置必须已经烘入网格。
        && transform.rotation.angle_between(Quat::IDENTITY) <= 0.001 // Blade没有独立节点旋转字段。
        && (transform.scale - Vec3::ONE).abs().max_element() <= 0.001 // 不把节点缩放误作真实尺寸。
}
