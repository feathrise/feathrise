//! 原场景标签验收动作的安装权限回归，覆盖父Fixture暂时移出World的嵌套调用。

use super::*;
use crate::ui::vegetation_preset_editor::{PresetEditorAction, PresetEditorButton};

#[test]
fn scene_tab_pulse_keeps_install_authority_while_fixture_is_scoped_out() {
    let mut world = World::new();
    assert!(
        request_scene_tab(&mut world).is_err(),
        "普通会话不得代发原标签动作"
    );
    world.insert_resource(FixtureInstalled);
    world.insert_resource(Fixture {
        phase: Phase::AuthorTools,
        proof: None,
        scene_id: None,
        count_before: 0,
        save_button: None,
        click_pulse: None,
        started_at: Instant::now(),
    });
    let pulse = world
        .resource_scope(|world, _: Mut<Fixture>| {
            assert!(!world.contains_resource::<Fixture>());
            request_scene_tab(world)
        })
        .expect("父阶段推进期间仍须能返回原场景标签");
    assert!(matches!(
        world.get::<PresetEditorButton>(pulse),
        Some(PresetEditorButton(PresetEditorAction::ActivateSceneTab))
    ));
    assert!(
        world
            .get::<feathrise_editor_ui::EditorButtonClick>(pulse)
            .is_some()
    );
    assert!(
        world.contains_resource::<Fixture>(),
        "借出结束后阶段资源归还"
    );
}
