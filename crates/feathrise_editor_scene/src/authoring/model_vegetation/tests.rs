//! 真实稀疏笔刷与Arc历史的中性叠层夹具；不依赖任何RPG测试地图。

use super::resources::{Sample, Work};
use super::*;
use feathrise_scene::paint_surface::{MeshSurfacePrimitive, MeshSurfaceSnapshot, PaintSurfaceSnapshot};
use feathrise_scene_content::{SceneId, SceneInstancePath, SceneVegetationAsset};

fn bridge() -> MeshSurfaceSnapshot {
    let mut positions = Vec::new();
    for y in [0.0, 0.15] {
        positions.extend_from_slice(&[[0.0, y, 0.0], [4.0, y, 0.0], [0.0, y, 4.0], [4.0, y, 4.0]]);
    }
    MeshSurfaceSnapshot::build(vec![MeshSurfacePrimitive {
        primitive: 0,
        positions: Arc::new(positions),
        indices: Arc::new(vec![0, 2, 1, 1, 2, 3, 4, 6, 5, 5, 6, 7]),
        primitive_to_object: Mat4::IDENTITY,
    }])
    .unwrap()
}

fn surface(mesh: MeshSurfaceSnapshot) -> PaintSurfaceSnapshot {
    PaintSurfaceSnapshot::from_model(SurfaceTarget::Model { object_id: "bridge".into() },
        SceneInstancePath::root(SceneId::parse("MAP-model-brush").unwrap()), mesh, Mat4::IDENTITY).unwrap()
}

fn paint(
    mesh: MeshSurfaceSnapshot,
    data: Arc<ModelSurfaceVegetation>,
    targets: Arc<Vec<Arc<ModelSurfaceVegetation>>>,
    erase: bool,
) -> Arc<Vec<Arc<ModelSurfaceVegetation>>> {
    let sample = Sample {
        ray: Ray3d::new(Vec3::new(2.0, 5.0, 2.0), Dir3::NEG_Y),
        radius: 1.25,
        amount: 0.8,
        exposure: 0.0,
        erase,
    };
    let Work::Paint {
        targets, changed, ..
    } = super::worker::paint(
        surface(mesh),
        data,
        "grass-author",
        vec![sample],
        None,
        targets,
        0,
    )
    .unwrap()
    else {
        panic!("必须走真实模型采样任务");
    };
    assert!(changed);
    targets
}

#[test]
fn bridge_brush_only_writes_hit_layer_and_erase_preserves_the_surface_anchors() {
    let mesh = bridge();
    let before = Arc::new(ModelSurfaceVegetation::empty(
        "bridge",
        mesh.topology(),
        0.25,
    ));
    let targets = paint(mesh.clone(), before.clone(), Arc::new(Vec::new()), false);
    assert!(
        before.samples.is_empty(),
        "后台写时复制不能修改历史之前的版本"
    );
    let resolved = mesh
        .resolved_vegetation(&targets[0], Mat4::IDENTITY)
        .unwrap();
    assert!(!resolved.is_empty());
    assert!(
        resolved
            .iter()
            .all(|sample| (sample.position.y - 0.15).abs() < 0.0001),
        "相距15厘米的桥下表面不能串层"
    );
    let erased = paint(mesh.clone(), targets[0].clone(), targets.clone(), true);
    let sum = |data: &ModelSurfaceVegetation| {
        data.samples
            .iter()
            .flat_map(|sample| &sample.layers)
            .map(|(_, density)| u64::from(*density))
            .sum::<u64>()
    };
    assert!(sum(&erased[0]) < sum(&targets[0]));
    assert!(
        mesh.resolved_vegetation(&erased[0], Mat4::from_translation(Vec3::X * 8.0))
            .unwrap()
            .iter()
            .all(|sample| sample.position.x >= 8.0),
        "对象变换由锚点局部坐标重新解析"
    );
}

#[test]
fn model_history_restores_whole_target_arcs_and_rejects_a_replaced_source() {
    let mesh = bridge();
    let before = Arc::new(Vec::new());
    let empty = Arc::new(ModelSurfaceVegetation::empty(
        "bridge",
        mesh.topology(),
        0.25,
    ));
    let after = paint(mesh, empty, before.clone(), false);
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    draft.vegetation = Some(SceneVegetationAsset {
        scene_id: draft.scene_id.clone(),
        model_surfaces: after.clone(),
        ..Default::default()
    });
    let command = EditorCommand::ModelVegetationChanged {
        index: 0,
        object_id: "bridge".into(),
        layer_id: "grass-author".into(),
        before: before.clone(),
        after: after.clone(),
        heap_bytes: 1024,
    };
    apply_history(&mut draft, &command, true).unwrap();
    assert!(Arc::ptr_eq(
        &draft.vegetation.as_ref().unwrap().model_surfaces,
        &before
    ));
    apply_history(&mut draft, &command, false).unwrap();
    assert!(Arc::ptr_eq(
        &draft.vegetation.as_ref().unwrap().model_surfaces,
        &after
    ));
    draft.vegetation.as_mut().unwrap().model_surfaces = Arc::new(Vec::new());
    assert!(apply_history(&mut draft, &command, true).is_err());
    assert!(draft.vegetation.as_ref().unwrap().model_surfaces.is_empty());
}

#[test]
fn topology_change_preserves_author_samples_and_returns_a_rebind_diagnostic() {
    let mesh = bridge();
    let mut data = ModelSurfaceVegetation::empty("bridge", mesh.topology(), 0.25);
    data.topology[0] ^= 1;
    let original = Arc::new(data);
    let sample = Sample {
        ray: Ray3d::new(Vec3::new(2.0, 5.0, 2.0), Dir3::NEG_Y),
        radius: 1.0,
        amount: 1.0,
        exposure: 0.0,
        erase: false,
    };
    let error = super::worker::paint(
        surface(mesh),
        original.clone(),
        "grass-author",
        vec![sample],
        None,
        Arc::new(vec![original.clone()]),
        0,
    )
    .err()
    .expect("失效拓扑必须失败关闭");
    assert!(error.contains("重新绑定"));
    assert!(original.samples.is_empty());
}
