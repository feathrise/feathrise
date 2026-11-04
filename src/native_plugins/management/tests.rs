//! 在独占夹具里执行单文件与双范围真实IO事务；静态共享库夹具不冒充动态加载验收。
use super::{packages, types::*};
use feathrise_plugin_api::metadata::test_support::write_metadata_fixture;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

fn legacy_package(fixture: &Fixture) -> PathBuf {
    let legacy = fixture.project().join("plugins/old-package");
    fs::create_dir_all(&legacy).unwrap();
    fs::write(legacy.join("plugin.json"), br#"{"format":1,"id":"fixture.legacy","tool":"legacy"}"#).unwrap();
    fs::write(legacy.join("old.dll"), b"legacy-content-must-survive").unwrap();
    legacy
}

#[test]
fn legacy_archive_receipt_failure_restores_the_original_directory() {
    let fixture = Fixture::new();
    let legacy = legacy_package(&fixture);
    let manager = fixture.manager(PluginScope::Project);
    fs::create_dir_all(&manager).unwrap();
    fs::write(manager.join("receipts"), b"cannot-publish-receipt").unwrap();
    let result = packages::stage_at(&fixture.roots, PluginScope::Project, ManagementCommand::Remove { id: "fixture.legacy".into() });
    assert!(result.as_ref().err().is_some_and(|error| error.contains("原目录已恢复")), "{result:?}");
    assert_eq!(fs::read(legacy.join("old.dll")).unwrap(), b"legacy-content-must-survive");
    assert!(fixture.catalog(PluginScope::Project).entries.iter().any(|entry| entry.legacy));
}

#[test]
fn legacy_archive_rejects_a_changed_identity_and_duplicate_ids() {
    let fixture = Fixture::new();
    let legacy = legacy_package(&fixture);
    let manifest = legacy.join("plugin.json");
    assert_eq!(fixture.catalog(PluginScope::Project).entries[0].id, "fixture.legacy");
    fs::write(&manifest, br#"{"format":1,"id":"fixture.changed","tool":"legacy"}"#).unwrap();
    assert!(packages::stage_at(&fixture.roots, PluginScope::Project, ManagementCommand::Remove { id: "fixture.legacy".into() }).is_err());
    assert!(legacy.is_dir());
    fs::write(&manifest, br#"{"format":1,"id":"fixture.legacy","tool":"legacy"}"#).unwrap();
    let second = fixture.project().join("plugins/second-old-package");
    fs::create_dir(&second).unwrap();
    fs::copy(&manifest, second.join("plugin.json")).unwrap();
    assert!(packages::stage_at(&fixture.roots, PluginScope::Project, ManagementCommand::Remove { id: "fixture.legacy".into() })
        .unwrap_err().contains("重复插件ID"));
    assert!(legacy.is_dir() && second.is_dir());
}

#[test]
fn legacy_archive_rejects_directory_budget_without_removing_any_file() {
    let fixture = Fixture::new();
    let legacy = legacy_package(&fixture);
    for index in 0..256 {
        fs::write(legacy.join(format!("extra-{index}")), b"preserve").unwrap();
    }
    assert!(packages::stage_at(&fixture.roots, PluginScope::Project, ManagementCommand::Remove { id: "fixture.legacy".into() })
        .unwrap_err().contains("归档预算"));
    assert_eq!(fs::read_dir(&legacy).unwrap().count(), 258);
    assert_eq!(fs::read(legacy.join("old.dll")).unwrap(), b"legacy-content-must-survive");
}

#[test]
fn legacy_archive_refuses_a_link_to_another_directory() {
    let fixture = Fixture::new();
    let target = fixture.root.join("unrelated");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("plugin.json"), br#"{"format":1,"id":"fixture.legacy","tool":"legacy"}"#).unwrap();
    fs::write(target.join("owner-data"), b"unrelated-must-survive").unwrap();
    let link = fixture.project().join("plugins/linked-package");
    fs::create_dir_all(link.parent().unwrap()).unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Junction无需开发者模式，创建与移除均只操作本测试owner目录内的确切路径。
        let result = std::process::Command::new("cmd.exe")
            .args(["/D", "/C", "mklink", "/J"]).arg(&link).arg(&target)
            .creation_flags(0x08000000).output().unwrap();
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let result = packages::stage_at(&fixture.roots, PluginScope::Project, ManagementCommand::Remove { id: "fixture.legacy".into() });
    assert!(result.is_err(), "{result:?}");
    assert_eq!(fs::read(target.join("owner-data")).unwrap(), b"unrelated-must-survive");
    #[cfg(windows)]
    fs::remove_dir(&link).unwrap();
    #[cfg(unix)]
    fs::remove_file(&link).unwrap();
}

struct Fixture {
    root: PathBuf,
    roots: PluginRoots,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "feathrise-single-plugin-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        fs::write(root.join("owner"), b"single-binary-plugin-unit-fixture").unwrap();
        let project = root.join("project");
        fs::create_dir(&project).unwrap();
        fs::create_dir(project.join("assets")).unwrap();
        fs::write(
            project.join("assets/author.txt"),
            b"author-data-must-survive",
        )
        .unwrap();
        fs::write(
            project.join("Cargo.toml"),
            "[package]\nname='fixture'\nversion='0.1.0'\n[dependencies]\n",
        )
        .unwrap();
        Self {
            roots: PluginRoots {
                project: Some(project),
                global: root.join("global-plugins"),
            },
            root,
        }
    }
    fn project(&self) -> &Path {
        self.roots.project.as_deref().unwrap()
    }
    fn package(&self, name: &str, id: &str, version: &str) -> PathBuf {
        let file = self.root.join(format!("{name}.{}", ext()));
        write_metadata_fixture(&file, &metadata(id, version)).unwrap();
        file
    }
    fn installed(&self, id: &str) -> PathBuf {
        self.project()
            .join("plugins")
            .join(packages::binary_filename(id))
    }
    fn stage(&self, scope: PluginScope, command: ManagementCommand) -> Catalog {
        packages::stage_at(&self.roots, scope, command).unwrap()
    }
    fn apply(&self) {
        assert!(packages::apply_pending_with_roots(&self.roots).is_empty());
    }
    fn install(&self, scope: PluginScope, name: &str, id: &str, version: &str) {
        self.stage(
            scope,
            ManagementCommand::Install {
                source: self.package(name, id, version),
                replace: false,
            },
        );
        self.apply();
    }
    fn catalog(&self, scope: PluginScope) -> Catalog {
        packages::catalog_at(&self.roots, scope).unwrap()
    }
    fn global_path(&self, id: &str) -> PathBuf {
        self.catalog(PluginScope::Global)
            .entries
            .into_iter()
            .find(|e| e.id == id)
            .unwrap()
            .path
    }
    fn journal(&self, scope: PluginScope) -> Journal {
        let path = if scope == PluginScope::Project {
            self.project()
                .join(".editor/plugin-management/pending.json")
        } else {
            self.roots.global.join(".management/pending.json")
        };
        serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
    }
    fn manager(&self, scope: PluginScope) -> PathBuf {
        if scope == PluginScope::Project {
            self.project().join(".editor/plugin-management")
        } else {
            self.roots.global.join(".management")
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let parent = std::env::temp_dir().canonicalize().unwrap();
        // 仅回收本测试create_dir成功且owner仍匹配的精确根；不扫描或清理其他工程。
        if self.root.parent() == Some(parent.as_path())
            && self
                .root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("feathrise-single-plugin-")
            && fs::read(self.root.join("owner")).ok().as_deref()
                == Some(b"single-binary-plugin-unit-fixture")
        {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}
fn ext() -> &'static str {
    match std::env::consts::OS {
        "windows" => "dll",
        "macos" => "dylib",
        _ => "so",
    }
}
fn metadata(id: &str, version: &str) -> PackageManifest {
    PackageManifest {
        format: 1,
        id: id.into(),
        tool: "事务夹具插件".into(),
        version: version.into(),
        target_os: std::env::consts::OS.into(),
        target_arch: std::env::consts::ARCH.into(),
        abi_major: feathrise_plugin_api::ABI_MAJOR,
        runtime_dependencies: Vec::new(),
        runtime: None,
    }
}
fn enable(id: &str, enabled: bool) -> ManagementCommand {
    ManagementCommand::SetEnabled {
        id: id.into(),
        enabled,
    }
}
fn remove(id: &str) -> ManagementCommand {
    ManagementCommand::Remove { id: id.into() }
}
fn cancel(id: &str) -> ManagementCommand {
    ManagementCommand::Cancel { id: id.into() }
}

#[test]
fn single_file_static_inspection_rejects_directory_foreign_target_and_renamed_binary() {
    let f = Fixture::new();
    let file = f.package("good", "fixture.good", "1.0.0");
    assert_eq!(packages::inspect_package(&file).unwrap().0.version, "1.0.0");
    assert!(packages::inspect_package(f.project()).is_err());
    let mut foreign = metadata("fixture.foreign", "1.0.0");
    foreign.target_os = if std::env::consts::OS == "linux" {
        "macos"
    } else {
        "linux"
    }
    .into();
    let file = f.root.join(if foreign.target_os == "macos" {
        "foreign.dylib"
    } else {
        "foreign.so"
    });
    write_metadata_fixture(&file, &foreign).unwrap();
    assert!(
        packages::inspect_package(&file)
            .unwrap_err()
            .contains("当前编辑器")
    );
    let renamed = f.root.join("renamed.dll");
    fs::copy(&file, &renamed).unwrap();
    assert!(packages::inspect_package(&renamed).is_err());
}

#[test]
fn invalid_sibling_and_two_new_files_with_duplicate_id_are_independently_diagnosed() {
    let f = Fixture::new();
    f.install(PluginScope::Project, "good", "fixture.good", "1.0.0");
    fs::write(f.installed("damaged"), b"not-a-plugin").unwrap();
    let catalog = f.catalog(PluginScope::Project);
    assert_eq!(catalog.entries.len(), 2);
    assert!(
        catalog
            .entries
            .iter()
            .find(|e| e.id == "fixture.good")
            .unwrap()
            .error
            .is_none()
    );
    assert!(
        catalog
            .entries
            .iter()
            .find(|e| e.id == "damaged")
            .unwrap()
            .error
            .is_some()
    );
    let duplicate = f.package("duplicate", "fixture.good", "2.0.0");
    fs::copy(
        duplicate,
        f.project()
            .join("plugins")
            .join(format!("duplicate.{}", ext())),
    )
    .unwrap();
    assert_eq!(
        f.catalog(PluginScope::Project)
            .entries
            .iter()
            .filter(|e| e.id == "fixture.good"
                && e.error.as_deref().is_some_and(|e| e.contains("重复")))
            .count(),
        2
    );
}

#[test]
fn project_install_enablement_and_removal_wait_for_reopen_and_keep_author_data() {
    let f = Fixture::new();
    let catalog = f.stage(
        PluginScope::Project,
        ManagementCommand::Install {
            source: f.package("v1", "fixture.tool", "1.0.0"),
            replace: false,
        },
    );
    assert_eq!(
        catalog.entries[0].pending.as_ref().unwrap().kind,
        PendingKind::Install
    );
    assert!(!f.installed("fixture.tool").exists());
    f.apply();
    assert!(f.installed("fixture.tool").is_file());
    f.stage(PluginScope::Project, enable("fixture.tool", false));
    assert!(f.catalog(PluginScope::Project).entries[0].enabled);
    f.apply();
    assert!(!f.catalog(PluginScope::Project).entries[0].enabled);
    f.stage(PluginScope::Project, enable("fixture.tool", true));
    f.apply();
    assert!(f.catalog(PluginScope::Project).entries[0].enabled);
    f.stage(PluginScope::Project, remove("fixture.tool"));
    assert!(f.installed("fixture.tool").exists());
    f.apply();
    assert!(!f.installed("fixture.tool").exists());
    assert_eq!(
        fs::read(f.project().join("assets/author.txt")).unwrap(),
        b"author-data-must-survive"
    );
    assert!(f.manager(PluginScope::Project).join("backups").is_dir());
}

#[test]
fn update_cancel_keeps_current_bytes_and_second_update_uses_new_metadata() {
    let f = Fixture::new();
    f.install(PluginScope::Project, "v1", "fixture.tool", "1.0.0");
    let before = fs::read(f.installed("fixture.tool")).unwrap();
    let source = f.package("v2", "fixture.tool", "2.0.0");
    let catalog = f.stage(
        PluginScope::Project,
        ManagementCommand::Install {
            source: source.clone(),
            replace: true,
        },
    );
    assert_eq!(catalog.entries[0].version.as_deref(), Some("1.0.0"));
    assert_eq!(
        catalog.entries[0]
            .pending
            .as_ref()
            .unwrap()
            .version
            .as_deref(),
        Some("2.0.0")
    );
    assert_eq!(fs::read(f.installed("fixture.tool")).unwrap(), before);
    assert!(packages::stage_at(&f.roots, PluginScope::Project, remove("fixture.tool")).is_err());
    f.stage(PluginScope::Project, cancel("fixture.tool"));
    f.apply();
    assert_eq!(fs::read(f.installed("fixture.tool")).unwrap(), before);
    f.stage(
        PluginScope::Project,
        ManagementCommand::Install {
            source,
            replace: true,
        },
    );
    f.apply();
    assert_eq!(
        f.catalog(PluginScope::Project).entries[0]
            .version
            .as_deref(),
        Some("2.0.0")
    );
}

#[test]
fn staged_tampering_rejects_update_without_touching_formal_binary() {
    let f = Fixture::new();
    f.install(PluginScope::Project, "v1", "fixture.tool", "1.0.0");
    let before = fs::read(f.installed("fixture.tool")).unwrap();
    f.stage(
        PluginScope::Project,
        ManagementCommand::Install {
            source: f.package("v2", "fixture.tool", "2.0.0"),
            replace: true,
        },
    );
    let record = f
        .journal(PluginScope::Project)
        .changes
        .remove("fixture.tool")
        .unwrap();
    fs::write(
        f.manager(PluginScope::Project)
            .join("staged")
            .join(record.ticket)
            .join(record.destination),
        b"external-change",
    )
    .unwrap();
    assert!(!packages::apply_pending_with_roots(&f.roots).is_empty());
    assert_eq!(fs::read(f.installed("fixture.tool")).unwrap(), before);
    assert!(f.catalog(PluginScope::Project).entries[0].pending.is_some());
}

#[test]
fn formal_modification_rejects_removal_and_interrupted_update_recovers_backup() {
    let f = Fixture::new();
    f.install(PluginScope::Project, "v1", "fixture.tool", "1.0.0");
    f.stage(PluginScope::Project, remove("fixture.tool"));
    fs::write(f.installed("fixture.tool"), b"external-version").unwrap();
    assert!(!packages::apply_pending_with_roots(&f.roots).is_empty());
    assert_eq!(
        fs::read(f.installed("fixture.tool")).unwrap(),
        b"external-version"
    );
    f.stage(PluginScope::Project, cancel("fixture.tool"));
    let original = f.package("restored", "fixture.tool", "1.0.0");
    fs::copy(original, f.installed("fixture.tool")).unwrap();
    let before = fs::read(f.installed("fixture.tool")).unwrap();
    f.stage(
        PluginScope::Project,
        ManagementCommand::Install {
            source: f.package("v2", "fixture.tool", "2.0.0"),
            replace: true,
        },
    );
    let record = f
        .journal(PluginScope::Project)
        .changes
        .remove("fixture.tool")
        .unwrap();
    let backup = f
        .manager(PluginScope::Project)
        .join("backups")
        .join(&record.ticket)
        .join(&record.destination);
    fs::create_dir_all(backup.parent().unwrap()).unwrap();
    fs::rename(f.installed("fixture.tool"), backup).unwrap();
    fs::write(
        f.manager(PluginScope::Project)
            .join("staged")
            .join(record.ticket)
            .join(record.destination),
        b"broken-after-interruption",
    )
    .unwrap();
    assert!(!packages::apply_pending_with_roots(&f.roots).is_empty());
    assert_eq!(fs::read(f.installed("fixture.tool")).unwrap(), before);
}

#[test]
fn runtime_requirements_check_alias_target_and_workspace_declarations() {
    let f = Fixture::new();
    let file = f.package("runtime", "fixture.runtime", "1.0.0");
    let mut meta = metadata("fixture.runtime", "1.0.0");
    meta.runtime_dependencies = vec![
        "required_alias".into(),
        "target_runtime".into(),
        "workspace_runtime".into(),
        "missing_runtime".into(),
    ];
    write_metadata_fixture(&file, &meta).unwrap();
    fs::write(f.project().join("Cargo.toml"),"[dependencies]\nrenamed={package='required_alias',version='1'}\nworkspace_runtime={workspace=true}\n[target.'cfg(windows)'.dependencies]\ntarget_runtime='1'\n").unwrap();
    f.stage(
        PluginScope::Project,
        ManagementCommand::Install {
            source: file,
            replace: false,
        },
    );
    f.apply();
    assert_eq!(
        f.catalog(PluginScope::Project).entries[0].runtime_missing,
        vec!["missing_runtime"]
    );
}

#[test]
fn damaged_single_binary_can_be_removed_without_loading() {
    let f = Fixture::new();
    fs::create_dir(f.project().join("plugins")).unwrap();
    fs::write(f.installed("damaged"), b"broken").unwrap();
    f.stage(PluginScope::Project, remove("damaged"));
    f.apply();
    assert!(!f.installed("damaged").exists());
    assert_eq!(
        fs::read(f.project().join("assets/author.txt")).unwrap(),
        b"author-data-must-survive"
    );
}

#[test]
fn duplicate_install_unknown_update_wrong_identity_and_occupied_filename_are_rejected() {
    let f = Fixture::new();
    let file = f.package("new", "fixture.new", "1.0.0");
    assert!(
        packages::stage_at(
            &f.roots,
            PluginScope::Project,
            ManagementCommand::Install {
                source: file.clone(),
                replace: true
            }
        )
        .is_err()
    );
    f.install(PluginScope::Project, "v1", "fixture.tool", "1.0.0");
    assert!(
        packages::stage_at(
            &f.roots,
            PluginScope::Project,
            ManagementCommand::Install {
                source: f.package("again", "fixture.tool", "2.0.0"),
                replace: false
            }
        )
        .is_err()
    );
    assert!(
        packages::stage_update_at(&f.roots, PluginScope::Project, file.clone(), "fixture.tool")
            .is_err()
    );
    fs::copy(
        f.package("unrelated", "fixture.other", "1.0.0"),
        f.installed("fixture.new"),
    )
    .unwrap();
    assert!(
        packages::stage_at(
            &f.roots,
            PluginScope::Project,
            ManagementCommand::Install {
                source: file,
                replace: false
            }
        )
        .is_err()
    );
    assert_eq!(
        packages::inspect_package(&f.installed("fixture.new"))
            .unwrap()
            .0
            .id,
        "fixture.other"
    );
}

#[test]
fn project_overrides_global_even_when_disabled_or_damaged() {
    let f = Fixture::new();
    f.install(PluginScope::Global, "global", "fixture.tool", "1.0.0");
    assert!(f.catalog(PluginScope::Project).entries[0].inherited);
    f.install(PluginScope::Project, "project", "fixture.tool", "2.0.0");
    let project = f.catalog(PluginScope::Project).entries.remove(0);
    assert_eq!(project.scope, PluginScope::Project);
    assert!(!project.inherited && project.global_available);
    let global = f.catalog(PluginScope::Global).entries.remove(0);
    assert!(global.overridden_by_project && !global.effective);
    f.stage(PluginScope::Project, enable("fixture.tool", false));
    f.apply();
    let entries = f.catalog(PluginScope::Project).entries;
    assert_eq!(entries.len(), 1);
    assert!(!entries[0].enabled && entries[0].scope == PluginScope::Project);
    fs::write(f.installed("fixture.tool"), b"damaged-project").unwrap();
    let entries = f.catalog(PluginScope::Project).entries;
    assert_eq!(entries.len(), 1);
    assert!(entries[0].error.is_some() && entries[0].scope == PluginScope::Project);
    assert!(
        super::super::loader::discover_with_roots(&f.roots)
            .unwrap()
            .packages
            .is_empty()
    );
}

#[test]
fn project_can_disable_inheritance_without_mutating_global_or_another_project() {
    let f = Fixture::new();
    f.install(PluginScope::Global, "global", "fixture.tool", "1.0.0");
    let global = f.global_path("fixture.tool");
    let bytes = fs::read(&global).unwrap();
    f.stage(PluginScope::Project, enable("fixture.tool", false));
    f.apply();
    assert!(!f.catalog(PluginScope::Project).entries[0].enabled);
    assert!(f.catalog(PluginScope::Global).entries[0].enabled);
    let other = f.root.join("other-project");
    fs::create_dir(&other).unwrap();
    fs::write(other.join("Cargo.toml"), "[dependencies]\n").unwrap();
    let other_roots = PluginRoots {
        project: Some(other),
        global: f.roots.global.clone(),
    };
    assert!(
        packages::catalog_at(&other_roots, PluginScope::Project)
            .unwrap()
            .entries[0]
            .enabled
    );
    assert_eq!(fs::read(global).unwrap(), bytes);
    assert!(packages::stage_at(&f.roots, PluginScope::Project, remove("fixture.tool")).is_err());
    f.stage(PluginScope::Project, enable("fixture.tool", true));
    f.apply();
    assert!(f.catalog(PluginScope::Project).entries[0].enabled);
}

#[test]
fn global_disablement_does_not_disable_project_copy_and_inherited_enable_respects_global_switch() {
    let f = Fixture::new();
    f.install(PluginScope::Global, "global", "fixture.tool", "1.0.0");
    f.stage(PluginScope::Global, enable("fixture.tool", false));
    f.apply();
    assert!(!f.catalog(PluginScope::Project).entries[0].enabled);
    assert!(
        packages::stage_at(&f.roots, PluginScope::Project, enable("fixture.tool", true)).is_err()
    );
    f.install(PluginScope::Project, "local", "fixture.tool", "2.0.0");
    assert!(f.catalog(PluginScope::Project).entries[0].enabled);
    assert!(!f.catalog(PluginScope::Global).entries[0].enabled);
}

#[test]
fn removing_project_copy_reveals_global_copy_on_reopen() {
    let f = Fixture::new();
    f.install(PluginScope::Global, "global", "fixture.tool", "1.0.0");
    f.install(PluginScope::Project, "project", "fixture.tool", "2.0.0");
    f.stage(PluginScope::Project, enable("fixture.tool", false));
    f.apply();
    assert!(!f.catalog(PluginScope::Project).entries[0].enabled);
    f.stage(PluginScope::Project, remove("fixture.tool"));
    assert_eq!(
        f.catalog(PluginScope::Project).entries[0]
            .version
            .as_deref(),
        Some("2.0.0")
    );
    f.apply();
    let inherited = f.catalog(PluginScope::Project).entries.remove(0);
    assert!(inherited.inherited && inherited.scope == PluginScope::Global);
    assert!(
        inherited.enabled,
        "工程副本的停用偏好不得带到继承的全局副本"
    );
    assert_eq!(inherited.version.as_deref(), Some("1.0.0"));
    let discovery = super::super::loader::discover_with_roots(&f.roots).unwrap();
    assert_eq!(discovery.packages.len(), 1);
    assert_eq!(discovery.packages[0].manifest.version, "1.0.0");
    assert_eq!(discovery.packages[0].origin.scope, PluginScope::Global);
}

#[test]
fn global_versions_are_immutable_and_removal_only_switches_atomic_index() {
    let f = Fixture::new();
    f.install(PluginScope::Global, "v1", "fixture.tool", "1.0.0");
    let old = f.global_path("fixture.tool");
    let before = fs::read(&old).unwrap();
    f.stage(
        PluginScope::Global,
        ManagementCommand::Install {
            source: f.package("v2", "fixture.tool", "2.0.0"),
            replace: true,
        },
    );
    assert_eq!(f.global_path("fixture.tool"), old);
    f.apply();
    let new = f.global_path("fixture.tool");
    assert_ne!(new, old);
    assert_eq!(fs::read(&old).unwrap(), before);
    assert_eq!(packages::inspect_package(&new).unwrap().0.version, "2.0.0");
    f.stage(PluginScope::Global, remove("fixture.tool"));
    f.apply();
    assert!(f.catalog(PluginScope::Global).entries.is_empty());
    assert!(
        old.exists() && new.exists(),
        "其他进程可能仍映射任意已发布版本，不能主动删除对象"
    );
}

#[test]
fn stale_global_plan_rejects_external_index_change() {
    let f = Fixture::new();
    f.install(PluginScope::Global, "v1", "fixture.tool", "1.0.0");
    let old = f.global_path("fixture.tool");
    f.stage(
        PluginScope::Global,
        ManagementCommand::Install {
            source: f.package("v2", "fixture.tool", "2.0.0"),
            replace: true,
        },
    );
    // 模拟另一个编辑器在计划登记后原子采用另一个有效版本；旧事务不能覆盖它。
    let external = f.package("external", "fixture.tool", "3.0.0");
    let digest = packages::binary_digest(&external).unwrap();
    let object = f
        .roots
        .global
        .join("objects")
        .join(&digest)
        .join(packages::binary_filename("fixture.tool"));
    fs::create_dir_all(object.parent().unwrap()).unwrap();
    fs::copy(external, &object).unwrap();
    let index = GlobalIndex {
        format: 1,
        entries: std::collections::BTreeMap::from([(
            "fixture.tool".into(),
            GlobalRecord {
                digest,
                filename: packages::binary_filename("fixture.tool"),
            },
        )]),
    };
    fs::write(
        f.roots.global.join("index.json"),
        serde_json::to_vec(&index).unwrap(),
    )
    .unwrap();
    assert!(!packages::apply_pending_with_roots(&f.roots).is_empty());
    assert_eq!(
        f.global_path("fixture.tool"),
        object.canonicalize().unwrap()
    );
    assert!(old.exists());
    assert_eq!(
        f.catalog(PluginScope::Global).entries[0].version.as_deref(),
        Some("3.0.0")
    );
}

#[test]
fn no_project_can_manage_global_files_but_cannot_write_project_scope() {
    let f = Fixture::new();
    let roots = PluginRoots {
        project: None,
        global: f.roots.global.clone(),
    };
    packages::stage_at(
        &roots,
        PluginScope::Global,
        ManagementCommand::Install {
            source: f.package("global", "fixture.tool", "1.0.0"),
            replace: false,
        },
    )
    .unwrap();
    assert!(packages::apply_pending_with_roots(&roots).is_empty());
    assert_eq!(
        packages::catalog_at(&roots, PluginScope::Global)
            .unwrap()
            .entries
            .len(),
        1
    );
    assert!(
        packages::stage_at(&roots, PluginScope::Project, enable("fixture.tool", false)).is_err()
    );
    assert!(!f.project().join(".editor").exists());
}

#[test]
fn legacy_directory_does_not_load_but_new_same_id_file_takes_over_without_deleting_it() {
    let f = Fixture::new();
    let legacy = f.project().join("plugins/old-equipment");
    fs::create_dir_all(&legacy).unwrap();
    fs::write(
        legacy.join("plugin.json"),
        br#"{"format":1,"id":"fixture.tool","tool":"old"}"#,
    )
    .unwrap();
    fs::write(legacy.join("old.dll"), b"preserved-old-binary").unwrap();
    assert!(f.catalog(PluginScope::Project).entries[0].legacy);
    let discovery = super::super::loader::discover_with_roots(&f.roots).unwrap();
    assert!(discovery.packages.is_empty());
    assert!(discovery.warnings.iter().any(|error| error.contains("旧plugin.json")));
    f.install(PluginScope::Project, "new", "fixture.tool", "1.0.0");
    let catalog = f.catalog(PluginScope::Project);
    assert_eq!(catalog.entries.len(), 1);
    assert!(catalog.entries[0].error.is_none() && !catalog.entries[0].legacy);
    assert!(catalog.warnings.is_empty());
    assert!(catalog.notices.iter().any(|notice| notice.contains("旧目录包")));
    let discovery = super::super::loader::discover_with_roots(&f.roots).unwrap();
    assert_eq!(discovery.packages.len(), 1);
    assert!(discovery.warnings.is_empty(), "迁移说明不能使正常单文件被误报加载失败");
    assert_eq!(
        fs::read(legacy.join("old.dll")).unwrap(),
        b"preserved-old-binary"
    );
    // 新文件真的损坏时仍必须报错，不能因为旧包已被归为迁移说明而吞掉实际失败。
    fs::write(f.installed("fixture.tool"), b"broken-new-binary").unwrap();
    let discovery = super::super::loader::discover_with_roots(&f.roots).unwrap();
    assert!(discovery.packages.is_empty());
    assert!(!discovery.warnings.is_empty());
}

#[test]
fn legacy_pending_is_never_executed_and_cancel_archives_journal_without_cleaning_old_files() {
    let f = Fixture::new();
    let manager = f.manager(PluginScope::Project);
    fs::create_dir_all(manager.join("staged/old-ticket")).unwrap();
    fs::write(
        manager.join("staged/old-ticket/preserved.txt"),
        b"legacy-staging",
    )
    .unwrap();
    let old = serde_json::json!({"format":1,"changes":{"fixture.tool":{"kind":"install","manifest":{"id":"fixture.tool","version":"1.0.0"},"destination":"fixture.tool","ticket":"old-ticket"}}});
    fs::write(
        manager.join("pending.json"),
        serde_json::to_vec(&old).unwrap(),
    )
    .unwrap();
    assert!(!packages::apply_pending_with_roots(&f.roots).is_empty());
    assert!(!f.installed("fixture.tool").exists());
    let catalog = f.catalog(PluginScope::Project);
    assert_eq!(
        catalog.entries[0].pending.as_ref().unwrap().kind,
        PendingKind::Install
    );
    f.stage(PluginScope::Project, cancel("fixture.tool"));
    f.apply();
    assert_eq!(f.journal(PluginScope::Project).format, 2);
    assert_eq!(
        fs::read(manager.join("staged/old-ticket/preserved.txt")).unwrap(),
        b"legacy-staging"
    );
    let archive = fs::read_dir(manager.join("legacy"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(archive).unwrap()).unwrap(),
        old
    );
}

#[test]
fn receipt_failure_rolls_back_project_file_and_global_index() {
    for scope in [PluginScope::Project, PluginScope::Global] {
        let f = Fixture::new();
        f.install(scope, "v1", "fixture.tool", "1.0.0");
        let old = if scope == PluginScope::Project {
            f.installed("fixture.tool")
        } else {
            f.global_path("fixture.tool")
        };
        let before = fs::read(&old).unwrap();
        f.stage(
            scope,
            ManagementCommand::Install {
                source: f.package("v2", "fixture.tool", "2.0.0"),
                replace: true,
            },
        );
        let receipts = f.manager(scope).join("receipts");
        let held = f.manager(scope).join("receipts-held");
        fs::rename(&receipts, &held).unwrap();
        fs::write(&receipts, b"external-blocker").unwrap();
        assert!(!packages::apply_pending_with_roots(&f.roots).is_empty());
        assert_eq!(fs::read(&old).unwrap(), before);
        assert_eq!(
            f.catalog(scope).entries[0].version.as_deref(),
            Some("1.0.0")
        );
    }
}

#[test]
fn foreign_scope_and_path_escape_journals_are_rejected_before_any_file_mutation() {
    let f = Fixture::new();
    f.install(PluginScope::Project, "v1", "fixture.tool", "1.0.0");
    f.stage(PluginScope::Project, remove("fixture.tool"));
    let original = fs::read(f.installed("fixture.tool")).unwrap();
    let path = f.manager(PluginScope::Project).join("pending.json");
    let mut journal = f.journal(PluginScope::Project);
    journal.scope = PluginScope::Global;
    fs::write(&path, serde_json::to_vec(&journal).unwrap()).unwrap();
    assert!(!packages::apply_pending_with_roots(&f.roots).is_empty());
    journal.scope = PluginScope::Project;
    journal.changes.get_mut("fixture.tool").unwrap().destination = "../outside.dll".into();
    fs::write(path, serde_json::to_vec(&journal).unwrap()).unwrap();
    assert!(!packages::apply_pending_with_roots(&f.roots).is_empty());
    assert_eq!(fs::read(f.installed("fixture.tool")).unwrap(), original);
}

#[test]
fn corrupt_global_object_does_not_hide_other_valid_plugin() {
    let f = Fixture::new();
    f.install(PluginScope::Global, "first", "fixture.first", "1.0.0");
    f.install(PluginScope::Global, "second", "fixture.second", "1.0.0");
    fs::write(f.global_path("fixture.first"), b"external-tampering").unwrap();
    let catalog = f.catalog(PluginScope::Global);
    assert_eq!(catalog.entries.len(), 2);
    assert!(
        catalog
            .entries
            .iter()
            .find(|e| e.id == "fixture.first")
            .unwrap()
            .error
            .is_some()
    );
    assert!(
        catalog
            .entries
            .iter()
            .find(|e| e.id == "fixture.second")
            .unwrap()
            .error
            .is_none()
    );
    let discovery = super::super::loader::discover_with_roots(&f.roots).unwrap();
    assert_eq!(discovery.packages.len(), 1);
    assert_eq!(discovery.packages[0].manifest.id, "fixture.second");
}

#[test]
fn damaged_and_missing_global_objects_can_be_explicitly_removed_without_deleting_other_versions() {
    let f = Fixture::new();
    f.install(PluginScope::Global, "damaged", "fixture.damaged", "1.0.0");
    f.install(PluginScope::Global, "missing", "fixture.missing", "1.0.0");
    let damaged = f.global_path("fixture.damaged");
    let missing = f.global_path("fixture.missing");
    fs::write(&damaged, b"external-corrupt-file").unwrap();
    fs::remove_file(missing).unwrap();
    f.stage(PluginScope::Global, remove("fixture.damaged"));
    f.stage(PluginScope::Global, remove("fixture.missing"));
    f.apply();
    assert!(f.catalog(PluginScope::Global).entries.is_empty());
    assert_eq!(
        fs::read(damaged).unwrap(),
        b"external-corrupt-file",
        "移除索引保留损坏对象，便于诊断与其他进程退役"
    );
}

#[test]
fn inherited_disable_survives_same_reopen_global_upgrade_or_removal_and_future_reinstall() {
    for remove_globally in [false, true] {
        let f = Fixture::new();
        f.install(PluginScope::Global, "v1", "fixture.tool", "1.0.0");
        if remove_globally {
            f.stage(PluginScope::Global, remove("fixture.tool"));
        } else {
            f.stage(
                PluginScope::Global,
                ManagementCommand::Install {
                    source: f.package("v2", "fixture.tool", "2.0.0"),
                    replace: true,
                },
            );
        }
        f.stage(PluginScope::Project, enable("fixture.tool", false));
        f.apply();
        let state: Settings = serde_json::from_slice(
            &fs::read(f.manager(PluginScope::Project).join("state.json")).unwrap(),
        )
        .unwrap();
        assert!(state.inherited_disabled.contains("fixture.tool"));
        assert!(!state.disabled.contains("fixture.tool"));
        if remove_globally {
            assert!(f.catalog(PluginScope::Project).entries.is_empty());
            f.install(PluginScope::Global, "v3", "fixture.tool", "3.0.0");
        }
        let row = f.catalog(PluginScope::Project).entries.remove(0);
        assert!(row.inherited && !row.enabled);
        assert!(f.catalog(PluginScope::Global).entries[0].enabled);
        assert!(
            super::super::loader::discover_with_roots(&f.roots)
                .unwrap()
                .packages
                .is_empty()
        );
    }
}

#[test]
fn explicit_inherited_disablement_survives_local_install_enablement_and_removal() {
    let f = Fixture::new();
    f.install(PluginScope::Global, "global", "fixture.tool", "1.0.0");
    f.stage(PluginScope::Project, enable("fixture.tool", false));
    f.apply();
    assert!(!f.catalog(PluginScope::Project).entries[0].enabled);
    f.install(PluginScope::Project, "local", "fixture.tool", "2.0.0");
    assert!(
        f.catalog(PluginScope::Project).entries[0].enabled,
        "继承禁用不影响新安装的工程副本"
    );
    f.stage(PluginScope::Project, enable("fixture.tool", false));
    f.apply();
    f.stage(PluginScope::Project, enable("fixture.tool", true));
    f.apply();
    let state: Settings = serde_json::from_slice(
        &fs::read(f.manager(PluginScope::Project).join("state.json")).unwrap(),
    )
    .unwrap();
    assert!(!state.disabled.contains("fixture.tool"));
    assert!(
        state.inherited_disabled.contains("fixture.tool"),
        "本地启用只能改变本地偏好，不能顺便重新启用继承全局"
    );
    f.stage(PluginScope::Project, remove("fixture.tool"));
    f.apply();
    let row = f.catalog(PluginScope::Project).entries.remove(0);
    assert!(row.inherited && !row.enabled);
    assert_eq!(row.version.as_deref(), Some("1.0.0"));
    assert!(
        super::super::loader::discover_with_roots(&f.roots)
            .unwrap()
            .packages
            .is_empty()
    );
    f.stage(PluginScope::Project, enable("fixture.tool", true));
    f.apply();
    assert!(f.catalog(PluginScope::Project).entries[0].enabled);
}

#[test]
fn legacy_settings_default_inherited_preference_to_empty_without_losing_local_disablement() {
    let f = Fixture::new();
    f.install(PluginScope::Global, "global", "fixture.tool", "1.0.0");
    let manager = f.manager(PluginScope::Project);
    fs::create_dir_all(&manager).unwrap();
    let legacy = br#"{"format":1,"disabled":["fixture.tool"]}"#;
    fs::write(manager.join("state.json"), legacy).unwrap();
    let state: Settings = serde_json::from_slice(legacy).unwrap();
    assert!(state.inherited_disabled.is_empty());
    assert!(state.disabled.contains("fixture.tool"));
    assert!(
        f.catalog(PluginScope::Project).entries[0].enabled,
        "旧状态只有工程副本的停用偏好，不能误判为显式禁用继承全局"
    );
    f.install(PluginScope::Project, "local", "fixture.tool", "2.0.0");
    assert!(!f.catalog(PluginScope::Project).entries[0].enabled);
    f.stage(PluginScope::Project, enable("fixture.tool", true));
    f.apply();
    let written: Settings =
        serde_json::from_slice(&fs::read(manager.join("state.json")).unwrap()).unwrap();
    assert!(written.disabled.is_empty() && written.inherited_disabled.is_empty());
    assert_eq!(written.format, 1);
}

#[test]
fn enablement_receipt_failure_restores_both_local_and_inherited_preferences() {
    for inherited in [false, true] {
        let f = Fixture::new();
        f.install(PluginScope::Global, "global", "fixture.tool", "1.0.0");
        if inherited {
            // 留下本地禁用历史后移除副本；继承全局当前应保持可用。
            f.install(PluginScope::Project, "local", "fixture.tool", "2.0.0");
            f.stage(PluginScope::Project, enable("fixture.tool", false));
            f.apply();
            f.stage(PluginScope::Project, remove("fixture.tool"));
            f.apply();
        } else {
            // 保留继承禁用历史，再安装可用的本地副本。
            f.stage(PluginScope::Project, enable("fixture.tool", false));
            f.apply();
            f.install(PluginScope::Project, "local", "fixture.tool", "2.0.0");
        }
        let before = f.catalog(PluginScope::Project).entries.remove(0);
        assert!(before.enabled && before.inherited == inherited);
        let manager = f.manager(PluginScope::Project);
        let previous: Settings =
            serde_json::from_slice(&fs::read(manager.join("state.json")).unwrap()).unwrap();
        f.stage(PluginScope::Project, enable("fixture.tool", false));
        // 正式配置写入后，让真实回执路径失败，验证整份偏好快照回滚，而非模拟成功位。
        fs::rename(manager.join("receipts"), manager.join("receipts-held")).unwrap();
        fs::write(manager.join("receipts"), b"external-receipt-blocker").unwrap();
        assert!(!packages::apply_pending_with_roots(&f.roots).is_empty());
        let actual: Settings =
            serde_json::from_slice(&fs::read(manager.join("state.json")).unwrap()).unwrap();
        assert_eq!(actual.disabled, previous.disabled);
        assert_eq!(actual.inherited_disabled, previous.inherited_disabled);
        assert!(f.catalog(PluginScope::Project).entries[0].enabled);
        assert!(f.catalog(PluginScope::Global).entries[0].enabled);
    }
}
