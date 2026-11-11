//! 最小glTF 2.0二进制地形构建器。
//!
//! 生成物固定为一个网格、一个primitive、POSITION/NORMAL、一个纯白材质且无UV。
//! 高度场仍是权威作者源；每次保存只重新生成派生GLB。

use serde_json::json; // JSON块通过结构化值生成，避免手工转义中文名称。

use super::geometry::build_terrain_geometry; // GLB与实时预览必须共享相同顶点、法线和绕序。

const GLB_MAGIC: u32 = 0x4654_6C67; // ASCII `glTF`小端值。
const GLB_VERSION: u32 = 2; // 当前固定输出glTF 2.0。
const JSON_CHUNK_TYPE: u32 = 0x4E4F_534A; // ASCII `JSON`。
const BIN_CHUNK_TYPE: u32 = 0x004E_4942; // ASCII `BIN\0`。

/// 构建一个由16位高度场驱动的规则网格白模GLB。
pub(super) fn build_white_terrain_glb(
    heights: &[u16],
    resolution: [u32; 2],
    world_min: [f32; 2],
    world_max: [f32; 2],
    base_height: f32,
    height_scale: f32,
) -> Result<Vec<u8>, String> {
    let geometry = build_terrain_geometry(
        heights,
        resolution,
        world_min,
        world_max,
        base_height,
        height_scale,
    )?;
    let vertex_count = geometry.positions.len();

    let mut binary = Vec::new();
    let positions_offset = binary.len();
    push_vec3_slice(&mut binary, &geometry.positions);
    let normals_offset = binary.len();
    push_vec3_slice(&mut binary, &geometry.normals);
    let indices_offset = binary.len();
    push_u32_slice(&mut binary, &geometry.indices);
    pad_to_four(&mut binary, 0);

    let position_bytes = geometry.positions.len() * 3 * std::mem::size_of::<f32>();
    let normal_bytes = geometry.normals.len() * 3 * std::mem::size_of::<f32>();
    let index_bytes = geometry.indices.len() * std::mem::size_of::<u32>();
    let document = json!({
        "asset": { "version": "2.0", "generator": "feathrise_editor_scene" },
        "scene": 0,
        "scenes": [{ "name": "动态场景白模场景", "nodes": [0] }],
        "nodes": [{ "name": "动态场景地形", "mesh": 0 }],
        "meshes": [{
            "name": "动态场景地形_网格",
            "primitives": [{
                "attributes": { "POSITION": 0, "NORMAL": 1 },
                "indices": 2,
                "material": 0,
                "mode": 4
            }]
        }],
        "materials": [{
            "name": "地表_白模",
            "pbrMetallicRoughness": {
                "baseColorFactor": [1.0, 1.0, 1.0, 1.0],
                "metallicFactor": 0.0,
                "roughnessFactor": 1.0
            },
            "doubleSided": false
        }],
        "buffers": [{ "byteLength": binary.len() }],
        "bufferViews": [
            { "buffer": 0, "byteOffset": positions_offset, "byteLength": position_bytes, "target": 34962 },
            { "buffer": 0, "byteOffset": normals_offset, "byteLength": normal_bytes, "target": 34962 },
            { "buffer": 0, "byteOffset": indices_offset, "byteLength": index_bytes, "target": 34963 }
        ],
        "accessors": [
            {
                "bufferView": 0, "componentType": 5126, "count": vertex_count,
                "type": "VEC3",
                "min": geometry.min,
                "max": geometry.max
            },
            { "bufferView": 1, "componentType": 5126, "count": vertex_count, "type": "VEC3" },
            { "bufferView": 2, "componentType": 5125, "count": geometry.indices.len(), "type": "SCALAR" }
        ]
    });
    let mut json_bytes =
        serde_json::to_vec(&document).map_err(|error| format!("序列化GLB JSON块失败：{error}"))?;
    pad_to_four(&mut json_bytes, b' ');

    let total_length = 12 + 8 + json_bytes.len() + 8 + binary.len();
    let total_length = u32::try_from(total_length).map_err(|_| "GLB超过4GiB".to_owned())?;
    let mut glb = Vec::with_capacity(total_length as usize);
    glb.extend_from_slice(&GLB_MAGIC.to_le_bytes());
    glb.extend_from_slice(&GLB_VERSION.to_le_bytes());
    glb.extend_from_slice(&total_length.to_le_bytes());
    glb.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    glb.extend_from_slice(&JSON_CHUNK_TYPE.to_le_bytes());
    glb.extend_from_slice(&json_bytes);
    glb.extend_from_slice(&(binary.len() as u32).to_le_bytes());
    glb.extend_from_slice(&BIN_CHUNK_TYPE.to_le_bytes());
    glb.extend_from_slice(&binary);
    Ok(glb)
}

fn push_vec3_slice(target: &mut Vec<u8>, values: &[[f32; 3]]) {
    for vector in values {
        for value in vector {
            target.extend_from_slice(&value.to_le_bytes());
        }
    }
}

fn push_u32_slice(target: &mut Vec<u8>, values: &[u32]) {
    for value in values {
        target.extend_from_slice(&value.to_le_bytes());
    }
}

fn pad_to_four(bytes: &mut Vec<u8>, fill: u8) {
    while !bytes.len().is_multiple_of(4) {
        bytes.push(fill);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_glb_contains_one_mesh_and_no_uv_attribute() {
        let bytes = build_white_terrain_glb(&[0; 9], [3, 3], [-1.0, -1.0], [1.0, 1.0], 0.0, 8.0)
            .expect("有效平地必须可构建");
        assert_eq!(&bytes[0..4], b"glTF");
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("动态场景地形_网格"));
        assert!(text.contains("地表_白模"));
        assert!(!text.contains("TEXCOORD_0"));
    }
}
