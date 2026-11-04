//! 文档构件固定契约。
pub mod components;
pub mod types;

/// JSON语法工具只接管显式.json，其他格式保持各自文本保存语义。
pub fn supported(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
}
