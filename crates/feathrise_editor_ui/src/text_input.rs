//! 共享官方单行输入初始构造；焦点、光标和组合状态仍属于唯一实体。
pub fn single_line_input(value: &str, max: usize) -> bevy::text::EditableText {
    // 保持字段原有限长策略。
    bevy::text::EditableText {
        // 使用已验证的官方编辑器，不拼接模拟光标。
        max_characters: Some(max), // 按 Unicode 字符限制而非 UTF-8 字节。
        allow_newlines: false,     // 名称字段保持单行业务语义。
        ..bevy::text::EditableText::new(value)  // 原业务值仅在新会话开始时装入。
    } // 后续编辑由官方组件保留光标和预编辑状态。
} // 完成单行输入初始化。
