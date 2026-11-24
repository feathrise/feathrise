//! 工程资料的原清单回读、外部冲突保护与本地图片复用。

use super::*;
use std::{fs, sync::atomic::{AtomicU64, Ordering}};

const PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1,
    8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 16, 73, 68, 65, 84, 120, 1, 1, 5, 0, 250, 255,
    0, 37, 144, 75, 255, 3, 222, 2, 0, 67, 133, 149, 62, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

struct Fixture(PathBuf);

impl Fixture {
    fn new(descriptor: &str, bytes: &[u8]) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!("feathrise-project-metadata-{}-{}-{}", std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("assets")).unwrap();
        fs::write(root.join("Cargo.toml"), b"[package]\nname='fixture'\nversion='0.1.0'\n").unwrap();
        fs::write(root.join(descriptor), bytes).unwrap();
        Self(root)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
}

#[test]
fn project_metadata_toml_preserves_comments_and_imports_original_icon_bytes() {
    let source = "# 工程清单\nformat = 1\nid = 'metadata-fixture'\nname = '旧名称' # 保留标题注释\ncargo_manifest = 'Cargo.toml'\npackage = 'fixture'\nasset_roots = ['assets']\n[unknown]\nkeep = '原格式' # 保留未知内容\n";
    let original = [b"\xef\xbb\xbf".as_slice(), source.replace('\n', "\r\n").as_bytes()].concat();
    let fixture = Fixture::new("feathrise.toml", &original);
    let old = crate::project::open(&fixture.0).unwrap();
    let before = read(&old).unwrap();
    let source_image = fixture.0.join("选中图标.png");
    fs::write(&source_image, PNG).unwrap();
    assert_eq!(preview_icon(&source_image).unwrap().size(), bevy::math::UVec2::ONE);
    let edit = || Edit { name: "新\"工程".into(), description: "第一行\n第二行".into(), icon: IconEdit::Import(source_image.clone()) };
    let (changed, after) = save(&old, &before, edit()).unwrap();
    assert_eq!(changed.name, "新\"工程");
    assert_eq!(changed.context.name, changed.name);
    assert!(old.same_session(&changed));
    assert_eq!(old.refresh_for_session().unwrap(), changed);
    assert_eq!(after.metadata.description, "第一行\n第二行");
    let icon = after.metadata.icon.as_ref().unwrap();
    assert!(icon.starts_with("assets/project-branding"));
    let saved_icon = saved_icon_path(&old, icon).unwrap();
    assert_eq!(fs::read(&saved_icon).unwrap(), PNG);
    let saved = fs::read(fixture.0.join("feathrise.toml")).unwrap();
    assert!(saved.starts_with(b"\xef\xbb\xbf"));
    let text = std::str::from_utf8(&saved).unwrap();
    assert!(text.contains(" # 保留标题注释\r\n"));
    assert!(text.contains("[unknown]\r\nkeep = '原格式' # 保留未知内容\r\n"));
    let (_, repeated) = save(&old, &after, edit()).unwrap();
    assert_eq!(repeated.raw.as_ref(), saved);
    let (_, removed) = save(&old, &after, Edit { name: changed.name, description: String::new(), icon: IconEdit::Remove }).unwrap();
    assert!(removed.metadata.icon.is_none() && removed.metadata.description.is_empty());
    assert_eq!(fs::read(saved_icon).unwrap(), PNG);
}

#[test]
fn project_metadata_legacy_json_preserves_unknown_fields_and_rejects_stale_or_bad_input() {
    let original = r#"{"format":1,"id":"legacy-fixture","name":"旧工程","cargo_manifest":"Cargo.toml","package":"fixture","unknown":{"preserve":true}}"#;
    let fixture = Fixture::new("editor-project.json", original.as_bytes());
    let project = crate::project::open(&fixture.0).unwrap();
    let before = read(&project).unwrap();
    let edit = |icon| Edit { name: "新工程".into(), description: "简介".into(), icon };
    let path = fixture.0.join("editor-project.json");
    let bad = fixture.0.join("坏图标.png");
    fs::write(&bad, b"not a PNG").unwrap();
    assert!(save(&project, &before, edit(IconEdit::Import(bad))).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    let external = original.replace("true", "false");
    fs::write(&path, &external).unwrap();
    assert!(save(&project, &before, edit(IconEdit::Keep)).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), external);
    let (_, after) = save(&project, &read(&project).unwrap(), edit(IconEdit::Keep)).unwrap();
    let saved: serde_json::Value = serde_json::from_slice(&after.raw).unwrap();
    assert_eq!(saved["unknown"]["preserve"], false);
    assert_eq!(saved["name"], "新工程");
    assert!(saved_icon_path(&project, Path::new("../unrelated.png")).is_err());

    fs::write(&path, String::from_utf8(after.raw.to_vec()).unwrap().replace("fixture\"", "changed-package\"")).unwrap();
    assert!(project.refresh_for_session().is_err());
}
