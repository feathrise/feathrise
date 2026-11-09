//! 复用正式F12根、右键菜单及原生输入的离屏GPU验证；外部打开仅记录意图。

use feathrise_editor_ui::UiInteraction;
use super::super::super::resources::DocumentState;
use super::super::{
    components::*, constants::ROW_SLOTS, gpu_tests as gpu, resources::AssetSearchState,
    types::{ContentIdentity, FileHits, Hit, ResultRow, SearchResult},
};
use crate::ui::{
    asset_context_menu,
    external_files::{self, ExternalEditor, ExternalEditors},
};
use bevy::{ecs::system::RunSystemOnce, prelude::*, text::EditableText};

fn menu_click(app: &mut App, window: Entity, camera: Entity, entity: Entity, button: MouseButton) {
    let position = app
        .world()
        .get::<UiGlobalTransform>(entity)
        .unwrap()
        .affine()
        .translation;
    let target = app
        .world()
        .get::<bevy::camera::RenderTarget>(camera)
        .unwrap()
        .clone();
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_physical_cursor_position(Some(position.as_dvec2()));
    *app.world_mut()
        .get_mut::<bevy::camera::RenderTarget>(camera)
        .unwrap() = bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(window));
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(button);
    app.world_mut()
        .run_system_cached(bevy::ui::ui_focus_system)
        .unwrap();
    app.world_mut().run_system_once(feathrise_editor_ui::update_editor_button_clicks).unwrap();
    assert_eq!(
        *app.world().get::<UiInteraction>(entity).unwrap(),
        if button == MouseButton::Right {
            UiInteraction::Hovered
        } else {
            UiInteraction::Pressed
        }
    );
    asset_context_menu::control_menu_for_tests(app.world_mut());
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(button);
    app.world_mut()
        .run_system_cached(bevy::ui::ui_focus_system)
        .unwrap();
    app.world_mut().run_system_once(feathrise_editor_ui::update_editor_button_clicks).unwrap();
    asset_context_menu::control_menu_for_tests(app.world_mut());
    *app.world_mut()
        .get_mut::<bevy::camera::RenderTarget>(camera)
        .unwrap() = target;
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    gpu::frames(app, 4); // 正式鼠标门经历菜单关闭与完整静默帧，不能在按住/松键帧启动查询。
}

fn assert_reference_pixels(app: &mut App, filename: &str) {
    // CPU文字/裁剪正常仍可能漏绘；读取实际GPU快照，检查顶部、中间和底部引用行。
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../.local/validation")
        .join(filename);
    let image = image::open(path).unwrap().to_rgba8();
    let world = app.world_mut();
    for (label, node, transform) in world
        .query::<(&SearchLabel, &ComputedNode, &UiGlobalTransform)>()
        .iter(world)
    {
        let SearchLabel::Row(slot @ (1 | 5 | 10)) = label else {
            continue;
        };
        let bounds = Rect::from_center_size(transform.affine().translation, node.size());
        let mut bright = 0;
        for y in bounds.min.y.max(0.0) as u32..(bounds.max.y.ceil() as u32).min(image.height()) {
            for x in bounds.min.x.max(0.0) as u32..(bounds.max.x.ceil() as u32).min(image.width()) {
                let pixel = image.get_pixel(x, y).0;
                bright += usize::from(pixel[0] > 100 && pixel[1] > 100 && pixel[2] > 100);
            }
        }
        assert!(
            bright > 32,
            "{filename} 的引用行没有可见文字：第{slot}行 · {bounds:?}"
        );
    }
}

fn assert_reference_text_visible(app: &mut App) {
    let world = app.world_mut();
    let (entity, _, text, node) = world
        .query::<(Entity, &SearchLabel, &Text, &ComputedNode)>()
        .iter(world)
        .find(|(_, label, _, _)| matches!(label, SearchLabel::Row(1)))
        .unwrap();
    assert!(text.0.contains("/presets/0/model_path"));
    assert!(node.size().x > 100.0, "引用文字宽度不足：{:?}", node.size());
    assert!(node.size().y > 0.0, "引用文字高度不足：{:?}", node.size());
    assert!(
        !world
            .get::<bevy::text::TextLayoutInfo>(entity)
            .unwrap()
            .glyphs
            .is_empty(),
        "引用文字没有可绘制字形"
    );
    assert!(
        world.get::<InheritedVisibility>(entity).unwrap().get(),
        "引用文字继承了隐藏状态"
    );
    let position = world
        .get::<UiGlobalTransform>(entity)
        .unwrap()
        .affine()
        .translation;
    if let Some(clip) = world.get::<bevy::ui::CalculatedClip>(entity) {
        assert!(
            clip.contains_point(position),
            "引用文字位于裁剪区外：{position:?} · {:?}",
            clip
        );
    }
    assert!(
        world.get::<TextColor>(entity).unwrap().0.to_linear().alpha > 0.5,
        "引用文字透明"
    );
}

pub(in super::super::super) fn gpu_assert_references(
    app: &mut App,
    window: Entity,
    camera: Entity,
    target: Handle<Image>,
    width: u32,
    snapshot: fn(&mut App, Handle<Image>, &str, u32),
) {
    external_files::install_probe(app);
    let source_entity = gpu::source(app);
    let original = app
        .world()
        .resource::<DocumentState>()
        .current()
        .unwrap()
        .source
        .clone();
    let previous = app
        .world()
        .resource::<crate::resources::MapEditorState>()
        .selected_asset
        .clone();
    let row = {
        let world = app.world_mut();
        world
            .query::<(Entity, &crate::components::MapEditorAssetRow)>()
            .iter(world)
            .find(|(_, row)| row.entry_index == Some(1))
            .unwrap()
            .0
    };
    menu_click(app, window, camera, row, MouseButton::Right);
    assert!(
        app.world()
            .resource::<asset_context_menu::AssetContextMenuState>()
            .owns_mouse()
    );
    snapshot(
        app,
        target.clone(),
        "asset-browser-references-menu.png",
        width,
    );
    let button = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<asset_context_menu::AssetMenuReferencesButton>>()
            .single(world)
            .unwrap()
    };
    menu_click(app, window, camera, button, MouseButton::Left);
    gpu::settle(app);
    let (ticket, index, expected, line, column) = {
        let state = app.world().resource::<AssetSearchState>();
        assert_eq!(
            state.reference_target.as_deref(),
            Some("gltf/中文 空格.glb")
        );
        let result = state.current().unwrap();
        assert!(result.references);
        assert_eq!((result.hits, result.files.len()), (25, 2));
        let index = result
            .rows
            .iter()
            .position(|row| matches!(row, ResultRow::Hit(..)))
            .unwrap();
        let ResultRow::Hit(file, hit) = result.rows[index] else {
            unreachable!()
        };
        let hit = &result.files[file].hits[hit];
        assert_eq!(hit.snippet, "/presets/0/model_path");
        (state.ticket, index, hit.range.clone(), hit.line, hit.column)
    };
    assert_eq!(
        app.world()
            .resource::<crate::resources::MapEditorState>()
            .selected_asset,
        previous
    );
    assert_eq!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .source
            .as_ref(),
        original.as_ref()
    );
    {
        let world = app.world_mut();
        assert_eq!(
            world
                .query_filtered::<Entity, With<SearchRow>>()
                .iter(world)
                .count(),
            ROW_SLOTS
        );
        assert!(
            world
                .query_filtered::<&Node, With<SearchTextControls>>()
                .iter(world)
                .all(|node| node.display == Display::None)
        );
        assert!(
            world
                .query_filtered::<&Node, With<SearchReferenceControls>>()
                .iter(world)
                .all(|node| node.display == Display::Flex)
        );
    }
    gpu::assert_result_height(app, super::super::constants::VIEW_HEIGHT);
    snapshot(
        app,
        target.clone(),
        "asset-browser-references-results.png",
        width,
    );
    assert_reference_pixels(app, "asset-browser-references-results.png");

    app.insert_resource(ExternalEditors::for_tests(&[ExternalEditor::Zed]));
    assert_reference_text_visible(app);
    gpu::frames(app, 3);
    gpu::assert_external_buttons(app);
    for in_project in [false, true] {
        let before = app.world().resource::<external_files::ExternalRequestProbe>().0.len();
        gpu::click(app, window, camera, SearchAction::External { ticket, index, editor: ExternalEditor::Zed, in_project });
        let probe = app.world().resource::<external_files::ExternalRequestProbe>();
        assert_eq!(probe.0.len(), before + 1);
        assert_eq!(probe.0.last().unwrap().path, "config/模型预设.json");
        assert_eq!(probe.0.last().unwrap().action, if in_project {
            external_files::ExternalFileAction::ZedProject(Some((line, column)))
        } else { external_files::ExternalFileAction::Zed(Some((line, column))) });
    }
    assert_eq!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .source
            .as_ref(),
        original.as_ref()
    );
    app.insert_resource(ExternalEditors::default());
    gpu::frames(app, 3);
    {
        let world = app.world_mut();
        assert!(
            world
                .query_filtered::<&Node, With<SearchExternalRow>>()
                .iter(world)
                .all(|node| node.display == Display::None)
        );
    }

    assert_reference_text_visible(app);
    // 选择同一引用行必须落在精确JSON原文范围；只新增一个中央标签，原文件仍保持。
    snapshot(
        app,
        target.clone(),
        "asset-browser-references-without-ide.png",
        width,
    );
    assert_reference_pixels(app, "asset-browser-references-without-ide.png");
    gpu::click(app, window, camera, SearchAction::Row { ticket, index });
    gpu::settle(app);
    let documents = app.world().resource::<DocumentState>();
    assert_eq!(documents.current().unwrap().path, "config/模型预设.json");
    assert!(documents.in_center());
    assert_eq!(documents.tab_count(), 2);
    assert!(!documents.current().unwrap().dirty());
    let selection = app
        .world()
        .get::<EditableText>(source_entity)
        .unwrap()
        .editor
        .raw_selection()
        .text_range();
    assert_eq!(selection, expected);
    assert_eq!(
        &documents.current().unwrap().source[selection],
        "\"gltf/中文 空格.glb#Mesh0/Primitive0\""
    );
    assert_eq!(gpu::source(app), source_entity);
    assert_reference_text_visible(app);
    snapshot(
        app,
        target.clone(),
        "asset-browser-references-center-location.png",
        width,
    );
    assert_reference_pixels(app, "asset-browser-references-center-location.png");
    gpu::click(app, window, camera, SearchAction::Submit);
    gpu::settle(app);
    assert_eq!(
        app.world()
            .resource::<AssetSearchState>()
            .current()
            .unwrap()
            .hits,
        25
    );
    gpu::chord(app, window, KeyCode::KeyF, true);
    assert!(
        app.world()
            .resource::<AssetSearchState>()
            .reference_target
            .is_none()
    );
    assert_eq!(
        app.world()
            .resource::<DocumentState>()
            .documents
            .iter()
            .find(|doc| doc.path == "基础文件.rs")
            .unwrap()
            .source
            .as_ref(),
        original.as_ref()
    );
    // 源码结果沿用同一行池，但点击只请求IDE；无IDE时仍显示引用并给出定位提示。
    let root = app.world().resource::<DocumentState>().root.clone();
    let ticket = {
        let mut state = app.world_mut().resource_mut::<AssetSearchState>();
        state.ticket = state.ticket.wrapping_add(1);
        state.open = true;
        state.reference_target = Some("gltf/中文 空格.glb".into());
        state.reset_scroll = true;
        state.hint = "1处直接引用 · 1个文件 · 点击定位".into();
        state.result = Some(std::sync::Arc::new(SearchResult {
            ticket: state.ticket, references: true, root, hits: 1,
            files: vec![FileHits { path: "crates/game/src/player.rs".into(), identity: ContentIdentity::ProjectSource([7; 32]), dirty: false,
                hits: vec![Hit { range: 10..22, line: 28, column: 34, snippet: "model_path: String::from(‹\"gltf/中文 空格.glb\"›)".into() }] }],
            rows: vec![ResultRow::File(0), ResultRow::Hit(0, 0)], ..default()
        }));
        state.ticket
    };
    gpu::frames(app, 4);
    gpu::assert_result_height(app, super::super::constants::ROW_HEIGHT * 2.0);
    snapshot(app, target.clone(), "asset-browser-references-source-without-ide.png", width);
    let tabs = app.world().resource::<DocumentState>().tab_count();
    let requests = app.world().resource::<external_files::ExternalRequestProbe>().0.len();
    gpu::click(app, window, camera, SearchAction::Row { ticket, index: 1 });
    assert!(app.world().resource::<AssetSearchState>().hint.contains("未检测到"));
    assert_eq!(app.world().resource::<external_files::ExternalRequestProbe>().0.len(), requests);
    app.insert_resource(ExternalEditors::for_tests(&[ExternalEditor::Zed]));
    gpu::frames(app, 4);
    snapshot(app, target.clone(), "asset-browser-references-source-results.png", width);
    gpu::click(app, window, camera, SearchAction::Row { ticket, index: 1 });
    let probe = app.world().resource::<external_files::ExternalRequestProbe>();
    assert_eq!(probe.0.len(), requests + 1);
    assert_eq!(probe.0.last().unwrap().path, "crates/game/src/player.rs");
    assert_eq!(probe.0.last().unwrap().action, external_files::ExternalFileAction::ProjectSource {
        editor: ExternalEditor::Zed, line: 28, column: 34, digest: [7; 32], in_project: true,
    });
    assert_eq!(app.world().resource::<DocumentState>().tab_count(), tabs);
    assert_eq!(gpu::source(app), source_entity);
    // 同时安装三种IDE时，正文默认项目定位；两个显式按钮分别验证单文件与项目定位。
    app.insert_resource(ExternalEditors::for_tests(&ExternalEditor::ALL));
    gpu::frames(app, 4);
    gpu::assert_external_buttons(app);
    for ide in ExternalEditor::ALL {
        let before = app.world().resource::<external_files::ExternalRequestProbe>().0.len();
        gpu::click(app, window, camera, SearchAction::ChooseEditor(ide));
        assert_eq!(app.world().resource::<AssetSearchState>().preferred_editor, Some(ide));
        assert_eq!(app.world().resource::<AssetSearchState>().ticket, ticket);
        assert_eq!(app.world().resource::<external_files::ExternalRequestProbe>().0.len(), before);
        for (action, in_project) in [
            (SearchAction::Row { ticket, index: 1 }, true),
            (SearchAction::External { ticket, index: 1, editor: ide, in_project: false }, false),
            (SearchAction::External { ticket, index: 1, editor: ide, in_project: true }, true),
        ] {
            gpu::click(app, window, camera, action);
            let probe = app.world().resource::<external_files::ExternalRequestProbe>();
            assert_eq!(probe.0.last().unwrap().action, external_files::ExternalFileAction::ProjectSource {
                editor: ide, line: 28, column: 34, digest: [7; 32], in_project,
            });
        }
        assert_eq!(app.world().resource::<external_files::ExternalRequestProbe>().0.len(), before + 3);
        assert_eq!(app.world().resource::<DocumentState>().tab_count(), tabs);
        assert_eq!(gpu::source(app), source_entity);
    }
    snapshot(app, target, "asset-browser-references-editor-choice.png", width);
    app.insert_resource(ExternalEditors::for_tests(&[ExternalEditor::Zed]));
    gpu::frames(app, 4);
    {
        let world = app.world_mut();
        let shown: Vec<_> = world.query::<(&SearchEditorOption, &Node)>().iter(world)
            .filter_map(|(option, node)| (node.display == Display::Flex).then_some(option.0)).collect();
        assert_eq!(shown, vec![ExternalEditor::Zed]);
        assert_eq!(world.resource::<AssetSearchState>().selected_editor(world.resource::<ExternalEditors>()), Some(ExternalEditor::Zed));
    }
}
