//! 工程启动场景的有界 JSON 读写；字段和身份校验由共享 SDK 定义。

pub(crate) use feathrise_scene_content::{
    MAX_SCENE_STARTUP_BYTES as MAX_BYTES, SCENE_STARTUP_FILE_NAME as FILE_NAME, SceneStartup as Document,
};

pub(crate) fn parse(bytes: &[u8], project_id: &str) -> Result<Document, String> {
    Document::parse(bytes, project_id)
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

    #[test]
    fn startup_scene_revision_and_project_identity_round_trip() {
        let mut document = Document::empty("project-a");
        assert_eq!(document.selected(), None);
        assert!(document.select_scene("SCENE-A").unwrap());
        assert!(!document.select_scene("SCENE-A").unwrap());
        let bytes = to_bytes(&document, "project-a").unwrap();
        assert_eq!(parse(&bytes, "project-a").unwrap(), document);
        assert!(parse(&bytes, "project-b").is_err());
        assert_eq!(document.revision, 2);

        document.revision = u64::MAX;
        let before = document.clone();
        assert!(document.select_scene("SCENE-B").is_err());
        assert_eq!(document, before);
    }

    #[test]
    fn startup_scene_rejects_old_format_and_invalid_identity() {
        for bytes in [
            br#"{"format":1,"project_id":"p","revision":1,"production_map":"MAP-A"}"#.as_slice(),
            br#"{"format":2,"project_id":"p","revision":1,"startup_scene_id":"../other"}"#,
            br#"{"format":2,"project_id":"p","revision":1,"startup_scene_id":null}"#,
        ] {
            assert!(parse(bytes, "p").is_err());
        }
    }
}
