//! 真实GPU鼠标验证缺失检查入口、虚拟结果、两种IDE意图与修复后的零结果收缩。

use super::super::{
    components::*, constants::*, gpu_tests as gpu, resources::AssetSearchState, types::ResultRow,
};
use crate::ui::{
    asset_preview::documents::resources::DocumentState,
    external_files::{self, ExternalEditor, ExternalEditors},
};
use bevy::prelude::*;

pub(in super::super::super) fn gpu_assert_missing_assets(
    app: &mut App,
    window: Entity,
    camera: Entity,
    target: Handle<Image>,
    width: u32,
    snapshot: fn(&mut App, Handle<Image>, &str, u32),
) {
    external_files::install_probe(app);
    let source = gpu::source(app);
    let active = app.world().resource::<DocumentState>().active.clone();
    assert!(!app.world().resource::<AssetSearchState>().open);
    gpu::click(app, window, camera, SearchAction::MissingAssets);
    gpu::settle(app);
    let (ticket, index, expected, line, column) = {
        let state = app.world().resource::<AssetSearchState>();
        assert!(state.open && state.missing_assets && state.reference_target.is_none());
        let result = state.current().unwrap();
        assert_eq!(
            (result.hits, result.files.len(), result.checked_paths),
            (25, 2, 25)
        );
        assert!(
            result.references && result.missing_assets && result.skipped == 0 && !result.limited
        );
        let index = result
            .rows
            .iter()
            .position(|row| matches!(row, ResultRow::Hit(..)))
            .unwrap();
        let ResultRow::Hit(file, hit) = result.rows[index] else {
            unreachable!()
        };
        let hit = &result.files[file].hits[hit];
        assert!(
            hit.snippet
                .contains("文件缺失 · gltf/中文 空格.glb#Mesh0/Primitive0")
        );
        (state.ticket, index, hit.range.clone(), hit.line, hit.column)
    };
    assert_eq!(app.world().resource::<DocumentState>().active, active);
    gpu::assert_result_height(app, VIEW_HEIGHT);
    {
        let world = app.world_mut();
        assert!(
            world
                .query_filtered::<&Node, With<SearchTextControls>>()
                .iter(world)
                .all(|node| node.display == Display::None)
        );
        assert_eq!(
            world
                .query_filtered::<Entity, With<SearchRow>>()
                .iter(world)
                .count(),
            ROW_SLOTS
        );
        let text = world
            .query::<(&SearchLabel, &Text, &bevy::text::TextLayoutInfo)>()
            .iter(world)
            .find(|(label, _, _)| matches!(label, SearchLabel::Row(1)))
            .unwrap();
        assert!(text.1.0.contains("文件缺失") && !text.2.glyphs.is_empty());
        assert!(
            world
                .query::<(&SearchLabel, &Text)>()
                .iter(world)
                .any(|(label, text)| matches!(label, SearchLabel::PathSubmit)
                    && text.0 == "重新检查")
        );
    }
    gpu::assert_external_buttons(app);
    snapshot(
        app,
        target.clone(),
        "asset-browser-missing-assets-results.png",
        width,
    );
    app.insert_resource(ExternalEditors::for_tests(&[ExternalEditor::Zed]));
    gpu::frames(app, 3);
    for in_project in [false, true] {
        let before = app
            .world()
            .resource::<external_files::ExternalRequestProbe>()
            .0
            .len();
        gpu::click(
            app,
            window,
            camera,
            SearchAction::External {
                ticket,
                index,
                editor: ExternalEditor::Zed,
                in_project,
            },
        );
        let probe = app
            .world()
            .resource::<external_files::ExternalRequestProbe>();
        assert_eq!(probe.0.len(), before + 1);
        assert_eq!(probe.0.last().unwrap().path, "config/模型预设.json");
        assert_eq!(
            probe.0.last().unwrap().action,
            if in_project {
                external_files::ExternalFileAction::ZedProject(Some((line, column)))
            } else {
                external_files::ExternalFileAction::Zed(Some((line, column)))
            }
        );
        assert_eq!(app.world().resource::<DocumentState>().active, active);
    }
    gpu::click(app, window, camera, SearchAction::Row { ticket, index });
    gpu::settle(app);
    let docs = app.world().resource::<DocumentState>();
    assert_eq!(docs.current().unwrap().path, "config/模型预设.json");
    assert!(docs.in_center() && !docs.current().unwrap().dirty());
    let selected = app
        .world()
        .get::<bevy::text::EditableText>(source)
        .unwrap()
        .editor
        .raw_selection()
        .text_range();
    assert_eq!(selected, expected);
    assert_eq!(
        &docs.current().unwrap().source[selected],
        "\"gltf/中文 空格.glb#Mesh0/Primitive0\""
    );
    assert_eq!(gpu::source(app), source);
    snapshot(
        app,
        target.clone(),
        "asset-browser-missing-assets-center.png",
        width,
    );
    // 恢复目标物理文件后重新检查，不要求保存或重新载入原配置；零问题不留空灰块。
    let root = app.world().resource::<DocumentState>().root.clone();
    std::fs::create_dir_all(root.join("gltf")).unwrap();
    std::fs::write(root.join("gltf/中文 空格.glb"), "只验证物理文件存在").unwrap();
    gpu::click(app, window, camera, SearchAction::Submit);
    gpu::settle(app);
    let state = app.world().resource::<AssetSearchState>();
    assert_eq!(
        (
            state.current().unwrap().hits,
            state.current().unwrap().checked_paths
        ),
        (0, 25)
    );
    assert!(state.hint.contains("0处路径问题") && !state.hint.contains("未完成"));
    gpu::assert_result_height(app, 0.0);
    {
        let world = app.world_mut();
        assert!(
            world
                .query_filtered::<&Node, With<SearchResults>>()
                .iter(world)
                .all(|node| node.display == Display::None)
        );
    }
    snapshot(
        app,
        target,
        "asset-browser-missing-assets-resolved.png",
        width,
    );
    gpu::chord(app, window, KeyCode::KeyF, true);
    assert!(!app.world().resource::<AssetSearchState>().path_query());
    assert_eq!(gpu::source(app), source);
}
