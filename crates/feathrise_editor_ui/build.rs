//! 在普通 Cargo 检查中校验内嵌词表，避免中英文参数错位进入发行文件。

use std::collections::BTreeSet;

const CATALOG: &str = "src/localization/editor.tsv";

fn main() {
    println!("cargo:rerun-if-changed={CATALOG}");
    let catalog = std::fs::read_to_string(CATALOG).expect("读取编辑器词表");
    let mut keys = BTreeSet::new();
    for line in catalog.lines().filter(|line| !line.is_empty() && !line.starts_with('#')) {
        let columns: Vec<_> = line.split('\t').collect();
        let [key, chinese, english] = columns.as_slice() else {
            panic!("编辑器词条必须包含稳定键及两种译文 {line}");
        };
        assert!(keys.insert(*key), "重复的编辑器词条 {key}");
        assert!(!key.trim().is_empty() && !chinese.trim().is_empty() && !english.trim().is_empty(), "空译文 {key}");
        assert_eq!(parameters(chinese), parameters(english), "中英文占位符不一致 {key}");
    }
}

fn parameters(mut copy: &str) -> Vec<&str> {
    let mut names = Vec::new();
    while let Some((_, parameter)) = copy.split_once('{') {
        let (name, rest) = parameter.split_once('}').expect("编辑器占位符缺少右括号");
        assert!(!name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "无效的编辑器占位符 {name}");
        names.push(name);
        copy = rest;
    }
    names.sort_unstable();
    names
}
