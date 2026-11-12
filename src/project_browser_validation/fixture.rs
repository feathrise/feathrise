//! 工程目录专项的中性素材。生成最小蒙皮人形、独立动作与媒体，不引用真实作者工程。

use super::{sha256, write_new};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

pub(super) const MOTIONS: &str = "motions.glb";
pub(super) const STATIC: &str = "roles/nested/static.glb";
const HERO: &str = "roles/nested/hero.glb";
const FILES: &[&str] = &[MOTIONS, STATIC, HERO, "swatch.png", "beep.wav", "sample.avi"];
const MODEL_JOINTS: usize = 49;
const MOTION_NODES: usize = 66;

pub(super) fn prepare(assets: &Path) -> Result<BTreeMap<String, String>, String> {
    for folder in ["roles/nested", "roles/empty", "empty/deeper"] {
        fs::create_dir_all(assets.join(folder)).map_err(|error| error.to_string())?;
    }
    for (path, bytes) in [
        (HERO, model(true)),
        (STATIC, model(false)),
        (MOTIONS, motions()),
        ("swatch.png", include_bytes!("../../tests/fixtures/scene/albedo.png").to_vec()),
        ("beep.wav", wave()),
        ("sample.avi", video()),
    ] {
        write_new(&assets.join(path), &bytes)?;
    }
    fingerprints(assets)
}

pub(super) fn fingerprints(assets: &Path) -> Result<BTreeMap<String, String>, String> {
    FILES.iter().map(|name| {
        let bytes = fs::read(assets.join(name)).map_err(|error| error.to_string())?;
        Ok(((*name).into(), sha256(&bytes)))
    }).collect()
}

// 对应现有 Mixamo 重定向的最小核心骨骼，角色与纯动作共享父链及静止轴向。
const BONES: [(&str, usize, [f32; 3]); 15] = [
    ("Hips", 0, [0.0, 0.72, 0.0]),
    ("Spine", 0, [0.0, 0.36, 0.0]),
    ("Head", 1, [0.0, 0.48, 0.0]),
    ("LeftArm", 1, [0.23, 0.27, 0.0]),
    ("LeftForeArm", 3, [0.28, 0.0, 0.0]),
    ("LeftHand", 4, [0.25, 0.0, 0.0]),
    ("RightArm", 1, [-0.23, 0.27, 0.0]),
    ("RightForeArm", 6, [-0.28, 0.0, 0.0]),
    ("RightHand", 7, [-0.25, 0.0, 0.0]),
    ("LeftUpLeg", 0, [0.13, -0.02, 0.0]),
    ("LeftLeg", 9, [0.0, -0.34, 0.0]),
    ("LeftFoot", 10, [0.0, -0.34, 0.0]),
    ("RightUpLeg", 0, [-0.13, -0.02, 0.0]),
    ("RightLeg", 12, [0.0, -0.34, 0.0]),
    ("RightFoot", 13, [0.0, -0.34, 0.0]),
];

fn skeleton(node_count: usize) -> Vec<Value> {
    let mut nodes: Vec<_> = BONES.iter().enumerate().map(|(index, (name, _, offset))| {
        let mut node = json!({"name":format!("mixamorig:{name}"),"translation":offset});
        let mut children = (1..BONES.len()).filter(|&child| BONES[child].1 == index).collect::<Vec<_>>();
        if index == 0 { children.extend(BONES.len()..node_count); }
        if !children.is_empty() { node["children"] = json!(children); }
        node
    }).collect();
    // 源动作比目标蒙皮多一组关节，重现 66 个源节点与 49 个目标关节的发布预算。
    nodes.extend((BONES.len()..node_count).map(|index| json!({"name":format!("mixamorig:FixtureJoint{index}")})));
    nodes
}

fn model(skinned: bool) -> Vec<u8> {
    // 两个四边形分别随躯干和手臂关节，抬臂动作能在实际截图中留下轮廓变化。
    let torso = [
        [-0.23_f32, 0.70, 0.0], [0.23, 0.70, 0.0], [0.23, 1.46, 0.0],
        [-0.23, 0.70, 0.0], [0.23, 1.46, 0.0], [-0.23, 1.46, 0.0],
    ];
    let arm = [
        [0.23_f32, 1.28, 0.0], [0.79, 1.28, 0.0], [0.79, 1.42, 0.0],
        [0.23, 1.28, 0.0], [0.79, 1.42, 0.0], [0.23, 1.42, 0.0],
    ];
    let mut glb = Glb::default();
    let position = glb.floats(torso.into_iter().chain(arm).flatten(), 12, "VEC3");
    glb.accessors[position]["min"] = json!([-0.23, 0.70, 0.0]);
    glb.accessors[position]["max"] = json!([0.79, 1.46, 0.0]);
    let mut attributes = json!({"POSITION":position});
    let (nodes, roots) = if skinned {
        let joints = glb.accessor(
            (0..12).flat_map(|vertex| [if vertex < 6 { 0_u8 } else { 3 }, 0, 0, 0]),
            5121, 12, "VEC4",
        );
        let weights = glb.floats((0..12).flat_map(|_| [1.0, 0.0, 0.0, 0.0]), 12, "VEC4");
        attributes["JOINTS_0"] = json!(joints);
        attributes["WEIGHTS_0"] = json!(weights);
        let mut translations = [BONES[0].2; MODEL_JOINTS];
        for (index, (_, parent, offset)) in BONES.iter().enumerate() {
            translations[index] = std::array::from_fn(|axis| {
                offset[axis] + if index == 0 { 0.0 } else { translations[*parent][axis] }
            });
        }
        let inverse_bind = glb.floats(translations.into_iter().flat_map(|[x, y, z]| [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -x, -y, -z, 1.0,
        ]), MODEL_JOINTS, "MAT4");
        let mut nodes = skeleton(MODEL_JOINTS);
        nodes.push(json!({"name":"HeroMesh","mesh":0,"skin":0}));
        glb.document["skins"] = json!([{
            "joints":(0..MODEL_JOINTS).collect::<Vec<_>>(),"inverseBindMatrices":inverse_bind,"skeleton":0
        }]);
        (nodes, vec![0, MODEL_JOINTS])
    } else {
        (vec![json!({"name":"StaticMesh","mesh":0})], vec![0])
    };
    glb.document["nodes"] = json!(nodes);
    glb.document["scenes"] = json!([{"nodes":roots}]);
    glb.document["meshes"] = json!([{"primitives":[{"attributes":attributes,"material":0}]}]);
    glb.document["materials"] = json!([{"doubleSided":true,"pbrMetallicRoughness":{
        "baseColorFactor":[0.82,0.31,0.08,1.0],"metallicFactor":0.0,"roughnessFactor":0.8
    }}]);
    glb.finish()
}

fn motions() -> Vec<u8> {
    let mut glb = Glb::default();
    let times = glb.floats([0.0, 2.0], 2, "SCALAR");
    glb.accessors[times]["min"] = json!([0.0]);
    glb.accessors[times]["max"] = json!([2.0]);
    let clips = [("原地", -0.2_f32, 6), ("抬臂", 1.4, 3)].map(|(name, angle, joint)| {
        let (z, w) = (angle / 2.0).sin_cos();
        let rotation = glb.floats([0.0, 0.0, 0.0, 1.0, 0.0, 0.0, z, w], 2, "VEC4");
        json!({"name":name,"samplers":[{"input":times,"output":rotation,"interpolation":"LINEAR"}],
            "channels":[{"sampler":0,"target":{"node":joint,"path":"rotation"}}]})
    });
    glb.document["nodes"] = json!(skeleton(MOTION_NODES));
    glb.document["scenes"] = json!([{"nodes":[0]}]);
    glb.document["animations"] = json!(clips);
    glb.finish()
}

#[derive(Default)]
struct Glb {
    document: Value,
    binary: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Glb {
    fn floats(&mut self, values: impl IntoIterator<Item = f32>, count: usize, kind: &str) -> usize {
        self.accessor(values.into_iter().flat_map(f32::to_le_bytes), 5126, count, kind)
    }

    fn accessor(&mut self, bytes: impl IntoIterator<Item = u8>, component: u32, count: usize, kind: &str) -> usize {
        let offset = self.binary.len();
        self.binary.extend(bytes);
        let view = self.views.len();
        self.views.push(json!({"buffer":0,"byteOffset":offset,"byteLength":self.binary.len() - offset}));
        let accessor = self.accessors.len();
        self.accessors.push(json!({"bufferView":view,"componentType":component,"count":count,"type":kind}));
        accessor
    }

    fn finish(mut self) -> Vec<u8> {
        self.document["asset"] = json!({"version":"2.0","generator":"Feathrise 工程目录验收"});
        self.document["scene"] = json!(0);
        self.document["buffers"] = json!([{"byteLength":self.binary.len()}]);
        self.document["bufferViews"] = json!(self.views);
        self.document["accessors"] = json!(self.accessors);
        let mut header = self.document.to_string().into_bytes();
        header.resize(header.len().next_multiple_of(4), b' ');
        let mut bytes = b"glTF".to_vec();
        bytes.extend(2_u32.to_le_bytes());
        bytes.extend(((28 + header.len() + self.binary.len()) as u32).to_le_bytes());
        // GLB chunk 先写字节长度，再写四字节类型。
        for (kind, body) in [(b"JSON", &header), (b"BIN\0", &self.binary)] {
            bytes.extend((body.len() as u32).to_le_bytes());
            bytes.extend(kind);
            bytes.extend(body);
        }
        bytes
    }
}

fn riff_chunk(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut bytes = kind.to_vec();
    bytes.extend((body.len() as u32).to_le_bytes());
    bytes.extend(body);
    if body.len() % 2 != 0 { bytes.push(0); }
    bytes
}

fn riff_container(kind: &[u8; 4], format: &[u8; 4], chunks: impl IntoIterator<Item = Vec<u8>>) -> Vec<u8> {
    let mut body = format.to_vec();
    body.extend(chunks.into_iter().flatten());
    riff_chunk(kind, &body)
}

fn wave() -> Vec<u8> {
    let mut format = vec![1, 0, 1, 0]; // PCM 单声道，8 kHz / 8 bit。
    format.extend(8_000_u32.to_le_bytes());
    format.extend(8_000_u32.to_le_bytes());
    format.extend([1, 0, 8, 0]);
    let samples: Vec<_> = (0..800).map(|i| if i % 20 < 10 { 112 } else { 144 }).collect();
    riff_container(b"RIFF", b"WAVE", [riff_chunk(b"fmt ", &format), riff_chunk(b"data", &samples)])
}

fn video() -> Vec<u8> {
    // AVI 包含两帧 2×2 的 24 位 RGB，行宽按 4 字节对齐。
    let words = |values: &[u32]| values.iter().flat_map(|value| value.to_le_bytes()).collect::<Vec<_>>();
    let main = words(&[500_000, 32, 0, 0, 2, 0, 1, 16, 2, 2, 0, 0, 0, 0]);
    let stream = words(&[
        u32::from_le_bytes(*b"vids"), u32::from_le_bytes(*b"DIB "), 0, 0, 0, 1, 2, 0, 2, 16, u32::MAX, 0, 0, 0x0002_0002,
    ]);
    let pixels = words(&[40, 2, 2, 0x0018_0001, 0, 16, 0, 0, 0, 0]);
    let stream_list = riff_container(b"LIST", b"strl", [riff_chunk(b"strh", &stream), riff_chunk(b"strf", &pixels)]);
    let headers = riff_container(b"LIST", b"hdrl", [riff_chunk(b"avih", &main), stream_list]);
    let movies = riff_container(b"LIST", b"movi", [riff_chunk(b"00db", &[80; 16]), riff_chunk(b"00db", &[160; 16])]);
    riff_container(b"RIFF", b"AVI ", [headers, movies])
}
