//! 对象面板回归：列表与视口同一ID、独立开关、单墙历史及ECS查询互斥。
use feathrise_editor_ui::UiInteraction;
use super::*; // 实际面板系统。
use crate::authoring::test_heightfield_draft; // 小型动态场景夹具。
use feathrise_editor_ui::EditorButtonClick; // 纯ECS作者测试显式注入已确认的释放动作。
use std::sync::Arc; // 高度缓冲共享。
fn app() -> App {
    // 不启动渲染也执行真实输入系统。
    let mut app = App::new();
    let mut draft = test_heightfield_draft(Arc::new(vec![0; 121])); // 十米测试场景。
    draft.boundaries = Some(
        SceneBoundariesAsset::rectangular_default(&draft.scene_id, [0.0; 2], [10.0; 2], 0.0, 20.0)
            .unwrap(),
    );
    let id = draft.boundaries.as_ref().unwrap().paths[0].id.clone();
    let map = draft.scene_id.clone();
    app.insert_resource(EditorAuthoringSession {
        draft: Some(draft),
        selected_boundary_path: Some((map, id)),
        ..default()
    })
    .insert_resource(SceneEditorState {
        open: true,
        ..default()
    })
    .init_resource::<BoundaryEditState>()
    .init_resource::<LayerCandidateState>()
    .init_resource::<InputFocus>()
    .init_resource::<PanelState>()
    .add_systems(Update, (actions, sync).chain()); // 也验证展示查询不会运行时冲突。
    app
}
#[test]
fn copy_selects_new_internal_wall_and_visibility_does_not_disable_collision() {
    let mut app = app();
    let button = app
        .world_mut()
        .spawn((Action::Copy, EditorButtonClick::triggered()))
        .id(); // 本测试只验证作者事务，指针捕获由公共按钮协议单独回归。
    app.update();
    let session = app.world().resource::<EditorAuthoringSession>();
    let paths = &session
        .draft
        .as_ref()
        .unwrap()
        .boundaries
        .as_ref()
        .unwrap()
        .paths;
    assert_eq!(paths.len(), 2); // 外框复制成独立内部对象。
    assert_eq!(paths[1].role, SceneBoundaryRole::Internal);
    assert_ne!(paths[0].id, paths[1].id);
    assert_eq!(
        app.world().resource::<LayerCandidateState>().selected,
        Some(LayerTarget::Boundary(paths[1].id.clone()))
    ); // 新副本同步视口。
    assert_eq!(session.undo.len(), 1); // 一次复制只产生一次撤销。
    app.world_mut()
        .entity_mut(button)
        .insert((Action::Visible, EditorButtonClick::triggered())); // 下一次已确认动作仍针对新副本。
    app.update();
    let session = app.world().resource::<EditorAuthoringSession>();
    let paths = &session
        .draft
        .as_ref()
        .unwrap()
        .boundaries
        .as_ref()
        .unwrap()
        .paths;
    assert!(!paths[1].visible);
    assert!(paths[1].enabled);
    assert!(paths[0].visible); // 显示、物理及另一墙互不串写。
    assert_eq!(session.undo.len(), 2);
}
#[test]
fn delete_last_wall_keeps_an_empty_saveable_document() {
    let mut app = app();
    app.world_mut()
        .spawn((Action::Delete, EditorButtonClick::triggered()));
    app.update();
    let session = app.world().resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().unwrap();
    let doc = draft.boundaries.as_ref().unwrap();
    assert!(doc.paths.is_empty()); // 允许完全无空气墙的场景。
    assert!(
        doc.validate(&draft.scene_id, draft.manifest.world_bounds().unwrap())
            .is_ok()
    );
    assert_eq!(session.selected_boundary_path, None); // 不留下失效对象。
}
#[test]
fn reopening_a_saved_scene_does_not_reuse_wall_ids() {
    let mut app = app(); // 模拟重开后作者序列为零。
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session
            .draft
            .as_mut()
            .unwrap()
            .boundaries
            .as_mut()
            .unwrap()
            .paths[0]
            .id = "wall-1-1".into(); // 已持久化首个ID。
        session.next_serial = 0;
        let id = new_wall_id(&mut session); // 不能复用wall-1-1。
        assert_ne!(id, "wall-1-1");
    }
}
#[test]
fn viewport_selection_reveals_its_page_but_manual_paging_stays_put() {
    let mut app = app(); // 只运行固定面板系统。
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let doc = draft.boundaries.as_mut().unwrap();
        let original = doc.paths[0].clone();
        for index in 1..10 {
            let mut path = original.clone();
            path.id = format!("wall-{index}");
            path.role = SceneBoundaryRole::Internal;
            doc.paths.push(path);
        } // 模拟跨页的内部墙。
        let map = draft.scene_id.clone();
        session.selected_boundary_path = Some((map, "wall-9".into())); // 视口选中第二页对象。
    }
    app.update();
    assert_eq!(app.world().resource::<PanelState>().page, 1); // 所选行自动可见。
    app.world_mut().resource_mut::<PanelState>().page = 0; // 作者手动浏览另一页。
    app.update();
    assert_eq!(app.world().resource::<PanelState>().page, 0); // 稳定选择不能每帧抢回页码。
}

#[test]
fn empty_stale_and_single_page_selections_do_not_show_inapplicable_controls() {
    let mut app = app();
    let pagination = app
        .world_mut()
        .spawn((WallControls::Pagination, Node::default()))
        .id();
    let selection = app
        .world_mut()
        .spawn((WallControls::Selection, Node::default()))
        .id();
    let name = app
        .world_mut()
        .spawn((
            WallName,
            WallControls::Selection,
            UiInteraction::None,
            crate::ui::text_input::single_line_input("", 64),
            Node::default(),
            BorderColor::all(colors::INPUT_BORDER),
        ))
        .id();
    let title = app.world_mut().spawn((WallTitle, Text::new(""))).id();
    app.update();
    assert_eq!(
        app.world().get::<Node>(pagination).unwrap().display,
        Display::None
    );
    assert_eq!(
        app.world().get::<Node>(selection).unwrap().display,
        Display::Flex
    );
    assert_eq!(
        app.world().get::<Text>(title).unwrap().as_str(),
        "空气墙 · 1 道"
    );
    let original = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .boundaries
        .as_ref()
        .unwrap()
        .paths[0]
        .clone();
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(name, bevy::input_focus::FocusCause::Pressed);
    app.world_mut()
        .entity_mut(name)
        .insert(crate::ui::text_input::single_line_input("未提交名称", 64));
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .revision += 1;
    app.update();
    assert_eq!(
        app.world()
            .get::<EditableText>(name)
            .unwrap()
            .value()
            .to_string(),
        "未提交名称"
    );
    assert_eq!(
        *app.world().get::<BorderColor>(name).unwrap(),
        BorderColor::all(colors::INPUT_ACTIVE_BORDER)
    );
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .draft
        .as_mut()
        .unwrap()
        .boundaries
        .as_mut()
        .unwrap()
        .paths
        .clear();
    app.update(); // 保留同一个失效ID，必须复验存在性而非只看Some。
    assert_eq!(
        app.world().get::<Node>(selection).unwrap().display,
        Display::None
    );
    assert_eq!(
        app.world().get::<Node>(name).unwrap().display,
        Display::None
    );
    assert_eq!(app.world().resource::<InputFocus>().get(), None);
    assert_eq!(
        app.world().get::<Node>(pagination).unwrap().display,
        Display::None
    );
    assert!(
        app.world()
            .get::<Text>(title)
            .unwrap()
            .as_str()
            .contains("暂无空气墙")
    );
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        for index in 0..9 {
            let mut path = original.clone();
            path.id = format!("wall-{index}");
            path.role = SceneBoundaryRole::Internal;
            draft.boundaries.as_mut().unwrap().paths.push(path);
        }
        session.selected_boundary_path = Some((
            feathrise_scene_content::SceneId::parse("MAP-another-map").unwrap(),
            "wall-8".into(),
        ));
    }
    app.update();
    assert_eq!(
        app.world().get::<Node>(pagination).unwrap().display,
        Display::Flex
    );
    assert_eq!(
        app.world().get::<Node>(selection).unwrap().display,
        Display::None
    );
    assert_eq!(
        app.world().get::<Text>(title).unwrap().as_str(),
        "空气墙 · 9 道 · 1/2 页"
    );
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let map = session.draft.as_ref().unwrap().scene_id.clone();
        session.selected_boundary_path = Some((map, "wall-8".into()));
    }
    app.update();
    assert_eq!(
        app.world().get::<Node>(selection).unwrap().display,
        Display::Flex
    );
    assert_eq!(
        app.world().get::<Node>(name).unwrap().display,
        Display::Flex
    );
    assert_eq!(
        app.world().get::<Text>(title).unwrap().as_str(),
        "空气墙 · 9 道 · 2/2 页"
    );
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
}

#[test]
fn wall_action_feedback_uses_editor_small_button_colors_without_rewriting_idle_nodes() {
    let mut app = app();
    let button = app
        .world_mut()
        .spawn((
            Action::Previous,
            UiInteraction::None,
            BackgroundColor(colors::SLOT_BG),
        ))
        .id();
    for (interaction, expected) in [
        (UiInteraction::None, colors::BUTTON_DEV_SMALL_NORMAL),
        (UiInteraction::Hovered, colors::BUTTON_DEV_SMALL_HOVERED),
        (UiInteraction::Pressed, colors::BUTTON_DEV_SMALL_PRESSED),
    ] {
        app.world_mut().entity_mut(button).insert(interaction);
        app.update();
        assert_eq!(
            app.world().get::<BackgroundColor>(button).unwrap().0,
            expected
        );
    }
    app.update();
    let world = app.world_mut();
    assert_eq!(
        world
            .query_filtered::<Entity, Changed<BackgroundColor>>()
            .iter(world)
            .count(),
        0
    );
}
