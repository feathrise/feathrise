//! 当前工程主资产根内的规则入口；空白和示例都是可直接保存的版本 1 文档。

pub const PATH: &str = "project_rules.json";
pub const EMPTY: &str = "{\n  \"version\": 1,\n  \"variables\": {},\n  \"rules\": []\n}\n";
pub const EXAMPLE: &str = include_str!("example.json");
