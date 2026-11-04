//! 实际v3作者输出、重开和驻留范围分离；测试仅写独立临时目录。
use super::*;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
struct Temp(std::path::PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "rpg-volume-editor-stream-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn volume_streamed_author_output_reopens_small_root_and_separates_resident_cutouts() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    let temp = Temp::new();
    let folder = "maps/STREAM-EDITOR";
    let objects = temp.0.join(folder).join("volume_sculpt/objects");
    let mut version = PagedVolume::new(0.25).unwrap();
    for p in [[4., 0., 4.], [164., 0., 164.]] {
        let brush = VolumeBrush {
            kind: feathrise_map_terrain::volume::VolumeBrushKind::Dig,
            from: p,
            to: p,
            radius: 2.,
            strength: 1.,
            ..Default::default()
        };
        let old = load_edit_window(&objects, &version, brush, Arc::default()).unwrap();
        let next = apply_sparse_brush(
            &old,
            brush,
            Rect::from_corners(Vec2::splat(-1024.), Vec2::splat(1024.)),
            |_| Ok(0.07),
        )
        .unwrap();
        version = write_delta(
            &objects,
            &version,
            prepare_delta(Some(&old), &next).unwrap(),
        )
        .unwrap();
    }
    version.focus = [4., 0., 4.];
    let record=serde_json::from_value(serde_json::json!({"id":"volume","asset_path":"maps/STREAM-EDITOR/volume_sculpt/pending.glb","translation":[0,0,0],"yaw_degrees":0,"scale":[1,1,1],"terrain_patch":{"sculpted_volume":true,"cutouts":[{"min":[0,0],"max":[8,8]}]}})).unwrap();
    let JobOutput::Prepared(prepared) = bevy::tasks::block_on(prepare(
        record,
        version.clone(),
        folder,
        &objects,
        &temp.0,
        None,
        default(),
    ))
    .unwrap() else {
        panic!()
    };
    assert!(prepared.bytes.len() < 4096);
    assert!(prepared.snapshot.source.memory_bytes() < 1024);
    assert_eq!(
        prepared.resident_window.unwrap(),
        feathrise_scene::StreamedVolumeResident {
            lod_signature: 0,
            center: [0; 3],
            physics_center: None,
        }
    );
    assert_ne!(
        prepared.snapshot.record.terrain_patch,
        prepared.render_record.terrain_patch
    );
    assert_eq!(
        prepared
            .render_record
            .terrain_patch
            .as_ref()
            .unwrap()
            .cutouts
            .len(),
        2
    );
    assert!(
        prepared.meshes.iter().any(|mesh| {
            let VertexAttributeValues::Float32x3(points) =
                mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap()
            else {
                panic!()
            };
            points.iter().any(|p| p[0] > 150.)
        }),
        "未驻留精细块的远处地形也必须显示"
    );
    let JobOutput::Opened(reopened) =
        super::super::jobs::open_at(&temp.0, prepared.snapshot.record.clone()).unwrap()
    else {
        panic!()
    };
    assert_eq!(reopened.source, prepared.snapshot.source);
    let JobOutput::Prepared(dual) = bevy::tasks::block_on(prepare(
        prepared.snapshot.record.clone(),
        version,
        folder,
        &objects,
        &temp.0,
        Some(Vec3::new(164., 0., 164.)),
        default(),
    ))
    .unwrap() else {
        panic!()
    };
    assert_eq!(
        dual.snapshot.record.asset_path,
        prepared.snapshot.record.asset_path
    );
    assert_eq!(
        dual.resident_window.unwrap(),
        feathrise_scene::StreamedVolumeResident {
            lod_signature: 0,
            center: [0; 3],
            physics_center: Some([20, 0, 20]),
        }
    );
    assert_eq!(
        dual.render_record
            .terrain_patch
            .as_ref()
            .unwrap()
            .cutouts
            .len(),
        2
    );
    let mut wrong = reopened.record.clone();
    wrong.terrain_patch.as_mut().unwrap().volume_cell_size = Some(0.125);
    assert!(super::super::jobs::open_at(&temp.0, wrong).is_err());
}
