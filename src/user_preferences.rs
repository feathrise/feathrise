//! Feathrise用户级偏好：默认目录、最近工程、主窗口位置及受身份保护的原子保存。
//!
//! 这里只处理有界数据和文件，不依赖Bevy World；调用方应在启动前读取，或把IO放入后台槽。
//! 偏好始终归当前用户，不读取或写入工程的settings.json，也不会因记录移除删除工程。

use serde::{Deserialize, Serialize};
use crate::writable_paths::{legacy_preferences_file, preferences_file, validate_absolute};
pub(crate) use crate::writable_paths::user_home;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) const MAX_RECENT_PROJECTS: usize = 32;
const MAX_PINNED_PROJECTS: usize = MAX_RECENT_PROJECTS - 1;
const MAX_CONFIG_BYTES: usize = 256 * 1024;
const MAX_NAME_BYTES: usize = 512;
static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecentProject {
    pub name: String,
    /// 调用方在工程预检成功后传入真实规范目录；记录不承担目录存在性扫描。
    pub path: PathBuf,
    pub last_opened_unix_seconds: u64,
    // 旧记录默认未固定，普通记录保存时仍沿用旧字段形状。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct WindowPlacement {
    /// Win32工作区坐标。最大化时这里仍保存还原后的外框。
    pub normal_rect: [i32; 4],
    pub maximized: bool,
}

impl WindowPlacement {
    pub(crate) fn is_valid(self) -> bool {
        let [left, top, right, bottom] = self.normal_rect;
        right.checked_sub(left).is_some_and(|width| width > 0)
            && bottom.checked_sub(top).is_some_and(|height| height > 0)
    }
}

fn read_window_placement<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<WindowPlacement>, D::Error> {
    // 单个窗口记录损坏时仍可读取最近工程和默认目录，下一次正常退出可写入有效记录。
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value::<WindowPlacement>(value)
        .ok()
        .filter(|placement| placement.is_valid()))
}

#[derive(Debug, Clone)]
pub(crate) struct Preferences {
    pub default_project_directory: PathBuf,
    pub recent_projects: Vec<RecentProject>,
    pub main_window: Option<WindowPlacement>,
    home: PathBuf,
    file: PathBuf,
    // 仅标准用户位置携带旧来源；显式验收/自定义文件不读取真实用户配置。
    legacy_file: Option<PathBuf>,
    load_failed: bool,
    // 保存时核对读取到的完整原文；坏配置或另一实例的新设置不能被默认值静默覆盖。
    original: Option<Vec<u8>>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredPreferences {
    #[serde(default = "current_format")]
    format: u32,
    #[serde(default)]
    default_project_directory: Option<PathBuf>,
    #[serde(default)]
    recent_projects: Vec<RecentProject>,
    #[serde(
        default,
        deserialize_with = "read_window_placement",
        skip_serializing_if = "Option::is_none"
    )]
    main_window: Option<WindowPlacement>,
}

const fn current_format() -> u32 {
    1
}

impl Preferences {
    /// 不存在配置时使用真实用户主目录下的feathrise；坏配置返回错误，不冒充首次启动。
    pub(crate) fn load() -> Result<Self, String> {
        let home = user_home()?;
        let file = preferences_file(&home)?;
        let legacy = legacy_preferences_file(&home);
        Self::load_migrating_at(file, home, legacy)
    }

    /// 用于读取失败后的只读托底。若磁盘上已有配置，save仍会拒绝覆盖它。
    pub(crate) fn defaults() -> Result<Self, String> {
        let home = user_home()?;
        let legacy = legacy_preferences_file(&home);
        let mut value = Self::fallback_at(preferences_file(&home)?, home)?;
        value.legacy_file = Some(legacy);
        Ok(value)
    }

    /// 明确传入主目录的默认数据，便于隔离测试；不创建默认工程目录或偏好文件。
    #[cfg(test)]
    pub(crate) fn default_for_home(home: &Path) -> Result<Self, String> {
        Self::default_at(preferences_file(home)?, home)
    }

    /// 显式位置的读取失败托底，隔离验收不得因此跳回真实APPDATA配置。
    /// 原文身份仍为“未成功读取”，save会拒绝覆盖该位置已有的损坏或未知配置。
    pub(crate) fn fallback_at(
        file: impl Into<PathBuf>,
        home: impl Into<PathBuf>,
    ) -> Result<Self, String> {
        let mut value = Self::default_at(file, home)?;
        value.load_failed = true;
        Ok(value)
    }

    /// 启动前迁移完整原文字节；新文件优先，旧文件保留为备份，作者配置稍后沿同一来源迁移。
    pub(crate) fn load_migrating_at(
        file: impl Into<PathBuf>,
        home: impl Into<PathBuf>,
        legacy: impl Into<PathBuf>,
    ) -> Result<Self, String> {
        let file = file.into();
        let home = home.into();
        let legacy = legacy.into();
        validate_absolute(&home, "用户主目录")?;
        migrate_file_if_missing(&legacy, &file)?;
        let mut value = Self::load_at(file, home)?;
        value.legacy_file = Some(legacy);
        Ok(value)
    }

    pub(crate) fn load_at(
        file: impl Into<PathBuf>,
        home: impl Into<PathBuf>,
    ) -> Result<Self, String> {
        let mut preferences = Self::default_at(file, home)?;
        let Some(original) = read_optional(&preferences.file)? else {
            return Ok(preferences);
        };
        let stored: StoredPreferences = serde_json::from_slice(&original)
            .map_err(|error| format!("编辑器个人设置损坏，已保留原文件：{error}"))?;
        if stored.format != current_format() {
            return Err(format!(
                "编辑器个人设置版本{}不受支持，已保留原文件",
                stored.format
            ));
        }
        if let Some(directory) = stored.default_project_directory {
            preferences.default_project_directory =
                resolve_directory(path_text(&directory)?, &preferences.home)?;
        }
        preferences.recent_projects = stored.recent_projects;
        preferences.main_window = stored.main_window;
        preferences.validate()?;
        // 稳定归并两组。旧文件缺少固定标记，原来的最近打开顺序保持有效。
        preferences.recent_projects.sort_by_key(|entry| !entry.pinned);
        preferences.original = Some(original);
        Ok(preferences)
    }

    fn default_at(file: impl Into<PathBuf>, home: impl Into<PathBuf>) -> Result<Self, String> {
        let file = file.into();
        let home = home.into();
        validate_absolute(&home, "用户主目录")?;
        validate_absolute(&file, "个人设置文件")?;
        Ok(Self {
            default_project_directory: home.join("feathrise"),
            recent_projects: Vec::new(),
            main_window: None,
            home,
            file,
            legacy_file: None,
            load_failed: false,
            original: None,
        })
    }

    pub(crate) fn file(&self) -> &Path {
        &self.file
    }

    pub(crate) fn legacy_file(&self) -> Option<&Path> {
        self.legacy_file.as_deref()
    }

    #[cfg(any(test, feature = "validation"))]
    pub(crate) fn home(&self) -> &Path {
        &self.home
    }

    /// 接受绝对目录或~/、~\写法；路径校验不访问磁盘，不把相对路径解释为工程cwd。
    pub(crate) fn set_default_project_directory(&mut self, value: &str) -> Result<(), String> {
        let directory = resolve_directory(value, &self.home)?;
        self.default_project_directory = directory;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn restore_default_project_directory(&mut self) {
        self.default_project_directory = self.home.join("feathrise");
    }

    /// 仅在工程成功打开后调用；不存在或失败的候选不能抢占最近记录首位。
    pub(crate) fn record_opened(&mut self, name: &str, path: &Path) -> Result<(), String> {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("无法记录工程打开时间：{error}"))?
            .as_secs();
        self.record_opened_at(name, path, seconds)
    }

    pub(crate) fn record_opened_at(
        &mut self,
        name: &str,
        path: &Path,
        last_opened_unix_seconds: u64,
    ) -> Result<(), String> {
        validate_recent(name, path)?;
        let mut entry = RecentProject {
            name: name.trim().to_owned(),
            path: path.to_path_buf(),
            last_opened_unix_seconds,
            pinned: false,
        };
        if let Some(index) = self.recent_index(path) {
            if self.recent_projects[index].pinned {
                // 重开可刷新名称和时间，但固定组内的位置由固定操作决定。
                entry.pinned = true;
                self.recent_projects[index] = entry;
                return Ok(());
            }
            self.recent_projects
                .retain(|project| !same_location(&project.path, path));
        }
        let first_unpinned = self.pinned_count();
        self.recent_projects.insert(first_unpinned, entry);
        self.recent_projects.truncate(MAX_RECENT_PROJECTS);
        Ok(())
    }

    /// 固定命令携带目标值，过期的重复命令不会把刚保存的状态再反转。
    pub(crate) fn set_recent_pinned(&mut self, path: &Path, pinned: bool) -> Result<bool, String> {
        let Some(index) = self.recent_index(path) else {
            return Ok(false);
        };
        if self.recent_projects[index].pinned == pinned {
            return Ok(false);
        }
        if pinned && self.pinned_count() >= MAX_PINNED_PROJECTS {
            return Err(format!("固定工程最多保留 {MAX_PINNED_PROJECTS} 项"));
        }
        let mut entry = self.recent_projects.remove(index);
        entry.pinned = pinned;
        let index = if pinned {
            0
        } else {
            // 取消固定后恢复到最后打开时间对应的普通记录位置，不制造一次新的打开。
            self.recent_projects
                .iter()
                .position(|other| {
                    !other.pinned && other.last_opened_unix_seconds < entry.last_opened_unix_seconds
                })
                .unwrap_or(self.recent_projects.len())
        };
        self.recent_projects.insert(index, entry);
        Ok(true)
    }

    fn recent_index(&self, path: &Path) -> Option<usize> {
        self.recent_projects
            .iter()
            .position(|entry| same_location(&entry.path, path))
    }

    fn pinned_count(&self) -> usize {
        self.recent_projects.partition_point(|entry| entry.pinned)
    }

    /// 只修改内存中的记录；本函数不对工程路径执行任何文件操作。
    pub(crate) fn remove_recent(&mut self, path: &Path) -> bool {
        let before = self.recent_projects.len();
        self.recent_projects
            .retain(|project| !same_location(&project.path, path));
        self.recent_projects.len() != before
    }

    /// 保存后更新原文身份，允许后续继续修改。调用方应把本函数放在有界后台工作槽。
    pub(crate) fn save(&mut self) -> Result<(), String> {
        if self.load_failed {
            return Err("编辑器个人设置未成功读取，已拒绝覆盖；请修复原文件并重新打开编辑器".into());
        }
        self.validate()?;
        let stored = StoredPreferences {
            format: current_format(),
            default_project_directory: Some(self.default_project_directory.clone()),
            recent_projects: self.recent_projects.clone(),
            main_window: self.main_window,
        };
        let mut bytes = serde_json::to_vec_pretty(&stored)
            .map_err(|error| format!("无法编码编辑器个人设置：{error}"))?;
        bytes.push(b'\n');
        publish_checked(&self.file, &self.original, &bytes)?;
        self.original = Some(bytes);
        Ok(())
    }

    fn validate(&self) -> Result<(), String> {
        validate_absolute(&self.default_project_directory, "默认工程目录")?;
        if self.recent_projects.len() > MAX_RECENT_PROJECTS {
            return Err(format!("最近工程记录不能超过{MAX_RECENT_PROJECTS}项"));
        }
        // 留一个普通最近记录槽，列表满时也能显示刚成功打开的新工程。
        if self.recent_projects.iter().filter(|entry| entry.pinned).count() > MAX_PINNED_PROJECTS {
            return Err(format!("固定工程最多保留 {MAX_PINNED_PROJECTS} 项"));
        }
        for project in &self.recent_projects {
            validate_recent(&project.name, &project.path)?;
        }
        Ok(())
    }
}

pub(crate) fn resolve_directory(value: &str, home: &Path) -> Result<PathBuf, String> {
    validate_absolute(home, "用户主目录")?;
    let value = value.trim();
    let value = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value);
    if value.is_empty() {
        return Err("请选择默认工程目录；恢复默认时使用~/feathrise".into());
    }
    let directory = if value == "~" {
        home.to_path_buf()
    } else if let Some(suffix) = value
        .strip_prefix("~/")
        .or_else(|| value.strip_prefix("~\\"))
    {
        if Path::new(suffix).components().any(|component| {
            matches!(
                component,
                Component::Prefix(_)
                    | Component::RootDir
                    | Component::CurDir
                    | Component::ParentDir
            )
        }) {
            return Err("~后应填写主目录内的相对目录，不能使用父级跳转、盘符或根目录".into());
        }
        home.join(suffix)
    } else {
        PathBuf::from(value)
    };
    validate_absolute(&directory, "默认工程目录")?;
    Ok(directory)
}

fn path_text(path: &Path) -> Result<&str, String> {
    path.to_str().ok_or_else(|| "目录名称不是有效UTF-8".into())
}

fn validate_recent(name: &str, path: &Path) -> Result<(), String> {
    if name.trim().is_empty() || name.len() > MAX_NAME_BYTES || name.chars().any(char::is_control) {
        return Err("最近工程名称为空、过长或包含控制字符".into());
    }
    validate_absolute(path, "最近工程路径")
}

fn same_location(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    // 正常打开路径已经canonicalize；这里额外消除Windows盘符与常见ASCII目录大小写差异。
    cfg!(windows)
        && a.to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy())
}

/// 首次启动的有界迁移；只复制已有普通文件，原文与并发目标均由原子发布规则保护。
pub(crate) fn migrate_file_if_missing(source: &Path, destination: &Path) -> Result<bool, String> {
    validate_absolute(destination, "个人设置文件")?;
    match fs::symlink_metadata(destination) {
        Ok(_) => return Ok(false), // 包括坏文件；必须由新位置的读取流程诊断，不能拿旧配置盖掉它。
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("无法检查新个人设置：{error}")),
    }
    validate_absolute(source, "旧个人设置文件")?;
    let Some(bytes) = read_optional(source)? else { return Ok(false); };
    // 不改写旧格式或原文，不删除来源；坏配置仍由各自加载器报告并保持只读。
    match publish_checked(destination, &None, &bytes) {
        Ok(()) => Ok(true),
        Err(error) => {
            // 其他实例已先完成迁移时采用新文件，不能因一次写入竞争回退到默认值。
            if fs::symlink_metadata(destination).is_ok() { return Ok(false); }
            Err(format!("迁移个人设置失败，旧文件已保留：{error}"))
        }
    }
}

pub(crate) fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("无法检查个人设置文件：{error}")),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("个人设置必须为普通文件，已拒绝跟随链接或目录".into());
    }
    if metadata.len() > MAX_CONFIG_BYTES as u64 {
        return Err("个人设置文件超过256KiB，已保留原文件".into());
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|error| format!("无法打开个人设置文件：{error}"))?
        .take(MAX_CONFIG_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("无法完整读取个人设置文件：{error}"))?;
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err("个人设置读取期间超过256KiB，已保留原文件".into());
    }
    Ok(Some(bytes))
}

/// 工程管理偏好和作者偏好各自保存独立文件，但复用同一个有界、原文校验和原子发布实现。
/// 调用方只能在启动前、后台IO槽或App退出后调用，不在窗口帧中执行文件操作。
pub(crate) fn publish_checked(
    destination: &Path,
    original: &Option<Vec<u8>>,
    bytes: &[u8],
) -> Result<(), String> {
    validate_absolute(destination, "个人设置文件")?;
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err("编辑器个人设置超过256KiB，未写入文件".into());
    }
    let parent = destination.parent().ok_or("个人设置文件没有父目录")?;
    fs::create_dir_all(parent).map_err(|error| format!("无法创建个人设置目录：{error}"))?;
    let _lease = WriteLease::acquire(destination)?;
    let check_original = || -> Result<(), String> {
        if read_optional(destination)? != *original {
            return Err(
                "编辑器个人设置已被外部修改或存在未成功读取的文件；已拒绝覆盖，请重新加载".into(),
            );
        }
        Ok(())
    };
    check_original()?;
    let mut temporary = Temporary::new(destination)?;
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary.path)
            .map_err(|error| format!("无法准备个人设置临时文件：{error}"))?;
        temporary.owned = true;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("无法完整写入个人设置：{error}"))?;
    }
    // 写临时文件期间也可能有外部编辑器修改原文；发布前再次核对而非只比较时间戳。
    check_original()?;
    fs::rename(&temporary.path, destination)
        .map_err(|error| format!("无法原子发布个人设置，原文件保留：{error}"))?;
    Ok(())
}

struct WriteLease {
    file: Option<File>,
    path: PathBuf,
}

impl WriteLease {
    fn acquire(destination: &Path) -> Result<Self, String> {
        let path = destination.with_extension("json.write-lock");
        let mut options = OpenOptions::new();
        options.read(true).write(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // OS句柄独占而非“文件存在即加锁”，进程崩溃也不会留下永久占用。
            options.create(true).share_mode(0);
        }
        #[cfg(not(windows))]
        options.create_new(true);
        let file = options.open(&path).map_err(|error| {
            format!("个人设置正在由其他写入操作占用，或无法取得写租约：{error}")
        })?;
        Ok(Self {
            file: Some(file),
            path,
        })
    }
}

impl Drop for WriteLease {
    fn drop(&mut self) {
        self.file.take();
        // 只清理本租约的精确标记文件；Windows并发新句柄若已接管，会拒绝本次删除。
        let _ = fs::remove_file(&self.path);
    }
}

struct Temporary {
    path: PathBuf,
    owned: bool,
}

impl Temporary {
    fn new(destination: &Path) -> Result<Self, String> {
        let parent = destination.parent().ok_or("个人设置文件没有父目录")?;
        let name = destination
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("个人设置文件名不是有效UTF-8")?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("无法生成设置保存票据：{error}"))?
            .as_nanos();
        Ok(Self {
            path: parent.join(format!(
                ".{name}.{}-{stamp}-{}.tmp",
                std::process::id(),
                NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed)
            )),
            owned: false,
        })
    }
}

impl Drop for Temporary {
    fn drop(&mut self) {
        if self.owned {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        root: PathBuf,
        home: PathBuf,
        config: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "feathrise-preferences-{}-{stamp}-{}",
                std::process::id(),
                NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            let root = root.canonicalize().unwrap();
            let home = root.join("用户 主目录");
            let config = root.join("用户设置/editor.json");
            Self { root, home, config }
        }

        fn load(&self) -> Preferences {
            Preferences::load_at(&self.config, &self.home).unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            // 递归删除仅用于本测试创建的系统临时目录，先核对规范根与专属前缀。
            let temporary = std::env::temp_dir().canonicalize().unwrap();
            if self.root.parent() == Some(temporary.as_path())
                && self.root.canonicalize().ok().as_ref() == Some(&self.root)
                && self.root.file_name().is_some_and(|name| {
                    name.to_string_lossy().starts_with("feathrise-preferences-")
                })
            {
                let _ = fs::remove_dir_all(&self.root);
            }
        }
    }

    #[test]
    fn legacy_preferences_migrate_exact_bytes_and_new_file_wins_without_rewriting_backup() {
        let fixture = Fixture::new();
        let legacy = fixture.root.join("旧设置/editor.json");
        let target = fixture.home.join(".feathrise/editor.json");
        let mut old = Preferences::load_at(&legacy, &fixture.home).unwrap();
        old.set_default_project_directory(fixture.root.join("原工程目录").to_str().unwrap()).unwrap();
        old.record_opened_at("保留的工程", &fixture.root.join("工程 A"), 42).unwrap();
        old.save().unwrap();
        let original = fs::read(&legacy).unwrap();

        let mut migrated = Preferences::load_migrating_at(&target, &fixture.home, &legacy).unwrap();
        assert_eq!(migrated.default_project_directory, old.default_project_directory);
        assert_eq!(migrated.recent_projects, old.recent_projects);
        assert_eq!(fs::read(&target).unwrap(), original);
        assert_eq!(fs::read(&legacy).unwrap(), original, "旧位置只作备份，不能删除或重写");

        migrated.set_default_project_directory("~/新的工程目录").unwrap();
        migrated.save().unwrap();
        let current = fs::read(&target).unwrap();
        // 新位置已有有效配置时，旧环境甚至无效也不能抢回写入归属。
        let reopened = Preferences::load_migrating_at(&target, &fixture.home, "relative-legacy").unwrap();
        assert_eq!(reopened.default_project_directory, migrated.default_project_directory);
        assert_eq!(fs::read(&target).unwrap(), current);
        assert_eq!(fs::read(&legacy).unwrap(), original);
    }

    #[test]
    fn missing_legacy_does_not_create_directories_and_explicit_files_keep_migration_disabled() {
        let fixture = Fixture::new();
        let target = fixture.home.join(".feathrise/editor.json");
        let legacy = fixture.root.join("缺席旧位置/editor.json");
        let migrated = Preferences::load_migrating_at(&target, &fixture.home, &legacy).unwrap();
        assert_eq!(migrated.legacy_file(), Some(legacy.as_path()));
        assert!(!fixture.home.exists());
        assert!(!legacy.exists());
        let explicit = fixture.load();
        assert!(explicit.legacy_file().is_none(), "隔离验收不能读取真实用户的旧配置");
        assert!(!fixture.config.exists());
    }

    #[test]
    fn corrupt_legacy_is_preserved_and_failed_migration_fallback_cannot_publish_defaults() {
        let fixture = Fixture::new();
        let target = fixture.home.join(".feathrise/editor.json");
        let legacy = fixture.root.join("旧配置/editor.json");
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        fs::write(&legacy, b"{broken legacy preferences").unwrap();
        assert!(Preferences::load_migrating_at(&target, &fixture.home, &legacy).is_err());
        assert_eq!(fs::read(&target).unwrap(), fs::read(&legacy).unwrap());
        let mut fallback = Preferences::fallback_at(&target, &fixture.home).unwrap();
        assert!(fallback.save().unwrap_err().contains("拒绝覆盖"));

        let missing_target = fixture.root.join("尚未创建的新位置/editor.json");
        assert!(Preferences::load_migrating_at(&missing_target, &fixture.home, legacy.parent().unwrap()).is_err());
        let mut fallback = Preferences::fallback_at(&missing_target, &fixture.home).unwrap();
        assert!(fallback.save().unwrap_err().contains("拒绝覆盖"));
        assert!(!missing_target.parent().unwrap().exists());
        assert_eq!(fs::read(&legacy).unwrap(), b"{broken legacy preferences");
    }

    #[test]
    fn missing_preferences_default_to_home_feathrise_without_creating_directories() {
        let fixture = Fixture::new();
        let preferences = fixture.load();
        assert_eq!(
            preferences.default_project_directory,
            fixture.home.join("feathrise")
        );
        assert!(preferences.recent_projects.is_empty());
        assert!(!fixture.home.exists());
        assert!(!fixture.config.exists());
        let explicit = Preferences::default_for_home(&fixture.home).unwrap();
        assert_eq!(
            explicit.default_project_directory,
            preferences.default_project_directory
        );
    }

    #[test]
    fn chinese_directory_and_recent_records_survive_atomic_save_and_reopen() {
        let fixture = Fixture::new();
        let mut preferences = fixture.load();
        let projects = fixture.root.join("中文 工程目录");
        preferences
            .set_default_project_directory(projects.to_str().unwrap())
            .unwrap();
        let project = projects.join("测试 A");
        preferences
            .record_opened_at("测试 A", &project, 12)
            .unwrap();
        preferences.save().unwrap();
        let first = fs::read(&fixture.config).unwrap();
        assert!(first.ends_with(b"\n"));
        assert_eq!(fixture.load().default_project_directory, projects);
        assert_eq!(fixture.load().recent_projects, preferences.recent_projects);
        preferences.restore_default_project_directory();
        preferences.save().unwrap();
        assert_eq!(
            fixture.load().default_project_directory,
            fixture.home.join("feathrise")
        );
    }

    #[test]
    fn tilde_is_expanded_against_explicit_home_and_relative_or_parent_paths_are_rejected() {
        let fixture = Fixture::new();
        assert_eq!(
            resolve_directory("~/feathrise", &fixture.home).unwrap(),
            fixture.home.join("feathrise")
        );
        assert_eq!(resolve_directory("~", &fixture.home).unwrap(), fixture.home);
        assert!(resolve_directory("relative", &fixture.home).is_err());
        assert!(resolve_directory("~/../other", &fixture.home).is_err());
        assert!(resolve_directory("~//other", &fixture.home).is_err());
        assert!(resolve_directory("~another/feathrise", &fixture.home).is_err());
        assert!(Preferences::default_for_home(Path::new("relative-home")).is_err());
    }

    #[test]
    fn corrupt_configuration_and_external_revisions_are_never_overwritten() {
        let fixture = Fixture::new();
        let mut before = fixture.load();
        fs::create_dir_all(fixture.config.parent().unwrap()).unwrap();
        fs::write(&fixture.config, b"{broken").unwrap();
        assert!(Preferences::load_at(&fixture.config, &fixture.home).is_err());
        assert!(before.save().unwrap_err().contains("拒绝覆盖"));
        assert_eq!(fs::read(&fixture.config).unwrap(), b"{broken");
        fs::remove_file(&fixture.config).unwrap();
        before.save().unwrap();
        let mut stale = fixture.load();
        before
            .set_default_project_directory(fixture.root.join("new-default").to_str().unwrap())
            .unwrap();
        before.save().unwrap();
        let changed = fs::read(&fixture.config).unwrap();
        assert!(stale.save().unwrap_err().contains("拒绝覆盖"));
        assert_eq!(fs::read(&fixture.config).unwrap(), changed);
    }

    #[test]
    fn explicit_fallback_keeps_fixture_location_and_protects_its_corrupt_configuration() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.config.parent().unwrap()).unwrap();
        fs::write(&fixture.config, b"{fixture configuration is broken").unwrap();
        assert!(Preferences::load_at(&fixture.config, &fixture.home).is_err());
        let mut fallback = Preferences::fallback_at(&fixture.config, &fixture.home).unwrap();
        assert_eq!(fallback.file(), fixture.config);
        assert_eq!(fallback.home(), fixture.home);
        assert_eq!(
            fallback.default_project_directory,
            fixture.home.join("feathrise")
        );
        fallback
            .set_default_project_directory("~/new-projects")
            .unwrap();
        assert!(fallback.save().unwrap_err().contains("拒绝覆盖"));
        assert_eq!(
            fs::read(&fixture.config).unwrap(),
            b"{fixture configuration is broken"
        );
    }

    #[test]
    fn recent_projects_are_bounded_deduplicated_and_removal_never_deletes_project() {
        let fixture = Fixture::new();
        let mut preferences = fixture.load();
        for number in 0..40 {
            preferences
                .record_opened_at(
                    &format!("工程 {number}"),
                    &fixture.root.join(format!("project-{number}")),
                    number,
                )
                .unwrap();
        }
        assert_eq!(preferences.recent_projects.len(), MAX_RECENT_PROJECTS);
        assert_eq!(preferences.recent_projects[0].name, "工程 39");
        let project = fixture.root.join("project-39");
        fs::create_dir(&project).unwrap();
        let marker = project.join("keep.txt");
        fs::write(&marker, b"keep project bytes").unwrap();
        preferences
            .record_opened_at("重命名后的工程", &project, 60)
            .unwrap();
        assert_eq!(preferences.recent_projects.len(), MAX_RECENT_PROJECTS);
        assert_eq!(preferences.recent_projects[0].name, "重命名后的工程");
        assert!(preferences.remove_recent(&project));
        assert!(!preferences.remove_recent(&project));
        preferences.save().unwrap();
        assert_eq!(fs::read(marker).unwrap(), b"keep project bytes");
        assert!(
            fixture
                .load()
                .recent_projects
                .iter()
                .all(|recent| recent.path != project)
        );
    }

    #[test]
    fn recent_pins_keep_distinct_worktrees_and_survive_reopen_and_mru_churn() {
        let fixture = Fixture::new();
        let first = fixture.root.join("worktree-a/project");
        let second = fixture.root.join("worktree-b/project");
        fs::create_dir_all(fixture.config.parent().unwrap()).unwrap();
        let legacy = serde_json::json!({"format": 1, "recent_projects": [
            {"name": "同名工程", "path": second, "last_opened_unix_seconds": 2},
            {"name": "同名工程", "path": first, "last_opened_unix_seconds": 1},
        ]});
        fs::write(&fixture.config, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let mut preferences = fixture.load();
        assert!(preferences.recent_projects.iter().all(|entry| !entry.pinned));

        assert!(preferences.set_recent_pinned(&first, true).unwrap());
        assert!(preferences.set_recent_pinned(&second, true).unwrap());
        assert!(!preferences.set_recent_pinned(&second, true).unwrap());
        preferences.record_opened_at("同名工程", &first, 100).unwrap();
        for number in 0..40 {
            preferences.record_opened_at(
                "普通工程", &fixture.root.join(format!("other-{number}")), 3 + number,
            ).unwrap();
        }
        assert_eq!(preferences.recent_projects.len(), MAX_RECENT_PROJECTS);
        assert_eq!(preferences.recent_projects[0].path, second);
        assert_eq!(preferences.recent_projects[1].path, first);
        assert_eq!(preferences.recent_projects[1].last_opened_unix_seconds, 100);
        assert_eq!(preferences.recent_projects[2].path, fixture.root.join("other-39"));
        assert!(preferences.recent_projects[..2].iter().all(|entry| entry.pinned));
        preferences.save().unwrap();

        let mut reloaded = fixture.load();
        assert_eq!(reloaded.recent_projects, preferences.recent_projects);
        assert!(reloaded.set_recent_pinned(&second, false).unwrap());
        assert_eq!(reloaded.recent_projects[0].path, first);
        let unpinned = reloaded.recent_projects.last().unwrap();
        assert_eq!(unpinned.path, second);
        assert_eq!(unpinned.last_opened_unix_seconds, 2);
        assert!(!unpinned.pinned);
        reloaded.save().unwrap();
        assert_eq!(fixture.load().recent_projects, reloaded.recent_projects);
    }

    #[test]
    fn recent_pin_limit_preserves_a_slot_for_the_next_successful_open() {
        let fixture = Fixture::new();
        let mut preferences = fixture.load();
        for number in 0..MAX_PINNED_PROJECTS {
            let path = fixture.root.join(format!("pinned-{number}"));
            preferences.record_opened_at("固定工程", &path, number as u64).unwrap();
            assert!(preferences.set_recent_pinned(&path, true).unwrap());
        }
        let old = fixture.root.join("old");
        preferences.record_opened_at("普通工程", &old, 100).unwrap();
        assert!(preferences.set_recent_pinned(&old, true).is_err());
        let new = fixture.root.join("new");
        preferences.record_opened_at("普通工程", &new, 101).unwrap();
        assert_eq!(preferences.pinned_count(), MAX_PINNED_PROJECTS);
        assert_eq!(preferences.recent_projects.last().unwrap().path, new);
        assert!(!preferences.recent_projects.iter().any(|entry| entry.path == old));
    }

    #[test]
    fn absent_directory_field_in_existing_configuration_keeps_home_default() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.config.parent().unwrap()).unwrap();
        fs::write(&fixture.config, br#"{"format":1,"recent_projects":[]}"#).unwrap();
        let mut preferences = fixture.load();
        assert!(preferences.main_window.is_none());
        assert_eq!(
            preferences.default_project_directory,
            fixture.home.join("feathrise")
        );
        preferences.save().unwrap();
        assert_eq!(
            fixture.load().default_project_directory,
            preferences.default_project_directory
        );
    }

    #[test]
    fn invalid_window_record_keeps_project_preferences_and_can_be_replaced() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.config.parent().unwrap()).unwrap();
        fs::write(
            &fixture.config,
            br#"{"format":1,"default_project_directory":"~/projects",
                "main_window":{"normal_rect":[150,180,149,640],"maximized":true}}"#,
        )
        .unwrap();
        let mut preferences = fixture.load();
        assert_eq!(
            preferences.default_project_directory,
            fixture.home.join("projects")
        );
        assert_eq!(preferences.main_window, None);
        let placement = WindowPlacement {
            normal_rect: [-1520, 30, -480, 730],
            maximized: true,
        };
        preferences.main_window = Some(placement);
        preferences.save().unwrap();
        assert_eq!(fixture.load().main_window, Some(placement));
    }
}
