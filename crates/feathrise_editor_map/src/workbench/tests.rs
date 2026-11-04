//! 原工作台独立装配的边界回归；目录后台换代和合法双击必须保留真实身份。
use super::*;
use crate::{
    map_browser::MapBrowserClick,
    resources::{MapBrowserIndexState, MapBrowserState},
    types::MapBrowserIndexSignature,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[test]
fn hosted_world_readiness_uses_guest_evidence_without_a_local_game_scene() {
    let mut world = World::new();
    world.init_resource::<WorkbenchModel>();
    let mut inputs =
        bevy::ecs::system::SystemState::<crate::resources::MapEditorWorldSession>::new(&mut world);
    assert!(!inputs.get(&world).unwrap().ready());
    world.resource_mut::<WorkbenchModel>().active_map = Some("M1-SAMPLE".into());
    assert!(inputs.get(&world).unwrap().ready());
    world.resource_mut::<WorkbenchModel>().visible = false;
    assert!(!inputs.get(&world).unwrap().ready());
}

#[test]
fn original_double_click_does_not_cross_a_project_or_catalog_generation() {
    let id = feathrise_map_content::MapId("SAME-MAP".into());
    let a = (
        MapBrowserIndexSignature {
            folder_revision: 1,
            catalog_revision: 0,
        },
        0,
    );
    let b = (
        MapBrowserIndexSignature {
            folder_revision: 2,
            catalog_revision: 0,
        },
        0,
    );
    let mut last = None;
    assert!(!MapBrowserClick::release(&mut last, &id, &a, 1.0));
    assert!(!MapBrowserClick::release(&mut last, &id, &b, 1.1));
    assert!(MapBrowserClick::release(&mut last, &id, &b, 1.2));
    assert!(!MapBrowserClick::release(&mut last, &id, &b, 1.3));
    assert!(!MapBrowserClick::release(&mut last, &id, &b, f64::NAN));
}

#[test]
fn late_directory_projection_is_rejected_for_same_map_id_in_another_project() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<WorkbenchModel>()
        .init_resource::<resources::CatalogJob>()
        .init_resource::<MapBrowserIndexState>()
        .init_resource::<MapBrowserState>()
        .add_systems(Update, catalog::advance);
    let snapshot = |label: &str| {
        Arc::new(
            vec![serde_json::from_value::<WorkbenchMap>(serde_json::json!({
        "id": "SAME-MAP", "display_name": label, "folder": "maps/SAME-MAP", "published": false,
    })).unwrap()],
        )
    };
    {
        let mut model = app.world_mut().resource_mut::<WorkbenchModel>();
        model.context = 1;
        model.maps = snapshot("工程A");
    }
    app.update(); // A的完整投影在后台运行；立即切到含同名ID的B。
    let expected = snapshot("工程B");
    {
        let mut model = app.world_mut().resource_mut::<WorkbenchModel>();
        model.context = 2;
        model.maps = expected.clone();
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.update();
        let index = app.world().resource::<MapBrowserIndexState>();
        assert!(
            index
                .entries
                .iter()
                .all(|entry| entry.display_name != "工程A")
        );
        if index
            .entries
            .first()
            .is_some_and(|entry| entry.display_name == "工程B")
        {
            break;
        }
        assert!(Instant::now() < deadline, "后台目录未完成");
        std::thread::yield_now();
    }
    let job = app.world().resource::<resources::CatalogJob>();
    assert!(Arc::ptr_eq(&job.adopted.as_ref().unwrap().1, &expected));
    assert_eq!(job.indices.get("SAME-MAP"), Some(&0));
}
