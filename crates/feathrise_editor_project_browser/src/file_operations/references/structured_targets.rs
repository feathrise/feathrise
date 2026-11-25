//! 使用正式摆放 GLB、旧摆放 JSON 及建筑预设验证独立资产绑定，只允许替换物理路径。

use super::*;
use feathrise_scene_content::{BuildingPresetDescriptor, ScenePlacementAssetTarget as Target, ScenePlacementsAsset};

const OLD: &str = "models/门.glb";
const NEW: &str = "out/新门.glb";

fn model(meshes: &[usize], materials: usize) -> Vec<u8> {
    model_with(meshes, materials, |_| {})
}

fn model_with(meshes: &[usize], materials: usize, configure: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
    // 独立图元可直接加载，无需 Scene0。各图元使用同一实际三角，材质表供保存的材质索引单独引用。
    let triangle: Vec<_> = [0_f32,0.,0., 1.,0.,0., 0.,1.,0.].into_iter().flat_map(f32::to_le_bytes).collect();
    let meshes: Vec<_> = meshes.iter().map(|&count| json!({"primitives":vec![json!({"attributes":{"POSITION":0}}); count]})).collect();
    let mut document = json!({"asset":{"version":"2.0"},"buffers":[{"byteLength":triangle.len()}],
        "bufferViews":[{"buffer":0,"byteLength":triangle.len()}],"accessors":[{"bufferView":0,
        "componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}],"meshes":meshes,"materials":vec![json!({}); materials]});
    configure(&mut document);
    let bytes = gltf::binary::Glb { header: gltf::binary::Header { magic:*b"glTF", version:2, length:0 },
        json: serde_json::to_vec(&document).unwrap().into(), bin:Some(triangle.into()) }.to_vec().unwrap();
    gltf::Gltf::from_slice(&bytes).expect("替代 GLB 应通过标准 glTF 验证");
    bytes
}

pub(super) fn mirrored_model(parent_x: f32, child_x: f32, material: usize, reachable: bool) -> Vec<u8> {
    model_with(&[1], 2, |model| {
        // 网格只挂在第二个场景。父节点存矩阵，子节点存 TRS，二者累计的符号决定发放的材质标签。
        model["scene"] = json!(0);
        model["scenes"] = json!([{"nodes":[0]}, {"nodes": if reachable { vec![1] } else { vec![] }}]);
        model["nodes"] = json!([{}, {"matrix":[parent_x,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1],"children":[2]},
            {"mesh":0, "scale":[child_x,1,1]}]);
        model["meshes"][0]["primitives"][0]["material"] = json!(material);
    })
}

fn placements(target: Target) -> ScenePlacementsAsset {
    serde_json::from_value(json!({"schema_version":2,"scene_id":"SCENE-a","placements":[{
        "id":"门","asset_path":OLD,"asset_target":target,"translation":[0,0,0],"yaw_degrees":0,"scale":[1,1,1]}]})).unwrap()
}

#[test]
fn relink_validates_mesh_primitive_and_material_bindings_from_each_saved_record_type() {
    let f = Fixture::new();
    let mut r = missing_fixture(&f);
    let replacement = f.path("assets/out/新门.glb");
    let scene = f.path("assets/scenes/a/scene.glb");
    let legacy = f.path("assets/scenes/b/placements.scene_placements.json");
    let preset = f.path("assets/config/example.building_presets.json");
    let target = |mesh, primitive, material| Target::Primitive { mesh, primitive, material, collision:true };
    let mut saved = placements(target(0,1,Some(0)));
    let scene_path = "scenes/a/scene.glb";
    let scene_bytes = feathrise_scene_content::write_scene_placements_glb(&saved, scene_path, None).unwrap();
    let legacy_text = serde_json::to_string_pretty(&placements(target(1,0,None))).unwrap();
    let house = BuildingPresetDescriptor::from_asset("gate".into(), "门楼".into(), OLD.into(), target(0,0,Some(1)));
    let preset_text = serde_json::to_string_pretty(&json!({"schema_version":1,"presets":[house]})).unwrap();
    fs::write(&scene, &scene_bytes).unwrap();
    fs::write(&legacy, &legacy_text).unwrap();
    fs::write(&preset, &preset_text).unwrap();
    for (bytes, label) in [(model(&[1,1],2), "Mesh0/Primitive1"), (model(&[2],2), "Mesh1/Primitive0"), (model(&[2,1],1), "Material1/std")] {
        fs::write(&replacement, bytes).unwrap();
        let error = run(r.clone(), false).1.unwrap_err();
        assert!(error.contains(label), "{error}");
        assert_eq!(fs::read(&scene).unwrap(), scene_bytes);
        assert_eq!(fs::read_to_string(&legacy).unwrap(), legacy_text);
        assert_eq!(fs::read_to_string(&preset).unwrap(), preset_text);
    }
    let complete = model(&[2,1],2);
    fs::write(&replacement, &complete).unwrap();
    let preview = inspect(&r);
    assert_eq!(preview.documents.len(), 3);
    r.references = Some(ReferenceUpdate::Confirm(preview));
    let (receipt, result) = run(r, false);
    result.unwrap();
    assert_eq!(receipt.repaired_documents, 3);
    saved.placements[0].asset_path = NEW.into();
    assert_eq!(feathrise_scene_content::read_scene_placements(&fs::read(scene).unwrap(), scene_path).unwrap().placements, saved.placements);
    assert_eq!(fs::read_to_string(legacy).unwrap(), legacy_text.replace(OLD, NEW));
    assert_eq!(fs::read_to_string(preset).unwrap(), preset_text.replace(OLD, NEW));
    assert_eq!(fs::read(replacement).unwrap(), complete);
}

#[test]
fn relink_requires_scene_zero_only_for_recognized_whole_scene_or_building_records() {
    let f = Fixture::new();
    let mut r = missing_fixture(&f);
    let FileOperation::Relink { missing, target, .. } = &mut r.operation else { unreachable!() };
    *missing = "models/bare.glb".into();
    fs::write(target, model(&[1],1)).unwrap();
    let generic = f.path("assets/config/private.json");
    let unrelated = r#"{"presets":[{"asset_path":"models/bare.glb","asset_target":{"private":"upstream"}}],"model_path":"models/bare.glb"}"#;
    fs::write(&generic, unrelated).unwrap();
    let legacy = f.path("assets/scenes/b/placements.scene_placements.json");
    let preset = f.path("assets/config/example.building_presets.json");
    // 文件类型和记录位置都须匹配。显式片段只需要该标签，普通配置的裸路径只需要文件。
    fs::write(&legacy, r#"{"placements":[{"asset_path":"models/bare.glb#Mesh0/Primitive0"}]}"#).unwrap();
    fs::write(&preset, r#"{"presets":[],"metadata":{"asset_path":"models/bare.glb"}}"#).unwrap();
    let preview = inspect(&r);
    assert_eq!(preview.documents.iter().map(|d| d.matches).sum::<usize>(), 4);
    for (doc, array) in [(&legacy,"placements"), (&preset,"presets")] {
        let before = fs::read(doc).unwrap();
        fs::write(doc, json!({(array):[{"asset_path":"models/bare.glb"}]}).to_string()).unwrap();
        assert!(run(r.clone(), false).1.unwrap_err().contains("Scene0"));
        fs::write(doc, before).unwrap();
    }
    r.references = Some(ReferenceUpdate::Confirm(inspect(&r)));
    run(r, false).1.unwrap();
    assert_eq!(fs::read_to_string(generic).unwrap(), unrelated.replace("models/bare.glb", NEW));
}

#[test]
fn relink_rejects_inverted_material_without_reachable_mirrored_use() {
    let f = Fixture::new();
    let r = missing_fixture(&f);
    let target = f.path("assets/out/新门.glb");
    let config = f.path("assets/config/mirrored.json");
    let original = r#"{"material_path":"models/门.glb#Material0 (inverted)/std"}"#;
    fs::write(&config, original).unwrap();
    for (parent, child, material, reachable) in [(1.,1.,0,true), (-1.,-1.,0,true), (-1.,1.,1,true), (-1.,1.,0,false)] {
        fs::write(&target, mirrored_model(parent, child, material, reachable)).unwrap();
        let error = run(r.clone(), false).1.unwrap_err();
        assert!(error.contains("#Material0 (inverted)/std"), "{error}");
        assert_eq!(fs::read_to_string(&config).unwrap(), original);
    }
}

#[test]
fn relink_accepts_a_scene_with_metadata_larger_than_the_browser_summary_budget() {
    let f = Fixture::new();
    let r = missing_fixture(&f);
    let target = model_with(&[1], 1, |model| {
        model["scenes"] = json!([{"nodes":[0]}]);
        model["nodes"] = json!([{"mesh":0}]);
        model["extras"] = json!({"author_description":"a".repeat(16 * 1024 * 1024)});
    });
    fs::write(f.path("assets/out/新门.glb"), target).unwrap();
    assert_eq!(inspect(&r).documents.len(), 3);
}
