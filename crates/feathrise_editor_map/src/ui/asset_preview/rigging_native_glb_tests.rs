//! 使用独占暂存和内存夹具验证原生蒙皮 GLB 的读写、场景变换及文件保真。

use super::super::super::files::Stage;
use super::*;
use std::fs;

const TRAILING: [u8; 12] = [4, 0, 0, 0, b'T', b'E', b'S', b'T', 17, 23, 31, 47];

fn fixture() -> (Value, Vec<u8>) {
    let mut binary = Vec::new();
    for row in [
        [0.0_f32, 0.0, 0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0, 1.0, 0.0],
        [0.0, 1.0, 0.0, 0.0, 1.0],
        [1.0, 1.0, 0.0, 1.0, 1.0],
    ] {
        for value in row {
            binary.extend(value.to_le_bytes());
        }
    }
    for index in [0u16, 1, 2, 1, 3, 2] {
        binary.extend(index.to_le_bytes());
    }
    binary.extend([137, 80, 78, 71]); // 编码器把图像数据原样搬运，真实纹理的 GPU 检查由窗口验收完成。
    let document = json!({
        "asset":{"version":"2.0","generator":"原生绑骨夹具"},
        "extras":{"description":"保留工程扩展数据"},
        "scene":0,
        "scenes":[{"nodes":[0,2]}, {"nodes":[2],"name":"备用场景"}],
        "nodes":[
            {"name":"父节点","translation":[3.0,0.0,0.0],"children":[1]},
            {"name":"网格甲","translation":[0.0,2.0,0.0],"scale":[2.0,3.0,1.0],"mesh":0},
            {"name":"网格乙","matrix":[-1.0,0.0,0.0,0.0, 0.0,1.0,0.0,0.0, 0.0,0.0,1.0,0.0, 0.0,0.0,2.0,1.0],"mesh":0},
        ],
        "meshes":[{"name":"共享网格","extras":{"original":true},"primitives":[
            {"attributes":{"POSITION":0,"TEXCOORD_0":1},"indices":2,"material":0},
            {"attributes":{"POSITION":0,"TEXCOORD_0":1},"mode":5,"material":0}
        ]}],
        "accessors":[
            {"bufferView":0,"byteOffset":0,"componentType":5126,"count":4,"type":"VEC3","min":[0.0,0.0,0.0],"max":[1.0,1.0,0.0]},
            {"bufferView":0,"byteOffset":12,"componentType":5126,"count":4,"type":"VEC2"},
            {"bufferView":1,"componentType":5123,"count":6,"type":"SCALAR"},
        ],
        "bufferViews":[
            {"buffer":0,"byteOffset":0,"byteLength":80,"byteStride":20,"target":34962},
            {"buffer":0,"byteOffset":80,"byteLength":12,"target":34963},
            {"buffer":0,"byteOffset":92,"byteLength":4}
        ],
        "buffers":[{"byteLength":binary.len()}],
        "images":[{"bufferView":2,"mimeType":"image/png"}],
        "textures":[{"source":0}],
        "materials":[{"pbrMetallicRoughness":{"baseColorTexture":{"index":0}}}],
    });
    (document, binary)
}

fn file(document: &Value, binary: &[u8]) -> Vec<u8> {
    let mut metadata = serde_json::to_vec(document).unwrap();
    metadata.resize(metadata.len().next_multiple_of(4), b' ');
    let mut file = b"glTF".to_vec();
    let size = 28 + metadata.len() + binary.len() + TRAILING.len();
    for value in [2, size as u32, metadata.len() as u32, JSON] {
        file.extend(value.to_le_bytes());
    }
    file.extend(metadata);
    for value in [binary.len() as u32, BIN] {
        file.extend(value.to_le_bytes());
    }
    file.extend(binary);
    file.extend(TRAILING);
    file
}

#[test]
fn glb_preserves_original_payload_and_rest_for_each_mesh_instance() {
    let cancel = AtomicBool::new(false);
    let (source, binary) = fixture();
    let model = decode(file(&source, &binary), &cancel).unwrap();
    assert_eq!(model.geometry.parts, [0..8, 8..16]);
    assert_eq!(model.geometry.vertices[1], Vec3::new(5.0, 2.0, 0.0));
    assert_eq!(model.geometry.vertices[9], Vec3::new(-1.0, 0.0, 2.0));
    assert_eq!(model.geometry.triangles[0], [0, 1, 2]);
    assert_eq!(model.geometry.triangles[3], [6, 5, 7]);
    assert_eq!(model.geometry.triangles[4], [8, 10, 9]);

    // 刻意颠倒关节顺序。skin 使用参考索引，节点父子关系及静止姿态不依赖数组顺序。
    let joints = [
        Joint {
            name: "mixamorig:Spine".into(),
            parent: Some(1),
            position: Vec3::Y,
            rotation: Quat::from_rotation_x(0.3),
        },
        Joint {
            name: "mixamorig:Hips".into(),
            parent: None,
            position: Vec3::ZERO,
            rotation: Quat::from_rotation_y(0.4),
        },
    ];
    let mut weights = vec![
        VertexSkin {
            joints: [0, 1, 0, 0],
            weights: [0.25, 0.75, 0.0, 0.0]
        };
        16
    ];
    weights[8..].fill(VertexSkin {
        joints: [1, 0, 0, 0],
        weights: [0.5, 0.5, 0.0, 0.0],
    });
    let output = encode(model, &joints, &weights, &cancel).unwrap();
    let stage = Stage::new(&std::env::temp_dir().join("assets")).unwrap();
    let path = stage.path("rigged.glb");
    output.write(&path, &cancel).unwrap();
    assert!(output.write(&path, &cancel).is_err()); // 编码器只写独占暂存，不覆盖同名文件。
    let bytes = fs::read(path).unwrap();
    let (result, payload) = container(&bytes, &cancel).unwrap();
    assert_eq!(&bytes[payload.start..payload.start + binary.len()], &binary);
    assert_eq!(&bytes[payload.end..], &TRAILING);
    assert_eq!(result["buffers"][0]["byteLength"], payload.len());
    for field in ["asset", "extras", "images", "textures", "materials"] {
        assert_eq!(result[field], source[field]);
    }
    assert_eq!(result["scenes"][0]["nodes"], json!([0, 2, 5]));
    assert_eq!(result["scenes"][1]["nodes"], json!([2, 5]));
    assert_eq!(result["nodes"][0], source["nodes"][0]);
    assert_eq!(result["nodes"][1]["scale"], source["nodes"][1]["scale"]);
    assert_eq!(result["nodes"][2]["matrix"], source["nodes"][2]["matrix"]);
    assert_eq!(result["nodes"][1]["mesh"], 0);
    assert_eq!(result["nodes"][2]["mesh"], 1);
    assert_eq!(result["skins"][0]["joints"], json!([3, 4]));
    assert_eq!(result["skins"][1]["joints"], json!([3, 4]));
    assert!(list(&result, "animations").unwrap().is_empty());
    let reopened = gltf::Gltf::from_slice(&bytes).unwrap();
    let binary = reopened.blob.as_deref().unwrap();
    let graph = NodeGraph::parse(list(&result, "nodes").unwrap(), &cancel).unwrap();
    for (instance, node) in [1, 2].into_iter().enumerate() {
        let mesh = reopened.meshes().nth(instance).unwrap();
        assert_eq!(mesh.primitives().count(), 2);
        for primitive in mesh.primitives() {
            let reader = primitive.reader(|_| Some(binary));
            assert_eq!(
                reader.read_positions().unwrap().nth(1),
                Some([1.0, 0.0, 0.0])
            );
            assert_eq!(
                reader.read_joints(0).unwrap().into_u16().next(),
                Some(weights[instance * 8].joints)
            );
            assert_eq!(
                reader.read_weights(0).unwrap().into_f32().next(),
                Some(weights[instance * 8].weights)
            );
        }
        let skin = reopened.skins().nth(instance).unwrap();
        let reader = skin.reader(|_| Some(binary));
        for (joint, inverse_bind) in skin
            .joints()
            .zip(reader.read_inverse_bind_matrices().unwrap())
        {
            let rest = graph.world[joint.index()] * Mat4::from_cols_array_2d(&inverse_bind);
            assert!(rest.abs_diff_eq(graph.world[node], 1e-5));
        }
    }
}

#[test]
fn glb_rejects_missing_geometry_and_unsupported_or_corrupt_layouts() {
    let cancel = AtomicBool::new(false);
    let (source, mut binary) = fixture();
    let mut compressed = source.clone();
    compressed["extensionsUsed"] = json!(["EXT_meshopt_compression"]);
    assert!(decode(file(&compressed, &binary), &cancel).is_err());
    let mut invalid_view = source.clone();
    invalid_view["bufferViews"][0]["byteOffset"] = json!(u64::MAX);
    assert!(decode(file(&invalid_view, &binary), &cancel).is_err());
    let mut outside_scene = source.clone();
    outside_scene["scenes"][0]["nodes"] = json!([0]);
    assert!(decode(file(&outside_scene, &binary), &cancel).is_err());
    binary[80..82].copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(decode(file(&source, &binary), &cancel).is_err());
    assert!(decode(file(&source, &binary), &AtomicBool::new(true)).is_err());
}

#[test]
fn reference_keeps_skin_order_and_rejects_nonuniform_axes() {
    let names = [
        "Hips",
        "Spine",
        "Spine1",
        "Spine2",
        "Neck",
        "Head",
        "LeftShoulder",
        "LeftArm",
        "LeftForeArm",
        "LeftHand",
        "LeftUpLeg",
        "LeftLeg",
        "LeftFoot",
        "LeftToeBase",
        "RightShoulder",
        "RightArm",
        "RightForeArm",
        "RightHand",
        "RightUpLeg",
        "RightLeg",
        "RightFoot",
        "RightToeBase",
    ];
    let rotation = Quat::from_rotation_y(0.7);
    let mut nodes = vec![json!({
        "name":"模板包装","children":[1],"rotation":rotation.to_array(),"scale":[0.01,0.01,0.01]
    })];
    for (i, name) in names.iter().enumerate() {
        let mut node = json!({"name":format!("mixamorig:{name}"),"translation":[0.0,2.0,0.0]});
        if i + 1 < names.len() {
            node["children"] = json!([i + 2]);
        }
        nodes.push(node);
    }
    let mut metadata = json!({
        "asset":{"version":"2.0"},"nodes":nodes,
        "skins":[{"joints":(1..=names.len()).rev().collect::<Vec<_>>()}],
    });
    let cancel = AtomicBool::new(false);
    let joints = reference(&metadata, &cancel).unwrap();
    assert_eq!(joints[0].name, "mixamorig:RightToeBase");
    assert_eq!(joints[0].parent, Some(1));
    assert_eq!(joints.last().unwrap().name, "mixamorig:Hips");
    assert_eq!(joints.last().unwrap().parent, None);
    assert!(joints[0].position.abs_diff_eq(Vec3::Y * 0.44, 1e-5));
    assert!(joints[0].rotation.dot(rotation).abs() > 0.9999);
    metadata["nodes"][0]["scale"] = json!([0.01, 0.02, 0.01]);
    assert!(reference(&metadata, &cancel).is_err());
}
