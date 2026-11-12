//! 后台生成逻辑行和冻结选区内的全文字面匹配。

use feathrise_editor_documents_model::navigation::{
    query::literal_regex,
    types::{IndexKey, TextIndex},
};

pub(super) fn build(key: IndexKey, source: &str) -> TextIndex {
    // 输入已统一为LF。末尾换行额外产生真实空行；字节索引可直接交给原生Parley导航。
    let mut lines = vec![0];
    lines.extend(
        source
            .bytes()
            .enumerate()
            .filter_map(|(i, ch)| (ch == b'\n').then_some(i + 1)),
    );
    let (matches, error) = if key.query.is_empty() {
        (Vec::new(), None)
    } else {
        // 用户始终输入字面文字；Unicode大小写和组合字符词界交给已锁定的成熟引擎。
        // 半词界只检查两侧，不把含引号、点号或斜线的查询强制限定为一个标识符。
        match literal_regex(
            &key.query,
            key.options.case_sensitive,
            key.options.whole_word,
        ) {
            Ok(regex) => {
                let scope = key.scope.clone().unwrap_or(0..source.len());
                let mut matches = Vec::new();
                let mut cursor = scope.start;
                // 从冻结范围起点开始，不能先让范围外的重叠匹配吃掉范围内候选；
                // find_at保留整篇上下文，选区边缘也不能伪造一个新的词界。
                while cursor < scope.end && cursor < source.len() {
                    let Some(found) = regex.find_at(source, cursor) else {
                        break;
                    };
                    if found.end() > scope.end {
                        break;
                    }
                    matches.push(found.range());
                    cursor = found.end(); // 非空字面查询始终前进，替换候选互不重叠。
                }
                (matches, None)
            }
            Err(_) => (
                Vec::new(),
                Some("查找条件超过后台预算，请缩短查询后重试".into()),
            ),
        }
    };
    TextIndex {
        key,
        lines,
        matches,
        error,
    }
}
