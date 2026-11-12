//! 文档内查找和跨文件搜索共用字面正则的预算与词界。

use super::constants::REGEX_BYTES;

/// 当前文件与跨文件搜索共享转义、Unicode大小写和外侧词界，避免两种入口结果不同。
pub fn literal_regex(
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
) -> Result<regex::Regex, regex::Error> {
    let escaped = regex::escape(query);
    let pattern = if whole_word {
        format!(r"\b{{start-half}}{escaped}\b{{end-half}}")
    } else {
        escaped
    };
    regex::RegexBuilder::new(&pattern)
        .case_insensitive(!case_sensitive)
        .size_limit(REGEX_BYTES)
        .dfa_size_limit(REGEX_BYTES)
        .build()
}
