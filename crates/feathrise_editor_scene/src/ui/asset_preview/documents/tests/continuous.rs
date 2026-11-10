//! 连续文档的完整读写、原字节保护及正文构造预算回归；只使用临时文件。

use super::*;
use crate::ui::asset_preview::documents::{render, resources::PreviewRenderState};

#[test]
fn asset_continuous_text_edits_beyond_old_page_boundaries_for_every_format() {
    let fixture = Fixture::new();
    let source = format!(
        "{}文件末尾",
        "中文、缩进与符号\n\t{{\"value\": 1}}\n".repeat(1_000)
    );
    for path in [
        "说明.md",
        "原文.txt",
        "数据.json",
        "配置.toml",
        "配置.yaml",
        "代码.rs",
        "着色器.wgsl",
    ] {
        std::fs::write(fixture.root.join(path), &source).unwrap();
        let mut doc = fixture.document_at(path);
        assert_eq!(doc.source_text(), source);
        assert!(doc.source_text().chars().count() > 8192);
        let key = doc.source_key();
        let changed = format!("开头新增\n{}末尾新增", doc.source_text());
        doc.edit(&changed);
        assert_eq!(doc.source_key(), key, "滚动编辑不能变更输入绑定");
        assert_eq!(doc.source_text(), changed);
        io::save(&fixture.root, path, &doc.disk, &doc.pages).unwrap();
        assert_eq!(
            std::fs::read_to_string(fixture.root.join(path)).unwrap(),
            changed
        );
    }
}

#[test]
fn asset_continuous_edit_preserves_mixed_newlines_and_unicode_around_replacement() {
    let fixture = Fixture::new();
    let original = "\u{feff}标题\r\n甲乙丙\n保持这行\r\n最后一行\n";
    fixture.write(original);
    let mut doc = fixture.document();
    let changed = doc.source_text().replace("甲乙丙", "甲中文丙");
    doc.edit(&changed);
    assert_eq!(doc.source_text(), changed);
    assert_eq!(
        doc.pages.concat(),
        original
            .trim_start_matches('\u{feff}')
            .replace("甲乙丙", "甲中文丙")
    );
    let changed = doc
        .source_text()
        .replace("保持这行\n", "保持这行\n插入一行\n");
    doc.edit(&changed);
    io::save(&fixture.root, &doc.path, &doc.disk, &doc.pages).unwrap();
    assert_eq!(
        std::fs::read_to_string(fixture.root.join(&doc.path)).unwrap(),
        "\u{feff}标题\r\n甲中文丙\n保持这行\r\n插入一行\r\n最后一行\n"
    );
}

#[test]
fn asset_continuous_insert_uses_dominant_newlines_and_preserves_existing_bytes() {
    for (original, newline) in [
        ("\u{feff}首行\n第二行\r\n第三行\r\n", "\r\n"),
        ("\u{feff}首行\r\n第二行\n第三行\n", "\n"),
        ("首行\r\n第二行\n", "\r\n"),
        ("首行\n第二行\r\n", "\n"),
        ("只有一行", "\n"),
    ] {
        let fixture = Fixture::new();
        fixture.write(original);
        let mut app = app(&fixture);
        let source = app.world().resource::<DocumentState>().current().unwrap().source_text().to_owned();
        {
            let mut state = app.world_mut().resource_mut::<DocumentState>();
            state.current_mut().unwrap().edit(&format!("{source}新增行\n"));
            assert_eq!(std::fs::read_to_string(fixture.root.join("说明.md")).unwrap(), original);
            state.request_save();
        }
        settle(&mut app);
        let expected = format!("{original}新增行{newline}");
        assert_eq!(std::fs::read_to_string(fixture.root.join("说明.md")).unwrap(), expected);
        assert!(!app.world().resource::<DocumentState>().current().unwrap().dirty());
        let reopened = fixture.document();
        assert_eq!(reopened.source_text(), expected.trim_start_matches('\u{feff}').replace("\r\n", "\n"));
        assert_eq!(reopened.pages.concat(), expected.trim_start_matches('\u{feff}'));
    }
}

#[test]
fn asset_continuous_preview_builds_with_a_fixed_budget_and_retires_old_tree() {
    let fixture = Fixture::new();
    let source = format!("{}最后一行", "连续正文\n".repeat(5_000));
    fixture.write(&source);
    let mut app = app(&fixture);
    app.add_plugins(AssetPlugin::default())
        .init_asset::<Font>()
        .init_resource::<PreviewRenderState>()
        .add_systems(Update, render::sync);
    let body = app
        .world_mut()
        .spawn((DocumentPreviewBody, Node::default()))
        .id();
    {
        let mut state = app.world_mut().resource_mut::<DocumentState>();
        state.visible = true;
        let doc = state.current_mut().unwrap();
        doc.format = DocumentFormat::Config;
        doc.mode = ViewMode::Preview;
        let parsed = parser::parse(doc.format, &doc.pages);
        doc.preview = parsed.pages;
        doc.preview_revision = Some(doc.revision);
    }
    for _ in 0..300 {
        let before = app.world().entities().len();
        app.update();
        assert!(
            app.world().entities().len().saturating_sub(before)
                <= (PREVIEW_BUILD_BLOCKS * (BLOCK_SPANS + 1) + 2) as u32
        );
        if app
            .world()
            .resource::<PreviewRenderState>()
            .active
            .is_some()
        {
            break;
        }
    }
    let first = app
        .world()
        .resource::<PreviewRenderState>()
        .active
        .as_ref()
        .unwrap()
        .root;
    let text = app.world().get::<Children>(first).unwrap()[0];
    let rendered: String = app
        .world()
        .get::<Children>(text)
        .unwrap()
        .iter()
        .map(|span| app.world().get::<TextSpan>(span).unwrap().as_str())
        .collect();
    assert_eq!(rendered, source, "整篇预览不能漏掉原来的页边界之后的内容");
    {
        let mut state = app.world_mut().resource_mut::<DocumentState>();
        let doc = state.current_mut().unwrap();
        doc.edit("替换后的完整正文");
        doc.preview = parser::parse(doc.format, &doc.pages).pages;
        doc.preview_revision = Some(doc.revision);
    }
    for _ in 0..300 {
        let before = app.world().entities().len();
        app.update();
        assert!(before.saturating_sub(app.world().entities().len()) <= PREVIEW_RETIRE_NODES as u32);
        let rendering = app.world().resource::<PreviewRenderState>();
        if rendering.retiring.is_none()
            && rendering
                .active
                .as_ref()
                .is_some_and(|active| active.root != first)
        {
            break;
        }
    }
    assert!(
        app.world().get_entity(first).is_err(),
        "旧正文必须完整退役，不能永久占据构造槽"
    );
    assert_eq!(app.world().get::<Children>(body).unwrap().len(), 1);
    let count = app.world().entities().len();
    app.update();
    assert_eq!(app.world().entities().len(), count, "稳定正文不重建");
}

#[test]
fn asset_continuous_preview_cancels_stale_build_and_restores_recreated_ui() {
    let fixture = Fixture::new();
    fixture.write("初始完整正文");
    let mut app = app(&fixture);
    app.add_plugins(AssetPlugin::default())
        .init_asset::<Font>()
        .init_resource::<PreviewRenderState>()
        .add_systems(Update, render::sync);
    let body = app
        .world_mut()
        .spawn((DocumentPreviewBody, Node::default()))
        .id();
    let set_preview = |app: &mut App, value: &str| {
        let mut state = app.world_mut().resource_mut::<DocumentState>();
        state.visible = true;
        let doc = state.current_mut().unwrap();
        doc.format = DocumentFormat::Config;
        doc.mode = ViewMode::Preview;
        doc.edit(value);
        doc.preview = parser::parse(doc.format, &doc.pages).pages;
        doc.preview_revision = Some(doc.revision);
    };
    set_preview(&mut app, "初始完整正文");
    app.update();
    let first = app
        .world()
        .resource::<PreviewRenderState>()
        .active
        .as_ref()
        .unwrap()
        .root;
    set_preview(&mut app, &"较长的过期正文\n".repeat(5_000));
    app.update();
    let stale = app
        .world()
        .resource::<PreviewRenderState>()
        .building
        .as_ref()
        .unwrap()
        .root;
    assert_eq!(
        app.world().get::<Visibility>(stale),
        Some(&Visibility::Hidden)
    );
    assert_eq!(
        app.world().get::<Node>(stale).unwrap().display,
        Display::None
    );
    // 多次切换内容时只保留最新目标，半成品不能显示，也不能占出第四个树槽。
    for index in 0..4 {
        set_preview(&mut app, &format!("最新完整正文{index}"));
        app.update();
        assert!(app.world().get::<Children>(body).unwrap().len() <= 3);
    }
    for _ in 0..300 {
        app.update();
        let rendering = app.world().resource::<PreviewRenderState>();
        if rendering.building.is_none()
            && rendering.retiring.is_none()
            && rendering.active.as_ref().is_some_and(|active| {
                active.identity.revision
                    == app
                        .world()
                        .resource::<DocumentState>()
                        .current()
                        .unwrap()
                        .revision
            })
        {
            break;
        }
    }
    assert!(app.world().get_entity(first).is_err());
    assert!(app.world().get_entity(stale).is_err());
    let active = app
        .world()
        .resource::<PreviewRenderState>()
        .active
        .as_ref()
        .unwrap()
        .root;
    let text = app.world().get::<Children>(active).unwrap()[0];
    let spans = app.world().get::<Children>(text).unwrap();
    let rendered: String = spans
        .iter()
        .map(|span| app.world().get::<TextSpan>(span).unwrap().as_str())
        .collect();
    assert_eq!(rendered, "最新完整正文3");
    // 宿主重建仅删UI，不丢业务草稿；正文必须重新生成，不能命中已失效的实体缓存。
    app.world_mut().entity_mut(active).despawn();
    app.update();
    let restored = app
        .world()
        .resource::<PreviewRenderState>()
        .active
        .as_ref()
        .unwrap()
        .root;
    assert_ne!(restored, active);
    assert_eq!(
        app.world().get::<Children>(body).unwrap().as_ref(),
        &[restored]
    );
}
