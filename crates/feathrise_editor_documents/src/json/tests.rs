//! 纯内容处理回归：语法、Unicode、词元保真与输出预算无需场景宿主。

use super::validation;
use bevy::prelude::default;
use feathrise_editor_documents_model::history::types as history;

#[test]
fn json_diagnostic_uses_unicode_columns_and_exact_source_bytes() {
    let source = "{\n  \"中文\": 1 \"后续\": 2\n}";
    let error = validation::validate(source).unwrap();
    assert_eq!((error.line, error.column), (2, 11));
    assert_eq!(&source[error.selection], "\"");
    assert!(error.message.contains("逗号"));
    let end = validation::validate("{\n").unwrap();
    assert_eq!((end.line, end.column), (2, 1));
    assert_eq!(end.selection, 2..2);
    let end = validation::validate("{\"a\":").unwrap();
    assert_eq!((end.line, end.column), (1, 6));
    assert_eq!(
        end.selection,
        5..5,
        "EOF定位在完整输入之后，不能指向已存在的冒号"
    );
}

#[test]
fn json_validation_checks_the_complete_document_and_strict_grammar() {
    for source in [
        "",
        "[1,]",
        "{\"a\":1,}",
        "{\"a\" 1}",
        "{a:1}",
        "01",
        "1e",
        "true false",
        "// 注释\n{}",
        "{\"a\":\"\\x\"}",
        "{\"a\":\"换\n行\"}",
        "[1}",
        "{\"a\":",
    ] {
        assert!(validation::validate(source).is_some(), "应拒绝：{source}");
    }
    for source in [
        "{}",
        "[]",
        "true",
        "null",
        "\"中文🙂\"",
        "1.2300e+99999",
        "{\"z\":123456789012345678901234567890,\"z\":-0,\"a\":[]}",
    ] {
        assert!(
            validation::validate(source).is_none(),
            "仅校验语法，不转换或折叠值：{source}"
        );
    }
}

#[test]
fn json_format_preserves_order_duplicate_keys_numbers_and_escaped_strings() {
    let source = r#"{ "z":1.2300e+04,"a":123456789012345678901234567890,"a":"中文\u4e00\"[]{}", "empty": { },"rows":[true,null,[]] }"#;
    assert!(validation::validate(source).is_none());
    let expected = "{\n  \"z\": 1.2300e+04,\n  \"a\": 123456789012345678901234567890,\n  \"a\": \"中文\\u4e00\\\"[]{}\",\n  \"empty\": {},\n  \"rows\": [\n    true,\n    null,\n    []\n  ]\n}\n";
    let result = super::format(source, false, false, default())
        .unwrap()
        .unwrap();
    assert_eq!(result.source.as_ref(), expected);
    assert!(validation::validate(&result.source).is_none());
    assert!(
        super::format(&result.source, false, false, default())
            .unwrap()
            .is_none()
    );
}

#[test]
fn json_format_scenes_reversed_unicode_selection_without_changing_token_bytes() {
    let source = "{\"名字\":\"中文🙂\",\"值\":1}";
    let start = source.find("中文🙂").unwrap();
    let selection = history::Selection {
        anchor: start + "中文🙂".len(),
        focus: start,
    };
    let result = super::format(source, true, true, selection)
        .unwrap()
        .unwrap();
    assert!(result.selection.anchor > result.selection.focus);
    assert_eq!(
        &result.source[result.selection.focus..result.selection.anchor],
        "中文🙂"
    );
    assert_eq!(
        result.pages[0].as_ref(),
        result.source.replace('\n', "\r\n")
    );
    assert!(result.source.is_char_boundary(result.selection.anchor));
    assert!(result.source.is_char_boundary(result.selection.focus));
}

#[test]
fn json_format_bounds_whitespace_growth_and_accounts_for_bom_and_crlf() {
    let dense = format!("[{}0]", "0,".repeat(60_000));
    assert!(validation::validate(&dense).is_none());
    assert!(
        super::format(&dense, false, false, default())
            .unwrap_err()
            .contains("256 KiB")
    );
    let near = format!(
        "\"{}\"",
        "a".repeat(feathrise_editor_documents_model::constants::MAX_FILE_BYTES - 3)
    );
    assert!(super::format(&near, false, false, default()).is_ok());
    assert!(super::format(&near, true, true, default()).is_err());
    let deep = format!("{}0{}", "[".repeat(2048), "]".repeat(2048));
    assert!(validation::validate(&deep).is_none()); // 显式栈，不生成递归Value树。
    assert!(super::format(&deep, false, false, default()).is_err());
}

#[test]
fn public_formatter_rejects_invalid_input_and_readonly_diagnostics_do_not_format() {
    for source in ["}", "[1}", "{\"x\":\"\\", "中文"] {
        assert!(super::format(source, false, false, default()).is_err());
    }
    let result = super::process("{\"x\":1}", false, false, None);
    assert!(result.error.is_none());
    assert!(result.formatted.unwrap().is_none());
    assert!(super::supported("folder/config.JSON"));
    assert!(!super::supported("folder/config.json5"));
}
