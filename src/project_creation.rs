//! 新建表单的纯路径规则；界面只做有界字符串校验，磁盘检查和模板发布仍由原后台任务负责。
use std::path::{Path, PathBuf};

pub(crate) fn input_path(value: &str) -> &str {
    let value = value.trim();
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value)
}

/// 父目录与工程名分开输入；工程名必须恰好是一个Windows目录名，不能被路径拼接解释成跳转。
pub(crate) fn destination(parent: &str, name: &str) -> Result<PathBuf, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("请填写工程名称。".into());
    }
    if name.len() > 256 || name.encode_utf16().count() > 255 {
        return Err("工程名称过长，请缩短后再创建。".into());
    }
    if name == "."
        || name == ".."
        || name.ends_with('.')
        || name
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
    {
        return Err("名称不能含路径分隔符或 <> : \" | ? *，也不能以句点结尾。".into());
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix).is_some_and(|n| {
                matches!(
                    n,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        })
    {
        return Err("这是Windows保留的目录名，请换一个工程名称。".into());
    }
    let parent = Path::new(input_path(parent));
    if !parent.is_absolute() {
        return Err("请选择父目录，或填写完整路径，例如 D:\\Games。".into());
    }
    Ok(parent.join(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_name_is_one_windows_directory_and_cannot_escape_parent() {
        assert_eq!(
            destination("D:\\作者工程", "  山水 工程  ").unwrap(),
            std::path::PathBuf::from("D:\\作者工程\\山水 工程")
        );
        for name in [
            "",
            ".",
            "..",
            "../外部",
            "D:\\外部",
            "a/b",
            "a\\b",
            "坏:名",
            "NUL",
            "con.txt",
            "LPT1",
            "COM¹",
            "结尾.",
        ] {
            assert!(destination("D:\\作者工程", name).is_err(), "未拒绝：{name}");
        }
        assert!(destination("相对目录", "工程").is_err());
        assert_eq!(
            destination("\"D:\\含 空格\"", "中文").unwrap(),
            std::path::PathBuf::from("D:\\含 空格\\中文")
        );
    }
}
