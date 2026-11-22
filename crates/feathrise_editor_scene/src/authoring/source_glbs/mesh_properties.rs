//! 源网格属性的 glTF 节点载荷，各通道缺省时分别继承该引用的整件属性。

use feathrise_editor_animation_model::data::GlbStructure;
use feathrise_scene_content::{
    MeshCollisionLodSettings, MeshDisplayLodSettings, MeshLodMode, MeshLodPolicy,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const KEY: &str = "feathrise_mesh_properties";

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub(crate) enum SourceMeshHeight {
    FollowTerrain { offset: f32 },
    FixedHeight,
    SnapToGround,
}

// 与配套 SDK 的节点载入合同共用现有 LOD 类型。旧 SDK 仍能正常读取标准 GLB。
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct SourceMeshProperties {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<SourceMeshHeight>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collision: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_lod: Option<MeshLodMode<MeshDisplayLodSettings>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collision_lod: Option<MeshLodMode<MeshCollisionLodSettings>>,
}

impl SourceMeshProperties {
    fn validate(self) -> Result<(), String> {
        if matches!(self.height, Some(SourceMeshHeight::FollowTerrain { offset }) if !offset.is_finite())
        {
            return Err("源网格跟随地形的高度偏移必须为有限值".into());
        }
        MeshLodPolicy {
            display: self.display_lod.unwrap_or_default(),
            collision: self.collision_lod.unwrap_or_default(),
        }
        .validate()
    }

    pub(super) fn read(document: &GlbStructure, node: usize) -> Result<Self, String> {
        document.node_extra(node, KEY)?.map_or_else(
            || Ok(Self::default()),
            |value| {
                serde_json::from_value(value.clone())
                    .map_err(|error| format!("源 GLB 的子网格属性格式无效：{error}"))
            },
        )
    }

    pub(super) fn write(self, document: &mut GlbStructure, node: usize) -> Result<(), String> {
        self.validate()?;
        let mut extras = document
            .node_extra(node, KEY)?
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        for field in ["height", "collision", "display_lod", "collision_lod"] {
            extras.remove(field); // 恢复继承时移除该项；未知字段留给写入它的工具。
        }
        let Value::Object(known) = serde_json::to_value(self).map_err(|error| error.to_string())?
        else {
            return Err("源网格属性未编码为对象".into());
        };
        extras.extend(known);
        document.set_node_extra(
            node,
            KEY,
            (!extras.is_empty()).then_some(Value::Object(extras)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn source_mesh_overrides_roundtrip_without_changing_other_nodes_or_metadata() {
        let mut json = serde_json::to_vec(&json!({
            "asset":{"version":"2.0"}, "scenes":[{"nodes":[0,1]}],
            "nodes":[{"mesh":0,"extras":{"author_marker":"keep","feathrise_mesh_properties":{"future":42}}},{"mesh":0}],
            "meshes":[{"primitives":[]}]
        })).unwrap();
        json.resize(json.len().next_multiple_of(4), b' ');
        let len = json.len() as u32;
        let mut bytes = b"glTF".to_vec();
        for word in [2, len + 20, len, 0x4e4f_534a] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend(json);

        let mut doc = GlbStructure::from_bytes(&bytes).unwrap();
        let settings = SourceMeshProperties {
            height: Some(SourceMeshHeight::FollowTerrain { offset: 2.5 }),
            collision: Some(false),
            display_lod: Some(MeshLodMode::Disabled),
            collision_lod: Some(MeshLodMode::Inherit),
        };
        assert_eq!(
            SourceMeshProperties::read(&doc, 0).unwrap(),
            SourceMeshProperties::default()
        );
        settings.write(&mut doc, 0).unwrap();
        let mut saved = GlbStructure::from_bytes(&doc.encode().unwrap()).unwrap();
        assert_eq!(SourceMeshProperties::read(&saved, 0).unwrap(), settings);
        assert_eq!(
            SourceMeshProperties::read(&saved, 1).unwrap(),
            SourceMeshProperties::default()
        );
        assert_eq!(
            saved.node_extra(0, "author_marker").unwrap(),
            Some(&json!("keep"))
        );
        assert_eq!(
            saved.node_extra(0, KEY).unwrap(),
            Some(&json!({
                "height":{"mode":"follow_terrain","offset":2.5}, "collision":false,
                "display_lod":{"mode":"disabled"}, "collision_lod":{"mode":"inherit"}, "future":42
            }))
        );
        SourceMeshProperties::default()
            .write(&mut saved, 0)
            .unwrap();
        assert_eq!(
            saved.node_extra(0, KEY).unwrap(),
            Some(&json!({"future":42}))
        );
    }
}
