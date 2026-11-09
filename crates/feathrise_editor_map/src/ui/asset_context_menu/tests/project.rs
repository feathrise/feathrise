//! 工程目录右键复用资产菜单的输入所有权；验证来源冻结与无副作用外部动作。

use feathrise_editor_ui::UiInteraction;
use super::*;
use crate::{asset_catalog::project_files::EditorProjectFileCatalog,
    ui::project_browser::{ProjectBrowserContent, ProjectBrowserState, ProjectBrowserView, ProjectFileRow, ProjectFileViewport}};
use crate::ui::external_files::{ExternalFileAction, OpenExternalProjectFile};
use super::super::{components::AssetMenuExternalButton, messages::CopyProjectPath};

fn project_fixture(path: &str, directory: bool) -> (Fixture, std::path::PathBuf, Entity) {
    let mut test = fixture("gltf/原资产.glb");
    let root = std::env::temp_dir().join("工程 右键测试");
    let mut catalog = EditorProjectFileCatalog::default();
    catalog.bind_project_source(29, Some(root.clone()));
    catalog.active = true;
    catalog.applied_revision = catalog.requested_revision;
    catalog.entries.push(EditorAssetEntry { asset_path: path.into(), display_name: "右键工程行".into(),
        is_directory: directory, kind: MapAssetKind::Data });
    let mut viewport = ProjectFileViewport::default();
    viewport.capacity = 1;
    viewport.revision = 7;
    test.app.insert_resource(catalog)
        .insert_resource(viewport)
        .insert_resource(ProjectBrowserState { view: ProjectBrowserView::ProjectFiles, ..default() })
        .add_message::<CopyProjectPath>()
        .add_message::<OpenExternalProjectFile>();
    test.app.world_mut().spawn((ProjectBrowserContent(ProjectBrowserView::ProjectFiles), ScrollPosition::default()));
    let row = test.app.world_mut().spawn((ProjectFileRow { slot: 0, entry_index: Some(0),
        revision: 1, viewport_revision: 7 }, UiInteraction::Hovered, Node::default())).id();
    step(&mut test.app);
    (test, root, row)
}

#[test]
fn project_menu_uses_the_right_clicked_file_and_project_root_without_changing_selection() {
    let (mut test, root, _) = project_fixture("src/中文 文件.rs", false);
    let favorite = test.app.world_mut().spawn((super::super::components::AssetMenuFavoriteButton,
        Node::default(), UiInteraction::None, BackgroundColor::default(), BorderColor::default())).id();
    open_menu(&mut test);
    let target = test.app.world().resource::<AssetContextMenuState>().target.as_ref().unwrap();
    assert_eq!(target.path, "src/中文 文件.rs");
    assert!(matches!(&target.source, AssetMenuSource::Project { root: frozen, context: 29 } if frozen == &root));
    assert_eq!(test.app.world().get::<Node>(favorite).unwrap().display, Display::None);
    assert_eq!((test.app.world().get::<Node>(test.popup).unwrap().width,
        test.app.world().get::<Node>(test.popup).unwrap().height), (px(240), px(138)));
    assert!(test.app.world().resource::<Messages<CopyProjectPath>>().is_empty(), "仅打开菜单不写剪贴板");
    *test.app.world_mut().get_mut::<UiInteraction>(test.copy).unwrap() = UiInteraction::Pressed;
    test.app.world_mut().entity_mut(test.copy).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    test.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    step(&mut test.app);
    let requests: Vec<_> = test.app.world_mut().resource_mut::<Messages<CopyProjectPath>>().drain().collect();
    assert_eq!(requests.len(), 1);
    assert_eq!((&requests[0].root, requests[0].path.as_str(), requests[0].kind),
        (&root, "src/中文 文件.rs", AssetPathCopyKind::Relative));
    assert!(test.app.world().resource::<EditorProjectFileCatalog>().selected_file.is_none());
    assert_eq!(test.app.world().resource::<MapEditorState>().selected_asset.as_deref(), Some("other/原预览.png"));
}

#[test]
fn project_path_copy_keeps_unicode_and_rejects_paths_outside_the_bound_root() {
    let root = std::env::temp_dir().join("工程 中文").join("子目录/..");
    let path = "src/配置 文件.rs";
    assert_eq!(project_path_to_copy(&root, path, AssetPathCopyKind::Relative).unwrap(), path);
    assert_eq!(project_path_to_copy(&root, path, AssetPathCopyKind::Absolute).unwrap(),
        std::env::temp_dir().join("工程 中文").join("src").join("配置 文件.rs").to_str().unwrap());
    for path in ["", "../外部.rs", "/外部.rs", "src\\外部.rs"] {
        assert!(project_path_to_copy(&root, path, AssetPathCopyKind::Relative).is_err());
    }
}

#[test]
fn project_menu_external_actions_keep_file_and_directory_targets() {
    for (path, directory, action) in [
        ("src/配置.rs", false, ExternalFileAction::VsCode(None)),
        ("src", true, ExternalFileAction::VsCodeProject(None)),
    ] {
        let (mut test, root, _) = project_fixture(path, directory);
        let button = test.app.world_mut().spawn((AssetMenuExternalButton(action), Node::default(),
            UiInteraction::None, BackgroundColor::default(), BorderColor::default())).id();
        open_menu(&mut test);
        *test.app.world_mut().get_mut::<UiInteraction>(button).unwrap() = UiInteraction::Pressed;
        test.app.world_mut().entity_mut(button).insert(feathrise_editor_ui::EditorButtonClick::triggered());
        test.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        step(&mut test.app);
        let requests: Vec<_> = test.app.world_mut().resource_mut::<Messages<OpenExternalProjectFile>>().drain().collect();
        assert_eq!(requests.len(), 1);
        assert_eq!((&requests[0].root, requests[0].request.path.as_str(), requests[0].request.action),
            (&root, path, action));
        assert!(test.app.world().resource::<EditorProjectFileCatalog>().selected_file.is_none());
    }
}

#[test]
fn project_menu_discards_old_rows_after_refresh_scroll_and_source_change() {
    for change in 0..4 {
        let (mut test, _, _) = project_fixture("src/旧文件.rs", false);
        open_menu(&mut test);
        match change {
            0 => test.app.world_mut().resource_mut::<EditorProjectFileCatalog>().requested_revision += 1,
            1 => test.app.world_mut().resource_mut::<ProjectFileViewport>().revision += 1,
            2 => {
                let mut query = test.app.world_mut().query::<(&ProjectBrowserContent, &mut ScrollPosition)>();
                for (_, mut scroll) in query.iter_mut(test.app.world_mut()) { scroll.y = 24.; }
            }
            _ => test.app.world_mut().resource_mut::<EditorProjectFileCatalog>().entries[0].asset_path = "src/新文件.rs".into(),
        }
        *test.app.world_mut().get_mut::<UiInteraction>(test.copy).unwrap() = UiInteraction::Pressed;
        test.app.world_mut().entity_mut(test.copy).insert(feathrise_editor_ui::EditorButtonClick::triggered());
        test.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        step(&mut test.app);
        assert!(test.app.world().resource::<AssetContextMenuState>().target.is_none());
        assert!(test.app.world().resource::<Messages<CopyProjectPath>>().is_empty());
        assert!(test.app.world().resource::<MapEditorNumericScrubMouseGuard>().blocked());
    }
}
