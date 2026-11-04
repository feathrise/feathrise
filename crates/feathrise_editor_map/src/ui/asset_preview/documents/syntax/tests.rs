//! 分词、UTF-8范围和文档身份回归；真实字形与输入行为由资产离屏GPU验收覆盖。

use super::{lexer::scan, paint::token_at, types::*};

fn tokens(language: Language, source: &str) -> Vec<(String, Kind)> {
    let result = scan(language, source);
    let mut previous = 0;
    for token in &result {
        assert!(token.start as usize >= previous && token.start < token.end);
        assert!(
            source.is_char_boundary(token.start as usize)
                && source.is_char_boundary(token.end as usize)
        );
        previous = token.end as usize;
    }
    result
        .into_iter()
        .map(|token| {
            (
                source[token.start as usize..token.end as usize].into(),
                token.kind,
            )
        })
        .collect()
}

#[test]
fn json_keys_values_escapes_numbers_and_literals() {
    use Kind::*;
    let source = "{\"标题\" : \"中文\\\"值\", \"倍率\": -1.25e+2, \"开启\": true, \"缺省\": null}";
    assert_eq!(
        tokens(Language::Json, source),
        vec![
            ("\"标题\"".into(), Key),
            ("\"中文\\\"值\"".into(), String),
            ("\"倍率\"".into(), Key),
            ("-1.25e+2".into(), Number),
            ("\"开启\"".into(), Key),
            ("true".into(), Keyword),
            ("\"缺省\"".into(), Key),
            ("null".into(), Keyword),
        ]
    );
}

#[test]
fn unterminated_json_string_and_trailing_escape_are_tolerated() {
    assert_eq!(
        tokens(Language::Json, "{\"键\": \"未写完\\"),
        vec![
            ("\"键\"".into(), Kind::Key),
            ("\"未写完\\".into(), Kind::String),
        ]
    );
    assert!(scan(Language::Json, "").is_empty());
}

#[test]
fn rust_nested_comments_and_strings_do_not_leak_keywords() {
    let source = "/* 外层 fn /* 内层 true */ 注释 */\nlet 中文 = \"// if 42\"; // return 7\n";
    assert_eq!(
        tokens(Language::Rust, source),
        vec![
            ("/* 外层 fn /* 内层 true */ 注释 */".into(), Kind::Comment),
            ("let".into(), Kind::Keyword),
            ("\"// if 42\"".into(), Kind::String),
            ("// return 7".into(), Kind::Comment),
        ]
    );
}

#[test]
fn rust_raw_byte_c_strings_and_raw_identifiers() {
    let source = "r##\"铜色 \"# fn\"## br#\"中文\"# cr\"桥\" b\"bytes\" c\"ffi\" r#type";
    let colored = tokens(Language::Rust, source);
    assert_eq!(colored.len(), 5);
    assert!(colored.iter().all(|(_, kind)| *kind == Kind::String));
    assert_eq!(colored[0].0, "r##\"铜色 \"# fn\"##");
    assert_eq!(
        tokens(Language::Rust, "r###\"尚未结束\"##"),
        vec![("r###\"尚未结束\"##".into(), Kind::String)]
    );
}

#[test]
fn rust_characters_are_distinct_from_lifetimes_and_labels() {
    let source = "'侠' b'X' '\\n' '\\u{4fa0}' '\\x41' 'static 'a 'outer:";
    let colored = tokens(Language::Rust, source);
    assert_eq!(colored.len(), 8);
    assert!(colored[..5].iter().all(|(_, kind)| *kind == Kind::String));
    assert!(colored[5..].iter().all(|(_, kind)| *kind == Kind::Type));
    assert_eq!(colored[5].0, "'static");
}

#[test]
fn rust_range_and_method_dots_are_not_part_of_numbers() {
    let source = "0..=10 42.to_string() 0xff_u8 0b101_u32 3.5e-2f32 1_000usize";
    let numbers: Vec<_> = tokens(Language::Rust, source)
        .into_iter()
        .filter(|(_, kind)| *kind == Kind::Number)
        .map(|(value, _)| value)
        .collect();
    assert_eq!(
        numbers,
        [
            "0",
            "10",
            "42",
            "0xff_u8",
            "0b101_u32",
            "3.5e-2f32",
            "1_000usize"
        ]
    );
}

#[test]
fn wgsl_keywords_types_hex_floats_and_nested_comments() {
    let source = "/* 外 /* 内 */ 层 */\n@fragment fn main() -> vec4<f32> { let x = 0x1.fp+2f; return vec4f(.5, 1.0, 2u, 1.); }";
    let colored = tokens(Language::Wgsl, source);
    assert!(colored.contains(&("/* 外 /* 内 */ 层 */".into(), Kind::Comment)));
    assert!(colored.contains(&("fn".into(), Kind::Keyword)));
    assert!(colored.contains(&("@fragment".into(), Kind::Key)));
    assert!(colored.contains(&("vec4".into(), Kind::Type)));
    assert!(colored.contains(&("vec4f".into(), Kind::Type)));
    let numbers: Vec<_> = colored
        .into_iter()
        .filter(|(_, kind)| *kind == Kind::Number)
        .map(|(value, _)| value)
        .collect();
    assert_eq!(numbers, ["0x1.fp+2f", ".5", "1.0", "2u", "1."]);
}

#[test]
fn identifier_boundaries_do_not_color_keyword_substrings() {
    assert!(tokens(Language::Rust, "let中文 iffy true_value 数据fn r#fn").is_empty());
    assert!(tokens(Language::Wgsl, "returnValue var_count falsehood").is_empty());
}

#[test]
fn malformed_unicode_input_never_emits_invalid_ranges() {
    for language in [Language::Json, Language::Rust, Language::Wgsl] {
        for source in [
            "😀 侠客\u{301}",
            "/* 注释尚未结束",
            "\"字符串😀\\",
            "b'",
            "'",
            "r####",
            "0x",
            "1e-",
            "\t\r\n",
        ] {
            tokens(language, source);
        }
    }
}

#[test]
fn extension_dispatch_is_case_insensitive_and_keeps_other_files_plain() {
    for (path, expected) in [
        ("config/A.JSON", Some(Language::Json)),
        ("log/events.jsonl", Some(Language::Json)),
        ("src/Main.RS", Some(Language::Rust)),
        ("shader.WGSL", Some(Language::Wgsl)),
        ("说明.md", None),
        ("data.toml", None),
        ("no_extension", None),
    ] {
        assert_eq!(Language::from_path(path), expected);
    }
}

#[test]
fn token_lookup_obeys_half_open_utf8_byte_ranges() {
    let source = "let x = \"中文\";";
    let result = scan(Language::Rust, source);
    assert_eq!(token_at(&result, 0), Some(Kind::Keyword));
    assert_eq!(token_at(&result, 3), None);
    assert_eq!(token_at(&result, 9), Some(Kind::String));
    assert_eq!(token_at(&result, source.len() - 1), None);
    assert_eq!(token_at(&result, source.len()), None);
}

#[test]
fn long_source_comments_and_raw_delimiters_stay_bounded() {
    let source = format!(
        "let value = r{}\"{}\"{};\n/*{}*/",
        "#".repeat(4096),
        "中文\"##".repeat(1024),
        "#".repeat(4096),
        "嵌套 /* 内容 */".repeat(4096)
    );
    let colored = tokens(Language::Rust, &source);
    assert_eq!(colored.len(), 3);
    assert_eq!(colored[1].1, Kind::String);
    assert_eq!(colored[2].1, Kind::Comment);
}

fn key(path: &str) -> SyntaxKey {
    SyntaxKey {
        document: super::super::types::SourceKey {
            path: path.into(),
            generation: 4,
        },
        revision: 3,
        language: Language::from_path(path),
    }
}

fn document() -> super::super::types::Document {
    use super::super::types::*;
    use crate::asset_catalog::DocumentFormat;
    Document {
        path: "code.rs".into(),
        format: DocumentFormat::Code,
        generation: 4,
        disk: DiskIdentity {
            path: "code.rs".into(),
            digest: [0; 32],
            bom: false,
            crlf: false,
        },
        pages: vec![],
        source: "let x = 1;".into(),
        revision: 3,
        saved_revision: 3,
        history: Default::default(),
        preview: std::sync::Arc::new(vec![]),
        preview_revision: Some(3),
        preview_clipped: false,
        mode: ViewMode::Edit,
        preview_scroll: bevy::prelude::Vec2::ZERO,
        confirm_reload: false,
        external: Default::default(),
        status: String::new(),
    }
}

#[test]
fn late_syntax_identity_rejects_other_paths_generations_and_drafts() {
    use super::super::types::Document;
    let doc = document();
    let original = key("code.rs");
    assert!(original.current(&doc));
    for changed in [
        Document {
            path: "other.rs".into(),
            ..doc.clone()
        },
        Document {
            generation: 5,
            ..doc.clone()
        },
        Document {
            revision: 4,
            ..doc.clone()
        },
    ] {
        assert!(!original.current(&changed));
    }
}

#[test]
fn invalidated_background_job_keeps_its_slot_then_adopts_only_latest_draft() {
    use super::{super::resources::DocumentState, resources::SyntaxIndex};
    use bevy::{prelude::*, tasks::AsyncComputeTaskPool};
    let doc = document();
    let mut app = App::new();
    app.insert_resource(feathrise_editor_documents::content_operations());
    app.add_plugins(MinimalPlugins)
        .insert_resource(DocumentState {
            active: Some(doc.path.clone()),
            visible: true,
            documents: vec![doc],
            ..default()
        })
        .init_resource::<SyntaxIndex>()
        .add_systems(Update, super::index::drive);
    app.world_mut().resource_mut::<SyntaxIndex>().task =
        Some(AsyncComputeTaskPool::get().spawn(std::future::pending()));
    {
        let mut state = app.world_mut().resource_mut::<DocumentState>();
        let doc = state.current_mut().unwrap();
        doc.revision = 4;
        doc.source = "fn 最新() { return 5; }".into();
    }
    for _ in 0..3 {
        app.update();
    }
    assert!(app.world().resource::<SyntaxIndex>().task.is_some());
    assert!(app.world().resource::<SyntaxIndex>().result.is_none());
    assert_eq!(
        std::sync::Arc::strong_count(
            &app.world()
                .resource::<DocumentState>()
                .current()
                .unwrap()
                .source
        ),
        1,
        "旧任务未完成时不能共享新草稿启动并行分词"
    );
    app.world_mut().resource_mut::<DocumentState>().visible = false;
    app.update();
    assert!(
        app.world().resource::<SyntaxIndex>().task.is_some(),
        "隐藏不能把在途任务移出唯一槽"
    );
    app.world_mut().resource_mut::<SyntaxIndex>().task =
        Some(AsyncComputeTaskPool::get().spawn(async {
            SyntaxResult {
                key: key("code.rs"),
                tokens: scan(Language::Rust, "let stale = 1;"),
            }
        }));
    app.world_mut().resource_mut::<DocumentState>().visible = true;
    for _ in 0..100 {
        app.update();
        if app
            .world()
            .resource::<SyntaxIndex>()
            .result
            .as_ref()
            .is_some_and(|result| result.key.revision == 4)
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let result = app
        .world()
        .resource::<SyntaxIndex>()
        .result
        .as_ref()
        .unwrap();
    let doc = app.world().resource::<DocumentState>().current().unwrap();
    assert!(result.key.current(doc));
    assert_eq!(result.tokens, scan(Language::Rust, &doc.source));
    app.world_mut().resource_mut::<DocumentState>().visible = false;
    app.update();
    assert!(app.world().resource::<SyntaxIndex>().result.is_none());
}

#[test]
fn extract_colors_original_glyph_indices_and_preserves_native_selection() {
    use super::{extract::apply, resources::*};
    use bevy::{
        prelude::*,
        ui_render::{ExtractedGlyph, ExtractedUiItem, ExtractedUiNode, ExtractedUiNodes},
    };
    let source = Entity::from_raw_u32(7).unwrap();
    let other = Entity::from_raw_u32(8).unwrap();
    let plain = crate::ui::theme::TEXT_PRIMARY.to_linear();
    let selected = LinearRgba::WHITE;
    let mut nodes = ExtractedUiNodes {
        uinodes: vec![ExtractedUiNode {
            z_order: 1.0,
            image: default(),
            clip: None,
            extracted_camera_entity: other,
            item: ExtractedUiItem::Glyphs { range: 2..4 },
            main_entity: source.into(),
            render_entity: other,
            transform: default(),
        }],
        glyphs: (0..5)
            .map(|i| ExtractedGlyph {
                color: if i == 3 { selected } else { plain },
                translation: Vec2::ZERO,
                rect: Rect::default(),
            })
            .collect(),
    };
    let paint = SyntaxPaint {
        visible: Some(VisibleColors {
            key: GlyphKey {
                source,
                syntax: key("code.rs"),
                generation: None,
                preview_tick: 0,
                lines: 0..1,
                extent: Vec2::ONE,
                width: 1.0,
                scale: 1.0,
                glyphs: 2,
            },
            colors: vec![
                GlyphColor {
                    glyph: 0,
                    start: 0,
                    end: 3,
                    kind: Kind::Keyword,
                },
                GlyphColor {
                    glyph: 1,
                    start: 4,
                    end: 5,
                    kind: Kind::Number,
                },
            ],
        }),
        selection: 4..5,
        selected_color: true,
    };
    let mut extraction = SyntaxExtraction::default();
    apply(&mut nodes, &paint, &mut extraction);
    assert_eq!(
        nodes.glyphs[2].color,
        crate::ui::theme::SYNTAX_KEYWORD.to_linear()
    );
    assert_eq!(nodes.glyphs[3].color, selected);
    assert_eq!(nodes.glyphs[0].color, plain);
    assert_eq!(nodes.glyphs[4].color, plain);
    assert_eq!(extraction.cached, Some(0));
}

/// 正式离屏预览/编辑共用断言；只检查字形映射和不变性，不通过另一份Text覆盖输入。
pub(in super::super) fn gpu_assert_syntax(app: &mut bevy::prelude::App, path: &str, editing: bool) {
    use super::super::{components::DocumentSource, resources::DocumentState};
    use super::resources::{SyntaxIndex, SyntaxPaint};
    use bevy::{
        prelude::*,
        text::{EditableText, TextLayoutInfo},
    };
    if Language::from_path(path).is_none() {
        return;
    }
    let doc = app.world().resource::<DocumentState>().current().unwrap();
    assert!(!doc.dirty(), "只读着色不得将文件标脏");
    let index = app
        .world()
        .resource::<SyntaxIndex>()
        .result
        .as_ref()
        .expect("后台分词未完成");
    assert!(index.key.current(doc));
    let paint = app.world().resource::<SyntaxPaint>();
    let visible = paint.visible.as_ref().expect("当前视区没有原生语法映射");
    assert!(visible.key.syntax.current(doc));
    assert!(!visible.colors.is_empty(), "可见源码应包含语法颜色");
    let layout = app
        .world()
        .get::<TextLayoutInfo>(visible.key.source)
        .unwrap();
    assert!(
        visible
            .colors
            .iter()
            .all(|color| (color.glyph as usize) < layout.glyphs.len()
                && token_at(&index.tokens, color.start as usize) == Some(color.kind))
    );
    let native_layout = if let Some(input) = app.world().get::<EditableText>(visible.key.source) {
        input.editor.try_layout().unwrap()
    } else {
        app.world()
            .get::<bevy::text::ComputedTextBlock>(visible.key.source)
            .unwrap()
            .buffer()
    };
    let first_byte = native_layout
        .get(visible.key.lines.start)
        .unwrap()
        .text_range()
        .start;
    let last_byte = native_layout
        .get(visible.key.lines.end - 1)
        .unwrap()
        .text_range()
        .end;
    let covered: std::collections::HashSet<_> = visible
        .colors
        .iter()
        .map(|color| {
            index
                .tokens
                .partition_point(|token| token.end <= color.start)
        })
        .collect();
    let first_token = index
        .tokens
        .partition_point(|token| token.end as usize <= first_byte);
    let last_token = index
        .tokens
        .partition_point(|token| (token.start as usize) < last_byte);
    for token in first_token..last_token {
        assert!(
            covered.contains(&token),
            "可见语法段未着色：{:?}，字节{}..{}，已着色字形{}，文件{path}",
            index.tokens[token].kind,
            index.tokens[token].start,
            index.tokens[token].end,
            visible.colors.len()
        );
    }
    if editing {
        assert!(
            app.world()
                .get::<DocumentSource>(visible.key.source)
                .is_some()
        );
        assert!(
            app.world()
                .get::<EditableText>(visible.key.source)
                .is_some()
        );
    } else {
        assert!(
            app.world()
                .get::<super::components::SyntaxPreview>(visible.key.source)
                .is_some()
        );
    }
    let world = app.world_mut();
    let mut sources = world.query_filtered::<Entity, With<DocumentSource>>();
    assert_eq!(sources.iter(world).count(), 1, "高亮不得新增第二个源码输入");
}

pub(in super::super) fn gpu_assert_input(
    app: &mut bevy::prelude::App,
    window: bevy::prelude::Entity,
    source: bevy::prelude::Entity,
    original: &str,
) {
    use super::super::resources::DocumentState;
    use super::resources::*;
    use bevy::{
        prelude::*,
        text::{EditableText, TextEdit},
    };
    let revision = app
        .world()
        .resource::<DocumentState>()
        .current()
        .unwrap()
        .revision;
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        original,
        "高亮开始前原生输入必须绑定完整原文"
    );
    app.world_mut()
        .get_mut::<EditableText>(source)
        .unwrap()
        .queue_edit(TextEdit::SelectAll);
    frames(app, 3);
    let selected = app
        .world()
        .get::<EditableText>(source)
        .unwrap()
        .editor
        .raw_selection()
        .text_range();
    assert_eq!(selected, 0..original.len());
    frames(app, 8);
    assert_eq!(
        app.world()
            .get::<EditableText>(source)
            .unwrap()
            .editor
            .raw_selection()
            .text_range(),
        selected
    );
    assert_eq!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .revision,
        revision
    );
    assert!(app.world().resource::<SyntaxPaint>().visible.is_some());

    app.world_mut()
        .get_mut::<EditableText>(source)
        .unwrap()
        .queue_edit(TextEdit::TextEnd(false));
    frames(app, 3);
    let committed = "中文测试";
    app.world_mut().write_message(bevy::window::Ime::Preedit {
        window,
        value: committed.into(),
        cursor: Some((committed.len(), committed.len())),
    });
    frames(app, 2);
    assert!(
        app.world()
            .get::<EditableText>(source)
            .unwrap()
            .is_composing()
    );
    assert_eq!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .source_text(),
        original
    );
    assert_eq!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .revision,
        revision
    );
    assert!(
        app.world().resource::<SyntaxPaint>().visible.is_none(),
        "候选期间不能把旧字节范围染到预编辑字符"
    );
    app.world_mut().write_message(bevy::window::Ime::Commit {
        window,
        value: committed.into(),
    });
    frames(app, 8);
    assert!(
        !app.world()
            .get::<EditableText>(source)
            .unwrap()
            .is_composing()
    );
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        format!("{original}{committed}").as_str()
    );
    let doc = app.world().resource::<DocumentState>().current().unwrap();
    assert!(doc.revision > revision);
    assert!(
        app.world()
            .resource::<SyntaxIndex>()
            .result
            .as_ref()
            .unwrap()
            .key
            .current(doc)
    );
    assert!(
        app.world()
            .resource::<SyntaxPaint>()
            .visible
            .as_ref()
            .unwrap()
            .key
            .syntax
            .current(doc)
    );

    // 原生组件当前没有Undo/Redo接口；删除测试提交后再验证正式保存，不能伪称已验证撤销。
    for _ in committed.chars() {
        app.world_mut()
            .get_mut::<EditableText>(source)
            .unwrap()
            .queue_edit(TextEdit::Backspace);
    }
    frames(app, 8);
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        original
    );
    assert_eq!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .source_text(),
        original
    );

    app.world_mut()
        .get_mut::<EditableText>(source)
        .unwrap()
        .queue_edit(TextEdit::TextStart(false));
    frames(app, 3); // 测量整屏着色，不能在密集文件的最后空行上测出误导性的零工作耗时。
    let generation = *app
        .world()
        .get::<bevy::text::EditableTextGeneration>(source)
        .unwrap();
    let selection = app
        .world()
        .get::<EditableText>(source)
        .unwrap()
        .editor
        .raw_selection()
        .text_range();
    let scroll = app
        .world()
        .get::<bevy::ui::widget::TextScroll>(source)
        .unwrap()
        .0;
    let mut times = Vec::new();
    use bevy::ecs::system::RunSystemOnce;
    for _ in 0..16 {
        app.world_mut().resource_mut::<SyntaxPaint>().visible = None;
        let started = std::time::Instant::now();
        app.world_mut().run_system_once(super::paint::sync).unwrap();
        times.push(started.elapsed().as_secs_f64() * 1_000.0);
    }
    times.sort_by(f64::total_cmp);
    println!(
        "语法可见映射开发机诊断：中位{:.3}ms，最大{:.3}ms",
        times[times.len() / 2],
        times[times.len() - 1]
    );
    assert!(
        *app.world()
            .get::<bevy::text::EditableTextGeneration>(source)
            .unwrap()
            == generation
    );
    assert_eq!(
        app.world()
            .get::<EditableText>(source)
            .unwrap()
            .editor
            .raw_selection()
            .text_range(),
        selection
    );
    assert_eq!(
        app.world()
            .get::<bevy::ui::widget::TextScroll>(source)
            .unwrap()
            .0,
        scroll
    );

    fn frames(app: &mut App, count: usize) {
        for _ in 0..count {
            app.update();
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
    }
}
