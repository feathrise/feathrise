//! 跨宿主静态夹具只证明ELF/Mach-O解析、来源与闭包，不能替代Linux/macOS实际运行验收。
use super::*;
use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(1);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "feathrise-unix-runtime-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        let path = path.canonicalize().unwrap();
        fs::write(path.join("owner"), b"unix-runtime-static-fixture").unwrap();
        Self(path)
    }
    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let parent = std::env::temp_dir().canonicalize().unwrap();
        if self.0.parent() == Some(parent.as_path())
            && self
                .0
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("feathrise-unix-runtime-")
            && fs::read(self.0.join("owner")).ok().as_deref()
                == Some(b"unix-runtime-static-fixture")
        {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
fn put16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}
fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn put64(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

fn elf(needed: &[&str], rpath: Option<&str>) -> Vec<u8> {
    let mut strings = vec![0];
    let mut dynamics = Vec::<(i64, u64)>::new();
    for name in needed {
        let offset = strings.len() as u64;
        strings.extend_from_slice(name.as_bytes());
        strings.push(0);
        dynamics.push((object::elf::DT_NEEDED.0, offset));
    }
    if let Some(path) = rpath {
        let offset = strings.len() as u64;
        strings.extend_from_slice(path.as_bytes());
        strings.push(0);
        dynamics.push((object::elf::DT_RUNPATH.0, offset));
    }
    dynamics.push((object::elf::DT_STRTAB.0, 0x400000 + 2048));
    dynamics.push((object::elf::DT_STRSZ.0, strings.len() as u64));
    dynamics.push((object::elf::DT_NULL.0, 0));
    let mut bytes = vec![0; 2048 + strings.len()];
    bytes[..4].copy_from_slice(b"\x7fELF");
    bytes[4] = 2;
    bytes[5] = 1;
    bytes[6] = 1;
    put16(&mut bytes, 16, object::elf::ET_DYN.0);
    put16(&mut bytes, 18, architecture(true) as u16);
    put32(&mut bytes, 20, 1);
    put64(&mut bytes, 32, 64);
    put16(&mut bytes, 52, 64);
    put16(&mut bytes, 54, 56);
    put16(&mut bytes, 56, 2);
    put32(&mut bytes, 64, object::elf::PT_LOAD.0);
    put64(&mut bytes, 64 + 16, 0x400000);
    let length = bytes.len() as u64;
    put64(&mut bytes, 64 + 32, length);
    put32(&mut bytes, 120, object::elf::PT_DYNAMIC.0);
    put64(&mut bytes, 120 + 8, 256);
    put64(&mut bytes, 120 + 16, 0x400000 + 256);
    put64(&mut bytes, 120 + 32, dynamics.len() as u64 * 16);
    for (i, (tag, value)) in dynamics.into_iter().enumerate() {
        put64(&mut bytes, 256 + i * 16, tag as u64);
        put64(&mut bytes, 264 + i * 16, value);
    }
    bytes[2048..].copy_from_slice(&strings);
    bytes
}
fn macho(needed: &[&str], rpath: Option<&str>) -> Vec<u8> {
    let mut commands = Vec::new();
    for name in needed {
        let length = (24 + name.len() + 1).div_ceil(8) * 8;
        let mut command = vec![0; length];
        put32(&mut command, 0, object::macho::LC_LOAD_DYLIB.0);
        put32(&mut command, 4, length as u32);
        put32(&mut command, 8, 24);
        command[24..24 + name.len()].copy_from_slice(name.as_bytes());
        commands.push(command);
    }
    if let Some(path) = rpath {
        let length = (12 + path.len() + 1).div_ceil(8) * 8;
        let mut command = vec![0; length];
        put32(&mut command, 0, object::macho::LC_RPATH.0);
        put32(&mut command, 4, length as u32);
        put32(&mut command, 8, 12);
        command[12..12 + path.len()].copy_from_slice(path.as_bytes());
        commands.push(command);
    }
    let mut bytes = vec![0; 32];
    put32(&mut bytes, 0, object::macho::MH_MAGIC_64);
    put32(&mut bytes, 4, architecture(false));
    put32(&mut bytes, 12, object::macho::MH_DYLIB.0);
    put32(&mut bytes, 16, commands.len() as u32);
    put32(
        &mut bytes,
        20,
        commands.iter().map(Vec::len).sum::<usize>() as u32,
    );
    for command in commands {
        bytes.extend(command);
    }
    bytes
}

#[test]
fn elf_dynamic_closure_uses_exact_cargo_and_origin_sources_without_scanning_siblings() {
    let f = Fixture::new();
    let exe = f.write("game", &elf(&["libfirst.so"], Some("$ORIGIN/deps")));
    let first = f.write("deps/libfirst.so", &elf(&["libsecond.so"], Some("$ORIGIN")));
    let second = f.write("deps/libsecond.so", &elf(&[], None));
    f.write("old-cache/libfirst.so", b"must-not-be-used");
    let records = vec![CargoDll {
        path: first.clone(),
        package_id: "fixture#first@1.0.0".into(),
        target_name: "first".into(),
        dylib: true,
    }];
    let files = collect(
        &exe,
        &f.0,
        &records,
        &[],
        &BuildControl::default(),
        Platform::Linux,
    )
    .unwrap();
    assert_eq!(files.len(), 2);
    assert!(files.iter().any(|v| v.source == first));
    assert!(files.iter().any(|v| v.source == second));
}
#[test]
fn macho_loader_and_rpath_closure_leave_system_cache_libraries_to_dyld() {
    let f = Fixture::new();
    let exe = f.write(
        "game",
        &macho(&["@rpath/libfirst.dylib"], Some("@loader_path/deps")),
    );
    let first = f.write(
        "deps/libfirst.dylib",
        &macho(
            &["@loader_path/libsecond.dylib", "/usr/lib/libSystem.B.dylib"],
            None,
        ),
    );
    let second = f.write("deps/libsecond.dylib", &macho(&[], None));
    let files = collect(
        &exe,
        &f.0,
        &[],
        &[],
        &BuildControl::default(),
        Platform::Mac,
    )
    .unwrap();
    assert_eq!(files.len(), 2);
    assert!(files.iter().any(|v| v.source == first));
    assert!(files.iter().any(|v| v.source == second));
}
#[test]
fn missing_import_and_oversized_tables_are_errors_instead_of_empty_success() {
    let f = Fixture::new();
    let exe = f.write("game", &elf(&["libmissing-fixture.so"], None));
    assert!(
        collect(
            &exe,
            &f.0,
            &[],
            &[],
            &BuildControl::default(),
            Platform::Linux
        )
        .unwrap_err()
        .contains("缺少实际导入")
    );
    let mut bytes = elf(&[], None);
    put16(&mut bytes, 56, 65535);
    let path = f.write("bad-elf", &bytes);
    assert!(image(&path, Platform::Linux, &BuildControl::default()).is_err());
    let mut bytes = macho(&[], None);
    put32(&mut bytes, 20, u32::MAX);
    let path = f.write("bad-macho", &bytes);
    assert!(image(&path, Platform::Mac, &BuildControl::default()).is_err());
}
#[test]
fn bevy_hashed_alias_must_match_this_cargo_receipt_bytes() {
    let f = Fixture::new();
    let name = "libbevy_dylib-0123456789abcdef.so";
    let exe = f.write("game", &elf(&[name], None));
    let bytes = elf(&[], None);
    let plain = f.write("libbevy_dylib.so", &bytes);
    let exact = f.write(&format!("deps/{name}"), &bytes);
    let records = vec![CargoDll {
        path: plain,
        package_id: "registry+fixture#bevy_dylib@0.19.1".into(),
        target_name: "bevy_dylib".into(),
        dylib: true,
    }];
    assert_eq!(
        collect(
            &exe,
            &f.0,
            &records,
            &[f.0.join("deps")],
            &BuildControl::default(),
            Platform::Linux
        )
        .unwrap()[0]
            .source,
        exact
    );
    fs::write(exact, b"stale-hash-file").unwrap();
    assert!(
        collect(
            &exe,
            &f.0,
            &records,
            &[f.0.join("deps")],
            &BuildControl::default(),
            Platform::Linux
        )
        .unwrap_err()
        .contains("身份不同")
    );
}
#[test]
fn parser_checks_cancellation_and_loader_path_expansion_does_not_accept_unknown_variables() {
    let f = Fixture::new();
    let path = f.write("game", &elf(&[], None));
    let control = BuildControl::default();
    control.cancel();
    assert!(
        image(&path, Platform::Linux, &control)
            .unwrap_err()
            .contains("取消")
    );
    assert!(expand("$UNKNOWN/lib", &path, &f.0).is_err());
    assert_eq!(
        expand("$ORIGIN/deps", &path, &f.0).unwrap(),
        f.0.join("deps")
    );
}
