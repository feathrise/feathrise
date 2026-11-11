//! 真实离屏验证搜索键盘/中文候选、虚拟滚动、中央跳转、草稿与外部文件失效。

use feathrise_editor_ui::UiInteraction;
use super::super::{components::DocumentSource, resources::DocumentState};
use super::{components::*, constants::*, control, resources::AssetSearchState, types::ResultRow};
use bevy::{
    ecs::system::RunSystemOnce,
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
        mouse::{MouseScrollUnit, MouseWheel},
    },
    input_focus::{FocusCause, InputFocus},
    prelude::*,
    text::{EditableText, TextEdit},
};
use std::{path::Path, time::Duration};

pub(in super::super) fn gpu_assert_search(
    app: &mut App,
    window: Entity,
    camera: Entity,
    target: Handle<Image>,
    width: u32,
    root: &Path,
    snapshot: fn(&mut App, Handle<Image>, &str, u32),
) {
    crate::ui::external_files::install_probe(app);
    // 同一作者会话可以停放工程源码，但资产搜索不能将它当成第四份资产结果。
    let project = super::tests::Fixture::new();
    project.write("src/main.rs", "// 长剑源码\n");
    let mut code = project.doc("src/main.rs");
    code.path = "project://src/main.rs".into();
    code.edit("// 长剑源码草稿\n");
    app.world_mut().resource_mut::<DocumentState>().documents.push(code);
    let source_entity = source(app);
    chord(app, window, KeyCode::KeyF, true);
    assert!(app.world().resource::<AssetSearchState>().open);
    let query = field(app, SearchField::Query);
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(query));
    // 候选Enter只能确认中文，不能启动尚未确认的搜索。
    app.world_mut().write_message(bevy::window::Ime::Preedit {
        window,
        value: "长剑".into(),
        cursor: Some((6, 6)),
    });
    frames(app, 2);
    chord(app, window, KeyCode::Enter, false);
    assert!(app.world().resource::<AssetSearchState>().task.is_none());
    app.world_mut().write_message(bevy::window::Ime::Commit {
        window,
        value: "长剑".into(),
    });
    frames(app, 3);
    set_field(app, query, "长剑");
    let filter = field(app, SearchField::Filter);
    set_field(app, filter, "json, rs");
    chord(app, window, KeyCode::Enter, false);
    settle(app);
    assert_eq!(
        app.world()
            .resource::<AssetSearchState>()
            .current()
            .unwrap()
            .files
            .len(),
        3
    );
    let result = app.world().resource::<AssetSearchState>().result.clone().unwrap();
    let ticket = app.world().resource::<AssetSearchState>().ticket;
    app.world_mut().resource_mut::<DocumentState>().documents.iter_mut()
        .find(|doc| doc.path == "project://src/main.rs").unwrap().edit("// 长剑源码继续修改\n");
    frames(app, 3);
    let state = app.world().resource::<AssetSearchState>();
    assert_eq!(state.ticket, ticket, "停放的工程源码变更不能取消资产搜索");
    assert!(std::sync::Arc::ptr_eq(state.result.as_ref().unwrap(), &result));
    assert!(state.current().is_some());
    assert_eq!(std::fs::read_to_string(project.0.join("src/main.rs")).unwrap(), "// 长剑源码\n");
    assert_layout(app, query);
    let (ticket, index, path, line) = {
        let state = app.world().resource::<AssetSearchState>();
        let result = state.current().unwrap();
        let index = result.rows.iter().position(|row| matches!(row, ResultRow::Hit(..))).unwrap();
        let ResultRow::Hit(file, hit) = result.rows[index] else { unreachable!() };
        (state.ticket, index, result.files[file].path.clone(), result.files[file].hits[hit].line)
    };
    let previous = app.world().resource::<DocumentState>().active.clone();
    for in_project in [false, true] {
        let before = app.world().resource::<crate::ui::external_files::ExternalRequestProbe>().0.len();
        click(app, window, camera, SearchAction::External { ticket, index, editor: crate::ui::external_files::ExternalEditor::VsCode, in_project });
        let probe = app.world().resource::<crate::ui::external_files::ExternalRequestProbe>();
        assert_eq!(probe.0.len(), before + 1, "打开文件与打开工程的真实按钮各只发出一次请求");
        assert_eq!(probe.0.last().unwrap().path, path);
        assert_eq!(probe.0.last().unwrap().action, if in_project {
            crate::ui::external_files::ExternalFileAction::VsCodeProject(Some((line, 1)))
        } else { crate::ui::external_files::ExternalFileAction::VsCode(Some((line, 1))) });
        assert_eq!(app.world().resource::<DocumentState>().active, previous, "两个外部按钮都不能触发父行内置定位");
    }
    snapshot(
        app,
        target.clone(),
        "asset-browser-cross-search-results.png",
        width,
    );

    app.insert_resource(crate::ui::external_files::ExternalEditors::default());
    frames(app, 3);
    {
        let world = app.world_mut();
        assert!(world.query_filtered::<&Node, With<SearchExternalRow>>().iter(world).all(|node| node.display == Display::None));
        for (label, text) in world.query::<(&SearchLabel, &Text)>().iter(world) {
            if matches!(label, SearchLabel::Scope) { assert!(!text.0.contains("VS Code")); }
        }
    }
    snapshot(app, target.clone(), "asset-browser-cross-search-without-vscode.png", width);
    app.insert_resource(crate::ui::external_files::ExternalEditors::for_tests(&crate::ui::external_files::ExternalEditor::ALL));
    frames(app, 3); // 重新安装可见性不改变结果票据、正文选择或查询。

    app.world_mut()
        .resource_mut::<crate::resources::EditorAssetCatalog>()
        .current_folder = "config".into();
    click(app, window, camera, SearchAction::CurrentFolder);
    click(app, window, camera, SearchAction::WholeWord);
    click(app, window, camera, SearchAction::CaseSensitive);
    click(app, window, camera, SearchAction::Submit);
    settle(app);
    {
        let state = app.world().resource::<AssetSearchState>();
        assert_eq!(state.current().unwrap().files.len(), 1);
        assert!(state.whole_word && !state.case_sensitive && state.current_folder);
    }
    click(app, window, camera, SearchAction::AllAssets);
    click(app, window, camera, SearchAction::Submit);
    settle(app);
    let (ticket, index, expected) = {
        let state = app.world().resource::<AssetSearchState>();
        let result = state.current().unwrap();
        let index = result.rows.iter().position(|row| matches!(row, ResultRow::Hit(file, hit) if result.files[*file].path == "scripts/剑.rs" && *hit == 12)).unwrap();
        let ResultRow::Hit(file, hit) = result.rows[index] else {
            unreachable!()
        };
        (
            state.ticket,
            index,
            result.files[file].hits[hit].range.clone(),
        )
    };
    let view = result_view(app);
    let height = app.world().get::<ComputedNode>(view).unwrap().size().y;
    let outer = {
        let world = app.world_mut();
        // 工程页已把资产来源放进独立滚动正文，外层导航页不再兼任这个滚动实体。
        world
            .query_filtered::<(Entity, &crate::ui::project_browser::ProjectBrowserContent), With<crate::components::SceneEditorLeftPanelScroll>>()
            .iter(world)
            .find(|(entity, content)| content.0 == crate::ui::project_browser::ProjectBrowserView::Assets
                && world.get::<Node>(*entity).unwrap().display != Display::None)
            .map(|(entity, _)| entity)
            .unwrap()
    };
    let outer_before = app.world().get::<ScrollPosition>(outer).unwrap().y;
    app.world_mut()
        .get_mut::<bevy::ui::RelativeCursorPosition>(view)
        .unwrap()
        .cursor_over = true;
    app.world_mut().write_message(MouseWheel {
        unit: MouseScrollUnit::Pixel,
        x: 0.0,
        y: -(index as f32 * ROW_HEIGHT - height * 0.25),
        window,
        phase: bevy::input::touch::TouchPhase::Moved,
    });
    app.world_mut().run_system_once(control::scroll).unwrap();
    assert!(app.world().resource::<AssetSearchState>().owns_wheel());
    app.world_mut()
        .init_resource::<crate::resources::SceneBrowserDragState>();
    app.world_mut()
        .run_system_once(crate::ui::systems::scroll_editor_side_panels)
        .unwrap();
    assert_eq!(
        app.world().get::<ScrollPosition>(outer).unwrap().y,
        outer_before
    );
    frames(app, 3);
    assert!(app.world().get::<ScrollPosition>(view).unwrap().y > 100.0);
    // 实际鼠标命中重用行槽，仍应导航它的新文件/匹配身份。
    click(app, window, camera, SearchAction::Row { ticket, index });
    settle(app);
    {
        let documents = app.world().resource::<DocumentState>();
        assert_eq!(documents.current().unwrap().path, "scripts/剑.rs");
        assert_eq!(documents.tab_count(), 2);
        assert!(documents.in_center());
        assert!(!documents.current().unwrap().dirty());
        let selection = app
            .world()
            .get::<EditableText>(source_entity)
            .unwrap()
            .editor
            .raw_selection()
            .text_range();
        assert_eq!(selection, expected);
        assert_eq!(&documents.current().unwrap().source[selection], "长剑");
        assert_eq!(
            app.world().resource::<InputFocus>().get(),
            Some(source_entity)
        );
        let layout = app
            .world()
            .get::<bevy::text::TextLayoutInfo>(source_entity)
            .unwrap();
        assert!(
            !layout.selection_rects.is_empty(),
            "真实排版必须显示定位选区"
        );
        let scroll = app
            .world()
            .get::<bevy::text::EditableText>(source_entity)
            .unwrap()
            .viewport.offset;
        let height = app
            .world()
            .get::<ComputedNode>(source_entity)
            .unwrap()
            .content_box()
            .height();
        assert!(
            layout
                .selection_rects
                .iter()
                .all(|rect| rect.min.y >= scroll.y && rect.max.y <= scroll.y + height),
            "定位选区必须处于正文可见范围"
        );
    }
    assert_eq!(source(app), source_entity);
    snapshot(
        app,
        target.clone(),
        "asset-browser-cross-search-center-location.png",
        width,
    );

    let disk_code = std::fs::read(root.join("scripts/剑.rs")).unwrap();
    app.world_mut()
        .get_mut::<EditableText>(source_entity)
        .unwrap()
        .queue_edit(TextEdit::Insert("草稿长剑".into()));
    frames(app, 4);
    assert!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .dirty()
    );
    assert!(
        app.world()
            .resource::<AssetSearchState>()
            .current()
            .is_none()
    );
    chord(app, window, KeyCode::KeyF, true);
    click(app, window, camera, SearchAction::WholeWord);
    set_field(app, query, "草稿");
    assert_result_height(app, 0.0); // 条件变化使旧结果失效时，也不保留灰色占位。
    chord(app, window, KeyCode::Enter, false);
    settle(app);
    assert_eq!(
        app.world()
            .resource::<AssetSearchState>()
            .current()
            .unwrap()
            .files
            .len(),
        1
    );
    assert!(
        app.world()
            .resource::<AssetSearchState>()
            .current()
            .unwrap()
            .files[0]
            .dirty
    );
    let ticket = app.world().resource::<AssetSearchState>().ticket;
    assert_result_height(app, ROW_HEIGHT * 2.0); // 一条命中加文件标题，仅需两行；从大量结果返回仍须收缩。
    click(app, window, camera, SearchAction::Row { ticket, index: 1 });
    settle(app);
    assert_eq!(
        std::fs::read(root.join("scripts/剑.rs")).unwrap(),
        disk_code
    );
    assert!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .dirty()
    );
    snapshot(
        app,
        target.clone(),
        "asset-browser-cross-search-draft.png",
        width,
    );

    chord(app, window, KeyCode::KeyF, true);
    set_field(app, query, "长剑");
    chord(app, window, KeyCode::Enter, false);
    settle(app);
    let (ticket, index) = {
        let state = app.world().resource::<AssetSearchState>();
        let result = state.current().unwrap();
        (state.ticket, result.rows.iter().position(|row| matches!(row, ResultRow::Hit(file, _) if result.files[*file].path == "config/武器.json")).unwrap())
    };
    std::fs::write(
        root.join("config/武器.json"),
        "{\"新字段\":\"外部改动后的长剑\"}",
    )
    .unwrap();
    app.world_mut().get_mut::<ScrollPosition>(view).unwrap().y = 0.0;
    frames(app, 2);
    click(app, window, camera, SearchAction::Row { ticket, index });
    settle(app);
    assert_eq!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .source_text(),
        "{\"新字段\":\"外部改动后的长剑\"}"
    );
    assert!(
        app.world()
            .resource::<AssetSearchState>()
            .hint
            .contains("已变化")
    );
    assert_eq!(source(app), source_entity);
    snapshot(
        app,
        target,
        "asset-browser-cross-search-stale-file.png",
        width,
    );
}

pub(super) fn frames(app: &mut App, n: usize) {
    for _ in 0..n {
        app.update();
        std::thread::sleep(Duration::from_millis(16));
    }
}
pub(super) fn settle(app: &mut App) {
    for _ in 0..200 {
        frames(app, 1);
        let state = app.world().resource::<AssetSearchState>();
        let documents = app.world().resource::<DocumentState>();
        if state.task.is_none()
            && state.pending.is_none()
            && !state.reference_requested
            && state.jump.is_none()
            && documents.io.is_none()
            && documents.pending_load.is_none()
        {
            frames(app, 3);
            return;
        }
    }
    panic!("跨文件搜索或中央定位未完成");
}
pub(super) fn field(app: &mut App, kind: SearchField) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &SearchField)>()
        .iter(world)
        .find(|(_, field)| **field == kind)
        .unwrap()
        .0
}
pub(super) fn source(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<DocumentSource>>()
        .single(world)
        .unwrap()
}
fn result_view(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<SearchResults>>()
        .single(world)
        .unwrap()
}
pub(super) fn set_field(app: &mut App, entity: Entity, value: &str) {
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(entity, FocusCause::Navigated);
    let mut text = app.world_mut().get_mut::<EditableText>(entity).unwrap();
    text.queue_edit(TextEdit::SelectAll);
    text.queue_edit(TextEdit::Insert(value.into()));
    frames(app, 3);
}
fn assert_layout(app: &mut App, query: Entity) {
    assert_result_height(app, VIEW_HEIGHT);
    assert_external_buttons(app);
    assert!(app.world().get::<ComputedNode>(query).unwrap().size().x > 50.0);
    let world = app.world_mut();
    assert_eq!(
        world
            .query_filtered::<Entity, With<SearchRow>>()
            .iter(world)
            .count(),
        ROW_SLOTS
    );
    for (_, node, transform) in world
        .query::<(&SearchAction, &ComputedNode, &UiGlobalTransform)>()
        .iter(world)
        .filter(|(action, _, _)| {
            !matches!(action, SearchAction::Row { .. } | SearchAction::Inactive)
        })
        .filter(|(_, node, _)| node.size().y > 0.0)
    {
        assert!(node.size().y > 0.0);
        assert!(transform.affine().translation.x >= 0.0);
    }
}
pub(super) fn assert_result_height(app: &mut App, expected: f32) {
    let view = result_view(app);
    let node = app.world().get::<ComputedNode>(view).unwrap();
    let actual = node.size().y * node.inverse_scale_factor();
    assert!((actual - expected).abs() < 0.1, "结果区实际布局高度应为{expected}，得到{actual}");
}
pub(super) fn assert_external_buttons(app: &mut App) {
    let world = app.world_mut();
    assert_eq!(world.query_filtered::<Entity, With<SearchExternalRow>>().iter(world).count(), ROW_SLOTS * 2);
    let buttons: Vec<_> = world.query::<(&SearchExternalRow, &Node, &ComputedNode, &UiGlobalTransform, &ChildOf)>()
        .iter(world)
        .filter(|(_, node, computed, _, _)| node.display == Display::Flex && computed.size().y > 0.0)
        .map(|(slot, _, node, transform, parent)| (slot.0, slot.1, Rect::from_center_size(transform.affine().translation, node.size()), parent.parent()))
        .collect();
    let clips: Vec<_> = world.query::<(&SearchLabel, &bevy::ui::CalculatedClip, &ComputedNode, &UiGlobalTransform)>().iter(world)
        .filter_map(|(label, clip, node, transform)| {
            let SearchLabel::Row(slot) = label else { return None; };
            let bounds = Rect::from_center_size(transform.affine().translation, node.size());
            feathrise_editor_ui::clipped_ui_bounds(clip, bounds).map(|bounds| (*slot, bounds.max.x))
        })
        .collect();
    assert!(!buttons.is_empty());
    for (slot, in_project, bounds, parent) in &buttons {
        let row = Rect::from_center_size(world.get::<UiGlobalTransform>(*parent).unwrap().affine().translation,
            world.get::<ComputedNode>(*parent).unwrap().size());
        assert!(bounds.min.x >= row.min.x && bounds.max.x <= row.max.x, "双入口不能超出窄侧栏行：{bounds:?} / {row:?}");
        let other = buttons.iter().find(|(other_slot, other_mode, _, _)| other_slot == slot && other_mode != in_project).unwrap().2;
        assert!(if *in_project { other.max.x < bounds.min.x } else { bounds.max.x < other.min.x }, "两个按钮必须分开显示并可分别点击");
        if !in_project {
            let (_, text_right) = clips.iter().find(|(text_slot, _)| text_slot == slot).expect("片段必须继承独立裁剪区域");
            assert!(*text_right <= bounds.min.x, "长片段裁剪必须停在第一个按钮之前");
        }
    }
}
pub(super) fn click(app: &mut App, window: Entity, camera: Entity, action: SearchAction) {
    let (entity, position) = {
        let world = app.world_mut();
        world
            .query::<(Entity, &SearchAction, &UiGlobalTransform, &ComputedNode)>()
            .iter(world)
            .find(|(_, candidate, _, node)| **candidate == action && node.size().y > 0.0)
            .map(|(entity, _, transform, _)| (entity, transform.affine().translation))
            .unwrap()
    };
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
        .press(MouseButton::Left);
    app.world_mut()
        .run_system_cached(bevy::ui::ui_focus_system)
        .unwrap();
    app.world_mut().run_system_once(feathrise_editor_ui::update_editor_button_clicks).unwrap();
    assert_eq!(
        *app.world().get::<UiInteraction>(entity).unwrap(),
        UiInteraction::Pressed,
        "真实鼠标未命中搜索行或按钮：{action:?}"
    );
    app.world_mut().run_system_once(control::input).unwrap();
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.world_mut()
        .run_system_cached(bevy::ui::ui_focus_system)
        .unwrap();
    app.world_mut().run_system_once(feathrise_editor_ui::update_editor_button_clicks).unwrap();
    app.world_mut().run_system_once(super::control::input).unwrap();
    *app.world_mut()
        .get_mut::<bevy::camera::RenderTarget>(camera)
        .unwrap() = target;
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    frames(app, 2);
}
pub(super) fn chord(app: &mut App, window: Entity, code: KeyCode, ctrl_shift: bool) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release_all();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    let mut keys = Vec::new();
    if ctrl_shift {
        keys.push((KeyCode::ControlLeft, Key::Control));
        keys.push((KeyCode::ShiftLeft, Key::Shift));
    }
    keys.push((
        code,
        if code == KeyCode::Enter {
            Key::Enter
        } else if code == KeyCode::KeyH {
            Key::Character("h".into())
        } else {
            Key::Character("f".into())
        },
    ));
    for (code, key) in &keys {
        app.world_mut().write_message(KeyboardInput {
            key_code: *code,
            logical_key: key.clone(),
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window,
        });
    }
    frames(app, 1);
    for (code, key) in keys {
        app.world_mut().write_message(KeyboardInput {
            key_code: code,
            logical_key: key,
            state: ButtonState::Released,
            text: None,
            repeat: false,
            window,
        });
    }
    frames(app, 2);
}
