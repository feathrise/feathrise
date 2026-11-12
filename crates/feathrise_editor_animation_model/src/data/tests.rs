//! 内存 GLB 与隔离临时文件覆盖保真、共享 sampler、三次切线和磁盘并发保护。

use super::{
    reader::{BIN_CHUNK, JSON_CHUNK},
    types::*,
    writer::encode_chunks,
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

fn fixture_parts() -> (Value, Vec<u8>) {
    let values = [0.0f32, 2.0, 0.0, 0.0, 0.0, 2.0, 4.0, 6.0, 0.0, 1.0];
    let mut binary: Vec<u8> = values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    binary.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]); // 未被任何 accessor 引用的数据也必须保留。
    let json = json!({
        "asset": { "version": "2.0", "generator": "动画编辑回归" },
        "extras": { "untouched": [1, "作者数据", {"future": true}] },
        "buffers": [{"byteLength": binary.len()}],
        "bufferViews": [
            {"buffer": 0, "byteOffset": 0, "byteLength": 8},
            {"buffer": 0, "byteOffset": 8, "byteLength": 24},
            {"buffer": 0, "byteOffset": 32, "byteLength": 8}
        ],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": 2, "type": "SCALAR", "min": [0], "max": [2]},
            {"bufferView": 1, "componentType": 5126, "count": 2, "type": "VEC3"},
            {"bufferView": 2, "componentType": 5126, "count": 2, "type": "SCALAR"}
        ],
        "nodes": [
            {"name": "骨架", "children": [1], "translation": [0, 1, 0]},
            {"name": "骨骼", "translation": [0, 0, 1]}
        ],
        "animations": [
            {"name": "挥剑", "extras": {"retain": "clip"},
             "channels": [
                {"sampler": 0, "target": {"node": 0, "path": "translation"}, "extras": {"retain": "channel"}},
                {"sampler": 0, "target": {"node": 1, "path": "scale"}},
                {"sampler": 1, "target": {"node": 0, "path": "weights"}}
             ],
             "samplers": [
                {"input": 0, "output": 1, "interpolation": "LINEAR", "extras": {"retain": "sampler"}},
                {"input": 0, "output": 2, "interpolation": "STEP"}
             ]},
            {"name": "待机", "channels": [{"sampler": 0, "target": {"node": 0, "path": "translation"}}],
             "samplers": [{"input": 0, "output": 1}]}
        ]
    });
    (json, binary)
}

fn glb(json: &Value, binary: &[u8]) -> Vec<u8> {
    encode_chunks(
        &[
            Chunk {
                kind: JSON_CHUNK,
                bytes: Arc::from([]),
            },
            Chunk {
                kind: BIN_CHUNK,
                bytes: Arc::from([]),
            },
            Chunk {
                kind: 0x44434241,
                bytes: Arc::from([9, 8, 7, 6]),
            },
        ],
        json,
        binary,
    )
    .expect("夹具编码")
}

fn fixture() -> AnimationDocument {
    let (json, binary) = fixture_parts();
    AnimationDocument::from_bytes(Path::new("fixture.glb"), glb(&json, &binary))
        .expect("合法动画夹具")
}

pub fn test_document() -> AnimationDocument {
    fixture()
}

pub fn test_scene(json: &Value) -> AnimationDocument {
    AnimationDocument::from_bytes(Path::new("humanoid.glb"), glb(json, &[]))
        .expect("人形骨架场景夹具")
}

/// 两根骨骼都有平移通道，重排后旧数字索引仍可匹配另一骨骼，用于拒绝串骨骼粘贴。
pub fn clipboard_document(reordered: bool) -> AnimationDocument {
    let (mut json, binary) = fixture_parts();
    json["animations"][0]["channels"][1]["target"]["path"] = json!("translation");
    if reordered {
        json["nodes"].as_array_mut().unwrap().swap(0, 1);
        json["nodes"][1]["children"] = json!([0]);
        for clip in json["animations"].as_array_mut().unwrap() {
            for channel in clip["channels"].as_array_mut().unwrap() {
                let node = channel["target"]["node"].as_u64().unwrap();
                channel["target"]["node"] = json!(1 - node);
            }
        }
    }
    AnimationDocument::from_bytes(Path::new("fixture.glb"), glb(&json, &binary))
        .expect("同源节点重排夹具")
}

#[cfg(test)]
fn cubic() -> Channel {
    Channel {
        node: 0,
        path: "translation".into(),
        node_name: "骨架".into(),
        interpolation: "CUBICSPLINE".into(),
        source_index: 0,
        keys: vec![
            Keyframe {
                time: 0.0,
                value: vec![0.0, 0.0, 0.0],
                in_tangent: vec![0.0; 3],
                out_tangent: vec![2.0, 1.0, -1.0],
            },
            Keyframe {
                time: 2.0,
                value: vec![3.0, 1.0, 4.0],
                in_tangent: vec![1.0, 0.0, 2.0],
                out_tangent: vec![0.0; 3],
            },
        ],
    }
}

#[test]
fn reads_nodes_all_clips_and_preserves_unsupported_channels() {
    let doc = fixture();
    assert_eq!(doc.clips.len(), 2);
    assert_eq!(doc.clips[0].channels.len(), 2);
    assert_eq!(doc.nodes[1].parent, Some(0));
    assert_eq!(doc.nodes[0].translation, [0.0, 1.0, 0.0]);
    assert_eq!(doc.clips[0].duration(), 2.0);
    assert_eq!(doc.clips[0].channels[0].sample(1.0), [1.0, 2.0, 3.0]);
    assert!(
        doc.warnings
            .iter()
            .any(|warning| warning.contains("weights"))
    );
    assert_eq!(doc.encode().expect("原样编码"), doc.source.bytes.as_ref());
}

#[test]
fn edited_channel_appends_data_without_changing_other_clips_or_shared_sampler() {
    let mut doc = fixture();
    let original = doc.source.clone();
    doc.clips[0].channels[0]
        .insert_key(1.0, vec![7.0, 8.0, 9.0])
        .expect("插入");
    doc.clips[0].name = "挥剑修订".into();
    let bytes = doc.encode().expect("追加编码");
    let reloaded = AnimationDocument::from_bytes(Path::new("fixture.glb"), bytes).expect("重载");
    assert_eq!(reloaded.clips[0].channels[0].keys[1].value, [7.0, 8.0, 9.0]);
    assert_eq!(reloaded.clips[0].name, "挥剑修订");
    assert_eq!(
        reloaded.clips[0].channels[1],
        original.original_clips[0].channels[1]
    );
    assert_eq!(
        &reloaded.source.buffers[0][..original.buffers[0].len()],
        original.buffers[0].as_ref()
    );
    assert_eq!(
        reloaded.source.json["animations"][1],
        original.json["animations"][1]
    );
    assert_eq!(
        reloaded.source.json["animations"][0]["channels"][2],
        original.json["animations"][0]["channels"][2]
    );
    assert_eq!(
        reloaded.source.json["animations"][0]["samplers"][0],
        original.json["animations"][0]["samplers"][0]
    );
    assert_eq!(
        reloaded.source.json["animations"][0]["channels"][0]["extras"],
        original.json["animations"][0]["channels"][0]["extras"]
    );
    assert_eq!(
        reloaded.source.json["animations"][0]["samplers"][2]["extras"],
        original.json["animations"][0]["samplers"][0]["extras"]
    );
    assert_eq!(reloaded.source.json["extras"], original.json["extras"]);
    assert_eq!(reloaded.source.chunks[2].bytes.as_ref(), [9, 8, 7, 6]);
}

#[test]
fn cubic_insert_preserves_curve_and_move_delete_carry_full_tangents() {
    let mut channel = cubic();
    let expected: Vec<_> = (0..=20)
        .map(|index| channel.sample(index as f32 * 0.1))
        .collect();
    let sampled = channel.sample(0.75);
    let inserted = channel.insert_key(0.75, sampled).expect("插入三次帧");
    assert_eq!(inserted, 1);
    for (index, expected) in expected.iter().enumerate() {
        for (actual, expected) in channel.sample(index as f32 * 0.1).iter().zip(expected) {
            assert!(
                (actual - expected).abs() < 0.00001,
                "细分应保留 Hermite 曲线"
            );
        }
    }
    let inserted_key = channel.keys[inserted].clone();
    let moved = channel.move_key(inserted, 3.0).expect("跨过末帧重排");
    assert_eq!(moved, 2);
    assert_eq!(channel.keys[moved].in_tangent, inserted_key.in_tangent);
    assert_eq!(channel.keys[moved].out_tangent, inserted_key.out_tangent);
    assert!(channel.move_key(moved, 2.0).is_err());
    channel.delete_key(moved).expect("删除");
    assert_eq!(channel.keys.len(), 2);
    assert_eq!(channel.keys[0].out_tangent, [2.0, 1.0, -1.0]);
}

#[test]
fn cubic_roundtrip_keeps_triplet_order_and_interpolation() {
    let mut doc = fixture();
    doc.clips[0].channels[0] = cubic();
    doc.clips[0].channels[0]
        .insert_key(0.5, vec![1.0, 2.0, 3.0])
        .expect("插入");
    let reloaded =
        AnimationDocument::from_bytes(Path::new("fixture.glb"), doc.encode().expect("编码"))
            .expect("读取");
    assert_eq!(reloaded.clips[0].channels[0], doc.clips[0].channels[0]);
}

#[test]
fn quaternion_linear_uses_shortest_spherical_path_and_step_holds_previous() {
    let mut channel = fixture().clips.remove(0).channels.remove(0);
    channel.path = "rotation".into();
    channel.keys[0].value = vec![0.0, 0.0, 0.0, 1.0];
    channel.keys[1].value = vec![0.0, 1.0, 0.0, 0.0];
    let midpoint = channel.sample(1.0);
    assert!((midpoint[1].abs() - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.00001);
    assert!((midpoint[3].abs() - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.00001);
    channel.set_interpolation("STEP").expect("阶梯");
    assert_eq!(channel.sample(1.99), [0.0, 0.0, 0.0, 1.0]);
    assert_eq!(channel.sample(2.0), [0.0, 1.0, 0.0, 0.0]);
    channel
        .insert_key(1.0, vec![0.0, 2.0, 0.0, 2.0])
        .expect("归一化插入");
    assert!(
        (channel.keys[1]
            .value
            .iter()
            .map(|value| value * value)
            .sum::<f32>()
            - 1.0)
            .abs()
            < 0.00001
    );
}

#[test]
fn invalid_timestamps_components_and_last_key_are_rejected_without_mutation() {
    let mut channel = cubic();
    let original = channel.clone();
    assert!(channel.insert_key(f32::NAN, vec![0.0; 3]).is_err());
    assert!(channel.insert_key(1.0, vec![0.0; 2]).is_err());
    assert!(channel.move_key(0, 2.0).is_err());
    assert_eq!(channel, original);
    channel.delete_key(1).expect("保留一帧");
    assert!(channel.delete_key(0).is_err());
    channel.keys[0].time = -1.0;
    assert!(channel.validate().is_err());
    channel.keys[0].time = 0.0;
    channel.keys[0].out_tangent.pop();
    assert!(channel.validate().is_err());
}

#[test]
fn strided_float_accessor_is_read_using_declared_offsets() {
    let (mut json, mut binary) = fixture_parts();
    binary.truncate(8);
    for values in [[1.0f32, 2.0, 3.0, 999.0], [4.0f32, 5.0, 6.0, 999.0]] {
        binary.extend(values.iter().flat_map(|value| value.to_le_bytes()));
    }
    json["buffers"][0]["byteLength"] = json!(binary.len());
    json["bufferViews"][1]["byteStride"] = json!(16);
    json["bufferViews"][1]["byteLength"] = json!(32);
    json["animations"][0]["channels"]
        .as_array_mut()
        .expect("数组")
        .pop();
    let doc = AnimationDocument::from_bytes(Path::new("strided.glb"), glb(&json, &binary))
        .expect("步长读取");
    assert_eq!(doc.clips[0].channels[0].keys[0].value, [1.0, 2.0, 3.0]);
    assert_eq!(doc.clips[0].channels[0].keys[1].value, [4.0, 5.0, 6.0]);
}

#[test]
fn malformed_bounds_and_sparse_accessors_are_read_only_channels() {
    let (mut json, binary) = fixture_parts();
    json["accessors"][1]["count"] = json!(usize::MAX);
    let doc = AnimationDocument::from_bytes(Path::new("overflow.glb"), glb(&json, &binary))
        .expect("保留坏通道");
    assert!(doc.clips[0].channels.is_empty());
    assert!(doc.warnings.iter().any(|warning| warning.contains("溢出")));
    json["accessors"][1]["count"] = json!(2);
    json["accessors"][1]["sparse"] = json!({"count": 1});
    let doc = AnimationDocument::from_bytes(Path::new("sparse.glb"), glb(&json, &binary))
        .expect("保留 sparse");
    assert!(
        doc.warnings
            .iter()
            .any(|warning| warning.contains("sparse"))
    );
}

#[test]
fn invalid_glb_header_truncation_and_cyclic_hierarchy_are_rejected() {
    let (mut json, binary) = fixture_parts();
    let mut bytes = glb(&json, &binary);
    bytes.pop();
    assert!(AnimationDocument::from_bytes(Path::new("truncated.glb"), bytes).is_err());
    json["nodes"][1]["children"] = json!([0]);
    assert!(AnimationDocument::from_bytes(Path::new("cycle.glb"), glb(&json, &binary)).is_err());
}

pub struct TemporaryDirectory(pub PathBuf);

impl TemporaryDirectory {
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "rpg-animation-data-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).expect("创建隔离目录");
        Self(path)
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn write_directory_clip_fixture(path: &Path, second_name: &str) {
    let (mut json, binary) = fixture_parts();
    json["animations"][1]["name"] = json!(second_name);
    std::fs::write(path, glb(&json, &binary)).expect("写目录动作夹具");
}

#[test]
fn atomic_save_reopens_and_rejects_external_edits() {
    let directory = TemporaryDirectory::new();
    let path = directory.0.join("动作.glb");
    let (json, binary) = fixture_parts();
    std::fs::write(&path, glb(&json, &binary)).expect("写夹具");
    let mut doc = AnimationDocument::load(&path).expect("读取");
    doc.clips[0].channels[0].keys[1].value[0] = 42.0;
    doc.save(&path).expect("原子保存");
    let mut reloaded = AnimationDocument::load(&path).expect("重载");
    assert_eq!(reloaded.clips[0].channels[0].keys[1].value[0], 42.0);
    assert_eq!(std::fs::read_dir(&directory.0).expect("目录").count(), 1);
    reloaded.clips[0].channels[0].keys[1].value[0] = 99.0;
    let external = b"Blender concurrent export";
    std::fs::write(&path, external).expect("模拟外部导出");
    let error = reloaded.save(&path).expect_err("拒绝过期文档");
    assert!(error.contains("外部修改"));
    assert_eq!(std::fs::read(&path).expect("检查原字节"), external);
}

#[test]
fn external_gltf_reads_all_channels_and_explicitly_refuses_writeback() {
    let directory = TemporaryDirectory::new();
    let path = directory.0.join("动作.gltf");
    let (mut json, binary) = fixture_parts();
    json["buffers"][0]["uri"] = json!("animation.bin");
    std::fs::write(directory.0.join("animation.bin"), binary).expect("外置 BIN");
    std::fs::write(&path, serde_json::to_vec(&json).expect("JSON")).expect("GLTF");
    let doc = AnimationDocument::load(&path).expect("读取 GLTF");
    assert!(doc.read_only);
    assert_eq!(doc.clips[0].channels.len(), 2);
    assert!(
        doc.save(&path)
            .expect_err("拒绝写回外置 GLTF")
            .contains("仅可查看")
    );
}

#[test]
fn save_rebased_keeps_unused_cubic_tangents_for_undo_without_reappending_data() {
    let directory = TemporaryDirectory::new();
    let path = directory.0.join("三次插值.glb");
    let (json, binary) = fixture_parts();
    std::fs::write(&path, glb(&json, &binary)).expect("写夹具");
    let mut cubic_doc = AnimationDocument::load(&path).expect("读取");
    cubic_doc.clips[0].channels[0] = cubic();
    let mut document = cubic_doc.save_rebased(&path).expect("保存原三次曲线");
    assert!(
        Arc::ptr_eq(&cubic_doc.source_identity, &document.source_identity),
        "原子保存后骨骼身份不变，应保留既有剪贴板的同源资格"
    );
    let original = document.clips[0].channels[0].clone();
    document.clips[0].channels[0]
        .set_interpolation("LINEAR")
        .expect("切为线性");
    let mut saved = document
        .save_rebased(&path)
        .expect("保存线性但保留作者切线");
    let disk_bytes = std::fs::read(&path).expect("读取已保存字节");
    assert_eq!(saved.clips[0].channels[0].keys, original.keys);
    assert_eq!(
        saved.encode().expect("没有新修改"),
        disk_bytes,
        "作者隐藏切线已作为新基线，不重复追加 accessor"
    );
    // 独立重新加载文件确实没有这些切线；保存后的编辑会话必须保有它们，撤销才不会变成零切线。
    let disk_only = AnimationDocument::load(&path).expect("独立磁盘加载");
    assert!(
        !Arc::ptr_eq(&saved.source_identity, &disk_only.source_identity),
        "独立重新解析必须建立新身份，不能仅凭相同路径或字节复用节点编号"
    );
    assert!(
        disk_only.clips[0].channels[0].keys[0]
            .out_tangent
            .is_empty()
    );
    saved.clips[0].channels[0]
        .set_interpolation("CUBICSPLINE")
        .expect("模拟撤销插值切换");
    assert_eq!(saved.clips[0].channels[0], original);
    for time in [0.25, 0.75, 1.25, 1.75] {
        assert_eq!(
            saved.clips[0].channels[0].sample(time),
            original.sample(time)
        );
    }
    let restored = saved.save_rebased(&path).expect("保存撤销后的原三次曲线");
    assert_eq!(restored.clips[0].channels[0], original);
    let reloaded = AnimationDocument::load(&path).expect("磁盘恢复三次切线");
    assert_eq!(reloaded.clips[0].channels[0], original);
}

#[test]
fn neutral_animation_asset_loads_all_channels_without_warnings() {
    // 独立编辑器使用自建五骨骼夹具，原游戏动作资产不再是公开仓库测试前提。
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../feathrise_editor_animation/tests/fixtures/animation_preview");
    for (name, key_count) in [("animated_rig.glb", 10)] {
        let path = root.join(name);
        let doc = AnimationDocument::load(&path).expect("中性动画完整载入");
        assert_eq!(doc.clips.len(), 1, "{name}");
        assert_eq!(doc.nodes.len(), 5, "{name}");
        assert_eq!(doc.clips[0].channels.len(), 5, "{name}");
        assert_eq!(
            doc.clips[0]
                .channels
                .iter()
                .map(|channel| channel.keys.len())
                .sum::<usize>(),
            key_count,
            "{name}"
        );
        assert!(doc.warnings.is_empty(), "{name}: {:?}", doc.warnings);
        assert!(!doc.read_only, "{name}");
        assert_eq!(
            doc.encode().expect("无改动字节保真"),
            doc.source.bytes.as_ref(),
            "{name}"
        );
    }
}
