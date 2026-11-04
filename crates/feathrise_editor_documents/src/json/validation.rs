//! 复用serde_json的流式语法解析，不构造Value树；诊断位置在后台换算为Unicode字符列。

use feathrise_editor_documents_model::json::types::Diagnostic;

pub fn validate(source: &str) -> Option<Diagnostic> {
    // IgnoredAny保留任意精度数字的语法语义，避免为了校验把数字转成f64或折叠重复键。
    // 其解析器以有界输入的显式栈扫描容器；没有递归Value树、序列化或字段排序。
    serde_json::from_str::<serde::de::IgnoredAny>(source)
        .err()
        .map(|error| {
            let line = error.line().max(1);
            let start = source
                .match_indices('\n')
                .nth(line.saturating_sub(2))
                .filter(|_| line > 1)
                .map_or(0, |(at, _)| at + 1);
            let end = source[start..]
                .find('\n')
                .map_or(source.len(), |at| start + at);
            let mut byte = if error.is_eof() {
                end
            } else {
                (start + error.column().saturating_sub(1)).min(end)
            };
            while !source.is_char_boundary(byte) {
                byte -= 1;
            }
            let next = source[byte..end]
                .chars()
                .next()
                .map_or(byte, |ch| byte + ch.len_utf8());
            let original = error.to_string();
            let message = original.split(" at line ").next().unwrap_or(&original);
            let message = match message {
                "expected `,` or `}`" => "字段之间缺少逗号，或缺少右花括号",
                "expected `,` or `]`" => "数组项之间缺少逗号，或缺少右方括号",
                "expected `:`" => "字段名后缺少冒号",
                "key must be a string" => "字段名必须用双引号包围",
                "trailing comma" => "最后一项后不能有逗号",
                "trailing characters" => "完整JSON后还有多余内容",
                "expected value" => "此处需要JSON值",
                "EOF while parsing an object" => "对象尚未结束，请补齐字段或右花括号",
                "EOF while parsing a list" => "数组尚未结束，请补齐数组项或右方括号",
                "EOF while parsing a string" => "字符串尚未结束，请补齐双引号",
                "EOF while parsing a value" => "JSON值尚未写完整",
                "invalid number" => "数字格式不正确",
                "invalid escape" => "字符串转义不正确",
                "invalid unicode escape" => "Unicode转义不正确",
                other => other,
            }
            .to_owned();
            Diagnostic {
                line,
                column: source[start..byte].chars().count() + 1,
                selection: byte..next,
                message,
            }
        })
}
