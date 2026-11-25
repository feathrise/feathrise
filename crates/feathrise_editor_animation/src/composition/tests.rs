//! 通过标准 GLB 输出及独立读回验证两份动作合入角色、撤销重做与所选资源依赖闭包。

use super::*;
use crate::data::ContentItem;
use std::{path::Path, sync::Arc};

const ACTION: &[u8] = include_bytes!("../../tests/fixtures/animation_preview/animated_rig.glb");
const MODEL: &[u8] = include_bytes!("../../tests/fixtures/animation_preview/skinned_rig.glb");

#[test]
fn composition_imports_two_files_reopens_independently_and_exports_selected_dependencies() {
    let dir = std::env::temp_dir().join(format!(
        "feathrise-glb-composition-{}",
        crate::data::new_scene_clip_id()
    ));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("角色.glb");
    std::fs::write(&path, MODEL).unwrap();
    let mut target = AnimationDocument::load(&path).unwrap();
    let row = [ContentSpan {
        kind: ContentKind::Animation,
        first: 0,
        last: 0,
    }];
    let mut last = None;
    for name in ["待机", "行走"] {
        let source_path = dir.join(format!("{name}.glb"));
        std::fs::write(&source_path, ACTION).unwrap();
        let mut source = AnimationDocument::load(&source_path).unwrap();
        source.clips[0].name = name.into();
        let edit = import(&target, &source, &row, false).unwrap();
        edit.apply(&mut target, true).unwrap();
        assert_eq!(std::fs::read(&source_path).unwrap(), ACTION);
        std::fs::remove_file(&source_path).unwrap();
        last = Some(edit);
    }
    let edit = last.unwrap();
    edit.apply(&mut target, false).unwrap();
    assert_eq!(target.clips.len(), 1);
    edit.apply(&mut target, true).unwrap();
    target.save(&path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let loaded = bevy::gltf::gltf::Gltf::from_slice(&bytes).unwrap();
    assert_eq!(
        loaded
            .animations()
            .map(|c| c.name().unwrap())
            .collect::<Vec<_>>(),
        ["待机", "行走"]
    );
    assert_eq!(loaded.skins().count(), 1);
    assert_eq!(loaded.meshes().count(), 1);
    let mut separate = AnimationDocument::load(&path).unwrap();
    separate.clips.reverse(); // 显示顺序可变，另存仍按选中的稳定 ID 取目标。
    let chosen = ContentItem {
        kind: ContentKind::Animation,
        id: 1,
    };
    let bytes = separate.encode_selection(&[chosen]).unwrap();
    let action = bevy::gltf::gltf::Gltf::from_slice(&bytes).unwrap();
    assert_eq!(action.animations().next().unwrap().name(), Some("行走"));
    assert_eq!(
        (
            action.animations().count(),
            action.meshes().count(),
            action.skins().count()
        ),
        (1, 0, 0)
    );
    let clip = AnimationDocument::from_glb_bytes(Path::new("独立.glb"), bytes).unwrap();
    let original = separate.selected_clip(chosen.id).unwrap();
    for (a, b) in clip.clips[0].channels.iter().zip(&original.channels) {
        assert_eq!(
            (&a.node_name, &a.path, &a.keys),
            (&b.node_name, &b.path, &b.keys)
        );
    }
    let model = separate
        .nodes
        .iter()
        .position(|n| n.name == "FixtureSurface")
        .unwrap();
    let bytes = separate
        .encode_selection(&[
            ContentItem {
                kind: ContentKind::Node,
                id: model,
            },
            chosen,
        ])
        .unwrap();
    let selected = bevy::gltf::gltf::Gltf::from_slice(&bytes).unwrap();
    assert_eq!(
        (
            selected.meshes().count(),
            selected.skins().count(),
            selected.animations().count()
        ),
        (1, 1, 1)
    );
    assert!(
        selected
            .skins()
            .next()
            .unwrap()
            .inverse_bind_matrices()
            .is_some()
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn ambiguous_joint_paths_reject_entire_import_and_source_snapshots_survive_edits() {
    let mut target =
        AnimationDocument::from_glb_bytes(Path::new("target.glb"), MODEL.to_vec()).unwrap();
    let source = Arc::new(
        AnimationDocument::from_glb_bytes(Path::new("source.glb"), ACTION.to_vec()).unwrap(),
    );
    let mut target_bytes = serde_json::Value::Null;
    target.inspect_glb(|json, _| target_bytes = json.clone());
    let raw = bevy::gltf::gltf::binary::Glb::from_slice(MODEL).unwrap();
    let animated = source.clips[0].channels[0].node_name.clone();
    let at = target
        .nodes
        .iter()
        .position(|n| n.name == animated)
        .unwrap();
    let dup = target_bytes["nodes"][at].clone();
    let nodes = target_bytes["nodes"].as_array_mut().unwrap();
    let duplicate = nodes.len();
    let mut extra = dup;
    extra.as_object_mut().unwrap().remove("children");
    nodes.push(extra);
    if let Some(parent) = target.nodes[at].parent {
        target_bytes["nodes"][parent]["children"]
            .as_array_mut()
            .unwrap()
            .push(duplicate.into());
    } else {
        target_bytes["scenes"][0]["nodes"]
            .as_array_mut()
            .unwrap()
            .push(duplicate.into());
    }
    let encoded = bevy::gltf::gltf::binary::Glb {
        json: serde_json::to_vec(&target_bytes).unwrap().into(),
        ..raw
    };
    target =
        AnimationDocument::from_glb_bytes(Path::new("ambiguous.glb"), encoded.to_vec().unwrap())
            .unwrap();
    let rows = [ContentSpan {
        kind: ContentKind::Animation,
        first: 0,
        last: 0,
    }];
    assert!(import(&target, &source, &rows, false).is_err());
    assert!(target.clips.is_empty());
}

#[test]
fn selected_material_embeds_relative_image_and_preserves_no_external_dependencies() {
    let root = std::env::temp_dir().join(format!(
        "feathrise-glb-image-{}",
        crate::data::new_scene_clip_id()
    ));
    std::fs::create_dir(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let image = include_bytes!("../../../../tests/fixtures/scene/albedo.png");
    std::fs::write(root.join("手 部.png"), image).unwrap();
    let raw = bevy::gltf::gltf::binary::Glb::from_slice(MODEL).unwrap();
    let mut json: serde_json::Value = serde_json::from_slice(&raw.json).unwrap();
    json["images"] = serde_json::json!([{"uri":"手%20部.png"}]);
    json["textures"] = serde_json::json!([{"source":0}]);
    json["materials"] =
        serde_json::json!([{"pbrMetallicRoughness":{"baseColorTexture":{"index":0}}}]);
    let raw = bevy::gltf::gltf::binary::Glb {
        json: serde_json::to_vec(&json).unwrap().into(),
        ..raw
    };
    let doc =
        AnimationDocument::from_glb_bytes(&root.join("source.glb"), raw.to_vec().unwrap()).unwrap();
    let selected = [ContentItem {
        kind: ContentKind::Material,
        id: 0,
    }];
    let output = doc.encode_selection_in(&selected, Some(&root)).unwrap();
    let gltf = bevy::gltf::gltf::Gltf::from_slice(&output).unwrap();
    assert_eq!(
        (
            gltf.meshes().count(),
            gltf.nodes().count(),
            gltf.materials().count(),
            gltf.images().count()
        ),
        (0, 0, 1, 1)
    );
    let bevy::gltf::gltf::image::Source::View { view, mime_type } =
        gltf.images().next().unwrap().source()
    else {
        panic!()
    };
    assert_eq!(mime_type, "image/png");
    assert_eq!(
        &gltf.blob.as_ref().unwrap()[view.offset()..view.offset() + view.length()],
        image
    );
    std::fs::remove_file(root.join("手 部.png")).unwrap();
    assert!(doc.encode_selection_in(&selected, Some(&root)).is_err());
    std::fs::remove_dir(&root).unwrap();
}
