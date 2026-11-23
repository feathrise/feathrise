//! 节点的文件级身份、原始载荷和跨源索引在保存重开后仍然正确。

use super::*;
use crate::data::AnimationDocument;
use std::path::Path;

const UNKNOWN_CHUNK: u32 = 0x0ab1_23cd;

fn fixture(json: &Value, binary: &[u8]) -> GlbStructure {
    let chunks = [
        Chunk {
            kind: JSON_CHUNK,
            bytes: Arc::default(),
        },
        Chunk {
            kind: UNKNOWN_CHUNK,
            bytes: Arc::from([0xde, 0xad, 0xbe, 0xef]),
        },
    ];
    GlbStructure::from_bytes(&encode_chunks(&chunks, json, binary).unwrap()).unwrap()
}

#[test]
fn mesh_apply_preserves_world_geometry_children_and_shared_mesh_after_reopen() {
    let positions = [0.0_f32,0.0,0.0, 1.0,0.0,0.0, 0.0,1.0,0.0];
    let bytes = positions.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>();
    let mut document = fixture(&json!({
        "asset":{"version":"2.0"}, "scenes":[{"nodes":[0,1]}],
        "nodes":[{"mesh":0,"children":[2],"translation":[4.0,5.0,6.0],"scale":[-2.0,3.0,4.0]},
            {"mesh":0}, {"translation":[0.0,3.0,0.0]}],
        "meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}],
        "buffers":[{"byteLength":36}], "bufferViews":[{"buffer":0,"byteLength":36}],
        "accessors":[{"bufferView":0,"type":"VEC3","componentType":5126,"count":3,"min":[0,0,0],"max":[1,1,0]}]
    }), &bytes);
    let world = document.node_world_matrix(0).unwrap();
    let child = document.node_world_matrix(2).unwrap();
    assert!(document.bake_mesh_transform(0, Transform::IDENTITY).unwrap());
    let document = GlbStructure::from_bytes(&document.encode().unwrap()).unwrap();
    assert_eq!(document.node_transform(0).unwrap(), Transform::IDENTITY);
    assert_eq!(document.node(1).unwrap().mesh, Some(0));
    assert!(document.node_world_matrix(2).unwrap().abs_diff_eq(child, 1.0e-6));
    let primitive = &document.json["meshes"][1]["primitives"][0];
    let index = index_field(&primitive["attributes"], "POSITION").unwrap();
    let floats = super::super::reader::read_accessor(&document.json, &[document.binary.clone()], index, 3).unwrap();
    for (before, after) in positions.chunks_exact(3).zip(floats.chunks_exact(3)) {
        assert_eq!(world.transform_point3(Vec3::from_slice(before)), Vec3::from_slice(after));
    }
    assert_eq!(&document.binary[document.binary.len()-12..], &[0,0,0,0,2,0,0,0,1,0,0,0]);
    assert_eq!(document.mesh_geometry(0).unwrap().size, Some(Vec3::new(2.0,3.0,0.0)));
}

#[test]
fn writes_real_folder_and_keeps_node_world_transform() {
    let mut document = fixture(
        &json!({
            "asset": {"version": "2.0"},
            "scenes": [{"nodes": [0]}],
            "nodes": [
                {"name": "来源", "translation": [4, 0, 0], "children": [1], "extras": {"custom": true}},
                {"name": "原名", "translation": [2, 0, 0]},
            ],
        }),
        &[],
    );
    let world = document.node_world_matrix(1).unwrap();
    let root = GlbParent::Scene(0);
    let folder = document.create_folder(root, "新组").unwrap();
    document.rename_node(1, "石头").unwrap();
    document.move_node(1, GlbParent::Node(folder)).unwrap();

    let bytes = document.encode().unwrap();
    let mut saved = GlbStructure::from_bytes(&bytes).unwrap();
    assert_eq!(saved.scene_index(), Some(0));
    assert!(saved.children(root).unwrap()[1].folder);
    let mesh = &saved.children(GlbParent::Node(folder)).unwrap()[0];
    assert_eq!((mesh.index, mesh.name.as_str()), (1, "石头"));
    assert_eq!(saved.node_world_matrix(1).unwrap(), world);
    assert_eq!(saved.json["nodes"][0]["extras"], json!({"custom": true}));
    assert!(saved.chunks.iter().any(
        |chunk| chunk.kind == UNKNOWN_CHUNK && chunk.bytes.as_ref() == [0xde, 0xad, 0xbe, 0xef]
    ));
    let mut pose = saved.node_transform(1).unwrap();
    pose.translation.y = 2.0;
    saved.set_node_transform(1, pose).unwrap();
    assert_eq!(saved.node_transform(1).unwrap(), pose);
    assert_eq!(saved.node_transform(0).unwrap().translation, 4.0 * Vec3::X);
}

#[test]
fn remove_empty_folder_preserves_other_nodes_and_rejects_a_parent() {
    let mut document = fixture(&json!({
        "asset": {"version": "2.0"}, "scenes": [{"nodes": [0]}], "nodes": [{"name": "已有模型"}],
    }), &[]);
    let root = GlbParent::Scene(0);
    let folder = document.create_folder(root, "外层").unwrap();
    let child = document.create_folder(GlbParent::Node(folder), "空的").unwrap();
    assert!(document.remove_empty_folder(folder).is_err());
    assert!(document.remove_empty_folder(0).is_err());
    document.remove_empty_folder(child).unwrap();
    assert!(document.children(GlbParent::Node(folder)).unwrap().is_empty());
    document.remove_empty_folder(folder).unwrap();
    let saved = GlbStructure::from_bytes(&document.encode().unwrap()).unwrap();
    assert_eq!(saved.children(root).unwrap()[0].name, "已有模型");
    assert_eq!(saved.children(root).unwrap().len(), 1);
}

#[test]
fn remove_empty_folder_keeps_the_last_scene_root_loadable() {
    let chunks = [Chunk { kind: JSON_CHUNK, bytes: Arc::default() }];
    let bytes = encode_chunks(&chunks, &json!({
        "asset": {"version": "2.0"}, "scene": 0, "scenes": [{"nodes": []}],
    }), &[]).unwrap();
    let mut document = GlbStructure::from_bytes(&bytes).unwrap();
    let folder = document.create_folder(GlbParent::Scene(0), "空的").unwrap();
    document.remove_empty_folder(folder).unwrap();

    let bytes = document.encode().unwrap();
    let glb = bevy::gltf::gltf::Gltf::from_slice(&bytes).unwrap();
    assert_eq!(glb.default_scene().unwrap().nodes().count(), 0);
}

#[test]
fn reparent_keeps_unchanged_animation_space_and_rejects_cycle_and_changed_basis() {
    let mut document = fixture(
        &json!({
            "asset":{"version":"2.0"},
            "scenes":[{"nodes":[0, 4]}],
            "nodes":[
                {"children":[1,2]},
                {"translation":[2,0,0], "children":[3]},
                {"translation":[2,0,0]},
                {"translation":[0,1,0]},
                {},
            ],
            "animations":[{"name":"父与子同一片段","samplers":[],
                "channels":[{"target":{"node":0,"path":"translation"}}, {"target":{"node":3,"path":"scale"}}]
            }],
        }),
        &[],
    );
    let world = document.node_world_matrix(3).unwrap();
    let animation = document.json["animations"].clone();
    document.move_node(3, GlbParent::Node(2)).unwrap();
    assert_eq!(document.node_world_matrix(3).unwrap(), world);
    assert_eq!(document.json["animations"], animation);

    let before = document.encode().unwrap();
    assert!(document.move_node(0, GlbParent::Node(3)).is_err());
    assert!(document.move_node(3, GlbParent::Node(4)).is_err());
    assert_eq!(document.encode().unwrap(), before);
}

fn animated_mesh() -> GlbStructure {
    let binary = [
        0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, // 一个三角面。
        0.0, 0.0, 1.0, 0.0, 0.0, 1.0, // TEXCOORD_0。
        0.0, 1.0, // 关键帧时间。
        0.0, 0.0, 0.0, 0.0, 1.0, 0.0, // 两个位置键。
    ]
    .into_iter()
    .flat_map(f32::to_le_bytes)
    .collect::<Vec<_>>();
    fixture(
        &json!({
            "asset":{"version":"2.0"},
            "scenes":[{"nodes":[0,2]}],
            "nodes":[
                {"name":"含动画的组","translation":[1,0,0],"children":[1]},
                {"name":"被移动节点","mesh":0},
                {"name":"共享原 Mesh 的另一个实例","mesh":0},
            ],
            "meshes":[{"name":"三角网格","primitives":[{"attributes":{"POSITION":0,"TEXCOORD_0":1},"material":0}]}],
            "materials":[{"pbrMetallicRoughness":{"baseColorTexture":{"index":0}}}],
            "textures":[{"source":0}],
            "images":[{"uri":"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+tmh0AAAAASUVORK5CYII="}],
            "buffers":[{"byteLength":binary.len()}],
            "bufferViews":[
                {"buffer":0,"byteLength":36},
                {"buffer":0,"byteOffset":36,"byteLength":24},
                {"buffer":0,"byteOffset":60,"byteLength":8},
                {"buffer":0,"byteOffset":68,"byteLength":24},
            ],
            "accessors":[
                {"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]},
                {"bufferView":1,"componentType":5126,"count":3,"type":"VEC2"},
                {"bufferView":2,"componentType":5126,"count":2,"type":"SCALAR","min":[0],"max":[1]},
                {"bufferView":3,"componentType":5126,"count":2,"type":"VEC3"},
            ],
            "animations":[{"name":"原动作","samplers":[{"input":2,"output":3,"interpolation":"LINEAR"}],
                "channels":[{"sampler":0,"target":{"node":1,"path":"translation"}},
                            {"sampler":0,"target":{"node":2,"path":"translation"}}]
            }],
        }),
        &binary,
    )
}

#[test]
fn transfer_remaps_shared_mesh_material_and_animation_and_keeps_other_indices() {
    let mut source = animated_mesh();
    let mut target = animated_mesh();
    let offset = Mat4::from_translation(10.0 * Vec3::X);
    let world = source.node_world_matrix(1).unwrap();
    let kept = source.json["animations"].clone();
    let root = GlbParent::Scene(0);
    let new = source
        .transfer_node_to_in_space(0, &mut target, GlbParent::Node(0), offset)
        .unwrap();
    let new_node = target.children(GlbParent::Node(new)).unwrap().remove(0);
    assert_eq!(source.children(root).unwrap()[0].index, 2);
    assert_eq!(source.node(2).unwrap().mesh, Some(0));
    assert_eq!(source.json["animations"], kept);
    assert_eq!(new_node.mesh, Some(1));
    assert_eq!(target.mesh_name(1).unwrap(), Some("三角网格"));
    assert_eq!(
        target.node_world_matrix(new_node.index).unwrap(),
        offset * world
    );
    assert_eq!(target.json["textures"][1]["source"], json!(1));
    assert_eq!(
        target.json["materials"][1]["pbrMetallicRoughness"]["baseColorTexture"]["index"],
        json!(1)
    );

    let saved =
        AnimationDocument::from_glb_bytes(Path::new("target.glb"), target.encode().unwrap())
            .unwrap();
    assert_eq!(saved.clips.len(), 2);
    assert_eq!(saved.clips[0].channels.len(), 2);
    let channel = &saved.clips[1].channels[0];
    assert_eq!(channel.node, new_node.index);
    assert_eq!(channel.keys[1].value, [0.0, 1.0, 0.0]);
}

#[test]
fn failed_cross_file_move_changes_neither_document() {
    let mut source = animated_mesh();
    let mut target = animated_mesh();
    source.json["images"][0]["uri"] = json!("assets/relative.png");
    let previous_source = source.encode().unwrap();
    let previous_target = target.encode().unwrap();
    assert!(
        source
            .transfer_node_to(0, &mut target, GlbParent::Scene(0))
            .unwrap_err()
            .contains("嵌入")
    );
    assert_eq!(source.encode().unwrap(), previous_source);
    assert_eq!(target.encode().unwrap(), previous_target);
}

#[test]
fn translating_animated_root_offsets_only_its_positions_and_splits_shared_sampler() {
    for interpolation in ["STEP", "LINEAR", "CUBICSPLINE"] {
        let mut source = animated_mesh();
        let mut target = animated_mesh();
        let mut binary = source.binary.to_vec();
        let mut append = |samples: &[f32], dimensions, json: &mut Value| {
            super::super::writer::append_accessor(json, &mut binary, samples, dimensions, false)
                .unwrap()
        };
        let values: &[f32] = if interpolation == "CUBICSPLINE" {
            // 每关键帧依次是入切线、值、出切线。非零切线可发现把偏移错误加进切线的回归。
            &[
                0.1, 0.2, 0.3, 1., 1., 1., 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 2., 3., 4., 1.0, 1.1, 1.2,
            ]
        } else {
            &[1., 1., 1., 2., 3., 4.]
        };
        let positions = append(values, 3, &mut source.json);
        let rotation_values: &[f32] = if interpolation == "CUBICSPLINE" {
            &[
                0., 0., 0., 0., 0., 0., 0., 1., 0., 0., 0., 0., 0., 0., 0., 0., 0., 1., 0., 0., 0.,
                0., 0., 0.,
            ]
        } else {
            &[0., 0., 0., 1., 0., 1., 0., 0.]
        };
        let rotations = append(rotation_values, 4, &mut source.json);
        source.json["animations"][0]["samplers"] = json!([
            {"input":2, "output":positions, "interpolation":interpolation},
            {"input":2, "output":rotations, "interpolation":interpolation},
        ]);
        for (property, sampler) in [("translation", 0), ("scale", 0), ("rotation", 1)] {
            source.json["animations"][0]["channels"]
                .as_array_mut()
                .unwrap()
                .push(json!({
                    "sampler":sampler, "target":{"node":0, "path":property},
                }));
        }
        source.json["buffers"][0]["byteLength"] = json!(binary.len());
        source.binary = binary.into();
        let read_animation = |asset: &GlbStructure| {
            AnimationDocument::from_glb_bytes(Path::new("fixture.glb"), asset.encode().unwrap())
                .unwrap()
        };
        let previous = read_animation(&source);
        let previous_dst = read_animation(&target);
        let world = source.node_world_matrix(0).unwrap();
        let offset = Vec3::new(10., -2., 7.);
        let expected_adjustment = offset - Vec3::X;
        let moved = source
            .transfer_node_to_in_space(
                0,
                &mut target,
                GlbParent::Node(0),
                Mat4::from_translation(offset),
            )
            .unwrap();
        let child = target.node(moved).unwrap().children[0];
        assert_eq!(
            target.node_world_matrix(moved).unwrap(),
            Mat4::from_translation(offset) * world
        );
        assert_eq!(source.children(GlbParent::Scene(0)).unwrap()[0].index, 2);
        assert_eq!(
            read_animation(&source).clips[0].channels,
            previous.clips[0].channels
        );

        let result = read_animation(&target);
        assert_eq!(result.clips[0].channels, previous_dst.clips[0].channels);
        let clip = &result.clips[1];
        let find = |node, property| {
            clip.channels
                .iter()
                .find(|channel| channel.node == node && channel.path == property)
                .unwrap()
        };
        assert_eq!(
            find(child, "translation").keys,
            previous.clips[0].channels[0].keys
        );
        assert_eq!(
            find(moved, "scale").keys,
            previous.clips[0].channels[3].keys
        );
        assert_eq!(
            find(moved, "rotation").keys,
            previous.clips[0].channels[4].keys
        );
        for (next, original) in find(moved, "translation")
            .keys
            .iter()
            .zip(&previous.clips[0].channels[2].keys)
        {
            assert_eq!(next.time, original.time);
            assert_eq!(
                next.value,
                (Vec3::from_slice(&original.value) + expected_adjustment).to_array()
            );
            assert_eq!(next.in_tangent, original.in_tangent);
            assert_eq!(next.out_tangent, original.out_tangent);
        }
        assert_eq!(
            target.json["animations"][1]["samplers"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
    }
}
