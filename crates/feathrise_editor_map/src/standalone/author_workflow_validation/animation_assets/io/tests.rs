//! 自有磁盘夹具验证热更字节与写入租约；不修改仓库资产，也不模拟AssetServer发布。

use super::*;

struct Fixture {
    paths: Paths,
    source: Vec<u8>,
}

impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "native-animation-hot-reload-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        let project = directory.canonicalize().unwrap();
        let assets = project.join("assets");
        fs::create_dir(&assets).unwrap();
        fs::create_dir(assets.join("scene")).unwrap();
        let paths = Paths { project, assets };
        let source = fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/scene/probe.glb"),
        )
        .unwrap();
        fs::write(paths.assets.join(MODEL), &source).unwrap();
        fs::write(
            paths.assets.join(TEXTURE),
            fs::read(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../tests/fixtures/scene/albedo.png"),
            )
            .unwrap(),
        )
        .unwrap();
        fs::write(
            paths.project.join("editor-project.json"),
            br#"{"id":"owned-animation-test"}"#,
        )
        .unwrap();
        let fixture = Self { paths, source };
        fixture.owner("owned-animation-test");
        fixture
    }
    fn owner(&self, id: &str) {
        fs::write(self.paths.project.join(OWNER),serde_json::to_vec(&json!({"schema":1,"purpose":"native-play-recovery","project_id":id,"project_root":self.paths.project})).unwrap()).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let temporary = std::env::temp_dir().canonicalize().unwrap();
        assert!(self.paths.project.starts_with(temporary));
        assert!(
            self.paths
                .project
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("native-animation-hot-reload-")
        );
        assert_eq!(
            self.paths.project.canonicalize().unwrap(),
            self.paths.project
        );
        fs::remove_dir_all(&self.paths.project).unwrap();
    }
}

#[test]
fn disk_hot_reload_updates_only_owned_geometry_then_material_without_corrupting_glb() {
    let fixture = Fixture::new();
    let IoResult::Written { bytes: mesh, .. } = write(
        &fixture.paths,
        Mutation::Mesh,
        Arc::new(fixture.source.clone()),
    )
    .unwrap() else {
        panic!("必须返回写入回执")
    };
    let parsed = gltf::Gltf::from_slice(&mesh).unwrap();
    let blob = parsed.blob.as_ref().unwrap();
    let positions = parsed
        .meshes()
        .next()
        .unwrap()
        .primitives()
        .next()
        .unwrap()
        .reader(|buffer| {
            assert_eq!(buffer.index(), 0);
            Some(blob.as_slice())
        })
        .read_positions()
        .unwrap()
        .collect::<Vec<_>>();
    assert_eq!(positions.len(), 4);
    assert_eq!(positions[0][0], -2.75);
    let IoResult::Written {
        bytes: material, ..
    } = write(&fixture.paths, Mutation::Material, mesh).unwrap()
    else {
        panic!("必须返回写入回执")
    };
    let parsed = gltf::Gltf::from_slice(&material).unwrap();
    let actual = parsed.materials().next().unwrap().pbr_metallic_roughness();
    assert_eq!(actual.base_color_factor(), COLOR);
    assert_eq!(actual.roughness_factor(), 0.45);
    assert_eq!(
        fs::read(fixture.paths.assets.join(MODEL))
            .unwrap()
            .as_slice(),
        material.as_slice()
    );
    // 原PNG依赖仍指向当前工程；不能把测试结果编码成SDK绝对来源。
    assert!(matches!(
        parsed.images().next().unwrap().source(),
        gltf::image::Source::Uri {
            uri: "albedo.png",
            mime_type: None
        }
    ));
    let texture = Arc::new(fs::read(fixture.paths.assets.join(TEXTURE)).unwrap());
    let IoResult::Written { bytes: texture, .. } =
        write(&fixture.paths, Mutation::Texture, texture).unwrap()
    else {
        panic!("必须返回贴图写入回执")
    };
    let decoded = image::load_from_memory(&texture).unwrap().to_rgba8();
    assert_eq!(decoded.dimensions(), (2, 2));
    assert_eq!(decoded.get_pixel(0, 0).0, PIXEL);
}

#[test]
fn wrong_owner_and_external_changes_reject_overwrite_but_repaired_attempt_can_retry() {
    let fixture = Fixture::new();
    let original = Arc::new(fixture.source.clone());
    fixture.owner("another-project");
    assert!(write(&fixture.paths, Mutation::Mesh, original.clone()).is_err());
    assert_eq!(
        fs::read(fixture.paths.assets.join(MODEL)).unwrap(),
        fixture.source
    );
    fixture.owner("owned-animation-test");
    fs::write(fixture.paths.assets.join(MODEL), b"external author bytes").unwrap();
    assert!(write(&fixture.paths, Mutation::Mesh, original.clone()).is_err());
    assert_eq!(
        fs::read(fixture.paths.assets.join(MODEL)).unwrap(),
        b"external author bytes"
    );
    fs::write(fixture.paths.assets.join(MODEL), fixture.source.as_slice()).unwrap();
    assert!(write(&fixture.paths, Mutation::Mesh, original).is_ok());
}
