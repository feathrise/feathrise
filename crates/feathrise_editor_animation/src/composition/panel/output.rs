//! 所选内容另存使用工程内绝对路径、独占创建和带版本校验的覆盖。

use super::*;

pub(super) fn export(
    root: &Path,
    name: &str,
    doc: Arc<AnimationDocument>,
    spans: Vec<ContentSpan>,
    overwrite: Option<(PathBuf, String)>,
    ticket: std::sync::Weak<()>,
) -> Result<SelectionOutput, String> {
    use feathrise_editor_assets::files::{
        atomic_create, destination_hash, replace_confirmed, safe_asset_path,
    };
    let path = safe_asset_path(root, name.trim())?;
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("glb"))
    {
        return Err("副本名称需要 .glb 扩展名".into());
    }
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let parent = path
        .parent()
        .ok_or("输出目录无效")?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !parent.starts_with(&root) {
        return Err("输出位置不在当前工程资产根内".into());
    }
    let path = parent.join(path.file_name().ok_or("输出文件名无效")?);
    let overwrite = overwrite.filter(|(confirmed, _)| confirmed == &path);
    if path.try_exists().map_err(|e| e.to_string())? {
        let resolved = path.canonicalize().map_err(|e| e.to_string())?;
        if resolved != path || resolved == doc.source_path() {
            return Err("副本需使用独立于原文件的常规文件路径".into());
        }
        if overwrite.is_none() {
            return Ok(SelectionOutput::ConfirmOverwrite {
                baseline: destination_hash(&path)?,
                path,
            });
        }
    }
    let bytes = doc.encode_selection_in(&doc.resolve_content(&spans)?, Some(&root))?;
    let current = || {
        ticket
            .upgrade()
            .map(|_| ())
            .ok_or_else(|| "来源文件已切换，已取消所选另存".to_owned())
    };
    if let Some((_, baseline)) = overwrite {
        replace_confirmed(&path, &bytes, &baseline, current)?;
    } else {
        atomic_create(&path, &bytes, current)?;
    }
    if std::fs::read(&path).map_err(|e| format!("已写入，重新读取失败：{e}"))? != bytes {
        return Err("已写入，回读字节不符".into());
    }
    Ok(SelectionOutput::Written(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overwrite_uses_confirmed_disk_version_and_never_replaces_source() {
        let dir = std::env::temp_dir().join(format!(
            "feathrise-selected-overwrite-{}",
            crate::data::new_scene_clip_id()
        ));
        std::fs::create_dir(&dir).unwrap();
        let root = dir.canonicalize().unwrap();
        let path = root.join("source.glb");
        let action = include_bytes!("../../../tests/fixtures/animation_preview/animated_rig.glb");
        std::fs::write(&path, action).unwrap();
        let doc = Arc::new(AnimationDocument::load(&path).unwrap());
        let write_with = |name: &str, confirm, ticket| {
            export(
                &root,
                name,
                doc.clone(),
                vec![ContentSpan {
                    kind: ContentKind::Animation,
                    first: 0,
                    last: 0,
                }],
                confirm,
                ticket,
            )
        };
        let author = Arc::new(());
        let write = |name, confirm| write_with(name, confirm, Arc::downgrade(&author));
        assert!(write_with("cancelled.glb", None, std::sync::Weak::new()).is_err());
        assert!(!root.join("cancelled.glb").exists());
        assert!(write("source.glb", None).is_err());
        assert!(matches!(
            write("output.glb", None),
            Ok(SelectionOutput::Written(_))
        ));
        let SelectionOutput::ConfirmOverwrite { path, baseline } =
            write("output.glb", None).unwrap()
        else {
            panic!()
        };
        std::fs::write(&path, b"other writer").unwrap();
        assert!(write("output.glb", Some((path.clone(), baseline))).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"other writer");
        let SelectionOutput::ConfirmOverwrite { path, baseline } =
            write("output.glb", None).unwrap()
        else {
            panic!()
        };
        assert!(
            write_with(
                "output.glb",
                Some((path.clone(), baseline.clone())),
                std::sync::Weak::new()
            )
            .is_err()
        );
        assert!(matches!(
            write("output.glb", Some((path.clone(), baseline))),
            Ok(SelectionOutput::Written(_))
        ));
        assert_eq!(AnimationDocument::load(&path).unwrap().clips.len(), 1);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
