//! 正式资产行与中央打开系统的双击回归；不加载项目资产或写入作者文件。

use super::*;
use crate::{components::MapEditorAssetRow, resources::EditorAssetCatalog, types::EditorAssetEntry};
use std::time::Duration;

fn fixture() -> (App, Entity) {
    let mut app = App::new();
    let mut sidebar = MapEditorSidebarState::default();
    sidebar.navigator = crate::ui::sidebars::EditorNavigatorTab::Assets;
    sidebar.right = EditorRightSidebarTab::Assets;
    app.insert_resource(Time::<Real>::default())
        .add_message::<OpenAssetPreview>()
        .insert_resource(MapEditorState { open: true, ..default() })
        .insert_resource(sidebar)
        .init_resource::<EditorAssetCatalog>()
        .init_resource::<MapEditorNumericScrubMouseGuard>()
        .init_resource::<EditorTextInputGuard>()
        .init_resource::<DocumentState>()
        .init_resource::<AssetPreviewWorkspace>()
        .init_resource::<AssetPreviewState>()
        .init_resource::<AssetPreviewNavigation>()
        .init_resource::<AssetPreviewFocus>()
        .init_resource::<VegetationPresetEditorState>()
        .add_systems(Update, (crate::asset_catalog::handle_asset_row_clicks, open).chain());
    app.world_mut().resource_mut::<EditorAssetCatalog>().entries = ["a.json", "b.json", "a.png", "b.glb", "audio/剑声.OGG", "audio/音乐.WAV"]
        .into_iter().map(|path| EditorAssetEntry { asset_path: path.into(), display_name: path.into(),
            is_directory: false, kind: feathrise_map_content::MapAssetKind::Data }).collect();
    let row = app.world_mut().spawn((Interaction::None, MapEditorAssetRow { slot: 0, entry_index: Some(0) })).id();
    (app, row)
}

fn click(app: &mut App, row: Entity, index: usize, seconds: f64) {
    app.world_mut().resource_mut::<Time<Real>>().advance_by(Duration::from_secs_f64(seconds));
    app.world_mut().get_mut::<MapEditorAssetRow>(row).unwrap().entry_index = Some(index);
    *app.world_mut().get_mut::<Interaction>(row).unwrap() = Interaction::Pressed;
    app.world_mut().entity_mut(row).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    app.update();
    *app.world_mut().get_mut::<Interaction>(row).unwrap() = Interaction::Hovered;
    app.update();
}

#[test]
fn asset_double_click_opens_multiple_files_and_reuses_existing_path() {
    let (mut app, row) = fixture();
    click(&mut app, row, 0, 0.05);
    assert!(!app.world().resource::<DocumentState>().expanded());
    click(&mut app, row, 0, 0.12);
    let first = app.world().resource::<DocumentState>().active_tab_id().unwrap();
    click(&mut app, row, 1, 0.05);
    click(&mut app, row, 1, 0.12);
    assert_eq!(app.world().resource::<DocumentState>().tab_count(), 2);
    click(&mut app, row, 0, 0.05);
    click(&mut app, row, 0, 0.12);
    let state = app.world().resource::<DocumentState>();
    assert_eq!(state.tab_count(), 2);
    assert_eq!(state.active_tab_id().unwrap(), first);
    click(&mut app, row, 2, 0.05);
    click(&mut app, row, 2, 0.12);
    click(&mut app, row, 3, 0.05);
    click(&mut app, row, 3, 0.12);
    assert_eq!(app.world().resource::<AssetPreviewWorkspace>().tabs().count(), 2);
    assert_eq!(app.world().resource::<DocumentState>().tab_count(), 2, "媒体文件不能替换文档标签");
}

#[test]
fn asset_double_click_rejects_slow_different_recycled_or_directory_rows() {
    let (mut app, row) = fixture();
    click(&mut app, row, 0, 0.05);
    click(&mut app, row, 0, 0.5);
    assert!(!app.world().resource::<DocumentState>().expanded());
    click(&mut app, row, 1, 0.1);
    assert!(!app.world().resource::<DocumentState>().expanded());
    app.world_mut().resource_mut::<EditorAssetCatalog>().applied_revision += 1;
    click(&mut app, row, 1, 0.1);
    assert!(!app.world().resource::<DocumentState>().expanded(), "换代后的固定槽不能沿用旧点击");
    app.world_mut().resource_mut::<EditorAssetCatalog>().entries[0].is_directory = true;
    click(&mut app, row, 0, 0.1);
    click(&mut app, row, 0, 0.1);
    assert!(!app.world().resource::<DocumentState>().expanded(), "目录仍使用原来的进入目录语义");
}

#[test]
fn asset_double_click_ime_block_does_not_replay_after_confirmation() {
    let (mut app, row) = fixture();
    app.world_mut().resource_mut::<EditorTextInputGuard>().block_shortcuts = true;
    click(&mut app, row, 0, 0.05);
    click(&mut app, row, 0, 0.12);
    assert!(!app.world().resource::<DocumentState>().expanded());
    app.world_mut().resource_mut::<EditorTextInputGuard>().block_shortcuts = false;
    app.update();
    assert!(!app.world().resource::<DocumentState>().expanded(), "被IME阻挡的双击不能事后抢走文档");
    click(&mut app, row, 0, 0.05);
    click(&mut app, row, 0, 0.12);
    assert!(app.world().resource::<DocumentState>().expanded());
}

#[test]
fn asset_audio_double_click_opens_multiple_media_tabs_without_documents_or_duplicate_paths() {
    let (mut app, row) = fixture();
    click(&mut app, row, 4, 0.05);
    assert!(!app.world().resource::<AssetPreviewWorkspace>().has_tabs());
    click(&mut app, row, 4, 0.12);
    let first = app.world().resource::<AssetPreviewWorkspace>().expanded.as_ref().unwrap().id;
    click(&mut app, row, 5, 0.05);
    click(&mut app, row, 5, 0.12);
    click(&mut app, row, 4, 0.05);
    click(&mut app, row, 4, 0.12);
    let media = app.world().resource::<AssetPreviewWorkspace>();
    assert_eq!(media.tabs().count(), 2);
    assert_eq!(media.expanded.as_ref().unwrap().id, first);
    assert_eq!(app.world().resource::<DocumentState>().tab_count(), 0);
}

#[test]
fn asset_row_press_and_hold_wait_for_inside_release_and_outside_release_cancels() {
    let (mut app, row) = fixture();
    app.init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .add_plugins(feathrise_editor_ui::EditorButtonInputPlugin);
    app.world_mut().entity_mut(row).insert((
        Button, Node::default(),
        InheritedVisibility::VISIBLE, // 裸ECS夹具没有可见性传播插件，显式提供已完成布局的可见状态。
        bevy::ui::RelativeCursorPosition { cursor_over: true, ..default() },
        ComputedNode { size: Vec2::new(120.0, 24.0), ..default() },
    ));
    app.world_mut().entity_mut(row).with_children(|parent| {
        parent.spawn((crate::components::MapEditorAssetRowText, Text::new(""), TextColor(Color::WHITE)));
    });
    app.add_systems(PostUpdate, crate::ui::rows::sync_asset_rows);
    app.update(); // 先由正式表现系统建立固定槽身份，按下后再检验换绑失效。
    let frame = |app: &mut App, interaction: Interaction, down: Option<bool>| {
        *app.world_mut().get_mut::<Interaction>(row).unwrap() = interaction;
        app.world_mut().get_mut::<bevy::ui::RelativeCursorPosition>(row).unwrap().cursor_over = interaction != Interaction::None;
        if let Some(down) = down {
            let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
            if down { mouse.press(MouseButton::Left); } else { mouse.release(MouseButton::Left); }
        }
        app.update();
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
    };
    frame(&mut app, Interaction::Pressed, Some(true));
    frame(&mut app, Interaction::Pressed, None);
    assert!(app.world().resource::<MapEditorState>().selected_asset.is_none(), "按住不能提前选择资产");
    frame(&mut app, Interaction::None, Some(false));
    assert!(app.world().resource::<MapEditorState>().selected_asset.is_none(), "移出松开应取消选择");
    frame(&mut app, Interaction::Pressed, Some(true));
    frame(&mut app, Interaction::Hovered, Some(false));
    assert_eq!(app.world().resource::<MapEditorState>().selected_asset.as_deref(), Some("a.json"));
    frame(&mut app, Interaction::Hovered, None);
    assert!(!app.world().resource::<DocumentState>().expanded(), "一次释放不能被下一帧重复算作双击");
    // 固定槽换绑时取消原手势，不能把松手交给新文件。
    frame(&mut app, Interaction::Pressed, Some(true));
    {
        let mut catalog = app.world_mut().resource_mut::<EditorAssetCatalog>();
        catalog.entries.swap(0, 1);
        catalog.applied_revision += 1;
        catalog.requested_revision = catalog.applied_revision;
    }
    frame(&mut app, Interaction::Pressed, None); // 真实目录采用点使同一槽位的旧按住失效。
    frame(&mut app, Interaction::Hovered, Some(false));
    assert_eq!(app.world().resource::<MapEditorState>().selected_asset.as_deref(), Some("a.json"));
}
