//! 有界后台源码摘要与本地依赖图：追踪 Rust、Cargo/工具链配置和 SDK，资产/作者草稿不触发代码编译。
//! 这不是任意 build.rs 输入的不可变快照；原生监听代次另拒绝构建期间改后还原的旧结果。

use crate::{build_process::BuildControl, project::Project};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
};
use toml::Value;

const PACKAGE_LIMIT: usize = 512;
const FILE_LIMIT: usize = 65536;
const ENTRY_LIMIT: usize = 131072;
const BYTE_LIMIT: u64 = 512 * 1024 * 1024;
const DEPTH_LIMIT: usize = 32;
const MANIFEST_LIMIT: usize = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SourceSnapshot {
    pub sha256: String,
    pub environment_sha256: String,
    /// 所有其他输入保持原摘要，仅受支持的登记函数体被语法形状替代；None一律走完整重启。
    pub hotpatch_shape: Option<String>,
    pub files: usize,
    pub bytes: u64,
    pub watches: BTreeSet<(PathBuf, bool)>,
    /// true为实际递归源码目录；false为包根/显式入口父目录或已摘要的精确文件。
    /// watches只登记OS目录，可据两表区分文件身份，避免扩展名过滤漏掉include_str!/内嵌Shader。
    pub source_watches: BTreeSet<(PathBuf, bool)>,
}

/// 显式目录合同优先于目录名字；保留逻辑和已存在祖先解析出的真实别名，未来新建目录也能命中。
#[derive(Clone, Debug)]
pub(super) struct Exclusions {
    prefixes: BTreeSet<PathBuf>,
    /// 显式编译资产也不能覆盖真实缓存、导出和作者数据；资产根本身只限制普通运行资产。
    compiled_forbidden: BTreeSet<PathBuf>,
}

impl Exclusions {
    pub fn new(project: &Project, target: &Path) -> Self {
        let mut excluded = Self {
            prefixes: BTreeSet::new(),
            compiled_forbidden: BTreeSet::new(),
        };
        excluded.add(target, &project.root, false);
        // 默认导出根在出现前就排除，包内的描述与源码型资产不能重新触发自身编译。
        excluded.add(&project.root.join("output"), &project.root, false);
        for root in &project.context.asset_roots {
            excluded.add(root, &project.root, true);
        }
        let data = &project.context.editor_data;
        let data_root = if data
            .file_name()
            .is_some_and(|name| name == project.context.storage_key.as_str())
        {
            data.parent().unwrap_or(data)
        } else {
            data.as_path()
        };
        excluded.add(data_root, &project.root, false);
        excluded
    }

    fn add(&mut self, path: &Path, base: &Path, asset: bool) {
        let absolute = if path.is_absolute() {
            path.to_owned()
        } else {
            base.join(path)
        };
        self.prefixes.insert(comparison_path(&absolute));
        if !asset { self.compiled_forbidden.insert(comparison_path(&absolute)); }
        let mut ancestor = absolute;
        let mut missing = Vec::new();
        loop {
            if let Ok(mut canonical) = ancestor.canonicalize() {
                for name in missing.iter().rev() {
                    canonical.push(name);
                }
                self.prefixes.insert(comparison_path(&canonical));
                if !asset { self.compiled_forbidden.insert(comparison_path(&canonical)); }
                break;
            }
            let Some(name) = ancestor.file_name().map(|name| name.to_owned()) else {
                break;
            };
            missing.push(name);
            if !ancestor.pop() {
                break;
            }
        }
    }

    pub fn contains(&self, path: &Path) -> bool {
        let path = comparison_path(path);
        self.prefixes.iter().any(|prefix| path.starts_with(prefix))
    }

    pub fn allows_compiled_file(&self, path: &Path) -> bool {
        let path = comparison_path(path);
        !path.components().any(|component| {
            let name = component.as_os_str().to_string_lossy();
            name != "assets" && ignored_component(&name)
        }) && !self.compiled_forbidden.iter().any(|prefix| path.starts_with(prefix))
    }
}

pub(super) fn comparison_path(path: &Path) -> PathBuf {
    let display = crate::project::display_path(path);
    #[cfg(windows)]
    let display = display.to_lowercase();
    let mut normalized = PathBuf::new();
    for component in Path::new(&display).components() {
        match component {
            std::path::Component::CurDir => (),
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

struct Capture<'a> {
    control: &'a BuildControl,
    files: BTreeMap<PathBuf, String>,
    packages: BTreeSet<PathBuf>,
    visited: BTreeSet<PathBuf>,
    walked: BTreeSet<PathBuf>,
    watches: BTreeSet<(PathBuf, bool)>,
    source_watches: BTreeSet<(PathBuf, bool)>,
    entries: usize,
    bytes: u64,
    exclusions: Exclusions,
}

pub(crate) fn capture(
    project: &Project,
    target: &Path,
    control: &BuildControl,
) -> Result<SourceSnapshot, String> {
    control.check()?;
    if crate::project::open(&project.root)? != *project {
        return Err("工程清单已变化，请重新打开工程后继续代码迭代".into());
    }
    let mut capture = Capture {
        control,
        files: BTreeMap::new(),
        packages: BTreeSet::new(),
        visited: BTreeSet::new(),
        walked: BTreeSet::new(),
        watches: BTreeSet::new(),
        source_watches: BTreeSet::new(),
        entries: 0,
        bytes: 0,
        exclusions: Exclusions::new(project, target),
    };
    capture.packages.insert(
        project
            .manifest
            .parent()
            .ok_or("工程 Cargo 入口缺少目录")?
            .to_owned(),
    );
    capture.add_file(&project.root.join("editor-project.json"))?;
    capture.watches.insert((project.root.clone(), false));
    while let Some(root) = capture
        .packages
        .iter()
        .find(|root| !capture.visited.contains(*root))
        .cloned()
    {
        if capture.packages.len() > PACKAGE_LIMIT {
            return Err("源码本地依赖超过 512 个包，未截断构建身份".into());
        }
        capture.visited.insert(root.clone());
        capture.package(&root)?;
    }
    let environment_sha256 = environment_digest();
    let mut digest = Sha256::new();
    field(&mut digest, b"feathrise-code-inputs-v1");
    field(&mut digest, environment_sha256.as_bytes());
    for (path, hash) in &capture.files {
        field(
            &mut digest,
            path.to_str().ok_or("源码路径不是有效 Unicode")?.as_bytes(),
        );
        field(&mut digest, hash.as_bytes());
    }
    control.check()?;
    let hotpatch_shape = hotpatch_shape(project, &capture.files, &environment_sha256, control);
    Ok(SourceSnapshot {
        sha256: digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        environment_sha256,
        hotpatch_shape,
        files: capture.files.len(),
        bytes: capture.bytes,
        watches: capture.watches,
        source_watches: capture.source_watches,
    })
}

/// 已完成完整来源枚举后再比较受控函数；读取和语法树仅发生在原后台扫描槽。
fn hotpatch_shape(
    project: &Project, files: &BTreeMap<PathBuf, String>, environment: &str, control: &BuildControl,
) -> Option<String> {
    use crate::code_iteration::hotpatch::compatibility::{Contract, shape};
    let manifest = read_manifest(&project.manifest).ok()?;
    let contract = Contract::parse(&toml::to_string(&manifest).ok()?).ok()??;
    let mut replacements = BTreeMap::new();
    for relative in contract.files() {
        control.check().ok()?;
        let path = project.root.join(relative).canonicalize().ok()?;
        if !path.starts_with(&project.root) { return None; }
        let expected = files.get(&path)?;
        let mut bytes = Vec::new();
        fs::File::open(&path).ok()?.take(64 * 1024 + 1).read_to_end(&mut bytes).ok()?;
        if bytes.len() > 64 * 1024 || crate::build_artifacts::digest(&bytes) != *expected { return None; }
        let source = std::str::from_utf8(&bytes).ok()?;
        replacements.insert(path, shape(&contract, relative, source).ok()?.digest);
    }
    let mut digest = Sha256::new();
    field(&mut digest, b"feathrise-function-shape-v1");
    field(&mut digest, environment.as_bytes());
    for (path, hash) in files {
        field(&mut digest, path.to_str()?.as_bytes());
        field(&mut digest, replacements.get(path).unwrap_or(hash).as_bytes());
    }
    Some(digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

impl Capture<'_> {
    fn package(&mut self, root: &Path) -> Result<(), String> {
        self.control.check()?;
        if self.exclusions.contains(root) {
            return Ok(());
        }
        let manifest_path = root.join("Cargo.toml");
        let manifest = read_manifest(&manifest_path)?;
        self.add_file(&manifest_path)?;
        self.watches.insert((root.to_owned(), false));
        self.source_watches.insert((root.to_owned(), false));
        self.compiled_inputs(&manifest, root)?;
        let workspace = workspace(root, &manifest)?;
        if let Some((directory, _)) = &workspace {
            self.add_file(&directory.join("Cargo.toml"))?;
            self.watches.insert((directory.clone(), false));
        }
        // Cargo 向上查找配置和锁文件；记录现有文件，并监听目录中新建对应文件。
        for ancestor in root.ancestors() {
            self.control.check()?;
            // 即使配置尚不存在也监听父目录，首次新增祖先 Cargo/工具链配置同样会撤销旧版本。
            self.watches.insert((ancestor.to_owned(), false));
            for name in ["Cargo.lock", "rust-toolchain", "rust-toolchain.toml"] {
                let path = ancestor.join(name);
                if path.is_file() {
                    self.add_file(&path)?;
                    self.watches.insert((ancestor.to_owned(), false));
                }
            }
            let configuration = ancestor.join(".cargo");
            if configuration.is_dir() {
                // 用户主目录的 .cargo 还包含整个 registry/git 缓存，只登记 Cargo 实际配置文件。
                for name in ["config", "config.toml"] {
                    let path = configuration.join(name);
                    if path.is_file() {
                        self.add_file(&path)?;
                    }
                }
                self.watches.insert((configuration, false));
            }
        }
        for entry in fs::read_dir(root)
            .map_err(|error| format!("源码包目录不可读：{}：{error}", root.display()))?
        {
            self.count_entry()?;
            let entry = entry.map_err(|error| error.to_string())?;
            let path = entry.path();
            if path.extension().is_some_and(|extension| extension == "rs") {
                self.add_file(&path)?;
            }
        }
        for directory in ["src", "tests", "examples", "benches", ".cargo"] {
            let path = root.join(directory);
            if path.is_dir() && !self.exclusions.contains(&path) {
                self.walk(&path, 0)?;
                self.source_watches.insert((path.clone(), true));
                self.watches.insert((path, true));
            }
        }
        for key in ["lib", "bin", "test", "bench", "example"] {
            if let Some(targets) = manifest.get(key) {
                let targets: Vec<&Value> = if let Some(array) = targets.as_array() {
                    array.iter().collect()
                } else {
                    vec![targets]
                };
                for target in targets {
                    if let Some(path) = target.get("path").and_then(Value::as_str) {
                        let path = root
                            .join(path)
                            .canonicalize()
                            .map_err(|error| format!("显式 Rust 入口不可读：{error}"))?;
                        self.add_file(&path)?;
                        if let Some(parent) = path.parent() {
                            self.watches.insert((parent.to_owned(), false));
                            self.source_watches.insert((parent.to_owned(), false));
                        }
                    }
                }
            }
        }
        if let Some(path) = manifest
            .get("package")
            .and_then(|package| package.get("build"))
            .and_then(Value::as_str)
        {
            let path = root.join(path);
            self.add_file(&path)?;
            // 显式构建脚本可以位于包外；与显式 Rust 入口一样登记真实父目录，不能靠祖先宽泛事件兜底。
            let path = path.canonicalize().map_err(|error| format!("显式构建脚本不可读：{error}"))?;
            if let Some(parent) = path.parent() {
                self.watches.insert((parent.to_owned(), false));
                self.source_watches.insert((parent.to_owned(), false));
            }
        }
        self.dependencies(&manifest, root, workspace.as_ref())
    }

    fn dependencies(
        &mut self,
        manifest: &Value,
        root: &Path,
        workspace: Option<&(PathBuf, Value)>,
    ) -> Result<(), String> {
        for key in ["dependencies", "build-dependencies", "dev-dependencies"] {
            if let Some(table) = manifest.get(key).and_then(Value::as_table) {
                for (name, dependency) in table {
                    let (base, dependency) =
                        if dependency.get("workspace").and_then(Value::as_bool) == Some(true) {
                            let (base, workspace) =
                                workspace.ok_or("依赖继承 workspace，但未找到所属清单")?;
                            let inherited = workspace
                                .get("workspace")
                                .and_then(|workspace| workspace.get("dependencies"))
                                .and_then(|dependencies| dependencies.get(name))
                                .ok_or_else(|| format!("workspace 未声明继承依赖：{name}"))?;
                            (base.as_path(), inherited)
                        } else {
                            (root, dependency)
                        };
                    if let Some(path) = dependency.get("path").and_then(Value::as_str) {
                        self.add_package(&base.join(path))?;
                    }
                }
            }
        }
        if let Some(targets) = manifest.get("target").and_then(Value::as_table) {
            for target in targets.values() {
                self.dependencies(target, root, workspace)?;
            }
        }
        if let Some(patches) = manifest.get("patch").and_then(Value::as_table) {
            for registry in patches.values().filter_map(Value::as_table) {
                for dependency in registry.values() {
                    if let Some(path) = dependency.get("path").and_then(Value::as_str) {
                        self.add_package(&root.join(path))?;
                    }
                }
            }
        }
        // 补丁的相对路径属于 workspace 根，不能用成员包目录解释。
        if let Some((base, workspace)) = workspace {
            if base != root {
                if let Some(patches) = workspace.get("patch").and_then(Value::as_table) {
                    for registry in patches.values().filter_map(Value::as_table) {
                        for dependency in registry.values() {
                            if let Some(path) = dependency.get("path").and_then(Value::as_str) {
                                self.add_package(&base.join(path))?;
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn add_package(&mut self, path: &Path) -> Result<(), String> {
        let path = path
            .canonicalize()
            .map_err(|error| format!("登记本地依赖不可读：{}：{error}", path.display()))?;
        if !path.is_dir() || !path.join("Cargo.toml").is_file() {
            return Err("登记本地依赖必须是 Cargo 包目录".into());
        }
        self.packages.insert(path);
        Ok(())
    }

    fn walk(&mut self, path: &Path, depth: usize) -> Result<(), String> {
        self.control.check()?;
        if self.exclusions.contains(path) {
            return Ok(());
        }
        if depth > DEPTH_LIMIT {
            return Err("源码目录超过 32 层，未截断构建身份".into());
        }
        let metadata = ordinary_metadata(path)?;
        if metadata.is_file() {
            return self.add_file(path);
        }
        if !metadata.is_dir() {
            return Err("源码身份只接受普通文件和目录".into());
        }
        // 自选的工程内导出目录同样不属于源码；此检查只发生在原后台扫描槽。
        if !self.packages.contains(path) && exported_directory(path, self.control)? {
            return Ok(());
        }
        if !self.walked.insert(path.to_owned()) {
            return Ok(());
        }
        for entry in fs::read_dir(path).map_err(|error| error.to_string())? {
            self.count_entry()?;
            let entry = entry.map_err(|error| error.to_string())?;
            if ignored_component(&entry.file_name().to_string_lossy()) {
                continue;
            }
            self.walk(&entry.path(), depth + 1)?;
        }
        Ok(())
    }

    fn count_entry(&mut self) -> Result<(), String> {
        self.control.check()?;
        self.entries += 1;
        if self.entries > ENTRY_LIMIT {
            return Err("源码扫描超过 131072 目录项，未截断构建身份".into());
        }
        Ok(())
    }

    /// 包显式登记的精确编译输入可位于assets中，不扫描或授权整个资产目录。
    fn compiled_inputs(&mut self, manifest: &Value, root: &Path) -> Result<(), String> {
        let Some(inputs) = manifest.get("package").and_then(|value| value.get("metadata"))
            .and_then(|value| value.get("feathrise")).and_then(|value| value.get("compiled-inputs")) else { return Ok(()); };
        let inputs = inputs.as_array().ok_or("compiled-inputs必须是包内精确相对文件的字符串数组")?;
        for input in inputs {
            self.control.check()?;
            let relative = input.as_str().ok_or("compiled-inputs条目必须是字符串")?;
            let path = Path::new(relative);
            if relative.is_empty() || relative.chars().any(char::is_control)
                || relative.contains(':') || path.is_absolute()
                || relative.split(['/', '\\']).any(|part| matches!(part, "" | "." | ".."))
                || path.components().any(|part| !matches!(part, std::path::Component::Normal(_)))
            {
                return Err(format!("compiled-inputs必须是规范的包内精确相对文件：{relative}"));
            }
            let path = root.join(path);
            if !self.exclusions.allows_compiled_file(&path) {
                return Err(format!("compiled-inputs不能登记缓存、导出或作者数据：{relative}"));
            }
            // canonical仍在包内也可能经过包内junction；逐段拒绝重解析目录，不能把易换向的别名当稳定文件身份。
            let mut current = root.to_owned();
            let mut components = Path::new(relative).components().peekable();
            while let Some(component) = components.next() {
                self.control.check()?;
                current.push(component.as_os_str());
                let metadata = ordinary_metadata(&current)?;
                if if components.peek().is_some() { !metadata.is_dir() } else { !metadata.is_file() } {
                    return Err(format!("compiled-inputs中间路径须为普通目录，末项须为普通文件：{relative}"));
                }
            }
            let canonical = path.canonicalize().map_err(|error| format!("compiled-inputs文件不可读：{relative}：{error}"))?;
            if !canonical.starts_with(root) || !self.exclusions.allows_compiled_file(&canonical) {
                return Err(format!("compiled-inputs文件越出所属包或进入缓存/作者数据：{relative}"));
            }
            self.add_input(&canonical, true)?;
            let mut ancestor = canonical.parent();
            while let Some(parent) = ancestor.filter(|parent| parent.starts_with(root)) {
                // 各级非递归登记仅用于发现精确输入祖先的改名/替换；其它资产不因此获得源码职责。
                self.watches.insert((parent.to_owned(), false));
                if parent == root { break; }
                ancestor = parent.parent();
            }
        }
        Ok(())
    }

    fn add_file(&mut self, path: &Path) -> Result<(), String> {
        self.add_input(path, false)
    }

    fn add_input(&mut self, path: &Path, declared: bool) -> Result<(), String> {
        self.control.check()?;
        if !declared && self.exclusions.contains(path) {
            return Ok(());
        }
        let metadata = ordinary_metadata(path)?;
        if !metadata.is_file() {
            return Err("源码输入必须是普通文件".into());
        }
        let path = path.canonicalize().map_err(|error| error.to_string())?;
        if self.files.contains_key(&path) {
            return Ok(());
        }
        self.bytes = self
            .bytes
            .checked_add(metadata.len())
            .ok_or("源码字节数溢出")?;
        if self.files.len() == FILE_LIMIT || self.bytes > BYTE_LIMIT {
            return Err("源码扫描超过 65536 文件或 512 MiB，未截断构建身份".into());
        }
        let (hash, bytes) = crate::build_artifacts::hash_file(&path, self.control)?;
        if bytes != metadata.len() {
            return Err("源码正在写入，等待稳定后重试".into());
        }
        // 已摘要的编译输入不限Rust扩展名；保留旧路径身份以识别删除/重命名，回调不再做文件IO。
        self.source_watches.insert((path.clone(), false));
        self.files.insert(path, hash);
        Ok(())
    }
}

fn read_manifest(path: &Path) -> Result<Value, String> {
    let mut text = String::new();
    fs::File::open(path)
        .map_err(|error| format!("Cargo 清单不可读：{}：{error}", path.display()))?
        .take(MANIFEST_LIMIT as u64 + 1)
        .read_to_string(&mut text)
        .map_err(|error| error.to_string())?;
    if text.len() > MANIFEST_LIMIT {
        return Err("源码 Cargo 清单超过 1 MiB".into());
    }
    toml::from_str(&text).map_err(|error| format!("Cargo 清单暂不可解析：{error}"))
}

fn workspace(root: &Path, manifest: &Value) -> Result<Option<(PathBuf, Value)>, String> {
    if manifest.get("workspace").is_some() {
        return Ok(Some((root.to_owned(), manifest.clone())));
    }
    if let Some(relative) = manifest
        .get("package")
        .and_then(|package| package.get("workspace"))
        .and_then(Value::as_str)
    {
        let directory = root
            .join(relative)
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let manifest = read_manifest(&directory.join("Cargo.toml"))?;
        return Ok(Some((directory, manifest)));
    }
    for ancestor in root.ancestors().skip(1) {
        let path = ancestor.join("Cargo.toml");
        if path.is_file() {
            let manifest = read_manifest(&path)?;
            if manifest.get("workspace").is_some() {
                return Ok(Some((ancestor.to_owned(), manifest)));
            }
        }
    }
    Ok(None)
}

fn ordinary_metadata(path: &Path) -> Result<fs::Metadata, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("源码输入不可读：{}：{error}", path.display()))?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("源码监听不接受符号链接或 junction 输入".into());
        }
    }
    if metadata.file_type().is_symlink() {
        return Err("源码监听不接受符号链接输入".into());
    }
    Ok(metadata)
}

fn field(digest: &mut Sha256, value: &[u8]) {
    digest.update((value.len() as u64).to_le_bytes());
    digest.update(value);
}

fn environment_digest() -> String {
    let mut digest = Sha256::new();
    for key in [
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "RUSTC",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "RUSTUP_TOOLCHAIN",
        "CARGO_BUILD_TARGET",
        "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS",
        "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER",
        "CARGO_HOME",
        "RUSTUP_HOME",
        "EDITOR_HOST_CARGO",
        "CARGO",
        "VULKAN_SDK",
    ] {
        field(&mut digest, key.as_bytes());
        match std::env::var_os(key) {
            Some(value) => {
                digest.update([1]);
                field(&mut digest, value.to_string_lossy().as_bytes());
            }
            None => digest.update([0]),
        }
    }
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// 文件名本身不是排除授权；只有有界且声明正式格式的标记才把自选目录认作生成包。
fn exported_directory(path: &Path, control: &BuildControl) -> Result<bool, String> {
    let marker = path.join("feathrise-export.json");
    let Ok(metadata) = fs::symlink_metadata(&marker) else { return Ok(false); };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 64 * 1024 { return Ok(false); }
    control.check()?;
    let mut bytes = Vec::new();
    let read = fs::File::open(marker).and_then(|file| file.take(64 * 1024 + 1).read_to_end(&mut bytes));
    control.check()?;
    if read.is_err() || bytes.len() > 64 * 1024 { return Ok(false); }
    let Ok(marker) = serde_json::from_slice::<serde_json::Value>(&bytes) else { return Ok(false); };
    Ok(marker["format"] == 1 && marker["kind"] == "feathrise-export"
        && marker["project_id"].as_str().is_some_and(|id| !id.is_empty() && id.len() <= 128)
        && marker["manifest"] == "export-manifest.json")
}

fn ignored_component(name: &str) -> bool {
    name.starts_with(".feathrise-export-") || matches!(
        name,
        "target" | ".git" | ".editor" | "assets" | "feathrise-runtime-versions"
    )
}

/// 原生回调只做路径/事件判定并推进原子代次；目录新增/删除也须触发，不能漏掉整目录搬入的源码。
pub(super) fn relevant_path(path: &Path) -> bool {
    if ignored_path(path) {
        return false;
    }
    source_file_path(path) || path.extension().is_none()
}

/// 精确编译输入与新建源码树同样遵守原缓存/作者资产目录排除，不能因扩展名放宽而自触发。
pub(super) fn ignored_path(path: &Path) -> bool {
    path.components().any(|component| ignored_component(&component.as_os_str().to_string_lossy()))
}

/// 无扩展名的 config/rust-toolchain 仍是文件；与可能代表目录的普通无扩展名路径分开分类。
pub(super) fn source_file_path(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    path.extension().is_some_and(|extension| extension == "rs")
        || matches!(
            name,
            "Cargo.toml"
                | "Cargo.lock"
                | "rust-toolchain"
                | "rust-toolchain.toml"
                | "config"
                | "config.toml"
                | "editor-project.json"
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_path_filter_ignores_assets_and_build_cache_but_keeps_rust_and_renames() {
        for path in [
            "D:/project/src/main.rs",
            "D:/project/Cargo.toml",
            "D:/project/src/new_module",
            "D:/project/src/output/mod.rs",
        ] {
            assert!(relevant_path(Path::new(path)));
        }
        for path in [
            "D:/project/target/debug/build.rs",
            "D:/project/assets/map.rs",
            "D:/project/.editor/draft.json",
            "D:/project/.feathrise-export-stage/Cargo.toml",
            "D:/project/src/temp.tmp",
        ] {
            assert!(!relevant_path(Path::new(path)));
        }
    }

    #[test]
    fn default_and_custom_export_packages_do_not_change_source_identity() {
        let fixture = Fixture::new();
        let target = fixture.project.root.join("unused-target");
        let control = BuildControl::default();
        let original = capture(&fixture.project, &target, &control).unwrap();
        for name in ["output", ".feathrise-export-stage-owned", "custom-build"] {
            let directory = fixture.project.root.join(name);
            fs::create_dir(&directory).unwrap();
            fs::write(directory.join("feathrise-export.json"),
                br#"{"format":1,"kind":"feathrise-export","project_id":"owned-source","manifest":"export-manifest.json"}"#).unwrap();
            fs::write(directory.join("editor-project.json"), b"{}").unwrap();
            fs::write(directory.join("generated.rs"), b"generated export asset").unwrap();
        }
        assert_eq!(capture(&fixture.project, &target, &control).unwrap(), original);
    }

    #[test]
    fn default_export_exclusion_does_not_hide_an_output_source_module() {
        let fixture = Fixture::new();
        let target = fixture.project.root.join("unused-target");
        let control = BuildControl::default();
        let exclusions = Exclusions::new(&fixture.project, &target);
        assert!(exclusions.contains(&fixture.project.root.join("output/generated.rs")));
        let module = fixture.project.root.join("src/output/mod.rs");
        assert!(!exclusions.contains(&module));
        assert!(relevant_path(&module));
        let before = capture(&fixture.project, &target, &control).unwrap();
        fs::create_dir(module.parent().unwrap()).unwrap();
        fs::write(&module, "pub fn export_name() {}\n").unwrap();
        fs::write(module.parent().unwrap().join("feathrise-export.json"), b"{}").unwrap();
        assert_ne!(capture(&fixture.project, &target, &control).unwrap().sha256, before.sha256);
    }

    struct Fixture {
        directory: PathBuf,
        project: Project,
    }

    impl Fixture {
        fn new() -> Self {
            let directory = std::env::temp_dir().join(format!(
                "feathrise-own-source-{}",
                bevy::asset::uuid::Uuid::new_v4()
            ));
            let root = directory.join("game");
            fs::create_dir_all(root.join("assets")).unwrap();
            fs::create_dir(root.join("src")).unwrap();
            fs::write(
                root.join("Cargo.toml"),
                "[package]\nname='owned_source_fixture'\nversion='0.1.0'\n",
            )
            .unwrap();
            fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
            fs::write(root.join("editor-project.json"), r#"{"format":1,"id":"owned-source","name":"源码夹具","cargo_manifest":"Cargo.toml","package":"owned_source_fixture"}"#).unwrap();
            let project = crate::project::open(&root).unwrap();
            Self { directory, project }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }

    #[test]
    fn source_identity_covers_added_removed_rust_and_ignores_author_assets() {
        let fixture = Fixture::new();
        let control = BuildControl::default();
        let target = fixture.project.root.join("unused-target");
        let first = capture(&fixture.project, &target, &control).unwrap();
        let module = fixture.project.root.join("src/added.rs");
        fs::write(&module, "pub fn added() {}\n").unwrap();
        assert_ne!(
            capture(&fixture.project, &target, &control).unwrap().sha256,
            first.sha256
        );
        fs::remove_file(module).unwrap();
        fs::write(
            fixture.project.root.join("assets/map.json"),
            "author changes",
        )
        .unwrap();
        assert_eq!(capture(&fixture.project, &target, &control).unwrap(), first);
    }

    #[test]
    fn declared_dependency_font_changes_identity_and_prevents_body_only_patch_compatibility() {
        let fixture = Fixture::new();
        let sdk = fixture.directory.join("sdk");
        fs::create_dir_all(sdk.join("src")).unwrap();
        fs::create_dir_all(sdk.join("assets/fonts")).unwrap();
        fs::write(sdk.join("Cargo.toml"),
            "[package]\nname='owned_compiled_sdk'\nversion='0.1.0'\n[package.metadata.feathrise]\ncompiled-inputs=['assets/fonts/test.otf','assets/fonts/OFL.txt']\n").unwrap();
        fs::write(sdk.join("src/lib.rs"),
            "pub const FONT: &[u8] = include_bytes!(\"../assets/fonts/test.otf\");\npub const LICENSE: &str = include_str!(\"../assets/fonts/OFL.txt\");\n").unwrap();
        let font = sdk.join("assets/fonts/test.otf");
        let license = sdk.join("assets/fonts/OFL.txt");
        fs::write(&font, b"owned test font one").unwrap();
        fs::write(&license, "owned test license one\n").unwrap();
        fs::write(&fixture.project.manifest,
            "[package]\nname='owned_source_fixture'\nversion='0.1.0'\n[dependencies]\nsdk={package='owned_compiled_sdk',path='../sdk'}\n[package.metadata.feathrise.hotpatch]\nenabled=true\nfunctions=['src/main.rs::speed']\n").unwrap();
        let entry = fixture.project.root.join("src/main.rs");
        fs::write(&entry, "pub fn speed() -> f32 { 1.0 }\nfn main() {}\n").unwrap();
        let control = BuildControl::default();
        let target = fixture.project.root.join("unused-target");
        let baseline = capture(&fixture.project, &target, &control).unwrap();
        assert!(baseline.hotpatch_shape.is_some());
        let font = font.canonicalize().unwrap();
        assert!(baseline.source_watches.contains(&(font.clone(), false)));
        assert!(baseline.watches.contains(&(font.parent().unwrap().to_owned(), false)));
        assert!(!baseline.watches.contains(&(font.parent().unwrap().to_owned(), true)));
        fs::write(sdk.join("assets/fonts/ordinary.otf"), b"unlisted runtime asset").unwrap();
        assert_eq!(capture(&fixture.project, &target, &control).unwrap(), baseline);

        fs::write(&font, b"owned test font two").unwrap();
        let changed_font = capture(&fixture.project, &target, &control).unwrap();
        assert_ne!(baseline.sha256, changed_font.sha256);
        assert_ne!(baseline.hotpatch_shape, changed_font.hotpatch_shape);
        assert_eq!(baseline.environment_sha256, changed_font.environment_sha256);
        fs::write(&entry, "pub fn speed() -> f32 { 2.0 }\nfn main() {}\n").unwrap();
        let changed_body = capture(&fixture.project, &target, &control).unwrap();
        assert_ne!(changed_font.sha256, changed_body.sha256);
        assert_eq!(changed_font.hotpatch_shape, changed_body.hotpatch_shape,
            "只有登记函数体被擦除；此前字体变化仍留在原兼容门比较的完整shape中");
        assert_ne!(baseline.hotpatch_shape, changed_body.hotpatch_shape);
        fs::write(&license, "owned test license two\n").unwrap();
        let changed_license = capture(&fixture.project, &target, &control).unwrap();
        assert_ne!(changed_body.sha256, changed_license.sha256);
        assert_ne!(changed_body.hotpatch_shape, changed_license.hotpatch_shape);
    }

    #[test]
    fn compiled_inputs_reject_nonfiles_escape_cache_and_author_data_but_allow_exact_assets() {
        let mut fixture = Fixture::new();
        let root = fixture.project.root.clone();
        fs::write(root.join("editor-project.json"), r#"{"format":1,"id":"owned-source","name":"源码夹具","cargo_manifest":"Cargo.toml","package":"owned_source_fixture","editor_data":"src/author-store"}"#).unwrap();
        fixture.project = crate::project::open(&root).unwrap();
        let target = root.join("src/custom-cache");
        for relative in ["assets/font.otf", "target/generated.bin", "src/custom-cache/input.bin", "src/author-store/input.bin", "output/input.bin", ".editor/input.bin"] {
            let path = root.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"owned input").unwrap();
        }
        let control = BuildControl::default();
        let exclusions = Exclusions::new(&fixture.project, &target);
        let asset = root.join("assets/font.otf");
        assert!(exclusions.contains(&asset));
        assert!(exclusions.allows_compiled_file(&asset));
        for input in ["assets/font.otf".to_owned(), "assets".to_owned(), "../outside.otf".to_owned(),
            "assets/../assets/font.otf".to_owned(), "./assets/font.otf".to_owned(), "target/generated.bin".to_owned(),
            "src/custom-cache/input.bin".to_owned(), "src/author-store/input.bin".to_owned(),
            "output/input.bin".to_owned(), ".editor/input.bin".to_owned(), asset.to_string_lossy().into_owned()] {
            let manifest = format!("[package]\nname='owned_source_fixture'\nversion='0.1.0'\n[package.metadata.feathrise]\ncompiled-inputs=[{input:?}]\n");
            fs::write(&fixture.project.manifest, manifest).unwrap();
            let result = capture(&fixture.project, &target, &control);
            if input == "assets/font.otf" {
                assert!(result.unwrap().source_watches.contains(&(asset.canonicalize().unwrap(), false)));
            } else {
                assert!(result.is_err(), "非法编译输入不能被忽略为成功：{input}");
            }
        }
        for value in ["42", "['assets/font.otf',42]", "'assets/font.otf'"] {
            fs::write(&fixture.project.manifest, format!(
                "[package]\nname='owned_source_fixture'\nversion='0.1.0'\n[package.metadata.feathrise]\ncompiled-inputs={value}\n")).unwrap();
            assert!(capture(&fixture.project, &target, &control).is_err());
        }
    }

    #[cfg(any(windows, unix))]
    #[test]
    fn compiled_inputs_reject_in_package_directory_links_but_accept_ordinary_nested_files() {
        let fixture = Fixture::new();
        let root = &fixture.project.root;
        let actual = root.join("assets/real-fonts");
        let nested = actual.join("nested/font.otf");
        fs::create_dir_all(nested.parent().unwrap()).unwrap();
        fs::write(&nested, b"owned nested font").unwrap();
        let link = root.join("assets/font-alias");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // Junction无需开发者模式；链接、目标及后续移除均在本轮独占夹具包内。
            let result = std::process::Command::new("cmd.exe")
                .args(["/D", "/C", "mklink", "/J"]).arg(&link).arg(&actual)
                .creation_flags(0x08000000).output().unwrap();
            assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&actual, &link).unwrap();
        assert!(link.join("nested/font.otf").canonicalize().unwrap().starts_with(root),
            "这个包内别名通过旧canonical包含检查，必须由逐段普通目录门拒绝");
        fs::write(&fixture.project.manifest,
            "[package]\nname='owned_source_fixture'\nversion='0.1.0'\n[package.metadata.feathrise]\ncompiled-inputs=['assets/font-alias/nested/font.otf']\n").unwrap();
        let target = root.join("unused-target");
        let error = capture(&fixture.project, &target, &BuildControl::default()).unwrap_err();
        assert!(error.contains("符号链接") || error.contains("junction"), "{error}");
        assert_eq!(fs::read(&nested).unwrap(), b"owned nested font");
        #[cfg(windows)]
        fs::remove_dir(&link).unwrap();
        #[cfg(unix)]
        fs::remove_file(&link).unwrap();
        fs::write(&fixture.project.manifest,
            "[package]\nname='owned_source_fixture'\nversion='0.1.0'\n[package.metadata.feathrise]\ncompiled-inputs=['assets/real-fonts/nested/font.otf']\n").unwrap();
        let snapshot = capture(&fixture.project, &target, &BuildControl::default()).unwrap();
        assert!(snapshot.source_watches.contains(&(nested.canonicalize().unwrap(), false)));
    }

    #[test]
    fn inherited_workspace_local_dependency_and_sdk_source_are_part_of_identity() {
        let fixture = Fixture::new();
        let sdk = fixture.directory.join("sdk");
        fs::create_dir_all(sdk.join("src")).unwrap();
        fs::write(
            sdk.join("Cargo.toml"),
            "[package]\nname='owned_sdk'\nversion='0.1.0'\n",
        )
        .unwrap();
        fs::write(sdk.join("src/lib.rs"), "pub fn value() -> u8 { 1 }\n").unwrap();
        fs::write(fixture.directory.join("Cargo.toml"), "[workspace]\nmembers=['game','sdk']\n[workspace.dependencies]\nsdk={package='owned_sdk',path='sdk'}\n").unwrap();
        fs::write(&fixture.project.manifest, "[package]\nname='owned_source_fixture'\nversion='0.1.0'\n[dependencies]\nsdk={workspace=true}\n").unwrap();
        let control = BuildControl::default();
        let target = fixture.project.root.join("unused-target");
        let first = capture(&fixture.project, &target, &control).unwrap();
        fs::write(sdk.join("src/lib.rs"), "pub fn value() -> u8 { 2 }\n").unwrap();
        assert_ne!(
            capture(&fixture.project, &target, &control).unwrap().sha256,
            first.sha256
        );
    }

    #[test]
    fn external_build_script_keeps_byte_identity_and_registers_its_parent_scope() {
        let fixture = Fixture::new();
        fs::write(&fixture.project.manifest,
            "[package]\nname='owned_source_fixture'\nversion='0.1.0'\nbuild='../shared_build.rs'\n").unwrap();
        let script = fixture.directory.join("shared_build.rs");
        fs::write(&script, "fn main() {}\n").unwrap();
        let control = BuildControl::default();
        let target = fixture.project.root.join("unused-target");
        let first = capture(&fixture.project, &target, &control).unwrap();
        let parent = script.canonicalize().unwrap().parent().unwrap().to_owned();
        assert!(first.watches.contains(&(parent.clone(), false)));
        assert!(first.source_watches.contains(&(parent, false)));
        fs::write(&script, "fn main() { let version = 2; }\n").unwrap();
        assert_ne!(capture(&fixture.project, &target, &control).unwrap().sha256, first.sha256);
        fs::write(&script, "fn main() {}\n").unwrap();
        assert_eq!(capture(&fixture.project, &target, &control).unwrap(), first,
            "字节恢复仍恢复原摘要；事件代次的ABA撤销由原生监听另行负责");
    }

    #[test]
    fn custom_target_assets_and_author_data_are_excluded_before_they_exist() {
        let mut fixture = Fixture::new();
        let root = fixture.project.root.clone();
        fs::write(root.join("editor-project.json"), r#"{"format":1,"id":"owned-source","name":"源码夹具","cargo_manifest":"Cargo.toml","package":"owned_source_fixture","asset_roots":["src/player-data"],"editor_data":"src/author-store"}"#).unwrap();
        fixture.project = crate::project::open(&root).unwrap();
        let target = root.join("src/temporary/../out-cache");
        let normalized_target = root.join("src/out-cache");
        let excluded = Exclusions::new(&fixture.project, &target);
        assert!(excluded.contains(&normalized_target.join("generated.rs")));
        assert!(!excluded.contains(&root.join("src/out-cache-other/real.rs")));
        let control = BuildControl::default();
        // 不存在路径的词法归属与随后创建后的 canonical 归属必须一致。
        let before = capture(&fixture.project, &target, &control).unwrap();
        let author_root = fixture
            .project
            .context
            .editor_data
            .parent()
            .unwrap()
            .to_owned();
        for directory in [
            &normalized_target,
            &fixture.project.context.asset_roots[0],
            &author_root,
        ] {
            fs::create_dir_all(directory).unwrap();
            fs::write(
                directory.join("generated.rs"),
                "generated output, never a source input",
            )
            .unwrap();
        }
        assert_eq!(
            capture(&fixture.project, &target, &control).unwrap(),
            before
        );
        let ordinary = root.join("src/real.rs");
        fs::write(ordinary, "pub fn real() {}\n").unwrap();
        assert_ne!(
            capture(&fixture.project, &target, &control).unwrap().sha256,
            before.sha256
        );
    }
}
