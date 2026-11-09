//! 所选文件、目录反馈与显式复制的纯ECS回归；不读写用户文件或系统剪贴板。

use feathrise_editor_ui::UiInteraction;
use super::{components::*, systems::*, types::*};
use crate::asset_catalog::AssetCatalogFeedback;
use crate::resources::{EditorAssetCatalog, MapEditorNumericScrubMouseGuard, MapEditorState};
use crate::ui::sidebars::{EditorRightSidebarTab, MapEditorSidebarState};
use bevy::prelude::*;

#[path = "layout_tests.rs"]
mod layout_tests;

fn sidebar_app() -> (App, Entity) {
    let mut app = App::new();
    app.insert_resource(MapEditorState {
        open: true,
        selected_asset: Some("gltf/中文模型.GLB".into()),
        ..default()
    })
    .init_resource::<EditorAssetCatalog>()
    .init_resource::<MapEditorNumericScrubMouseGuard>()
    .init_resource::<MapEditorSidebarState>()
    .add_systems(Update, (copy_selected_path, sync_sidebar).chain());
    app.world_mut()
        .resource_mut::<MapEditorSidebarState>()
        .right = EditorRightSidebarTab::Assets;
    let button = app
        .world_mut()
        .spawn((
            AssetSidebarCopyPath::default(),
            UiInteraction::None,
            BackgroundColor::default(),
            BorderColor::default(),
        ))
        .id();
    for field in [
        SidebarField::DirectoryFeedback,
        SidebarField::Name,
        SidebarField::Kind,
        SidebarField::Path,
        SidebarField::CopyFeedback,
    ] {
        app.world_mut().spawn((
            AssetSidebarText(field),
            Text::new(""),
            TextColor::default(),
            Node::default(),
        ));
    }
    app.world_mut()
        .spawn((AssetSidebarPathRow, Node::default()));
    app.update();
    (app, button)
}

fn field_text(app: &mut App, field: SidebarField) -> String {
    let world = app.world_mut();
    world
        .query::<(&AssetSidebarText, &Text)>()
        .iter(world)
        .find(|(marker, _)| marker.0 == field)
        .unwrap()
        .1
        .0
        .clone()
}

#[test]
fn directory_requires_completed_evidence_and_preserves_real_error() {
    let mut catalog = EditorAssetCatalog::default();
    assert_eq!(directory_feedback(&catalog), ("等待目录同步", false));
    catalog.requested_revision = 1;
    assert!(directory_feedback(&catalog).0.contains("同步"));
    catalog.applied_revision = 1;
    catalog.feedback = Some(AssetCatalogFeedback {
        revision: 1,
        result: Err("无法打开完整中文资源目录".into()),
    });
    assert_eq!(
        directory_feedback(&catalog),
        ("无法打开完整中文资源目录", true)
    );
    catalog.requested_revision = 2;
    assert!(!directory_feedback(&catalog).1);
    catalog.applied_revision = 2;
    assert!(directory_feedback(&catalog).0.contains("取消"));
    catalog.feedback = Some(AssetCatalogFeedback {
        revision: 2,
        result: Ok(()),
    });
    assert_eq!(directory_feedback(&catalog), ("当前目录没有资源", false));
}

#[test]
fn selected_file_uses_full_path_and_shared_case_insensitive_format() {
    let (mut app, _) = sidebar_app();
    assert_eq!(field_text(&mut app, SidebarField::Name), "中文模型.GLB");
    assert_eq!(
        field_text(&mut app, SidebarField::Path),
        "assets/gltf/中文模型.GLB"
    );
    assert_eq!(field_text(&mut app, SidebarField::Kind), "模型 · GLB");
    for (path, kind) in [
        ("config/data.JSON", "配置"),
        ("shaders/custom.WGSL", "代码"),
        ("docs/README", "文本"),
        ("textures/sky.KTX2", "图片 / 纹理"),
        ("maps/height.r16", "二进制数据"),
    ] {
        assert!(kind_label(path).starts_with(kind), "{path}");
    }
    app.world_mut()
        .resource_mut::<EditorAssetCatalog>()
        .feedback = Some(AssetCatalogFeedback {
        revision: 0,
        result: Err("目录读取失败：具体原因".into()),
    });
    app.update();
    app.world_mut().resource_mut::<MapEditorState>().status = "其他工具的新提示".into();
    app.update();
    assert_eq!(
        field_text(&mut app, SidebarField::DirectoryFeedback),
        "目录读取失败：具体原因"
    );
}

#[test]
fn stale_path_press_cannot_copy_rebound_file_or_replay_after_selection() {
    let (mut app, button) = sidebar_app();
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .selected_asset = Some("textures/另一个文件.png".into());
    *app.world_mut().get_mut::<UiInteraction>(button).unwrap() = UiInteraction::Pressed;
    app.world_mut().entity_mut(button).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    app.update();
    assert!(
        app.world()
            .get::<AssetSidebarCopyPath>(button)
            .unwrap()
            .feedback
            .is_empty()
    );
    app.update();
    assert!(
        app.world()
            .get::<AssetSidebarCopyPath>(button)
            .unwrap()
            .feedback
            .is_empty()
    );
    *app.world_mut().get_mut::<UiInteraction>(button).unwrap() = UiInteraction::None;
    app.update();
    *app.world_mut().get_mut::<UiInteraction>(button).unwrap() = UiInteraction::Pressed;
    app.world_mut().entity_mut(button).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    app.update();
    assert!(field_text(&mut app, SidebarField::CopyFeedback).contains("剪贴板尚未就绪"));
    assert!(!app.world().resource::<MapEditorState>().dirty);
}

#[test]
fn blocked_press_is_consumed_and_copy_feedback_is_bound_to_selection() {
    let (mut app, button) = sidebar_app();
    app.world_mut()
        .resource_mut::<MapEditorNumericScrubMouseGuard>()
        .advance(true, true, false);
    *app.world_mut().get_mut::<UiInteraction>(button).unwrap() = UiInteraction::Pressed;
    app.world_mut().entity_mut(button).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    app.update();
    app.world_mut()
        .resource_mut::<MapEditorNumericScrubMouseGuard>()
        .advance(false, false, false);
    app.update();
    assert!(field_text(&mut app, SidebarField::CopyFeedback).is_empty());
    *app.world_mut().get_mut::<UiInteraction>(button).unwrap() = UiInteraction::None;
    app.update();
    *app.world_mut().get_mut::<UiInteraction>(button).unwrap() = UiInteraction::Pressed;
    app.world_mut().entity_mut(button).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    app.update();
    assert!(field_text(&mut app, SidebarField::CopyFeedback).contains("失败"));
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .selected_asset = None;
    app.update();
    assert_eq!(field_text(&mut app, SidebarField::Name), "尚未选择文件");
    assert!(field_text(&mut app, SidebarField::CopyFeedback).is_empty());
}

#[derive(Resource, Default)]
struct Writes(usize);

#[test]
fn modal_and_ime_copy_press_is_not_replayed_after_unblocking() {
    let (mut app, button) = sidebar_app();
    app.insert_resource(crate::resources::EditorTextInputGuard {
        block_shortcuts: true,
    });
    *app.world_mut().get_mut::<UiInteraction>(button).unwrap() = UiInteraction::Pressed;
    app.world_mut().entity_mut(button).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    app.update();
    app.world_mut()
        .resource_mut::<crate::resources::EditorTextInputGuard>()
        .block_shortcuts = false;
    app.update();
    assert!(field_text(&mut app, SidebarField::CopyFeedback).is_empty());
    *app.world_mut().get_mut::<UiInteraction>(button).unwrap() = UiInteraction::None;
    app.update();
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .new_map_dialog_open = true;
    *app.world_mut().get_mut::<UiInteraction>(button).unwrap() = UiInteraction::Pressed;
    app.world_mut().entity_mut(button).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    app.update();
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .new_map_dialog_open = false;
    app.update();
    assert!(field_text(&mut app, SidebarField::CopyFeedback).is_empty());
}

fn observe_writes(
    texts: Query<(), (With<AssetSidebarText>, Changed<Text>)>,
    mut writes: ResMut<Writes>,
) {
    writes.0 += texts.iter().count();
}

#[test]
fn unrelated_status_and_hidden_page_do_not_rewrite_selected_fields() {
    let (mut app, _) = sidebar_app();
    app.init_resource::<Writes>()
        .add_systems(Update, observe_writes.after(sync_sidebar));
    app.update();
    app.world_mut().resource_mut::<Writes>().0 = 0;
    app.world_mut().resource_mut::<MapEditorState>().status = "笔刷提示变化".into();
    app.update();
    assert_eq!(app.world().resource::<Writes>().0, 0);
    app.world_mut()
        .resource_mut::<MapEditorSidebarState>()
        .right = EditorRightSidebarTab::Scene;
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .selected_asset = Some("config/新的配置.json".into());
    app.update();
    assert_eq!(field_text(&mut app, SidebarField::Name), "中文模型.GLB");
    app.world_mut()
        .resource_mut::<MapEditorSidebarState>()
        .right = EditorRightSidebarTab::Assets;
    app.update();
    assert_eq!(field_text(&mut app, SidebarField::Name), "新的配置.json");
}

#[test]
fn preview_phase_comes_from_real_state_not_success_text_or_handle() {
    use crate::ui::asset_preview::{presentation::phase_label, types::*};
    let mut state = AssetPreviewState::default();
    for (phase, label) in [
        (AssetPreviewPhase::Loading, "加载中"),
        (AssetPreviewPhase::Scanning, "准备完整范围"),
        (AssetPreviewPhase::RevealPending, "等待完整发布"),
        (AssetPreviewPhase::Ready, "已就绪"),
        (AssetPreviewPhase::Failed, "加载失败"),
    ] {
        state.content = AssetPreviewContent::Image {
            handle: Handle::default(),
            phase,
        };
        state.status = "已复制引用：只是操作成功".into();
        assert_eq!(phase_label(&state, false), label);
    }
    assert_eq!(phase_label(&state, true), "加载失败");
    state.content = AssetPreviewContent::Image {
        handle: Handle::default(),
        phase: AssetPreviewPhase::Loading,
    };
    assert_eq!(phase_label(&state, true), "恢复观察中");
    state.content = AssetPreviewContent::Document;
    assert_eq!(phase_label(&state, false), ""); // 文档使用自己的真实底栏状态，不重复插入教程。
}
