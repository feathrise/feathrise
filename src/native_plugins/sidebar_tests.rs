//! 真实宿主输入及回执路由回归：插件互切、隐藏点击、迟到响应与中央文档隔离。
use feathrise_editor_ui::InteractionButton;
use super::resources::Event;
use super::{components::*, resources::*, systems};
use bevy::prelude::*;
use feathrise_editor_core::{
    AssetPreviewFocus, EditorTextInputGuard, PresetWorkspaceTab, WorkspaceDomainState,
};
use feathrise_editor_scene::game_authoring::scene_api::{
    resources::{EditorAssetCatalog, SceneEditorState},
    ui::{sidebars::*, vegetation_preset_editor::VegetationPresetEditorState},
};
use feathrise_editor_ui::{
    ActiveNumericInput, ActiveNumericInputScrub, EditorButtonClick, NumericInputCommitted,
};
use feathrise_plugin_api::{Document, Request, Snapshot};

pub(super) fn fixture() -> (
    App,
    async_channel::Sender<Event>,
    async_channel::Receiver<Job>,
) {
    let (events, receiver) = async_channel::bounded(8);
    let (requests, jobs) = async_channel::bounded(4);
    let tools = ["fixture.equipment", "fixture.skill"]
        .into_iter()
        .enumerate()
        .map(|(index, domain)| {
            (
                index,
                Tool {
                    domain,
                    snapshot: Snapshot {
                        plugin_id: domain.into(),
                        tool: domain.into(),
                        revision: 1,
                        active: 1,
                        tabs: vec![Document {
                            id: 1,
                            title: "未保存草稿".into(),
                            dirty: true,
                            saved_sequence: 0,
                        }],
                        ..default()
                    },
                    requests: requests.clone(),
                    queue: default(),
                    inflight: None,
                    completed: 0,
                    sequence: 0,
                    fault: None,
                    library_open: false,
                    close_confirmation: None,
                    choices: None,
                    latest_activation: 0,
                    scene_pending: false,
                },
            )
        })
        .collect();
    let mut app = App::new();
    app.insert_resource(NativeTools {
        tools,
        receiver,
        discovery_pending: false,
        loading: 0,
        errors: vec![],
        management_errors: Default::default(),
        load_states: Default::default(),
        load_origins: Default::default(),
        ui_revision: 1,
        diagnostics: Default::default(),
        input_pending: false,
        save_tickets: default(),
    })
    .init_resource::<SceneEditorSidebarState>()
    .init_resource::<SceneEditorState>()
    .init_resource::<EditorAssetCatalog>()
    .init_resource::<VegetationPresetEditorState>()
    .init_resource::<WorkspaceDomainState>()
    .init_resource::<AssetPreviewFocus>()
    .init_resource::<EditorTextInputGuard>()
    .init_resource::<bevy::input_focus::InputFocus>()
    .init_resource::<ButtonInput<KeyCode>>()
    .init_resource::<ActiveNumericInput>()
    .init_resource::<ActiveNumericInputScrub>()
    .init_resource::<bevy::clipboard::Clipboard>()
    .add_message::<NumericInputCommitted>()
    .add_systems(Update, (systems::input, systems::poll).chain());
    app.world_mut().resource_mut::<SceneEditorState>().open = true;
    app.world_mut()
        .resource_mut::<VegetationPresetEditorState>()
        .active_tab = PresetWorkspaceTab::Document;
    (app, events, jobs)
}
fn click(app: &mut App, index: usize, action: UiAction) {
    let entity = app
        .world_mut()
        .spawn((ToolButton { index, action }, EditorButtonClick::triggered()))
        .id();
    app.update();
    app.world_mut().despawn(entity);
}

#[test]
fn asset_locate_uses_the_visible_buffer_without_submitting_or_losing_it_next_frame() {
    let (mut app, _, jobs) = fixture();
    click(&mut app, 0, UiAction::OpenTool);
    let field_id = "arbitrary/asset/path";
    app.world_mut()
        .resource_mut::<NativeTools>()
        .tools
        .get_mut(&0)
        .unwrap()
        .snapshot
        .fields
        .push(feathrise_plugin_api::Field {
            id: field_id.into(),
            label: "资源".into(),
            value: "old/model.glb".into(),
            kind: feathrise_plugin_api::FieldKind::Asset {
                extensions: vec!["glb".into()],
            },
        });
    let entity = app
        .world_mut()
        .spawn((
            ToolText {
                index: 0,
                document: 1,
                field: field_id.into(),
                baseline: "old/model.glb".into(),
                multiline: false,
            },
            bevy::text::EditableText::new("new/可见缓冲.glb#Scene0"),
        ))
        .id();
    // 模拟按钮先取得焦点；生产处理必须还给原行，防止下一帧失焦隐式提交。
    click(
        &mut app,
        0,
        UiAction::Locate {
            document: 1,
            field: field_id.into(),
        },
    );
    assert!(jobs.try_recv().is_err());
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .status
            .contains("new/可见缓冲.glb#Scene0")
    );
    assert_eq!(
        app.world()
            .resource::<bevy::input_focus::InputFocus>()
            .get(),
        Some(entity)
    );
    assert!(app.world().resource::<NativeTools>().input_pending);
    app.update();
    assert!(jobs.try_recv().is_err(), "仅导航不能在后续一帧补交Edit");
    assert_eq!(
        app.world().get::<ToolText>(entity).unwrap().baseline,
        "old/model.glb"
    );
    assert_eq!(
        app.world()
            .get::<bevy::text::EditableText>(entity)
            .unwrap()
            .value(),
        "new/可见缓冲.glb#Scene0"
    );
    assert_eq!(
        app.world().resource::<NativeTools>().tools[&0]
            .snapshot
            .fields[0]
            .value,
        "old/model.glb"
    );
}

#[test]
fn stale_document_and_non_asset_field_cannot_route_a_locate_request() {
    for (document, kind) in [
        (
            2,
            feathrise_plugin_api::FieldKind::Asset {
                extensions: vec!["glb".into()],
            },
        ),
        (1, feathrise_plugin_api::FieldKind::Label),
    ] {
        let (mut app, _, jobs) = fixture();
        click(&mut app, 0, UiAction::OpenTool);
        app.world_mut()
            .resource_mut::<NativeTools>()
            .tools
            .get_mut(&0)
            .unwrap()
            .snapshot
            .fields
            .push(feathrise_plugin_api::Field {
                id: "asset".into(),
                label: "资源".into(),
                value: "model.glb".into(),
                kind,
            });
        app.world_mut().spawn((
            ToolText {
                index: 0,
                document,
                field: "asset".into(),
                baseline: "model.glb".into(),
                multiline: false,
            },
            bevy::text::EditableText::new("model.glb"),
        ));
        click(
            &mut app,
            0,
            UiAction::Locate {
                document,
                field: "asset".into(),
            },
        );
        assert!(jobs.try_recv().is_err());
        assert!(
            app.world()
                .resource::<SceneEditorState>()
                .status
                .contains("字段已换代")
        );
    }
}
#[test]
fn native_tools_switch_only_the_shared_sidebar_and_preserve_each_draft() {
    let (mut app, _, jobs) = fixture();
    for index in [0, 1, 0] {
        click(&mut app, index, UiAction::OpenTool);
        let domain = app.world().resource::<NativeTools>().tools[&index].domain;
        assert!(
            app.world()
                .resource::<SceneEditorSidebarState>()
                .tool_page_visible(EditorLeftSidebarTab::Domain(domain))
        );
        assert_eq!(
            app.world()
                .resource::<VegetationPresetEditorState>()
                .active_tab,
            PresetWorkspaceTab::Document
        );
        assert!(
            app.world()
                .resource::<WorkspaceDomainState>()
                .open_tabs
                .is_empty()
        );
        assert!(
            app.world()
                .resource::<NativeTools>()
                .tools
                .values()
                .all(|tool| tool.snapshot.active == 1 && tool.snapshot.tabs[0].dirty)
        );
    }
    assert!(
        jobs.try_recv().is_err(),
        "返回工具不应发Open清空当前目录选择"
    );
}
#[test]
fn hidden_tool_click_is_rejected_and_late_reply_cannot_reclaim_sidebar() {
    let (mut app, events, jobs) = fixture();
    click(&mut app, 0, UiAction::OpenTool);
    click(
        &mut app,
        0,
        UiAction::Request(Request::Save { document: 1 }),
    );
    let job = jobs.try_recv().unwrap();
    click(&mut app, 1, UiAction::OpenTool);
    click(
        &mut app,
        0,
        UiAction::Request(Request::Action {
            document: 1,
            action: "delete".into(),
            value: String::new(),
        }),
    );
    assert!(jobs.try_recv().is_err(), "隐藏页面旧点击不能修改草稿");
    let mut snapshot = app.world().resource::<NativeTools>().tools[&0]
        .snapshot
        .clone();
    snapshot.revision = 2;
    snapshot.tabs[0].dirty = false;
    snapshot.tabs[0].saved_sequence = 1;
    events
        .try_send(Event::Updated {
            index: 0,
            sequence: job.sequence,
            activate: true,
            snapshot: Ok(snapshot),
            request: job.request,
        })
        .unwrap();
    app.update();
    assert!(
        app.world()
            .resource::<SceneEditorSidebarState>()
            .tool_page_visible(EditorLeftSidebarTab::Domain("fixture.skill"))
    );
    assert_eq!(
        app.world()
            .resource::<VegetationPresetEditorState>()
            .active_tab,
        PresetWorkspaceTab::Document
    );
    assert_eq!(
        app.world().resource::<NativeTools>().tools[&0]
            .snapshot
            .tabs[0]
            .saved_sequence,
        1
    );
}

#[test]
fn document_workbench_uses_central_tabs_and_late_create_cannot_steal_navigation() {
    let (mut app, events, jobs) = fixture();
    app.world_mut()
        .resource_mut::<NativeTools>()
        .tools
        .get_mut(&0)
        .unwrap()
        .snapshot
        .workbench = Some(feathrise_plugin_api::Workbench {
        library_title: "预设库".into(),
        ..default()
    });
    click(&mut app, 0, UiAction::OpenTool);
    assert_eq!(
        app.world()
            .resource::<VegetationPresetEditorState>()
            .active_tab,
        PresetWorkspaceTab::Domain("fixture.equipment", 0)
    );
    assert!(
        app.world()
            .resource::<WorkspaceDomainState>()
            .is_open(PresetWorkspaceTab::Domain("fixture.equipment", 1))
    );
    // 先消费首次回到目录的请求，再从目录创建；切换场景后的迟到结果只保留草稿。
    let job = jobs.try_recv().unwrap();
    let mut s = app.world().resource::<NativeTools>().tools[&0]
        .snapshot
        .clone();
    s.active = 0;
    s.revision = 2;
    events
        .try_send(Event::Updated {
            index: 0,
            sequence: job.sequence,
            activate: true,
            snapshot: Ok(s),
            request: job.request,
        })
        .unwrap();
    app.update();
    click(
        &mut app,
        0,
        UiAction::Request(Request::Action {
            document: 0,
            action: "new".into(),
            value: String::new(),
        }),
    );
    let job = jobs.try_recv().unwrap();
    app.world_mut()
        .resource_mut::<VegetationPresetEditorState>()
        .active_tab = PresetWorkspaceTab::Scene;
    let mut s = app.world().resource::<NativeTools>().tools[&0]
        .snapshot
        .clone();
    s.active = 1;
    s.revision = 3;
    events
        .try_send(Event::Updated {
            index: 0,
            sequence: job.sequence,
            activate: true,
            snapshot: Ok(s),
            request: job.request,
        })
        .unwrap();
    app.update();
    assert_eq!(
        app.world()
            .resource::<VegetationPresetEditorState>()
            .active_tab,
        PresetWorkspaceTab::Scene
    );
    click(&mut app, 0, UiAction::Tab(1));
    assert_eq!(
        app.world()
            .resource::<VegetationPresetEditorState>()
            .active_tab,
        PresetWorkspaceTab::Domain("fixture.equipment", 1)
    );
}

#[test]
fn button_focus_allows_document_history_but_text_focus_owns_undo() {
    let (mut app, _, jobs) = fixture();
    click(&mut app, 0, UiAction::OpenTool);
    let entity = app.world_mut().spawn(InteractionButton).id();
    app.world_mut()
        .resource_mut::<bevy::input_focus::InputFocus>()
        .set(entity, bevy::input_focus::FocusCause::Pressed);
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::ControlLeft);
        keys.press(KeyCode::KeyZ);
    }
    app.update();
    let job = jobs.try_recv().unwrap();
    assert!(matches!(job.request,Request::Action {action,..} if action=="undo"));
    app.world_mut()
        .entity_mut(entity)
        .insert(feathrise_editor_core::EditorTextInput);
    app.world_mut()
        .resource_mut::<NativeTools>()
        .tools
        .get_mut(&0)
        .unwrap()
        .inflight = None;
    app.update();
    assert!(
        jobs.try_recv().is_err(),
        "文本自己的撤销不能同时修改领域草稿"
    );
}

#[test]
fn changed_draft_requires_a_fresh_discard_confirmation() {
    let (mut app, _, jobs) = fixture();
    click(&mut app, 0, UiAction::OpenTool);
    click(&mut app, 0, UiAction::Close(1));
    assert_eq!(
        app.world().resource::<NativeTools>().tools[&0].close_confirmation,
        Some(1)
    );
    click(
        &mut app,
        0,
        UiAction::Request(Request::Edit {
            document: 1,
            field: "name".into(),
            value: "新修改".into(),
        }),
    );
    let job = jobs.try_recv().unwrap();
    assert!(matches!(job.request, Request::Edit { .. }));
    assert!(
        app.world().resource::<NativeTools>().tools[&0]
            .close_confirmation
            .is_none()
    );
    click(&mut app, 0, UiAction::Close(1));
    assert_eq!(
        app.world().resource::<NativeTools>().tools[&0].close_confirmation,
        Some(1)
    );
    assert!(
        jobs.try_recv().is_err(),
        "确认后的新修改不能被下一次关闭直接丢弃"
    );
}

#[test]
fn save_failure_and_real_receipt_are_published_to_the_shared_status() {
    let (mut app, events, jobs) = fixture();
    click(&mut app, 0, UiAction::OpenTool);
    for received in [false, true] {
        click(
            &mut app,
            0,
            UiAction::Request(Request::Action {
                document: 1,
                action: "save".into(),
                value: String::new(),
            }),
        );
        let job = jobs.try_recv().unwrap();
        let mut snapshot = app.world().resource::<NativeTools>().tools[&0]
            .snapshot
            .clone();
        snapshot.revision += 1;
        snapshot.error = (!received).then(|| "保存装备失败：缺少模板模型；草稿保留".into());
        snapshot.status = if received {
            "装备已原子保存".into()
        } else {
            snapshot.error.clone().unwrap()
        };
        snapshot.tabs[0].dirty = !received;
        snapshot.tabs[0].saved_sequence = u64::from(received);
        let expected = snapshot.status.clone();
        events
            .try_send(Event::Updated {
                index: 0,
                sequence: job.sequence,
                activate: job.activate,
                request: job.request,
                snapshot: Ok(snapshot),
            })
            .unwrap();
        app.update();
        assert_eq!(app.world().resource::<SceneEditorState>().status, expected);
    }
    // 仅回复“已保存”但没有新回执时，不允许底栏显示成功。
    click(
        &mut app,
        0,
        UiAction::Request(Request::Save { document: 1 }),
    );
    let job = jobs.try_recv().unwrap();
    let mut snapshot = app.world().resource::<NativeTools>().tools[&0]
        .snapshot
        .clone();
    snapshot.revision += 1;
    events
        .try_send(Event::Updated {
            index: 0,
            sequence: job.sequence,
            activate: job.activate,
            request: job.request,
            snapshot: Ok(snapshot),
        })
        .unwrap();
    app.update();
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .status
            .contains("未返回实际保存回执")
    );
}
