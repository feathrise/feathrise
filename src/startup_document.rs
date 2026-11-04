//! 已保存启动地图的兼容文档；只解释工程身份、修订和地图选择，旧布局与扩展字段原样往返。
//! 正式作者工具保存各自的地图文件；这里不再承担原型对象编辑、刷草、LOD或撤销历史。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) const FILE_NAME: &str = "scene.layout.json";
pub(crate) const MAX_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Document {
    format: u32,
    project_id: String,
    #[serde(default = "initial_revision")]
    pub revision: u64,
    // objects、next_id及未来扩展属于文件原有内容，不按旧32对象/32×32草场限制重新解释。
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

fn initial_revision() -> u64 {
    1
}

impl Document {
    pub fn empty(project_id: &str) -> Self {
        Self {
            format: 1,
            project_id: project_id.into(),
            revision: 1,
            extra: BTreeMap::new(),
        }
    }

    pub fn select_map(&mut self, id: &str) -> Result<bool, String> {
        feathrise_map_content::MapId::parse(id)?;
        if feathrise_project::selection(&self.extra)? == Some(id) {
            return Ok(false);
        }
        let revision = self.revision.checked_add(1).ok_or("启动设置修订已达上限")?;
        feathrise_project::set_selection(&mut self.extra, id)?;
        self.revision = revision;
        Ok(true)
    }

    fn validate(&self, project_id: &str) -> Result<(), String> {
        if self.format != 1 {
            return Err("不支持的启动设置格式".into());
        }
        if self.project_id != project_id {
            return Err("启动设置不属于当前工程".into());
        }
        if self.revision == 0 {
            return Err("启动设置修订不能为零".into());
        }
        feathrise_project::selection(&self.extra)?;
        Ok(())
    }
}

pub(crate) fn parse(bytes: &[u8], project_id: &str) -> Result<Document, String> {
    if bytes.len() > MAX_BYTES {
        return Err("启动设置超过64KiB上限".into());
    }
    let document: Document =
        serde_json::from_slice(bytes).map_err(|error| format!("启动设置JSON无效：{error}"))?;
    document.validate(project_id)?;
    Ok(document)
}

pub(crate) fn to_bytes(document: &Document, project_id: &str) -> Result<Vec<u8>, String> {
    document.validate(project_id)?;
    let bytes = serde_json::to_vec_pretty(document).map_err(|error| error.to_string())?;
    if bytes.len() > MAX_BYTES {
        return Err("启动设置超过64KiB上限".into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn opaque_legacy_layout_does_not_impose_prototype_limits_or_drop_fields() {
        let original = json!({"format":1,"project_id":"project-a","revision":7,
            "objects":vec![json!({"id":1,"future_shape":{"keep":true}}); 33],
            "next_id":99,"runtime_authoring":{"future_field":[1,2,3]},"production_map":"MAP-A"});
        let mut document = parse(&serde_json::to_vec(&original).unwrap(), "project-a").unwrap();
        assert!(document.select_map("MAP-B").unwrap());
        let saved: Value =
            serde_json::from_slice(&to_bytes(&document, "project-a").unwrap()).unwrap();
        let mut expected = original;
        expected["production_map"] = json!("MAP-B");
        expected["revision"] = json!(8);
        assert_eq!(saved, expected);
        assert!(!document.select_map("MAP-B").unwrap());
        assert!(parse(&to_bytes(&document, "project-a").unwrap(), "project-b").is_err());
    }

    #[test]
    fn plain_startup_file_is_readable_without_prototype_layout_fields() {
        let document = parse(
            br#"{"format":1,"project_id":"p","production_map":"MAP-A"}"#,
            "p",
        )
        .unwrap();
        assert_eq!(document.revision, 1);
        assert!(!document.extra.contains_key("objects"));
        let mut document = document;
        document.revision = u64::MAX;
        let before = document.clone();
        assert!(document.select_map("MAP-B").is_err());
        assert_eq!(document, before);
    }
}
