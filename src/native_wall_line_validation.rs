//! 沿线围墙独占夹具的模型、预设和落盘验收，窗口启动沿用场景作者工具的公共入口。
use super::*;

pub(super) fn enabled() -> bool {
    std::env::var("FEATHRISE_WALL_LINE_VALIDATION").as_deref() == Ok("1")
}

pub(super) fn run(
    assets: &Path,
    project: &project::Project,
    work: &Path,
    target: &Path,
    control: &BuildControl,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    seed_wall_modules(assets)?;
    let authored = editor_stage(
        project,
        work,
        target,
        control,
        "scene-author",
        "scene-motion",
        steps,
    )?;
    let reopened = editor_stage(
        project,
        work,
        target,
        control,
        "scene-reopen",
        "scene-motion",
        steps,
    )?;
    let scene_path = "scenes/M1-SAMPLE/scene.glb";
    let bytes = fs::read(assets.join(scene_path)).map_err(|e| e.to_string())?;
    let records = feathrise_scene_content::read_scene_placements(&bytes, scene_path)?;
    let mut walls: Vec<_> = records
        .placements
        .into_iter()
        .filter(|r| r.id.starts_with("wall-line-"))
        .collect();
    if walls.len() != 10
        || authored["save_sequence"].as_u64().unwrap_or_default() == 0
        || reopened["reopened"] != true
    {
        return Err(format!("围墙保存或重开缺少回执，磁盘有 {} 个模块", walls.len()));
    }
    // 围墙夹具的模块都是根节点。验证实际写入的 scale，另核对 SDK 分解世界矩阵时的 f32 舍入。
    let gltf = bevy::gltf::gltf::Gltf::from_slice_without_validation(&bytes).map_err(|e| e.to_string())?;
    let raw_scales: std::collections::HashMap<_, _> = gltf.nodes().filter_map(|node| {
        Some((node.name()?, node.transform().decomposed().2))
    }).collect();
    for record in &mut walls {
        let raw = *raw_scales.get(record.id.as_str()).ok_or_else(|| format!("磁盘缺少围墙节点 {}", record.id))?;
        if !bevy::prelude::Vec3::from(record.scale).abs_diff_eq(raw.into(), 2.0 * f32::EPSILON) {
            return Err(format!("围墙 {} 的世界缩放与 GLB 节点不一致", record.id));
        }
        record.scale = raw;
    }
    // 按正式摆放类型逐字段比较，确认工程里的原始节点缩放和两个窗口的作者记录完全相同。
    for (stage, report) in [("保存", authored), ("重开", reopened)] {
        let observed: Vec<feathrise_scene_content::ScenePlacementRecord> =
            serde_json::from_value(report["placements"].clone()).map_err(|e| e.to_string())?;
        if observed != walls {
            let differing = observed.iter().zip(&walls).find(|(a, b)| a != b);
            return Err(format!("围墙{stage}回执与磁盘数据不一致，窗口 {} 项，磁盘 {} 项，首个差异 {differing:?}", observed.len(), walls.len()));
        }
    }
    steps.push(
        json!({"stage":"saved_wall_route","passed":true,"placement_count":walls.len(),
        "preview":read_author_screenshot(&work.join("scene-author/wall-route-preview.png"))?}),
    );
    Ok(())
}

fn seed_wall_modules(assets: &Path) -> Result<(), String> {
    use feathrise_scene_content::BUILDING_PRESET_CATALOG_ASSET_PATH;
    let path = assets.join(BUILDING_PRESET_CATALOG_ASSET_PATH);
    let mut catalog = read_json(&path)?;
    let presets = catalog["presets"].as_array_mut().ok_or("无建筑预设目录")?;
    for (id, category, name, half) in [
        ("fixture-wall", "wall", "验收三米围墙", 1.5),
        ("fixture-gate", "gate", "验收六米门楼", 3.),
    ] {
        let source = format!("scene/{id}.glb");
        write_wall_fixture_mesh(&assets.join(&source), category == "gate")?;
        presets.push(json!({"id":id,"label":name,"category":category,"asset_path":source,
            "footprint":{"center":[0,0],"half_extents":[half,0.3]},
            "vegetation_clearance":{"center":[0,0],"half_extents":[half,0.3]},
            "entrance":{"position":[0,0,0.3],"outward_yaw_degrees":0,"clearance_half_extents":[0.3,0.3]},
            "connections":[{"id":"left","kind":"fixture-wall","position":[-half,0],"outward_yaw_degrees":270},
                           {"id":"right","kind":"fixture-wall","position":[half,0],"outward_yaw_degrees":90}]}));
    }
    write_json(&path, &catalog)
}

/// 在独占夹具中用同一个 Bevy 方体网格组装墙段和有门洞的门楼，截图可直接辨认实际加载的模型。
fn write_wall_fixture_mesh(path: &Path, gate: bool) -> Result<(), String> {
    use bevy::{
        gltf::gltf::{Glb, binary::Header},
        mesh::VertexAttributeValues,
        prelude::{Cuboid, Mesh},
    };
    use std::borrow::Cow;
    let mesh = Mesh::from(Cuboid::new(1., 1., 1.));
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return Err("方体缺少顶点".into());
    };
    let indices = mesh.indices().ok_or("方体缺少索引")?;
    let bin: Vec<u8> = indices
        .iter()
        .flat_map(|i| positions[i])
        .flat_map(f32::to_le_bytes)
        .collect();
    let pieces: &[([f32; 3], [f32; 3])] = if gate {
        &[
            ([-2.5, 1.25, 0.], [1., 2.5, 0.6]),
            ([2.5, 1.25, 0.], [1., 2.5, 0.6]),
            ([0., 2.75, 0.], [6., 0.5, 0.6]),
        ]
    } else {
        &[([0., 1., 0.], [3., 2., 0.6])]
    };
    let nodes: Vec<_> = pieces
        .iter()
        .map(|(translation, scale)| json!({"mesh":0,"translation":translation,"scale":scale}))
        .collect();
    let color = if gate {
        [0.55, 0.24, 0.08, 1.0]
    } else {
        [0.5, 0.54, 0.57, 1.0]
    };
    let doc = json!({"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":(0..nodes.len()).collect::<Vec<_>>()}],"nodes":nodes,
        "meshes":[{"primitives":[{"attributes":{"POSITION":0},"material":0}]}],
        "materials":[{"pbrMetallicRoughness":{"baseColorFactor":color,"metallicFactor":0,"roughnessFactor":0.9}}],
        "buffers":[{"byteLength":bin.len()}],"bufferViews":[{"buffer":0,"byteLength":bin.len(),"target":34962}],
        "accessors":[{"bufferView":0,"componentType":5126,"count":indices.len(),"type":"VEC3","min":[-0.5,-0.5,-0.5],"max":[0.5,0.5,0.5]}]});
    let glb = Glb {
        header: Header {
            magic: *b"glTF",
            version: 2,
            length: 0,
        },
        json: Cow::Owned(serde_json::to_vec(&doc).map_err(|e| e.to_string())?),
        bin: Some(Cow::Owned(bin)),
    };
    fs::write(path, glb.to_vec().map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
