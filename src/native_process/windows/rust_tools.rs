//! Windows Rust 工具兜底：读取当前用户的安装环境，只补齐本次子进程需要的路径。

use super::{find_on_path, native_executable, process_image_path};
use std::{
    env,
    ffi::{OsStr, OsString, c_void},
    os::windows::{
        ffi::OsStringExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
    process::Command,
    ptr::null_mut,
};
use windows::core::PCWSTR;
use windows_sys::Win32::{
    Security::{TOKEN_DUPLICATE, TOKEN_QUERY},
    System::{
        Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock},
        Threading::{GetCurrentProcess, OpenProcessToken},
    },
};

#[derive(Clone, Copy)]
pub(super) enum Tool {
    Cargo,
    Rustc,
    Rustup,
}

impl Tool {
    pub(super) fn from_name(name: &OsStr) -> Option<Self> {
        let name = name.to_str()?;
        [Self::Cargo, Self::Rustc, Self::Rustup]
            .into_iter()
            .find(|tool| {
                name.eq_ignore_ascii_case(tool.filename())
                    || name.eq_ignore_ascii_case(tool.filename().trim_end_matches(".exe"))
            })
    }

    fn filename(self) -> &'static str {
        match self {
            Self::Cargo => "cargo.exe",
            Self::Rustc => "rustc.exe",
            Self::Rustup => "rustup.exe",
        }
    }

    pub(super) fn requires_rust(self) -> bool {
        // Rustup 用来管理工具链，独立安装的 Cargo 和 rustc 也能构建。
        !matches!(self, Self::Rustup)
    }

    pub(super) fn recover(
        self,
        command: &Command,
        inherited_path: Option<&OsStr>,
    ) -> Result<Option<(PathBuf, RecoveredEnvironment)>, String> {
        let registered = RegisteredEnvironment::read();
        let (cargo_home, recovered_cargo_home) =
            tool_home(command, "CARGO_HOME", registered.cargo_home);
        let (_, recovered_rustup_home) = tool_home(command, "RUSTUP_HOME", registered.rustup_home);
        let profile = env::var_os("USERPROFILE").or(registered.profile);
        let Some(executable) = self.search(
            registered.path.as_deref(),
            cargo_home.as_deref(),
            profile.as_deref(),
        ) else {
            return Ok(None);
        };
        // Cargo 还会启动编译器。将已找到的 bin 放在本次 PATH 前端，使它和后续 Rust 工具处于同一环境。
        let path = recovered_path(&executable, inherited_path.or(registered.path.as_deref()))?;
        Ok(Some((
            executable,
            RecoveredEnvironment {
                path,
                recovered_cargo_home,
                recovered_rustup_home,
            },
        )))
    }

    fn search(
        self,
        path: Option<&OsStr>,
        cargo_home: Option<&OsStr>,
        profile: Option<&OsStr>,
    ) -> Option<PathBuf> {
        let filename = Path::new(self.filename());
        path.and_then(|path| find_on_path(filename, path))
            .or_else(|| {
                cargo_home
                    .map(PathBuf::from)
                    .into_iter()
                    .chain(profile.map(|profile| Path::new(profile).join(".cargo")))
                    .filter(|home| home.is_absolute())
                    .find_map(|home| native_executable(home.join("bin").join(filename)))
            })
    }
}

pub(super) struct RecoveredEnvironment {
    path: OsString,
    recovered_cargo_home: Option<OsString>,
    recovered_rustup_home: Option<OsString>,
}

impl RecoveredEnvironment {
    pub(super) fn entries(&self) -> impl Iterator<Item = (&'static str, &OsStr)> {
        [
            ("PATH", Some(self.path.as_os_str())),
            ("CARGO_HOME", self.recovered_cargo_home.as_deref()),
            ("RUSTUP_HOME", self.recovered_rustup_home.as_deref()),
        ]
        .into_iter()
        .filter_map(|(name, value)| value.map(|value| (name, value)))
    }
}

fn tool_home(
    command: &Command,
    name: &str,
    registered: Option<OsString>,
) -> (Option<OsString>, Option<OsString>) {
    if let Some((_, value)) = command
        .get_envs()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
    {
        return (value.map(OsStr::to_owned), None);
    }
    if let Some(inherited) = env::var_os(name) {
        return (Some(inherited), None);
    }
    // 新装的自定义 Rustup 目录可能同样尚未进入旧编辑器环境。
    (registered.clone(), registered)
}

fn recovered_path(executable: &Path, remaining: Option<&OsStr>) -> Result<OsString, String> {
    let directory = executable.parent().ok_or("Rust 工具缺少所在目录")?;
    let mut path =
        env::join_paths([process_image_path(directory)]).map_err(|error| error.to_string())?;
    if let Some(remaining) = remaining.filter(|value| !value.is_empty()) {
        path.push(";");
        path.push(remaining);
    }
    Ok(path)
}

#[derive(Default)]
struct RegisteredEnvironment {
    path: Option<OsString>,
    profile: Option<OsString>,
    cargo_home: Option<OsString>,
    rustup_home: Option<OsString>,
}

#[cfg(feature = "validation")]
static ISOLATED_REGISTRY: std::sync::OnceLock<()> = std::sync::OnceLock::new();

#[cfg(feature = "validation")]
pub(crate) fn isolate_registry_for_validation() {
    let _ = ISOLATED_REGISTRY.set(());
}

impl RegisteredEnvironment {
    fn read() -> Self {
        // 验收进程已经核对独占工程和空安装目录，只隔离机器的已登记环境。
        #[cfg(feature = "validation")]
        if ISOLATED_REGISTRY.get().is_some() {
            return Self::default();
        }
        let mut token = null_mut();
        // 不继承旧进程环境，让 Windows 合并当前用户和系统设置并展开注册表变量。
        // 调用只在既有后台任务中进行，普通 PATH 命中时无需读取 Windows 环境块。
        if unsafe {
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_QUERY | TOKEN_DUPLICATE,
                &mut token,
            )
        } == 0
        {
            return Self::default();
        }
        // SAFETY：上面的调用成功，token 是独占的有效句柄。
        let token = unsafe { OwnedHandle::from_raw_handle(token) };
        let mut block = null_mut();
        if unsafe { CreateEnvironmentBlock(&mut block, token.as_raw_handle(), 0) } == 0
            || block.is_null()
        {
            return Self::default();
        }
        let block = EnvironmentBlock(block);
        let mut next = block.0.cast::<u16>().cast_const();
        let mut environment = Self::default();
        loop {
            // SAFETY：Windows 返回一组以 NUL 结尾的 UTF-16 字符串，空项标记环境块结束。
            let string = PCWSTR(next);
            let entry = unsafe { string.as_wide() };
            if entry.is_empty() {
                break;
            }
            next = unsafe { next.add(entry.len() + 1) };
            if let Some(equal) = entry.iter().position(|unit| *unit == b'=' as u16) {
                environment.insert(&entry[..equal], &entry[equal + 1..]);
            }
        }
        environment
    }

    fn insert(&mut self, key: &[u16], value: &[u16]) {
        let key = OsString::from_wide(key);
        for (name, field) in [
            ("PATH", &mut self.path),
            ("USERPROFILE", &mut self.profile),
            ("CARGO_HOME", &mut self.cargo_home),
            ("RUSTUP_HOME", &mut self.rustup_home),
        ] {
            if key.eq_ignore_ascii_case(name) {
                *field = Some(OsString::from_wide(value));
                break;
            }
        }
    }
}

struct EnvironmentBlock(*mut c_void);

impl Drop for EnvironmentBlock {
    fn drop(&mut self) {
        // SAFETY：本对象唯一持有 CreateEnvironmentBlock 返回的缓冲区。
        unsafe { DestroyEnvironmentBlock(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registered_path_precedes_custom_and_default_rustup_directories() {
        let root = env::temp_dir().join(format!(
            "feathrise-rust-path-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let registered = root.join("注册路径");
        let cargo_home = root.join("custom-home");
        let default_home = root.join(".cargo");
        let directories = [
            registered.clone(),
            cargo_home.join("bin"),
            default_home.join("bin"),
        ];
        for directory in &directories {
            std::fs::create_dir_all(directory).unwrap();
            // 夹具只验证查找顺序，定位结果不会被执行。
            std::fs::write(directory.join("cargo.exe"), b"resolver only").unwrap();
        }
        let search = || {
            Tool::Cargo.search(
                Some(registered.as_os_str()),
                Some(cargo_home.as_os_str()),
                Some(root.as_os_str()),
            )
        };
        for directory in &directories {
            let file = directory.join("cargo.exe");
            let found = search().unwrap();
            assert_eq!(found, file.canonicalize().unwrap());
            let inherited = OsStr::new(r"C:\Windows\System32");
            let path = recovered_path(&found, Some(inherited)).unwrap();
            let mut entries = env::split_paths(&path);
            assert_eq!(
                entries.next().unwrap().canonicalize().unwrap(),
                directory.canonicalize().unwrap()
            );
            assert_eq!(entries.next().as_deref(), Some(Path::new(inherited)));
            std::fs::remove_file(file).unwrap();
            std::fs::remove_dir(directory).unwrap();
        }
        assert!(search().is_none());
        for directory in [cargo_home, default_home, root] {
            std::fs::remove_dir(directory).unwrap();
        }
    }

    #[test]
    fn registered_environment_matches_windows_variable_names_case_insensitively() {
        let mut environment = RegisteredEnvironment::default();
        environment.insert(
            &"Path".encode_utf16().collect::<Vec<_>>(),
            &r"C:\工具\bin".encode_utf16().collect::<Vec<_>>(),
        );
        environment.insert(
            &"Cargo_Home".encode_utf16().collect::<Vec<_>>(),
            &r"D:\安装路径".encode_utf16().collect::<Vec<_>>(),
        );
        assert_eq!(
            environment.path.as_deref(),
            Some(OsStr::new(r"C:\工具\bin"))
        );
        assert_eq!(
            environment.cargo_home.as_deref(),
            Some(OsStr::new(r"D:\安装路径"))
        );
    }
}
