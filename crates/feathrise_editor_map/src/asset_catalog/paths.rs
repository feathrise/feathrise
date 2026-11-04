//! 资产引用输入的纯词法规范化：允许粘贴 Windows 分隔符，保留 glTF 子资产标签。

/// 返回相对 assets 根的规范引用；不访问文件系统，也不把 assets/ 前缀猜成显示装饰。
pub(crate) fn normalize_asset_reference(raw_path: &str) -> Result<String, String> {
    let reference = raw_path.trim().replace('\\', "/"); // 只对交互输入转换分隔符，不改已有持久文件。
    if reference.is_empty() {
        return Err("资产引用为空；请输入相对 assets/ 的文件路径".to_owned()); // 空字段不能误定位根目录。
    }
    if reference.starts_with('/') || reference.contains(':') {
        return Err("资产引用必须是相对 assets/ 的路径，不能使用绝对路径或资产源前缀".to_owned()); // 拒绝盘符、UNC 和其他根。
    }
    if reference.chars().any(|character| {
        character.is_control() || matches!(character, '*' | '?' | '"' | '<' | '>' | '|') // 保持跨平台确定的文件与标签语法。
    }) {
        return Err("资产引用包含控制字符或非法路径字符".to_owned()); // 不把无效作者输入交给后台 IO。
    }
    let (file_path, label) = reference
        .split_once('#') // 子资产标签属于引用，不属于所在目录或文件名。
        .map_or((reference.as_str(), None), |(file, label)| {
            (file, Some(label))
        }); // 普通文件仍使用相同验证链。
    validate_segments(file_path, "资产文件路径")?; // 文件路径必须明确位于 assets 根内。
    if file_path.split('/').any(|segment| segment == ".trash") {
        return Err("回收区中的资产不可作为正式资产定位，请先恢复文件".to_owned()); // 不绕过浏览器既有回收区隐藏约束。
    }
    if let Some(label) = label {
        validate_segments(label, "资产 # 子资产标签")?; // Mesh0/Primitive0 与 Material0/std 保持原样。
        if label.contains('#') {
            return Err("资产引用只能包含一个 # 子资产分隔符".to_owned()); // 防止多标签被错误当作可加载引用。
        }
    }
    Ok(reference) // 调用方可按 split_once('#') 取得所属文件，标签大小写不得改写。
}

/// 文件和子资产段都拒绝空段、当前目录别名和父级跳转，保证完整路径可精确匹配。
fn validate_segments(path: &str, context: &str) -> Result<(), String> {
    if path
        .split('/')
        .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
    {
        return Err(format!("{context}不能包含空路径段、. 或 ..")); // 不靠规范化猜测越界路径或同名别名。
    }
    Ok(()) // 合法中文目录和文件名不设人为长度上限。
}

#[cfg(test)]
mod tests {
    use super::normalize_asset_reference; // 测试只验证字符串，不读作者磁盘。

    #[test]
    fn windows_separators_and_gltf_labels_are_normalized_without_losing_identity() {
        assert_eq!(
            normalize_asset_reference("  植被\\树木.glb#Mesh0\\Primitive2  ").unwrap(), // 常见复制路径和网格标签。
            "植被/树木.glb#Mesh0/Primitive2" // 仅分隔符和外侧空白变化。
        );
        assert_eq!(
            normalize_asset_reference("gltf/tree.glb#Material0/std").unwrap(), // Bevy 0.19 标准材质子资产。
            "gltf/tree.glb#Material0/std"                                      // 不丢弃 /std。
        );
        assert_eq!(
            normalize_asset_reference("assets/real-subfolder.png").unwrap(), // assets 可能是真实同名子目录。
            "assets/real-subfolder.png" // 不擅自剥离显示前缀并定位其他文件。
        );
    }

    #[test]
    fn empty_absolute_parent_and_malformed_references_are_rejected() {
        for path in [
            "",
            "  ",
            "/outside.glb",
            "C:\\outside.glb",
            "\\\\server\\a.glb", // 空值、磁盘根与 UNC。
            "a/../b.glb",
            "../b.glb",
            "a/./b.glb",
            "a//b.glb",
            "a/", // 非唯一路径与父级跳转。
            "a.glb#",
            "a.glb#/Mesh0",
            "a.glb#Mesh0/../x",
            "a.glb#Mesh0#bad", // 损坏子资产标签。
            "maps/.trash/old.glb",
            "a\n.glb",
            "other://a.glb",
            "a?.glb", // 回收区、控制字符和外部资产源。
        ] {
            assert!(
                normalize_asset_reference(path).is_err(),
                "不应接受 {path:?}"
            ); // 每条非法路径必须明确失败。
        }
    }
}
