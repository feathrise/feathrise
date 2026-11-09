//! 文档真实文件事务、连续全文、格式分流、输入隔离和迟到结果回归；不写正式资产。

use feathrise_editor_ui::UiInteraction;
use super::{components::*, constants::*, io, parser, resources::DocumentState, session, types::*};
use crate::asset_catalog::{DocumentFormat, document_format};
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, IoTaskPool},
    text::EditableText,
};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

static COUNTER: AtomicU64 = AtomicU64::new(0);
mod workspace; // 中央放大/还原与完整草稿和输入隔离回归。
mod continuous; // 超过旧页边界的完整输入、混合换行及有界正文构造/退役。
mod history_save; // 真实保存回执、历史保存点与按未保存状态批量关闭。
mod close_validation; // 普通关闭夹具通过真实文档保存队列处理冲突和重试。
mod themes; // 三套正式主题的GPU绘制、设置模态与原生草稿保留。
mod external; // 外部修改提示、真实菜单命中与单一原生输入刷新。
mod reload; // 真实重载期间的输入冻结、失败恢复及保存/核对/其他会话的可编辑边界。
struct Fixture {
    root: PathBuf,
    parent: PathBuf,
    owner: String,
}
impl Fixture {
    fn new() -> Self {
        let parent = std::env::temp_dir().canonicalize().unwrap();
        let owner = format!(
            "rpg-markdown-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let root = parent.join(&owner);
        std::fs::create_dir(&root).unwrap(); // 独占创建失败即停止，不能接管旧验收留下的目录。
        let root = root.canonicalize().unwrap();
        std::fs::write(root.join(".feathrise-test-owner"), &owner).unwrap();
        Self { root, parent, owner }
    }
    fn write(&self, value: impl AsRef<[u8]>) {
        std::fs::write(self.root.join("说明.md"), value).unwrap();
    }
    fn document(&self) -> Document {
        self.document_at("说明.md")
    }
    fn document_at(&self, path: &str) -> Document {
        let (disk, pages) = io::load(&self.root, path).unwrap();
        Document {
            path: path.into(),
            format: document_format(path).unwrap(),
            generation: 1,
            disk,
            pages: pages.pages,
            source: pages.text,
            revision: 0,
            saved_revision: 0,
            history: default(),
            preview: Arc::new(Vec::new()),
            preview_revision: None,
            preview_clipped: false,
            mode: ViewMode::Edit,
            preview_scroll: Vec2::ZERO,
            confirm_reload: false,
            external: default(),
            status: String::new(),
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // 递归清理前复核绝对路径、临时父根与本轮标记；目录被替换或改归属时保留现场。
        if self.root.is_absolute()
            && self.root.parent() == Some(self.parent.as_path())
            && self.root.canonicalize().ok().as_ref() == Some(&self.root)
            && std::fs::read_to_string(self.root.join(".feathrise-test-owner")).ok().as_deref() == Some(self.owner.as_str())
        {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

fn app(fixture: &Fixture) -> App {
    let mut app = App::new();
    app.insert_resource(feathrise_editor_documents::content_operations());
    app.add_plugins(MinimalPlugins)
        .insert_resource(DocumentState {
            root: fixture.root.clone(),
            active: Some("说明.md".into()),
            documents: vec![fixture.document()],
            ..default()
        })
        .add_systems(Update, session::drive_tasks);
    app
}

fn settle(app: &mut App) {
    for _ in 0..1000 {
        app.update();
        let state = app.world().resource::<DocumentState>();
        if state.io.is_none() && state.parser.is_none() && state.pending_load.is_none() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("后台文档任务未在1秒内完成");
}

fn preview_text(preview: &ParsedPreview) -> String {
    preview
        .pages
        .iter()
        .flat_map(|page| &page.blocks)
        .flat_map(|block| &block.spans)
        .map(|span| span.text.as_str())
        .collect()
}

#[test]
fn asset_text_formats_keep_literal_content_bom_crlf_and_explicit_save() {
    let fixture = Fixture::new();
    let original = "\u{feff}# 原始中文与标点\r\n\t**不要排版**\r\n<script>literal</script>\r\n";
    for path in [
        "说明.TXT",
        "记录.log",
        "表格.CSV",
        "配置.JSON",
        "配置.toml",
        "配置.yaml",
        "材质.mtl",
        "着色器.WGSL",
        "工具.PY",
        "对白.yarn",
        "README",
        ".editorconfig",
    ] {
        std::fs::write(fixture.root.join(path), original).unwrap();
        let mut doc = fixture.document_at(path);
        let preview = parser::parse(doc.format, &doc.pages);
        assert_eq!(
            preview_text(&preview),
            original.trim_start_matches('\u{feff}'),
            "{path}"
        );
        assert!(!preview.clipped);
        assert!(!doc.dirty());
        assert_eq!(
            std::fs::read_to_string(fixture.root.join(path)).unwrap(),
            original
        );
        doc.edit("# 已编辑中文\n\t**仍按原文保存**\n");
        assert!(doc.dirty());
        io::save(&fixture.root, path, &doc.disk, &doc.pages).unwrap();
        assert_eq!(
            std::fs::read_to_string(fixture.root.join(path)).unwrap(),
            "\u{feff}# 已编辑中文\r\n\t**仍按原文保存**\r\n",
            "{path}"
        );
    }
}

#[test]
fn asset_literal_preview_preserves_page_continuity_and_has_independent_display_limit() {
    let source = format!(
        "{}尾部保留",
        "# 中文\n\tlet a = \"**原文**\";\r\n".repeat(2_000)
    );
    let pages = io::source_pages(&source);
    for format in [
        DocumentFormat::Text,
        DocumentFormat::Code,
        DocumentFormat::Config,
    ] {
        let preview = parser::parse(format, &pages);
        assert_eq!(preview_text(&preview), source);
        assert!(preview.pages.iter().all(
            |page| page.chars <= PREVIEW_PAGE_CHARS && page.blocks.len() <= PREVIEW_PAGE_BLOCKS
        ));
        assert!(
            preview
                .pages
                .iter()
                .flat_map(|page| &page.blocks)
                .all(|block| block.kind == BlockKind::Paragraph
                    && block.chars <= BLOCK_CHARS
                    && block
                        .spans
                        .iter()
                        .all(|span| span.style == InlineStyle::default()))
        );
    }
    let large = "文".repeat(MAX_PREVIEW_CHARS + 1);
    let pages = io::source_pages(&large);
    let preview = parser::parse(DocumentFormat::Code, &pages);
    assert!(preview.clipped);
    assert_eq!(preview_text(&preview).chars().count(), MAX_PREVIEW_CHARS);
    assert_eq!(pages.iter().map(AsRef::as_ref).collect::<String>(), large);
}

#[test]
fn asset_document_selection_uses_new_format_and_restores_previous_draft() {
    use crate::{
        resources::MapEditorState,
        ui::sidebars::{EditorRightSidebarTab, MapEditorSidebarState},
    };
    let fixture = Fixture::new();
    fixture.write("# Markdown标题\n");
    std::fs::write(
        fixture.root.join("设置.json"),
        "{\"名称\":\"# 原文 **原文**\"}\n",
    )
    .unwrap();
    let mut app = app(&fixture);
    app.world_mut()
        .resource_mut::<DocumentState>()
        .current_mut()
        .unwrap()
        .edit("# 未保存的草稿\n");
    let mut sidebar = MapEditorSidebarState::default();
    sidebar.right = EditorRightSidebarTab::Assets;
    app.insert_resource(sidebar)
        .insert_resource(MapEditorState {
            open: true,
            selected_asset: Some("设置.json".into()),
            ..default()
        })
        .insert_resource(super::super::types::AssetPreviewState {
            selected_path: Some("设置.json".into()),
            content: super::super::types::AssetPreviewContent::Document,
            ..default()
        })
        .add_systems(Update, session::sync_selection.before(session::drive_tasks));
    settle(&mut app);
    let doc = app.world().resource::<DocumentState>().current().unwrap();
    assert_eq!(doc.format, DocumentFormat::Config);
    assert_eq!(doc.preview_revision, Some(0));
    assert_eq!(
        preview_text(&ParsedPreview {
            pages: doc.preview.clone(),
            clipped: doc.preview_clipped
        }),
        "{\"名称\":\"# 原文 **原文**\"}\n"
    );
    app.world_mut()
        .resource_mut::<super::super::types::AssetPreviewState>()
        .selected_path = Some("说明.md".into());
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .selected_asset = Some("说明.md".into());
    settle(&mut app);
    let state = app.world().resource::<DocumentState>();
    let doc = state.current().unwrap();
    assert_eq!(doc.format, DocumentFormat::Markdown);
    assert!(doc.dirty());
    assert_eq!(doc.source_text(), "# 未保存的草稿\n");
    assert_eq!(state.documents.len(), 2);
}

#[test]
fn asset_document_continuous_edit_keeps_bom_crlf_and_unmodified_tail() {
    let fixture = Fixture::new();
    let original = format!(
        "\u{feff}# 说明\r\n{}末尾不能丢失",
        "中文行\r\n".repeat(2500)
    );
    fixture.write(original.as_bytes());
    let mut doc = fixture.document();
    assert!(doc.pages.len() > 2);
    let original_tail = original
        .trim_start_matches('\u{feff}')
        .trim_start_matches("# 说明\r\n");
    let complete = doc
        .source_text()
        .replacen("# 说明\n", "# 编辑后的标题\n\n新增内容\n", 1);
    doc.edit(&complete);
    io::save(&fixture.root, &doc.path, &doc.disk, &doc.pages).unwrap();
    let saved = std::fs::read_to_string(fixture.root.join(&doc.path)).unwrap();
    assert_eq!(
        saved,
        format!("\u{feff}# 编辑后的标题\r\n\r\n新增内容\r\n{original_tail}")
    );
}

#[test]
fn markdown_pages_preserve_unicode_and_do_not_split_crlf_or_cross_page_fences() {
    let text = format!(
        "{}\r\n```rust\r\n{}\r\n```\r\n[说明][doc]\r\n\r\n[doc]: relative.md",
        "甲".repeat(4095),
        "let 值 = 1;\r\n".repeat(700)
    );
    let pages = io::source_pages(&text);
    assert_eq!(pages.iter().map(AsRef::as_ref).collect::<String>(), text);
    assert!(
        pages
            .iter()
            .all(|page| page.chars().count() <= SOURCE_PAGE_CHARS)
    );
    assert!(
        !pages
            .windows(2)
            .any(|pair| pair[0].ends_with('\r') && pair[1].starts_with('\n'))
    );
    let preview = parser::parse(DocumentFormat::Markdown, &pages).pages;
    assert!(
        preview
            .iter()
            .flat_map(|page| &page.blocks)
            .any(|block| block.kind == BlockKind::Code)
    );
    assert!(
        preview
            .iter()
            .flat_map(|page| &page.blocks)
            .flat_map(|block| &block.spans)
            .any(|span| span.text.contains("relative.md"))
    );
}

#[test]
fn markdown_commonmark_preview_has_headings_lists_emphasis_and_inert_html() {
    let pages = io::source_pages(
        "# 标题\n\n**粗体**和*斜体*，`代码`。\n\n- 列表\n- [x] 完成\n\n> 引用\n\n<script>alert('原样')</script>\n\n![图](image.png)",
    );
    let preview = parser::parse(DocumentFormat::Markdown, &pages).pages;
    let blocks: Vec<_> = preview.iter().flat_map(|page| &page.blocks).collect();
    assert_eq!(blocks[0].kind, BlockKind::Heading(1));
    assert!(blocks.iter().any(|block| block.kind == BlockKind::Quote));
    let spans: Vec<_> = blocks.iter().flat_map(|block| &block.spans).collect();
    assert!(
        spans
            .iter()
            .any(|span| span.style.bold && span.text == "粗体")
    );
    assert!(
        spans
            .iter()
            .any(|span| span.style.italic && span.text == "斜体")
    );
    let text: String = spans.iter().map(|span| span.text.as_str()).collect();
    for expected in ["• 列表", "[完成]", "<script>", "[图片：图 · image.png]"] {
        assert!(text.contains(expected), "{expected}: {text}");
    }
}

#[test]
fn markdown_dense_preview_obeys_block_span_and_page_budgets_without_losing_text() {
    let input = "**粗**细".repeat(10_000);
    let preview = parser::parse(DocumentFormat::Markdown, &io::source_pages(&input)).pages;
    assert!(
        preview.iter().all(
            |page| page.blocks.len() <= PREVIEW_PAGE_BLOCKS && page.chars <= PREVIEW_PAGE_CHARS
        )
    );
    assert!(
        preview
            .iter()
            .flat_map(|page| &page.blocks)
            .all(|block| block.chars <= BLOCK_CHARS && block.spans.len() <= BLOCK_SPANS)
    );
    let text: String = preview
        .iter()
        .flat_map(|page| &page.blocks)
        .flat_map(|block| &block.spans)
        .map(|span| span.text.as_str())
        .collect();
    assert_eq!(text, "粗细".repeat(10_000));
}

#[test]
fn markdown_derived_preview_limits_dense_styles_and_long_reference_destinations() {
    let pages = io::source_pages(&"**粗**细".repeat(40_000));
    let parsed = parser::parse(DocumentFormat::Markdown, &pages);
    assert!(parsed.clipped);
    let blocks: Vec<_> = parsed.pages.iter().flat_map(|page| &page.blocks).collect();
    assert_eq!(
        blocks.iter().map(|block| block.spans.len()).sum::<usize>(),
        MAX_PREVIEW_SPANS
    );
    assert!(blocks.iter().all(|block| {
        block.chars
            == block
                .spans
                .iter()
                .map(|span| span.text.chars().count())
                .sum::<usize>()
    }));
    let input = format!(
        "{}\n\n[doc]: https://example.com/{}",
        "[说明][doc] ".repeat(5000),
        "x".repeat(4000)
    );
    let pages = io::source_pages(&input);
    let parsed = parser::parse(DocumentFormat::Markdown, &pages);
    let rendered: String = parsed
        .pages
        .iter()
        .flat_map(|page| &page.blocks)
        .flat_map(|block| &block.spans)
        .map(|span| span.text.as_str())
        .collect();
    assert!(rendered.contains("https://example.com/"));
    assert!(rendered.contains('…')); // CommonMark解析器自身也限制引用展开，长地址在UI独立缩短。
    assert!(!rendered.contains(&"x".repeat(4000)));
    assert!(parsed.pages.iter().map(|page| page.chars).sum::<usize>() <= MAX_PREVIEW_CHARS);
    assert_eq!(pages.iter().map(AsRef::as_ref).collect::<String>(), input); // 格式预览限流不能截断源文档。
    let parsed = parser::parse(
        DocumentFormat::Markdown,
        &io::source_pages(&"文".repeat(MAX_PREVIEW_CHARS + 7)),
    );
    assert!(parsed.clipped);
    assert_eq!(
        parsed.pages.iter().map(|page| page.chars).sum::<usize>(),
        MAX_PREVIEW_CHARS
    );
}

#[test]
fn markdown_old_io_keeps_slot_until_finished_and_only_latest_request_is_loaded() {
    use std::sync::atomic::AtomicBool;
    let fixture = Fixture::new();
    fixture.write("原文");
    std::fs::write(fixture.root.join("最新.md"), "最新文档").unwrap();
    let mut app = app(&fixture);
    let released = Arc::new(AtomicBool::new(false));
    let worker = released.clone();
    {
        let mut state = app.world_mut().resource_mut::<DocumentState>();
        state.io = Some(IoTaskPool::get().spawn(async move {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while !worker.load(Ordering::Acquire) && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            IoResult::Loaded(
                LoadRequest {
                    path: "说明.md".into(),
                    ticket: 0,
                    expected_document: None,
                },
                Err("过期任务".into()),
            )
        }));
        state.request_load("被合并.md".into());
        state.request_load("最新.md".into());
        state.active = Some("最新.md".into());
    }
    app.update();
    assert!(app.world().resource::<DocumentState>().io.is_some());
    assert_eq!(
        app.world()
            .resource::<DocumentState>()
            .pending_load
            .as_ref()
            .unwrap()
            .path,
        "最新.md"
    );
    released.store(true, Ordering::Release);
    settle(&mut app);
    assert_eq!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .source_text(),
        "最新文档"
    );
    assert!(app.world().resource::<DocumentState>().error.is_empty());
}

#[test]
fn markdown_save_rejects_same_length_external_edit_and_keeps_both_versions() {
    let fixture = Fixture::new();
    fixture.write("原始文本");
    let mut doc = fixture.document();
    doc.edit("自己的草稿");
    fixture.write("外部文本"); // 字节长度相同也必须检查摘要。
    let error = io::save(&fixture.root, &doc.path, &doc.disk, &doc.pages).unwrap_err();
    assert!(error.contains("外部修改"));
    assert_eq!(
        std::fs::read_to_string(fixture.root.join(&doc.path)).unwrap(),
        "外部文本"
    );
    assert_eq!(doc.source_text(), "自己的草稿");
    // 归属元数据属于夹具本身；实际文档以外仍不能留下stage/backup。
    assert_eq!(std::fs::read_dir(&fixture.root).unwrap()
        .filter(|entry| entry.as_ref().unwrap().file_name() != ".feathrise-test-owner").count(), 1);
}

#[test]
fn markdown_rejects_invalid_encoding_binary_large_files_and_escape_paths() {
    let fixture = Fixture::new();
    for bytes in [vec![0xff], b"a\0b".to_vec(), vec![b'a'; MAX_FILE_BYTES + 1]] {
        fixture.write(bytes);
        assert!(io::load(&fixture.root, "说明.md").is_err());
    }
    fixture.write("有效");
    for path in ["../说明.md", "/说明.md", "C:/说明.md", "说明.bin"] {
        assert!(io::load(&fixture.root, path).is_err());
    }
    assert_eq!(
        document_format("folder/README.MD"),
        Some(DocumentFormat::Markdown)
    );
    assert_eq!(
        document_format("notes.MarkDown"),
        Some(DocumentFormat::Markdown)
    );
    assert_eq!(document_format("notes.md.png"), None);
}

#[test]
fn markdown_readonly_and_deleted_target_preserve_draft_and_disk() {
    let fixture = Fixture::new();
    fixture.write("原文");
    let mut doc = fixture.document();
    doc.edit("草稿");
    let path = fixture.root.join(&doc.path);
    let mut permissions = path.metadata().unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&path, permissions).unwrap();
    let error = io::save(&fixture.root, &doc.path, &doc.disk, &doc.pages).unwrap_err();
    let mut permissions = path.metadata().unwrap().permissions();
    permissions.set_readonly(false);
    std::fs::set_permissions(&path, permissions).unwrap();
    assert!(error.contains("只读"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "原文");
    std::fs::remove_file(path).unwrap();
    assert!(io::save(&fixture.root, &doc.path, &doc.disk, &doc.pages).is_err());
    assert_eq!(doc.source_text(), "草稿");
}

#[test]
fn markdown_capture_committed_last_character_before_save_and_ignores_old_binding() {
    let fixture = Fixture::new();
    fixture.write("中文");
    let mut app = app(&fixture);
    app.add_systems(Update, session::capture_edits.before(session::drive_tasks));
    let key = app
        .world()
        .resource::<DocumentState>()
        .current()
        .unwrap()
        .source_key();
    app.world_mut().spawn((
        DocumentSource { key: Some(key) },
        EditableText::new("中文最后一个字"),
    ));
    app.update();
    assert_eq!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .source_text(),
        "中文最后一个字"
    );
    app.world_mut()
        .resource_mut::<DocumentState>()
        .request_save();
    settle(&mut app);
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("说明.md")).unwrap(),
        "中文最后一个字"
    );
    app.world_mut()
        .resource_mut::<DocumentState>()
        .current_mut()
        .unwrap()
        .generation += 1;
    let world = app.world_mut();
    let mut query = world.query::<&mut EditableText>();
    query
        .single_mut(world)
        .unwrap()
        .editor
        .set_text("旧实体不能覆盖");
    app.update();
    assert_eq!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .source_text(),
        "中文最后一个字"
    );
}

#[test]
fn markdown_late_load_and_parse_results_cannot_replace_current_document() {
    let fixture = Fixture::new();
    fixture.write("原文");
    let mut app = app(&fixture);
    let result = io::load(&fixture.root, "说明.md");
    {
        let mut state = app.world_mut().resource_mut::<DocumentState>();
        state.load_ticket = 2;
        state.io = Some(IoTaskPool::get().spawn(async move {
            IoResult::Loaded(
                LoadRequest {
                    path: "说明.md".into(),
                    ticket: 1,
                    expected_document: None,
                },
                result,
            )
        }));
        state.current_mut().unwrap().edit("当前新草稿");
        state.parser = Some(AsyncComputeTaskPool::get().spawn(async {
            PreviewResult {
                path: "说明.md".into(),
                generation: 1,
                revision: 0,
                pages: parser::parse(DocumentFormat::Markdown, &io::source_pages("旧预览")).pages,
                clipped: false,
            }
        }));
    }
    settle(&mut app);
    let doc = app.world().resource::<DocumentState>().current().unwrap();
    assert_eq!(doc.source_text(), "当前新草稿");
    assert_eq!(doc.preview_revision, None);
}

#[test]
fn markdown_save_receipt_keeps_edits_after_submitted_revision_dirty() {
    let fixture = Fixture::new();
    fixture.write("原文");
    let mut app = app(&fixture);
    app.world_mut()
        .resource_mut::<DocumentState>()
        .current_mut()
        .unwrap()
        .edit("提交时的内容");
    app.world_mut()
        .resource_mut::<DocumentState>()
        .request_save();
    app.world_mut()
        .resource_mut::<DocumentState>()
        .current_mut()
        .unwrap()
        .edit("之后的内容");
    settle(&mut app);
    let doc = app.world().resource::<DocumentState>().current().unwrap();
    assert!(doc.dirty());
    assert_eq!(doc.source_text(), "之后的内容");
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("说明.md")).unwrap(),
        "提交时的内容"
    );
    app.world_mut()
        .resource_mut::<DocumentState>()
        .request_save();
    settle(&mut app);
    assert!(
        !app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .dirty()
    );
}

#[test]
fn markdown_all_dirty_sessions_survive_close_and_capacity_refuses_new_document() {
    let fixture = Fixture::new();
    fixture.write("原文");
    let mut state = DocumentState::default();
    for index in 0..MAX_DOCUMENTS {
        let mut doc = fixture.document();
        doc.path = format!("文档{index}.md");
        doc.edit("保留草稿");
        state.documents.push(doc);
    }
    state.active = None; // F12关闭只清活动选择，不清文档草稿。
    state.request_load("新文档.md".into());
    assert_eq!(state.documents.len(), MAX_DOCUMENTS);
    assert!(state.pending_load.is_none());
    assert!(state.error.contains("8份"));
    assert!(
        state
            .documents
            .iter()
            .all(|doc| doc.dirty() && doc.source_text() == "保留草稿")
    );
    state.documents[2].saved_revision = state.documents[2].revision;
    state.request_load("新文档.md".into());
    assert_eq!(state.documents.len(), MAX_DOCUMENTS - 1);
    assert_eq!(state.pending_load.as_ref().unwrap().path, "新文档.md");
}

#[test]
fn markdown_reload_requires_discard_confirmation_and_ime_blocks_save() {
    use crate::resources::{EditorTextInputGuard, MapEditorNumericScrubMouseGuard, MapEditorState};
    use crate::ui::sidebars::{EditorRightSidebarTab, MapEditorSidebarState};
    let fixture = Fixture::new();
    fixture.write("原文");
    let mut state = DocumentState {
        active: Some("说明.md".into()),
        documents: vec![fixture.document()],
        ..default()
    };
    state.current_mut().unwrap().edit("草稿");
    let mut app = App::new();
    app.insert_resource(feathrise_editor_documents::content_operations());
    let mut sidebar = MapEditorSidebarState::default();
    sidebar.right = EditorRightSidebarTab::Assets;
    app.insert_resource(state)
        .insert_resource(MapEditorState {
            open: true,
            selected_asset: Some("说明.md".into()),
            ..default()
        })
        .insert_resource(sidebar)
        .init_resource::<EditorTextInputGuard>()
        .init_resource::<MapEditorNumericScrubMouseGuard>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<bevy::input_focus::InputFocus>()
        .init_resource::<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>()
        .add_systems(Update, super::interaction::actions);
    let button = app
        .world_mut()
        .spawn((feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed, DocumentAction::Reload))
        .id();
    app.update();
    assert!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .confirm_reload
    );
    assert!(
        app.world()
            .resource::<DocumentState>()
            .pending_load
            .is_none()
    );
    *app.world_mut().get_mut::<UiInteraction>(button).unwrap() = UiInteraction::None;
    app.update();
    *app.world_mut().get_mut::<UiInteraction>(button).unwrap() = UiInteraction::Pressed;
    app.world_mut().entity_mut(button).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    app.update();
    assert!(
        app.world()
            .resource::<DocumentState>()
            .pending_load
            .is_some()
    );
    assert_eq!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .source_text(),
        "草稿"
    ); // 读取完整成功前不丢原草稿。
    app.world_mut().resource_mut::<DocumentState>().pending_load = None;
    app.world_mut()
        .resource_mut::<EditorTextInputGuard>()
        .block_shortcuts = true;
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ControlLeft);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyS);
    app.update();
    assert!(app.world().resource::<DocumentState>().io.is_none());
    assert!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .status
            .contains("中文输入")
    );
}

#[test]
fn markdown_reload_keeps_old_native_input_unbound_until_real_read_is_adopted() {
    use bevy::{ecs::system::RunSystemOnce, input_focus::InputFocus,
        text::{FontCx, LayoutCx}};
    use crate::resources::{EditorTextInputGuard, MapEditorNumericScrubMouseGuard, MapEditorState};
    use crate::ui::sidebars::{EditorRightSidebarTab, MapEditorSidebarState};

    let fixture = Fixture::new();
    fixture.write("载入时的磁盘原文\n");
    let mut document = fixture.document();
    document.edit("确认丢弃前的旧草稿\n");
    let original_key = document.source_key();
    let mut sidebar = MapEditorSidebarState::default();
    sidebar.right = EditorRightSidebarTab::Assets;
    let mut font = FontCx::default();
    let embedded = Font::from_bytes(feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES.to_vec());
    font.context.collection.register_fonts(embedded.data, None);
    font.set_sans_serif_family("Noto Sans CJK SC").unwrap();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(feathrise_editor_documents::content_operations())
        .insert_resource(font).init_resource::<LayoutCx>()
        .init_resource::<bevy::clipboard::Clipboard>()
        .init_resource::<InputFocus>().init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<EditorTextInputGuard>().init_resource::<MapEditorNumericScrubMouseGuard>()
        .init_resource::<super::resources::DocumentInputs>()
        .init_resource::<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>()
        .insert_resource(MapEditorState { open: true, selected_asset: Some("说明.md".into()), ..default() })
        .insert_resource(sidebar)
        .insert_resource(DocumentState {
            root: fixture.root.clone(), active: Some("说明.md".into()), visible: true,
            load_ticket: original_key.generation, documents: vec![document], ..default()
        })
        .add_systems(Update, (bevy::text::apply_text_edits, session::capture_edits, super::native::bind).chain());
    let source = app.world_mut().spawn((DocumentSource::default(), EditableText::default(),
ComputedNode::default())).id();
    app.update();
    app.update(); // 官方队列采用原生绑定的TextStart，后续原文件动作不被初始化队列拦截。
    assert_eq!(app.world().get::<DocumentSource>(source).unwrap().key.as_ref(), Some(&original_key));
    let button = app.world_mut().spawn((DocumentAction::Reload,
        feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed)).id();
    app.world_mut().run_system_once(super::interaction::actions).unwrap();
    assert!(app.world().resource::<DocumentState>().current().unwrap().confirm_reload);
    assert!(app.world().resource::<DocumentState>().pending_load.is_none());
    fixture.write("本次重新载入的真实磁盘版本\n");
    app.world_mut().entity_mut(button).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    app.world_mut().run_system_once(super::interaction::actions).unwrap();
    assert!(app.world().resource::<DocumentState>().pending_load.is_some());
    app.world_mut().run_system_once(session::drive_tasks).unwrap();
    assert!(app.world().resource::<DocumentState>().io.is_some());
    app.update();
    assert!(app.world().get::<DocumentSource>(source).unwrap().key.is_none());

    // 原IO已启动真实受检读取；只暂缓主线程轮询，确定性保留结果尚未采用的输入窗口。
    // 不伪造Pending/Loaded，不阻塞共享IO池；后续必须在固定期限内采用并核对真实磁盘正文。
    app.world_mut().entity_mut(button).insert((DocumentAction::Edit,
        feathrise_editor_ui::EditorButtonClick::triggered()));
    app.world_mut().run_system_once(super::interaction::actions).unwrap();
    app.update();
    let rebound = app.world().get::<DocumentSource>(source).unwrap().key.clone();
    let waiting_mode = app.world().resource::<DocumentState>().current().unwrap().mode;
    let started = std::time::Instant::now();
    loop {
        app.world_mut().run_system_once(session::drive_tasks).unwrap();
        app.update();
        let state = app.world().resource::<DocumentState>();
        if state.io.is_none() && state.pending_load.is_none() && state.parser.is_none() { break; }
        assert!(started.elapsed() < std::time::Duration::from_secs(5), "真实重载未在5秒内结清");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let state = app.world().resource::<DocumentState>();
    assert_eq!(state.current().unwrap().source_text(), "本次重新载入的真实磁盘版本\n");
    assert_ne!(state.current().unwrap().source_key(), original_key);
    assert_eq!(std::fs::read_to_string(fixture.root.join("说明.md")).unwrap(), "本次重新载入的真实磁盘版本\n");
    assert_eq!(rebound, None, "重载回执采用前，原Edit动作不能重新绑定即将丢弃的旧正文");
    assert_eq!(waiting_mode, ViewMode::Preview);
}

/// 手动验收实际资产侧栏与官方中文输入；只保存临时文档，离屏渲染不会留下占用游戏exe的进程。
#[test]
#[ignore = "需要真实GPU；在feathrise执行cargo test -p feathrise_editor_map --lib gpu_markdown -- --ignored --test-threads=1"]
fn gpu_markdown_sidebar_preview_native_edit_and_ctrl_s_save() {
    let original = "# 地图资产说明\n\n这里是**中文文档预览**，可以直接在资产浏览器编辑。\n\n## 使用方式\n\n- 点击编辑修改文字\n- Ctrl+S保存完整文档\n- [x] 保留中文与换行\n\n> 切换资产后仍保留草稿。\n\n```rust\nlet 地图 = \"开封城郊\";\n```\n";
    gpu_document_sidebar(
        "说明.md",
        original,
        "\n新编辑的中文内容。\n",
        "markdown",
        false,
    );
}

#[test]
#[ignore = "需要真实GPU；在feathrise执行cargo test -p feathrise_editor_map --lib gpu_text -- --ignored --test-threads=1"]
fn gpu_text_sidebar_config_and_shader_literal_preview_native_edit_and_save() {
    gpu_document_sidebar(
        "设置.json",
        "{\n  \"name\": \"# 中文标题 **按原文显示**\",\n  \"notes\": \"<script>只显示文字</script>\",\n  \"enabled\": true,\n  \"scale\": 1.5\n}\n",
        "\n",
        "config",
        false,
    );
    gpu_document_sidebar(
        "预览.wgsl",
        "// 中文着色器说明\n// # 注释与 **符号** 保持原样\n\n@fragment\nfn fragment() -> @location(0) vec4<f32> {\n    return vec4<f32>(1.0, 0.8, 0.6, 1.0);\n}\n",
        "\n// 新编辑的中文内容。\n",
        "shader",
        false,
    );
}

#[test]
#[ignore = "需要真实GPU；使用正式中央工作区、侧栏和原生输入验证放大、标签切换及保存"]
fn gpu_asset_document_center_preview_edit_restore_and_hidden_sidebar_save() {
    gpu_document_sidebar(
        "说明.md",
        "# 剑与侠客\n\n在中央工作区 **预览和编辑文档**。\n\n- 中文列表\n- 同一份草稿\n\n```rust\nlet weapon = \"长剑\";\n```\n",
        "\n新编辑的中文内容。\n",
        "center",
        true,
    );
}

#[test]
#[ignore = "需要真实GPU；验证资产行双击、多个文件标签、原生选区/手动视区与独立草稿"]
fn gpu_asset_multiple_documents_double_click_switch_preserves_native_view() {
    let lines: String = (0..300).map(|index| format!("  \"条目{index:03}\": {index},\n")).collect();
    let original = format!("{{\n{lines}  \"末尾\": 0\n}}");
    gpu_document_sidebar("第一配置.json", &original, "\n", "multi-document", true);
}

#[test]
#[ignore = "需要真实GPU；验证长JSON的整篇滚动、末尾输入、完整保存和中央还原"]
fn gpu_asset_continuous_json_scroll_full_native_edit_and_save() {
    let rows: Vec<_> = (0..1_800)
        .map(|id| serde_json::json!({"id": id, "name": "中文记录", "value": id}))
        .collect();
    let original = serde_json::to_string_pretty(
        &serde_json::json!({"rows": rows, "tail": "文件末尾必须可编辑"}),
    )
    .unwrap();
    assert!(original.len() < MAX_FILE_BYTES && original.chars().count() > 8192);
    gpu_document_sidebar("长配置.json", &original, "\n", "continuous", true);
}

#[test]
#[ignore = "需要真实GPU；验证中文Rust代码在1080p中央小地图中的缩进、轮廓与原位还原"]
fn gpu_asset_document_minimap_code_outline() {
    let original: String = (0..16).map(|id| {
        let notes = "角色技能范围和冷却参数".repeat(id % 4 + 1);
        format!("// {notes}\nfn skill_{id}(level: u32) -> u32 {{\n    if level > {id} {{\n        level + {id}\n    }} else {{\n        0\n    }}\n}}\n\n")
    }).collect();
    gpu_document_sidebar("小地图导航.rs", &original, "\n// 中文编辑仍写回完整文档\n", "minimap-code", true);
}

#[test]
#[ignore = "需要真实GPU；验证窄侧栏中文查找、软换行行号、跳转和完整保存"]
fn gpu_asset_document_navigation_wrapped_sidebar() {
    let original = format!("// 中文行号\nlet story = \"{}\";\n\n// 最后一行\n", "角色绕过长廊与石桥，继续寻找线索。".repeat(64));
    gpu_document_sidebar("侧栏导航.rs", &original, "\n// 保存完整文本\n", "navigation-sidebar", false);
}

#[test]
#[ignore = "需要真实GPU；验证JSON/Rust/WGSL预览和编辑颜色、中文候选及原生选区"]
fn gpu_asset_syntax_json_rust_wgsl_preview_edit_ime_selection() {
    for (path, original, name) in [
        ("语法配置.json", "{\n  \"地图\": \"开封城郊\",\n  \"刷新秒数\": 30,\n  \"倍率\": 1.25,\n  \"启用\": true,\n  \"可选项\": null\n}\n", "syntax-json"),
        ("语法代码.rs", "// 角色技能的范围和冷却\nstruct Skill { radius: f32, cooldown: u32 }\n\nfn skill(level: u32) -> f32 {\n    let title = r#\"剑与侠客\"#;\n    if level > 3 { 12.5 } else { 8.0 }\n}\n", "syntax-rust"),
        ("语法着色器.wgsl", "// 铜木地表的着色示例\n@group(0) @binding(0) var source: texture_2d<f32>;\n\n@fragment\nfn fragment() -> @location(0) vec4<f32> {\n    let warm = vec3f(0.8, 0.6, 0.4);\n    return vec4f(warm, 1.0);\n}\n", "syntax-wgsl"),
    ] {
        gpu_document_sidebar(path, original, "\n", name, true);
    }
}

#[test]
#[ignore = "需要真实GPU；密集单行JSON整屏高亮、后台分词及高亮固定预算诊断"]
fn gpu_asset_syntax_dense_json_covers_entire_visible_screen() {
    // 6万字符足以填满1080p视区；24万字符单段会触发现有原生选区截短，另记为排版限制。
    let original = format!("[{}0]", "1,".repeat(30_000));
    gpu_document_sidebar("密集配置.json", &original, "\n", "syntax-dense", true);
}

#[test]
#[ignore = "需要真实GPU；验证中央与320像素侧栏JSON诊断、格式化、原生撤销及语法保存门"]
fn gpu_asset_json_diagnostics_format_undo_and_checked_save() {
    for (central, tag) in [(true, "json-tools-center"), (false, "json-tools-sidebar")] {
        gpu_document_sidebar("校验配置.json", "{\"中文\":\"预览\",\"数值\":1.2300e+04}", "\n", tag, central);
    }
}

#[test]
#[ignore = "需要真实GPU；验证中央/窄侧栏查找选项、中文候选、当前/全部替换、冻结选区与受检保存"]
fn gpu_asset_current_document_find_replace_center_and_sidebar() {
    let original = "{\n  \"weapon\": \"长剑\",\n  \"Weapon\": \"长剑\",\n  \"weapon_extra\": \"长剑\",\n  \"路径\": \"assets/a.json\",\n  \"repeat\": \"旧 旧 旧\"\n}\n";
    gpu_document_sidebar("替换测试.json", original, "\n", "replace-tools-center", true);
    gpu_document_sidebar("替换测试.json", original, "\n", "replace-tools-sidebar", false);
}

#[test]
#[ignore = "需要真实GPU；验证跨文件搜索、中文候选、虚拟结果滚动与只读中央定位"]
fn gpu_asset_cross_file_search_and_center_navigation() {
    gpu_document_sidebar("基础文件.rs", "fn main() {}\n", "\n", "cross-search", true);
}

#[test]
#[ignore = "需要真实GPU；验证引用菜单、字段结果、已安装IDE定位与中央原文选区"]
fn gpu_asset_references_menu_results_and_center_navigation() {
    gpu_document_sidebar("基础文件.rs", "fn main() {}\n", "\n", "references", true);
}

#[test]
#[ignore = "需要真实GPU与字体资源，单独运行离屏鼠标和中央定位回归"]
fn gpu_asset_missing_check_results_locations_and_resolved_shrink() {
    gpu_document_sidebar("基础文件.rs", "fn main() {}\n", "\n", "missing-assets", true);
}

#[test]
#[ignore = "需要真实GPU；验证文件/目录收藏菜单、折叠、定位、失效移除与紧凑列表"]
fn gpu_asset_favorites_recent_access_and_folder_navigation() {
    gpu_document_sidebar("基础文件.rs", "fn main() {}\n", "\n", "shortcuts", true);
}

#[test]
#[ignore = "需要真实GPU；验证中央跨文件替换差异、勾选、单处/单文件/整批应用与拒收"]
fn gpu_asset_cross_file_replacement_preview_and_atomic_drafts() {
    gpu_document_sidebar("基础文件.rs", "fn main() {}\n", "\n", "cross-replace", true);
}

#[test]
#[ignore = "需要真实GPU；验证保存本批/全部草稿、快捷键/菜单、失败回显及原生输入"]
fn gpu_asset_batch_save_and_failure_report() {
    gpu_document_sidebar("基础文件.rs", "fn main() {}\n", "\n", "batch-save", true);
}

#[test]
#[ignore = "需要真实GPU；验证三套配色、设置选择、文档/小地图绘制及切换时草稿保留"]
fn gpu_editor_color_themes_and_draft_preservation() {
    let original = format!("{{\n  \"名称\": \"庭院建筑\",\n  \"启用\": true,\n  \"部件\": [\n{}\n  ]\n}}",
        (0..35).map(|i| format!("    {{\"编号\": {i}, \"说明\": \"木柱与青砖\"}}{}", if i < 34 { "," } else { "" })).collect::<Vec<_>>().join("\n"));
    gpu_document_sidebar("建筑配色.json", &original, "\n", "themes", true);
}

#[test]
fn gpu_external_document_roundtrip_refreshes_clean_content_and_confirms_draft_reload() {
    gpu_document_sidebar("外部联动.rs", "// 外部编辑器联动\nfn main() {}\n", "\n", "external", true);
}

fn gpu_document_sidebar(
    path: &str,
    original: &str,
    addition: &str,
    screenshot_type: &str,
    central: bool,
) {
    use crate::{
        components::MapEditorPane,
        resources::{EditorAssetCatalog, MapEditorState},
        types::EditorAssetEntry,
        ui::sidebars::{EditorRightSidebarTab, MapEditorSidebarState},
    };
    use bevy::{
        asset::AssetPlugin,
        ecs::system::RunSystemOnce,
        render::{
            gpu_readback::{Readback, ReadbackComplete},
            render_resource::{TextureFormat, TextureUsages},
        },
        window::{ExitCondition, WindowPlugin},
        winit::WinitPlugin,
    };
    // 密集单行JSON虽字符多，软换行高度仍可能不到一万像素；长滚动入口另由多行JSON验证。
    let tall_document = original.chars().count() > 8192 && screenshot_type != "syntax-dense";
    // 搜索、路径查询及快捷入口均已随工程浏览迁至左栏；夹具须构造原导航树与来源显隐。
    let with_asset_navigation = matches!(screenshot_type,
        "cross-search" | "cross-replace" | "batch-save" | "references" | "missing-assets" | "shortcuts" | "multi-document");
    let fixture = Fixture::new();
    // 混合保存需要独立工程/资产受检根；两者仍只位于本轮独占夹具之内。
    let asset_root = if screenshot_type == "batch-save" {
        let assets = fixture.root.join("assets");
        std::fs::create_dir(&assets).unwrap();
        assets
    } else { fixture.root.clone() };
    std::fs::write(asset_root.join(path), original).unwrap();
    if matches!(screenshot_type, "cross-search" | "cross-replace" | "batch-save") {
        std::fs::create_dir_all(asset_root.join("config")).unwrap();
        std::fs::create_dir_all(asset_root.join("scripts")).unwrap();
        std::fs::write(asset_root.join("config/武器.json"), "{\n  \"名称\": \"长剑\",\n  \"说明\": \"长剑\"\n}\n").unwrap();
        std::fs::write(asset_root.join("另一个.JSON"), "{\"名称\":\"长剑\"}\n").unwrap();
        std::fs::write(asset_root.join("scripts/剑.rs"), "// 长剑 角色技能\n".repeat(40)).unwrap();
    }
    if matches!(screenshot_type, "references" | "missing-assets") {
        std::fs::create_dir_all(fixture.root.join("config")).unwrap();
        std::fs::create_dir_all(fixture.root.join("maps/HN-01")).unwrap();
        let json = serde_json::json!({"presets": (0..24).map(|i| serde_json::json!({"model_path": format!("gltf/中文 空格.glb#Mesh{i}/Primitive0")})).collect::<Vec<_>>()});
        std::fs::write(fixture.root.join("config/模型预设.json"), serde_json::to_string_pretty(&json).unwrap()).unwrap();
        std::fs::write(fixture.root.join("maps/HN-01/map.manifest.json"), "{\n  \"terrain\": {\"glb_path\": \"gltf/中文 空格.glb#Scene0\"}\n}\n").unwrap();
    }
    if screenshot_type == "multi-document" {
        std::fs::write(fixture.root.join("第二代码.rs"), "fn main() {\n    println!(\"第二文件\");\n}\n").unwrap();
    }
    let target_font = asset_root.join(crate::ui::scene::FONT_PATH);
    std::fs::create_dir_all(target_font.parent().unwrap()).unwrap();
    std::fs::write(target_font, feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES).unwrap();
    let mut app = App::new();
    app.insert_resource(feathrise_editor_documents::content_operations());
    let mut sidebar = MapEditorSidebarState::default();
    sidebar.right = EditorRightSidebarTab::Assets;
    if with_asset_navigation {
        sidebar.navigator = crate::ui::sidebars::EditorNavigatorTab::Assets;
    }
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .set(AssetPlugin {
                file_path: asset_root.to_string_lossy().into(),
                watch_for_changes_override: (screenshot_type.starts_with("json-tools-") || screenshot_type.starts_with("replace-tools-") || matches!(screenshot_type, "cross-search" | "cross-replace" | "batch-save" | "references" | "missing-assets" | "shortcuts")).then_some(false), // 工具验证检查实际文件字节，无需临时目录退出时的文件监视回调。
                ..default()
            })
            .disable::<WinitPlugin>()
            .disable::<bevy::log::LogPlugin>(), // 多个离屏App共用测试进程，不能重复注册全局日志器。
    )
    .init_resource::<feathrise_editor_core::EditorSettings>()
    .insert_resource(feathrise_editor_ui::EditorPalette::warm_copper()) // 既有截图夹具保留原版，主题夹具走正式设置桥。
    .init_resource::<crate::resources::MapBrowserIndexState>()
    .init_resource::<crate::resources::EditorTextInputGuard>()
    .init_resource::<crate::resources::MapEditorNumericScrubMouseGuard>()
    .init_resource::<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>()
    .init_resource::<crate::resources::MapEditorLayoutState>()
    .init_resource::<super::super::AssetPreviewFocus>()
    .init_resource::<EditorAssetCatalog>()
    .insert_resource(MapEditorState {
        open: true,
        selected_asset: Some(path.into()),
        ..default()
    })
    .insert_resource(sidebar)
    .insert_resource(super::super::types::AssetPreviewState {
        selected_path: Some(path.into()),
        content: super::super::types::AssetPreviewContent::Document,
        ..default()
    })
    .configure_sets(
        Update,
        (
            crate::MapEditorSystemSet::Input,
            crate::MapEditorSystemSet::Tasks,
            crate::MapEditorSystemSet::Presentation,
        )
            .chain(),
    )
    .add_systems(
        Update,
        (
            crate::ui::rows::spawn_dynamic_rows,
            crate::ui::rows::sync_dynamic_list_layouts,
            crate::ui::rows::sync_asset_rows,
            super::super::presentation::sync_asset_preview_presentation,
        )
            .chain()
            .in_set(crate::MapEditorSystemSet::Presentation),
    )
    .add_systems(
        Update,
        bevy::text::apply_text_edits.before(crate::MapEditorSystemSet::Input),
    );
    if !app.is_plugin_added::<bevy::ui_widgets::TextInputPlugin>() {
        app.add_plugins(bevy::ui_widgets::TextInputPlugin);
    }
    app.add_plugins((feathrise_editor_ui::EditorButtonInputPlugin, feathrise_editor_documents::DocumentsEditorPlugin));
    super::install(&mut app);
    if with_asset_navigation {
        crate::ui::project_browser::install_asset_view_for_tests(&mut app);
    }
    app.insert_resource(crate::ui::external_files::ExternalEditors::for_tests(&crate::ui::external_files::ExternalEditor::ALL)); // UI夹具固定三种安装结果，不依赖开发机实际安装。
    if screenshot_type == "themes" {
        app.add_plugins(feathrise_editor_ui::EditorThemePlugin)
            .init_resource::<crate::resources::MapEditorSettingsState>()
            .add_systems(Update, crate::ui::editor_settings::sync_settings_panel.in_set(crate::MapEditorSystemSet::Presentation));
        crate::ui::theme::install(&mut app);
        themes::install(&mut app); // 在官方Prepare清空绘制列表前记录真实画布颜色。
    }
    if matches!(screenshot_type, "multi-document" | "batch-save" | "external" | "references" | "missing-assets" | "shortcuts") {
        app.add_message::<super::super::OpenAssetPreview>()
            .init_resource::<feathrise_editor_ui::ActiveNumericInputScrub>()
            .init_resource::<super::super::AssetPreviewWorkspace>()
            .init_resource::<super::super::AssetPreviewNavigation>()
            .add_systems(Update, (crate::asset_catalog::handle_asset_row_clicks, super::super::opening::open)
                .chain().in_set(crate::MapEditorSystemSet::Input));
        super::super::tab_menu::install(&mut app);
        // 本轮还要在菜单关闭后继续编辑/重开文件，必须沿用正式逐帧鼠标静默释放和布局复位。
        app.add_systems(Update, (
            crate::ui::numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard,
            crate::ui::layout::handle_editor_layout_input,
        ).chain().in_set(crate::MapEditorSystemSet::Input));
        app.add_systems(Update, super::super::workspace::sync_presentation.in_set(crate::MapEditorSystemSet::Presentation));
    }
    if matches!(screenshot_type, "references" | "missing-assets" | "shortcuts") {
        app.init_resource::<crate::resources::MapBrowserDragState>()
            .init_resource::<crate::resources::MapBrowserState>()
            .init_resource::<crate::resources::MapDeletionTask>()
            .init_resource::<crate::authoring::TerrainExternalChangeState>();
        crate::ui::asset_context_menu::install(&mut app);
    }
    app.add_systems(PreUpdate, crate::ui::text_input::collect_ime_guard.after(bevy::input::InputSystems))
        .add_systems(Update, crate::ui::text_input::style_fields.in_set(crate::MapEditorSystemSet::Presentation));
    if central {
        app.init_resource::<crate::ui::surface_preset_editor::SurfacePresetEditorState>()
            .init_resource::<crate::ui::water_preset_editor::WaterPresetEditorState>()
            .init_resource::<crate::ui::building_presets::BuildingPresetEditorState>()
            .add_systems(
                Update,
                crate::ui::world_session::sync_editor_world_controls
                    .in_set(crate::MapEditorSystemSet::Presentation),
            );
        crate::ui::vegetation_preset_editor::install_workspace_presentation_for_tests(&mut app);
        app.insert_resource(feathrise_editor_core::WorkspaceDomainState {open_tabs:vec![feathrise_editor_core::PresetWorkspaceTab::Domain("test.author",0)],..default()});
        app.world_mut()
            .resource_mut::<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>()
            .active_tab = crate::ui::vegetation_preset_editor::PresetWorkspaceTab::Domain("test.author", 0);
    }
    app.world_mut().resource_mut::<DocumentState>().root = asset_root.clone();
    if screenshot_type == "batch-save" {
        app.world_mut().resource_mut::<DocumentState>().project_root = Some(fixture.root.clone());
    }
    if screenshot_type == "shortcuts" {
        let project = fixture.root.join("导航临时项目");
        let root = project.join("assets");
        std::fs::create_dir_all(root.join("常用目录")).unwrap();
        std::fs::write(project.join("Cargo.toml"), "# 临时项目\n").unwrap();
        std::fs::write(root.join(path), original).unwrap();
        for index in 0..24 { std::fs::write(root.join(format!("常用目录/文件{index:02}.txt")), "临时资产").unwrap(); }
        app.world_mut().resource_mut::<DocumentState>().root = root.clone();
        crate::ui::asset_shortcuts::install_for_tests(&mut app, root);
        app.add_systems(Update, (crate::asset_catalog::start_asset_scan, crate::asset_catalog::poll_asset_scan).chain().in_set(crate::MapEditorSystemSet::Tasks));
        app.add_plugins(feathrise_editor_ui::EditorThemePlugin);
        app.insert_resource(feathrise_editor_ui::EditorPalette::neutral_gray());
    }
    app.world_mut().resource_mut::<EditorAssetCatalog>().entries = vec![EditorAssetEntry {
        asset_path: path.into(),
        display_name: path.into(),
        is_directory: false,
        kind: feathrise_map_content::MapAssetKind::Data,
    }];
    if screenshot_type == "multi-document" {
        app.world_mut().resource_mut::<EditorAssetCatalog>().entries.push(EditorAssetEntry {
            asset_path: "第二代码.rs".into(), display_name: "第二代码.rs".into(),
            is_directory: false, kind: feathrise_map_content::MapAssetKind::Data,
        });
    }
    if screenshot_type == "references" {
        app.world_mut().resource_mut::<EditorAssetCatalog>().entries.push(EditorAssetEntry {
            asset_path: "gltf/中文 空格.glb".into(), display_name: "中文 空格.glb".into(),
            is_directory: false, kind: feathrise_map_content::MapAssetKind::Scene,
        });
    }
    app.finish();
    app.cleanup();
    let window = app.world_mut().spawn((Window { focused: true, ..default() }, bevy::window::PrimaryWindow)).id();
    let width = if central { 1920 } else { 320 };
    let height = if central { 1080 } else { 768 };
    app.world_mut().get_mut::<Window>(window).unwrap().resolution.set_physical_resolution(width, height);
    let target = app.world_mut().resource_mut::<Assets<Image>>().add({
        let mut image = Image::new_target_texture(width, height, TextureFormat::Bgra8UnormSrgb, None);
        image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        image
    });
    let camera = app
        .world_mut()
        .spawn((
            Camera2d,
            Camera::default(),
            bevy::camera::RenderTarget::from(target.clone()),
        ))
        .id();
    let menu_root = (with_asset_navigation || matches!(screenshot_type, "themes" | "external")).then(|| app.world_mut().spawn((
        crate::components::MapEditorRoot, bevy::ui::UiTargetCamera(camera), Node {
            position_type: PositionType::Absolute, width: px(width as f32), height: px(height as f32), ..default()
        },
    )).id()); // 和正式F12一样先建立根，所有页面和菜单从首次布局即归同一棵树。
    if screenshot_type == "themes" {
        use bevy::ecs::system::RunSystemOnce;
        app.world_mut().run_system_once::<_, (), _>(crate::ui::editor_settings::settings_panel.spawn()).unwrap();
        let world = app.world_mut();
        let panel = world.query_filtered::<Entity, With<crate::components::MapEditorSettingsPanel>>().single(world).unwrap();
        world.entity_mut(panel).insert(ChildOf(menu_root.unwrap()));
    }
    app.world_mut()
        .run_system_once::<_, (), _>((|| crate::ui::sidebars::right_sidebar(&feathrise_editor_documents::scenes())).spawn())
        .unwrap();
    let navigation_width = if with_asset_navigation { 320 } else { 0 };
    if with_asset_navigation {
        app.world_mut()
            .run_system_once::<_, (), _>((|| crate::ui::sidebars::left_sidebar(&feathrise_editor_documents::scenes())).spawn())
            .unwrap();
        let world = app.world_mut();
        let pane = world.query::<(Entity, &MapEditorPane)>().iter(world)
            .find(|(_, pane)| pane.0 == crate::types::EditorPane::Content).unwrap().0;
        world.entity_mut(pane).insert(bevy::ui::UiTargetCamera(camera));
        if let Some(root) = menu_root { world.entity_mut(pane).insert(ChildOf(root)); }
        let mut node = world.get_mut::<Node>(pane).unwrap();
        node.width = px(navigation_width as f32);
        node.position_type = PositionType::Absolute;
        node.left = px(0);
        node.top = px(0);
    }
    if central {
        app.world_mut()
            .run_system_once::<_, (), _>((|| crate::ui::scene::viewport_panel(&feathrise_editor_documents::scenes())).spawn())
            .unwrap();
        let world = app.world_mut();
        let mut query = world.query_filtered::<&ChildOf, With<DocumentCenterHost>>();
        let center = query.single(world).unwrap().parent();
        world
            .entity_mut(center)
            .insert(bevy::ui::UiTargetCamera(camera));
        if let Some(root) = menu_root { world.entity_mut(center).insert(ChildOf(root)); }
        let mut node = world.get_mut::<Node>(center).unwrap();
        node.width = px((width - 320 - navigation_width) as f32);
        node.position_type = PositionType::Absolute;
        node.left = px(navigation_width as f32);
        node.top = px(0);
    }
    {
        let world = app.world_mut();
        let pane = world.query::<(Entity, &MapEditorPane)>().iter(world)
            .find(|(_, pane)| pane.0 == crate::types::EditorPane::Inspector).unwrap().0;
        world
            .entity_mut(pane)
            .insert(bevy::ui::UiTargetCamera(camera));
        if let Some(root) = menu_root { world.entity_mut(pane).insert(ChildOf(root)); }
        let mut node = world.get_mut::<Node>(pane).unwrap();
        node.width = px(320);
        node.position_type = PositionType::Absolute;
        node.left = px((width - 320) as f32);
        node.top = px(0);
        let mut pages = world.query::<(&crate::ui::sidebars::MapEditorSidebarPage, &mut Node)>();
        for (page, mut node) in pages.iter_mut(world) {
            node.display = if page.0
                == crate::ui::sidebars::EditorSidebarTab::Right(EditorRightSidebarTab::Assets)
                || (with_asset_navigation && page.0
                    == crate::ui::sidebars::EditorSidebarTab::Navigator(crate::ui::sidebars::EditorNavigatorTab::Assets))
            {
                Display::Flex
            } else {
                Display::None
            };
        }
    }
    frames(&mut app, 100);
    if original.chars().count() > 8192 {
        assert!(
            app.world()
                .resource::<super::resources::PreviewRenderState>()
                .active
                .is_some(),
            "完整连续预览没有完成构造"
        );
        let doc = app.world().resource::<DocumentState>().current().unwrap();
        assert_eq!(doc.source_text(), original);
    }
    assert!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .preview_revision
            .is_some()
    );
    if central {
        if screenshot_type == "multi-document" {
            double_click_asset(&mut app, window, camera, 0);
        } else {
            press(&mut app, DocumentAction::ToggleExpand);
        }
        frames(&mut app, 5);
        assert!(app.world().resource::<DocumentState>().in_center());
        let world = app.world_mut();
        let mut panels = world.query_filtered::<&ChildOf, With<DocumentPanel>>();
        let parent = panels.single(world).unwrap().parent();
        assert!(world.get::<DocumentCenterHost>(parent).is_some());
        assert!(world.get::<ComputedNode>(parent).unwrap().size().x > 800.0);
    }
    snapshot(
        &mut app,
        target.clone(),
        &format!("asset-browser-{screenshot_type}-preview.png"),
        width,
    );
    if screenshot_type == "cross-search" {
        super::search::gpu_assert_search(&mut app, window, camera, target, width, &fixture.root, snapshot);
        return;
    }
    if screenshot_type == "references" {
        super::search::gpu_assert_references(&mut app, window, camera, target, width, snapshot);
        return;
    }
    if screenshot_type == "missing-assets" {
        super::search::gpu_assert_missing_assets(&mut app, window, camera, target, width, snapshot);
        return;
    }
    if screenshot_type == "shortcuts" {
        crate::ui::asset_shortcuts::assert_gpu(&mut app, window, camera, target, width, path, snapshot);
        return;
    }
    if screenshot_type == "external" {
        external::gpu_assert_external(&mut app, window, camera, target, width, &fixture.root, path, snapshot);
        return;
    }
    if screenshot_type == "cross-replace" {
        super::search::replacement::gpu_assert_cross_replacement(&mut app, window, camera, target, width, &fixture.root, snapshot);
        return;
    }
    if screenshot_type == "batch-save" {
        super::batch_save::gpu_assert_batch_save(&mut app, window, camera, target, width, &asset_root, &fixture.root, snapshot);
        return;
    }
    super::syntax::gpu_assert_syntax(&mut app, path, false);
    if tall_document {
        // 使用真实预览滚轮入口到达全文末尾，验证长正文高度与裁剪后的显示。
        let area = {
            let world = app.world_mut();
            let mut query = world.query_filtered::<Entity, With<DocumentPreviewScroll>>();
            query.single(world).unwrap()
        };
        let node = app.world().get::<ComputedNode>(area).unwrap();
        let max = (node.content_size().y - node.size().y) * node.inverse_scale_factor();
        assert!(max > 10_000.0, "连续预览必须有完整文档的滚动高度");
        app.world_mut().get_mut::<bevy::ui::RelativeCursorPosition>(area).unwrap().cursor_over = true;
        app.world_mut().write_message(bevy::input::mouse::MouseWheel {
            unit: bevy::input::mouse::MouseScrollUnit::Pixel,
            x: 0.0,
            y: -1_000_000.0,
            window: Entity::PLACEHOLDER,
            phase: bevy::input::touch::TouchPhase::Moved,
        });
        app.world_mut().run_system_once(super::interaction::scroll).unwrap();
        assert_eq!(app.world().get::<ScrollPosition>(area).unwrap().y, max);
        assert!(app.world().resource::<DocumentState>().owns_wheel());
        frames(&mut app, 3);
        snapshot(
            &mut app,
            target.clone(),
            &format!("asset-browser-{screenshot_type}-preview-end.png"),
            width,
        );
    }
    let button = {
        let world = app.world_mut();
        let mut query = world.query::<(Entity, &DocumentAction)>();
        query
            .iter(world)
            .find(|(_, action)| **action == DocumentAction::Edit)
            .unwrap()
            .0
    };
    // 走真实按钮系统，设置实体交互并单次Update避免PreUpdate的鼠标系统覆盖测试边沿。
    *app.world_mut().get_mut::<UiInteraction>(button).unwrap() = UiInteraction::Pressed;
    app.world_mut().entity_mut(button).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    app.world_mut()
        .run_system_once(super::interaction::actions)
        .unwrap();
    app.world_mut().get_mut::<feathrise_editor_ui::EditorButtonClick>(button).unwrap().invalidate();
    frames(&mut app, 3);
    let source = {
        let world = app.world_mut();
        let mut query = world.query_filtered::<Entity, With<DocumentSource>>();
        query.single(world).unwrap()
    };
    assert!(
        app.world().get::<ComputedNode>(source).unwrap().size().y > 100.0,
        "原生编辑框没有正文高度"
    );
    super::syntax::gpu_assert_syntax(&mut app, path, true);
    if screenshot_type == "themes" {
        themes::verify(&mut app, window, camera, source, target, width, snapshot);
        assert_eq!(std::fs::read_to_string(fixture.root.join(path)).unwrap(), original, "配色切换不得保存文档");
        return;
    }
    if screenshot_type.starts_with("syntax-") {
        super::syntax::gpu_assert_input(&mut app, window, source, original);
    }
    if tall_document {
        // 真实控件点击后连续滚轮必须能越过光标，静止排版和闪烁不能把视区拉回。
        app.world_mut().init_resource::<crate::resources::MapBrowserDragState>();
        app.world_mut().get_mut::<EditableText>(source).unwrap()
            .queue_edit(bevy::text::TextEdit::MoveToPoint(Vec2::new(16.0, 36.0)));
        frames(&mut app, 3);
        let selection = app.world().get::<EditableText>(source).unwrap()
            .editor.raw_selection().text_range();
        for (unit, y) in [
            (bevy::input::mouse::MouseScrollUnit::Line, -30.0),
            (bevy::input::mouse::MouseScrollUnit::Pixel, -440.0),
            (bevy::input::mouse::MouseScrollUnit::Pixel, 117.0),
            (bevy::input::mouse::MouseScrollUnit::Pixel, -1_000_000.0),
        ] {
            app.world_mut().get_mut::<bevy::ui::RelativeCursorPosition>(source).unwrap()
                .cursor_over = true;
            app.world_mut().write_message(bevy::input::mouse::MouseWheel {
                unit, x: 0.0, y, window: Entity::PLACEHOLDER,
                phase: bevy::input::touch::TouchPhase::Moved,
            });
            app.world_mut().run_system_once(crate::ui::systems::scroll_editor_side_panels).unwrap();
            let expected = app.world().get::<EditableText>(source).unwrap().viewport.offset.y;
            assert!(expected > 0.0, "滚轮必须真的改变原生文字视区");
            frames(&mut app, 6);
            let actual = app.world().get::<EditableText>(source).unwrap().viewport.offset.y;
            assert!((actual - expected).abs() < 1.0,
                "点击后的静止光标不能阻止滚轮：期望{expected}，实际{actual}");
            assert_eq!(app.world().get::<EditableText>(source).unwrap()
                .editor.raw_selection().text_range(), selection,
                "手动滚动不能移动光标或选区");
        }
        // 手动浏览到末尾后，键盘移动重新启用原生跟随，不能让后续编辑留在屏幕之外。
        app.world_mut().get_mut::<EditableText>(source).unwrap()
            .queue_edit(bevy::text::TextEdit::Down(false));
        frames(&mut app, 3);
        let scroll = app.world().get::<EditableText>(source).unwrap().viewport.offset.y;
        let cursor = app.world().get::<bevy::text::TextLayoutInfo>(source).unwrap().cursor.unwrap().1;
        let view = app.world().get::<ComputedNode>(source).unwrap().content_box().height();
        assert!(cursor.min.y >= scroll && cursor.max.y <= scroll + view,
            "键盘移动后必须重新显示真实光标：scroll={scroll}, cursor={cursor:?}, view={view}, selection={:?}, focus={:?}",
            app.world().get::<EditableText>(source).unwrap().editor.raw_selection().text_range(),
            app.world().resource::<bevy::input_focus::InputFocus>().get());
    }
    if central {
        // 小地图读取真实原生排版，等待固定预算快照与唯一后台结果完成。
        for _ in 0..100 {
            frames(&mut app, 1);
            if app.world().resource::<super::resources::DocumentMinimapState>().picture.is_some() { break; }
        }
        let minimap = app.world().resource::<super::resources::DocumentMinimapState>();
        assert!(minimap.picture.is_some(), "轮廓未生成: shown={}, task={}, collecting={}, mode={:?}, binding={:?}, layout={:?}, node={:?}",
            minimap.shown, minimap.task.is_some(), minimap.collecting.is_some(),
            app.world().resource::<DocumentState>().current().map(|doc| doc.mode),
            app.world().get::<DocumentSource>(source).unwrap().key,
            app.world().get::<bevy::text::TextLayoutInfo>(source).unwrap().size,
            app.world().get::<ComputedNode>(source).unwrap().content_box().size());
        let picture = minimap.picture.as_ref().unwrap().image.clone();
        let image = app.world().resource::<Assets<Image>>().get(&picture).unwrap();
        assert_eq!(image.width(), MINIMAP_WIDTH);
        assert!(image.data.as_ref().unwrap().chunks_exact(4).any(|pixel| pixel[3] > 0), "轮廓必须来自真实字体排版");
        let rail = {
            let world = app.world_mut();
            let mut query = world.query_filtered::<Entity, With<DocumentMinimapRail>>();
            query.single(world).unwrap()
        };
        assert!(app.world().get::<ComputedNode>(rail).unwrap().size().y > 100.0);
        let rail_center = app.world().get::<UiGlobalTransform>(rail).unwrap().affine().translation;
        let rail_size = app.world().get::<ComputedNode>(rail).unwrap().size();
        let move_pointer = |app: &mut App, relative: Vec2| {
            app.world_mut().get_mut::<Window>(window).unwrap()
                .set_physical_cursor_position(Some((rail_center + relative * rail_size).as_dvec2()));
            // 离屏相机仅在命中系统运行时映射到同尺寸虚拟窗口；渲染仍用原Image目标。
            // 不注入Interaction/normalized，让正式命中同时覆盖轮廓图片和视区框的穿透策略。
            let target = app.world().get::<bevy::camera::RenderTarget>(camera).unwrap().clone();
            *app.world_mut().get_mut::<bevy::camera::RenderTarget>(camera).unwrap() =
                bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(window));
            app.world_mut().run_system_cached(bevy::ui::ui_focus_system).unwrap();
            *app.world_mut().get_mut::<bevy::camera::RenderTarget>(camera).unwrap() = target;
        };
        if tall_document {
            let selection = app.world().get::<EditableText>(source).unwrap().editor.raw_selection().text_range();
            let revision = app.world().resource::<DocumentState>().current().unwrap().revision;
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
            move_pointer(&mut app, Vec2::new(0.0, 0.25));
            assert_eq!(*app.world().get::<UiInteraction>(rail).unwrap(), UiInteraction::Pressed);
            app.world_mut().run_system_once(super::minimap::handle_input).unwrap();
            let expected = app.world().get::<EditableText>(source).unwrap().viewport.offset.y;
            assert!(expected > 10_000.0, "小地图点击必须跳到长文件深处");
            // 释放再运行正式帧，验证旧光标不会把小地图导航拉回。
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
            app.world_mut().run_system_once(super::minimap::handle_input).unwrap();
            frames(&mut app, 5);
            assert!((app.world().get::<EditableText>(source).unwrap().viewport.offset.y - expected).abs() < 1.0);
            assert_eq!(app.world().get::<EditableText>(source).unwrap().editor.raw_selection().text_range(), selection);
            assert_eq!(app.world().resource::<DocumentState>().current().unwrap().revision, revision);
            // 真实轨道拖出到下方仍滚到末尾，滚轮悬停只改同一EditableText.viewport。
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
            move_pointer(&mut app, Vec2::new(0.0, 0.25));
            assert_eq!(*app.world().get::<UiInteraction>(rail).unwrap(), UiInteraction::Pressed);
            app.world_mut().run_system_once(super::minimap::handle_input).unwrap();
            move_pointer(&mut app, Vec2::new(0.75, 0.5));
            assert!(!app.world().get::<bevy::ui::RelativeCursorPosition>(rail).unwrap().cursor_over());
            app.world_mut().run_system_once(super::minimap::handle_input).unwrap();
            let max = app.world().get::<bevy::text::TextLayoutInfo>(source).unwrap().size.y
                - app.world().get::<ComputedNode>(source).unwrap().content_box().height();
            assert!((app.world().get::<EditableText>(source).unwrap().viewport.offset.y - max).abs() < 1.0);
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
            app.world_mut().run_system_once(super::minimap::handle_input).unwrap();
            frames(&mut app, 3);
            move_pointer(&mut app, Vec2::new(0.0, 0.0));
            app.world_mut().write_message(bevy::input::mouse::MouseWheel { window,
                unit: bevy::input::mouse::MouseScrollUnit::Pixel, x: 0.0, y: 250.0,
                phase: bevy::input::touch::TouchPhase::Moved });
            app.world_mut().run_system_once(super::minimap::handle_input).unwrap();
            assert!((app.world().get::<EditableText>(source).unwrap().viewport.offset.y - (max - 250.0)).abs() < 1.0);
            frames(&mut app, 3);
            snapshot(&mut app, target.clone(), "asset-browser-minimap-navigation.png", width);
        }
    }
    if screenshot_type == "continuous" || screenshot_type == "minimap-code" || screenshot_type == "navigation-sidebar" {
        super::navigation::gpu_assert_navigation(&mut app, window, source, original, target.clone(), width, snapshot);
    }
    app.world_mut()
        .get_mut::<EditableText>(source)
        .unwrap()
        .queue_edit(bevy::text::TextEdit::TextEnd(false));
    app.world_mut()
        .get_mut::<EditableText>(source)
        .unwrap()
        .queue_edit(bevy::text::TextEdit::Insert(addition.into()));
    let mut edit_times = Vec::new();
    for _ in 0..4 {
        let started = std::time::Instant::now();
        app.update();
        edit_times.push(started.elapsed().as_secs_f64() * 1_000.0);
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
    if tall_document {
        let layout = app.world().get::<bevy::text::TextLayoutInfo>(source).unwrap();
        let scroll = app.world().get::<EditableText>(source).unwrap();
        assert!(
            layout.size.y > 10_000.0 && scroll.viewport.offset.y > 10_000.0,
            "光标必须能滚到长文件末尾"
        );
        assert_eq!(
            app.world().get::<EditableText>(source).unwrap().value().to_string(),
            format!("{original}{addition}")
        );
        println!(
            "连续文档：{}字节、{}字符，末尾输入/重排Update耗时(ms)：{edit_times:?}",
            original.len(), original.chars().count()
        );
    }
    assert!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .dirty()
    );
    snapshot(
        &mut app,
        target.clone(),
        &format!("asset-browser-{screenshot_type}-edit.png"),
        width,
    );
    if screenshot_type == "multi-document" {
        let first = app.world().resource::<DocumentState>().active_tab_id().unwrap();
        app.world_mut().get_mut::<EditableText>(source).unwrap().viewport.offset.y = 240.0;
        frames(&mut app, 4);
        let selection = app.world().get::<EditableText>(source).unwrap().editor.raw_selection().text_range();
        let offset = app.world().get::<EditableText>(source).unwrap().viewport.offset;
        assert!(offset.y > 200.0);
        double_click_asset(&mut app, window, camera, 1);
        frames(&mut app, 40);
        assert_eq!(app.world().resource::<DocumentState>().tab_count(), 2);
        let second = app.world().resource::<DocumentState>().active_tab_id().unwrap();
        press(&mut app, DocumentAction::Edit);
        frames(&mut app, 4);
        app.world_mut().get_mut::<EditableText>(source).unwrap().queue_edit(bevy::text::TextEdit::TextEnd(false));
        app.world_mut().get_mut::<EditableText>(source).unwrap().queue_edit(bevy::text::TextEdit::Insert("// 第二份草稿\n".into()));
        frames(&mut app, 4);
        assert!(app.world().resource::<DocumentState>().current().unwrap().dirty());
        press(&mut app, DocumentAction::ActivateTab(first));
        frames(&mut app, 5);
        assert_eq!(app.world().get::<EditableText>(source).unwrap().value(), format!("{original}{addition}").as_str());
        assert_eq!(app.world().get::<EditableText>(source).unwrap().editor.raw_selection().text_range(), selection);
        assert!((app.world().get::<EditableText>(source).unwrap().viewport.offset - offset).length() < 1.0,
            "返回文件必须保留其手动视区，不被末尾光标拉回：期望{offset:?}，实际{:?}",
            app.world().get::<EditableText>(source).unwrap().viewport.offset);
        assert_eq!(app.world_mut().query_filtered::<Entity, With<DocumentSource>>().single(app.world()).unwrap(), source);
        snapshot(&mut app, target.clone(), "asset-browser-multiple-document-tabs.png", width);
        double_click_asset(&mut app, window, camera, 0);
        frames(&mut app, 4);
        assert_eq!(app.world().resource::<DocumentState>().tab_count(), 2, "重复双击不能生成同文件的第二个标签");
        press(&mut app, DocumentAction::CloseTab(second));
        frames(&mut app, 3);
        assert_eq!(app.world().resource::<DocumentState>().active_tab_id(), Some(first));
        assert!(app.world().resource::<DocumentState>().documents.iter().find(|doc| doc.path == "第二代码.rs").unwrap().dirty());
        assert_eq!(std::fs::read_to_string(fixture.root.join("第二代码.rs")).unwrap(), "fn main() {\n    println!(\"第二文件\");\n}\n");
    }
    if central {
        // 右栏隐藏并切到非资产页后，中央正文仍可编辑和保存固定路径。
        app.world_mut()
            .resource_mut::<crate::resources::MapEditorLayoutState>()
            .inspector_visible = false;
        app.world_mut()
            .resource_mut::<MapEditorSidebarState>()
            .right = EditorRightSidebarTab::Scene;
        app.world_mut()
            .resource_mut::<MapEditorState>()
            .selected_asset = Some("其他图片.png".into());
        frames(&mut app, 3);
        assert!(app.world().resource::<DocumentState>().owns_shortcuts());
    }
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ControlLeft);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyS);
    app.world_mut()
        .run_system_once(super::interaction::actions)
        .unwrap();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    frames(&mut app, 30);
    assert_eq!(
        std::fs::read_to_string(fixture.root.join(path)).unwrap(),
        format!("{original}{addition}")
    );
    if path == "长配置.json" {
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(fixture.root.join(path)).unwrap()).unwrap();
        assert_eq!(saved["rows"].as_array().unwrap().len(), 1_800);
        assert_eq!(saved["tail"], "文件末尾必须可编辑");
    }
    assert!(
        !app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .dirty()
    );

    if screenshot_type.starts_with("json-tools-") {
        super::json::gpu_assert_tools(&mut app, window, camera, source, &fixture.root, path,
            |app, stage| snapshot(app, target.clone(), &format!("asset-browser-{screenshot_type}-{stage}.png"), width));
        return; // JSON专用场景保留中央画面；通用放大/还原回归由其他GPU夹具继续覆盖。
    }

    if screenshot_type.starts_with("replace-tools-") {
        super::navigation::gpu_assert_replacement(&mut app, window, camera, source, &fixture.root, path,
            |app, stage| snapshot(app, target.clone(), &format!("asset-browser-{screenshot_type}-{stage}.png"), width));
        return; // 本场景保留替换工具画面，通用还原/多标签由既有GPU回归覆盖。
    }

    if central {
        use crate::ui::vegetation_preset_editor::{
            PresetWorkspaceTab, VegetationPresetEditorState,
        };
        {
            let mut workspace = app
                .world_mut()
                .resource_mut::<VegetationPresetEditorState>();
            workspace.active_tab = PresetWorkspaceTab::Map;
            workspace.touch_ui();
        }
        frames(&mut app, 3);
        assert_ne!(
            app.world()
                .resource::<bevy::input_focus::InputFocus>()
                .get(),
            Some(source)
        );
        assert!(!app.world().resource::<DocumentState>().owns_shortcuts());
        let tab = app.world().resource::<DocumentState>().active_tab_id().unwrap();
        press(&mut app, DocumentAction::ActivateTab(tab));
        frames(&mut app, 3);
        assert!(app.world().resource::<DocumentState>().in_center());
        press(&mut app, DocumentAction::ToggleExpand);
        frames(&mut app, 4);
        let world = app.world_mut();
        let mut sources = world.query_filtered::<Entity, With<DocumentSource>>();
        assert_eq!(
            sources.single(world).unwrap(),
            source,
            "放大和还原不能销毁原生输入实体"
        );
        assert!(!world.resource::<DocumentState>().in_center());
        let mut minimaps = world.query_filtered::<&Node, With<DocumentMinimapRoot>>();
        assert_eq!(minimaps.single(world).unwrap().display, Display::None, "归还侧栏后收起小地图");
        assert_eq!(
            world
                .resource::<DocumentState>()
                .current()
                .unwrap()
                .source_text(),
            format!("{original}{addition}")
        );
        assert!(
            world
                .resource::<crate::resources::MapEditorLayoutState>()
                .inspector_visible
        );
        // 不再次移动光标：移位后继续插入仍应接在刚才的光标位置。
        world
            .get_mut::<EditableText>(source)
            .unwrap()
            .queue_edit(bevy::text::TextEdit::Insert("光标保持".into()));
        frames(&mut app, 4);
        assert_eq!(
            app.world()
                .resource::<DocumentState>()
                .current()
                .unwrap()
                .source_text(),
            format!("{original}{addition}光标保持")
        );
        snapshot(
            &mut app,
            target.clone(),
            "asset-browser-center-restored-sidebar.png",
            width,
        );
    }

    if screenshot_type == "multi-document" {
        // 同一正式标签栏同时展示两份脏草稿、图片和模型；右击及菜单左击均走真实UI命中。
        app.world_mut().resource_mut::<MapEditorState>().selected_asset = Some(path.into());
        press(&mut app, DocumentAction::OpenSelected);
        frames(&mut app, 4);
        app.world_mut().resource_mut::<MapEditorState>().selected_asset = Some("第二代码.rs".into());
        press(&mut app, DocumentAction::OpenSelected);
        frames(&mut app, 8);
        {
            let mut media = app.world_mut().remove_resource::<super::super::AssetPreviewWorkspace>().unwrap();
            let mut workspace = app.world_mut().resource_mut::<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>();
            for path in ["自有槐树公告板.png", "预览模型.glb"] {
                super::super::workspace::expand(&mut media, &mut workspace, path,
                    &super::super::AssetPreviewState::default(), &super::super::AssetPreviewNavigation::default()).unwrap();
            }
            workspace.active_tab = crate::ui::vegetation_preset_editor::PresetWorkspaceTab::Document;
            workspace.touch_ui(); // 和正式打开一致地发布中央标签修订，不直接绕过共享工作区。
            drop(workspace);
            app.insert_resource(media);
        }
        {
            let world = app.world_mut();
            world.init_resource::<feathrise_editor_ui::ActiveNumericInputScrub>();
            world.resource_mut::<crate::resources::MapEditorLayoutState>().owns_mouse_this_frame = false;
        }
        frames(&mut app, 8);
        assert_eq!(app.world().resource::<super::super::AssetPreviewWorkspace>().tabs().count(), 2);
        let media_root = {
            let world = app.world_mut();
            world.query_filtered::<Entity, With<super::super::components::AssetPreviewTabRoot>>().single(world).unwrap()
        };
        frames(&mut app, 3);
        {
            let world = app.world_mut();
            let mut media_roots = world.query_filtered::<(Entity, &ComputedNode), With<super::super::components::AssetPreviewTabRoot>>();
            let (root, node) = media_roots.single(world).unwrap();
            assert_eq!(root, media_root, "稳定帧不重建文件标签");
            assert!(node.size().min_element() > 0.0, "媒体标签必须真实参与布局");
        }
        super::history::gpu_assert_history(&mut app, window, source);
        super::history::gpu_click_history(&mut app, window, camera, false);
        assert!(!app.world().resource::<DocumentState>().current().unwrap().dirty());
        frames(&mut app, 3);
        super::history::gpu_click_history(&mut app, window, camera, true);
        assert!(app.world().resource::<DocumentState>().current().unwrap().dirty());
        frames(&mut app, 3);
        snapshot(&mut app, target.clone(), "asset-browser-document-undo-redo.png", width);
        // 撤销回保存点后，关闭未修改应移除当前源码与媒体，仅留下第一份脏草稿。
        super::history::gpu_chord(&mut app, window, KeyCode::KeyZ, false);
        let position = media_tab_position(&mut app);
        super::super::tab_menu::gpu_open_menu(&mut app, window, camera, position);
        frames(&mut app, 3);
        snapshot(&mut app, target.clone(), "asset-browser-file-tabs-close-unmodified-menu.png", width);
        super::super::tab_menu::gpu_click_close_unmodified(&mut app, window, camera);
        frames(&mut app, 6);
        assert_eq!(app.world().resource::<DocumentState>().tab_count(), 1);
        assert_eq!(app.world().resource::<DocumentState>().current().unwrap().path, path);
        assert!(app.world().resource::<DocumentState>().current().unwrap().dirty());
        assert_eq!(app.world().get::<EditableText>(source).unwrap().value(), format!("{original}{addition}光标保持").as_str());
        snapshot(&mut app, target.clone(), "asset-browser-file-tabs-close-unmodified.png", width);
        let document_position = {
            let world = app.world_mut(); let mut buttons = world.query::<(&DocumentAction, &UiGlobalTransform)>();
            buttons.iter(world).find(|(action, _)| matches!(action, DocumentAction::ActivateTab(_))).unwrap().1.affine().translation
        };
        super::super::tab_menu::gpu_open_menu(&mut app, window, camera, document_position);
        frames(&mut app, 3);
        super::super::tab_menu::gpu_click_close_unmodified(&mut app, window, camera);
        frames(&mut app, 4);
        assert_eq!(app.world().resource::<DocumentState>().tab_count(), 1);
        assert!(app.world().resource::<MapEditorState>().status.contains("没有未修改"));
        // 关闭身份后重新打开仍能重做第二份文档，证明菜单没有清除原生缓存或历史。
        app.world_mut().resource_mut::<MapEditorState>().selected_asset = Some("第二代码.rs".into());
        press(&mut app, DocumentAction::OpenSelected);
        frames(&mut app, 5);
        super::history::gpu_chord(&mut app, window, KeyCode::KeyY, false);
        assert!(app.world().resource::<DocumentState>().current().unwrap().dirty());
        assert_eq!(app.world().get::<EditableText>(source).unwrap().value(), "fn main() {\n    println!(\"第二文件\");\n}\n// 第二份草稿\n");
        {
            let mut media = app.world_mut().remove_resource::<super::super::AssetPreviewWorkspace>().unwrap();
            let mut workspace = app.world_mut().resource_mut::<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>();
            for path in ["自有槐树公告板.png", "预览模型.glb"] {
                super::super::workspace::expand(&mut media, &mut workspace, path,
                    &super::super::AssetPreviewState::default(), &super::super::AssetPreviewNavigation::default()).unwrap();
            }
            workspace.active_tab = crate::ui::vegetation_preset_editor::PresetWorkspaceTab::Document;
            app.insert_resource(media);
        }
        frames(&mut app, 4);
        let drafts: Vec<_> = app.world().resource::<DocumentState>().documents.iter()
            .map(|doc| (doc.path.clone(), doc.source.clone(), doc.revision, doc.saved_revision)).collect();
        let position = media_tab_position(&mut app);
        super::super::tab_menu::gpu_open_menu(&mut app, window, camera, position);
        frames(&mut app, 4);
        snapshot(&mut app, target.clone(), "asset-browser-file-tabs-close-all-menu.png", width);
        super::super::tab_menu::gpu_click_close_all(&mut app, window, camera);
        let returned_path = app.world().resource::<MapEditorState>().selected_asset.clone();
        app.world_mut().resource_mut::<super::super::AssetPreviewState>().selected_path = returned_path; // 最小夹具收敛正式加载系统的侧栏来源。
        frames(&mut app, 4);
        for (path, source_text, revision, saved_revision) in drafts {
            let doc = app.world().resource::<DocumentState>().documents.iter().find(|doc| doc.path == path).unwrap();
            assert_eq!(doc.source, source_text); assert_eq!(doc.revision, revision); assert_eq!(doc.saved_revision, saved_revision);
        }
        assert_eq!(app.world_mut().query_filtered::<Entity, With<DocumentSource>>().single(app.world()).unwrap(), source);
        assert_eq!(std::fs::read_to_string(fixture.root.join("第二代码.rs")).unwrap(), "fn main() {\n    println!(\"第二文件\");\n}\n");
        snapshot(&mut app, target, "asset-browser-file-tabs-closed.png", width);
    }

    fn media_tab_position(app: &mut App) -> Vec2 {
        let world = app.world_mut();
        let mut tabs = world.query::<(&super::super::components::AssetPreviewWorkspaceAction, &UiGlobalTransform)>();
        tabs.iter(world).find(|(action, _)| matches!(action, super::super::components::AssetPreviewWorkspaceAction::ActivateTab(_))).unwrap().1.affine().translation
    }

    fn press(app: &mut App, action: DocumentAction) {
        let world = app.world_mut();
        let mut query = world.query::<(Entity, &DocumentAction)>();
        let button = query
            .iter(world)
            .find(|(_, candidate)| **candidate == action)
            .unwrap_or_else(|| panic!("未生成正式文档按钮：{action:?}"))
            .0;
        *world.get_mut::<UiInteraction>(button).unwrap() = UiInteraction::Pressed;
        world.entity_mut(button).insert(feathrise_editor_ui::EditorButtonClick::triggered());
        world.run_system_once(super::interaction::actions).unwrap();
        world.get_mut::<feathrise_editor_ui::EditorButtonClick>(button).unwrap().invalidate();
    }

    fn double_click_asset(app: &mut App, window: Entity, camera: Entity, index: usize) {
        let (row, position) = {
            let world = app.world_mut();
            let mut rows = world.query::<(Entity, &crate::components::MapEditorAssetRow, &UiGlobalTransform)>();
            let (row, _, transform) = rows.iter(world).find(|(_, row, _)| row.entry_index == Some(index)).unwrap();
            (row, transform.affine().translation)
        };
        app.world_mut().get_mut::<Window>(window).unwrap().set_physical_cursor_position(Some(position.as_dvec2()));
        for _ in 0..2 {
            let target = app.world().get::<bevy::camera::RenderTarget>(camera).unwrap().clone();
            *app.world_mut().get_mut::<bevy::camera::RenderTarget>(camera).unwrap() =
                bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(window));
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
            app.world_mut().run_system_cached(bevy::ui::ui_focus_system).unwrap();
            app.world_mut().run_system_once(feathrise_editor_ui::update_editor_button_clicks).unwrap();
            assert_eq!(*app.world().get::<UiInteraction>(row).unwrap(), UiInteraction::Pressed, "正式鼠标命中必须到达资产行");
            app.world_mut().run_system_once(crate::asset_catalog::handle_asset_row_clicks).unwrap();
            app.world_mut().run_system_once(super::super::opening::open).unwrap();
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
            app.world_mut().run_system_cached(bevy::ui::ui_focus_system).unwrap();
            app.world_mut().run_system_once(feathrise_editor_ui::update_editor_button_clicks).unwrap();
            app.world_mut().run_system_once(crate::asset_catalog::handle_asset_row_clicks).unwrap();
            app.world_mut().run_system_once(super::super::opening::open).unwrap();
            *app.world_mut().get_mut::<bevy::camera::RenderTarget>(camera).unwrap() = target;
            frames(app, 1);
        }
    }

    fn frames(app: &mut App, count: usize) {
        for _ in 0..count {
            app.update();
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
    }
    fn snapshot(app: &mut App, target: Handle<Image>, filename: &str, width: u32) {
        let height = app.world().resource::<Assets<Image>>().get(&target).unwrap().height();
        let (sender, receiver) = std::sync::mpsc::channel();
        app.world_mut().spawn(Readback::texture(target)).observe(
            move |event: On<ReadbackComplete>, mut commands: Commands| {
                let _ = sender.send(event.data.clone());
                commands.entity(event.entity).try_despawn();
            },
        );
        let started = std::time::Instant::now();
        let mut bytes = loop {
            app.update();
            if let Ok(bytes) = receiver.try_recv() {
                break bytes;
            }
            assert!(started.elapsed().as_secs() < 10, "GPU文档快照超时");
            std::thread::sleep(std::time::Duration::from_millis(16));
        };
        assert_eq!(bytes.len(), width as usize * height as usize * 4);
        assert!(
            bytes
                .chunks_exact(4)
                .filter(|pixel| pixel[0] > 100 && pixel[1] > 100 && pixel[2] > 100)
                .count()
                > 500,
            "截图没有可见文字"
        );
        for pixel in bytes.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        // 分库验收可指定独立输出目录，避免覆盖其他会话持有的旧截图；默认路径保持兼容。
        let output = std::env::var_os("RPG_EDITOR_VALIDATION_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.local/validation"));
        std::fs::create_dir_all(&output).unwrap();
        image::RgbaImage::from_raw(width, height, bytes)
            .unwrap()
            .save(output.join(filename))
            .unwrap();
    }
}
