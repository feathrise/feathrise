//! Blender 进程的收件与发布。标准 GLB 是唯一模型数据，所有文件操作都在 IO 任务中执行。

use feathrise_editor_assets::{
    blender_export::{self, EXPORTER_FILENAME},
    glb_metadata::{open_glb, ordinary},
};
use feathrise_scene_content::{
    ScenePlacementRecord, ScenePlacementsAsset,
    authoring_io::{bytes_hash, file_hash, safe_content_path},
    write_scene_placements_glb,
};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

const BRIDGE: &str = include_str!("网格编辑会话.py");
const VERSION: &str = "save.version";
const RESULT: &str = "model.glb";
const GLB_FORMAT_MAX_BYTES: u64 = u32::MAX as u64; // GLB 文件头以 32 位无符号整数记录文件总长度。

pub(super) struct BlenderProcess {
    child: Child,
    root: PathBuf,
    folder: String,
    directory: PathBuf,
    revision: u64,
    digest: String,
}

impl BlenderProcess {
    pub fn start(
        root: PathBuf,
        folder: String,
        record: &ScenePlacementRecord,
        saved_executable: Option<&Path>,
    ) -> Result<Self, String> {
        let executable = super::super::find_blender_executable(saved_executable)?
            .canonicalize().map_err(|error| error.to_string())?;
        let root = root.canonicalize().map_err(|error| error.to_string())?;
        safe_content_path(&root, &folder)?;

        // 旧点面往返会话另属 v1。新进程只创建、清理自己独占的目录。
        let cache = std::env::temp_dir().join("feathrise/blender-glb-v2");
        fs::create_dir_all(&cache).map_err(|error| error.to_string())?;
        let time = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|error| error.to_string())?;
        let directory = cache.join(format!("session-{}-{}", std::process::id(), time.as_nanos()));
        fs::create_dir(&directory).map_err(|error| error.to_string())?;

        let launch = (|| -> Result<Child, String> {
            let exporter = blender_export::prepare(&directory, std::env::var_os("FEATHRISE_BLENDER_TOOLS_DIR"))?;
            let input = input_glb(&root, &directory, record)?;
            let script = directory.join("bridge.py");
            fs::write(&script, BRIDGE).map_err(|error| error.to_string())?;
            let mut command = Command::new(executable);
            // 正常 Blender 启动读取用户偏好，脚本只替换本进程里的场景。
            command.args(["--disable-autoexec", "--python-exit-code", "1", "--python"])
                .arg(script).arg("--").arg(&directory).arg(input).arg(exporter)
                .current_dir(&directory)
                .env("PYTHONDONTWRITEBYTECODE", "1")
                .env("PYTHONIOENCODING", "utf-8")
                .stdin(Stdio::null())
                .stdout(File::create(directory.join("stdout.log")).map_err(|error| error.to_string())?)
                .stderr(File::create(directory.join("stderr.log")).map_err(|error| error.to_string())?);
            command.spawn().map_err(|error| format!("无法启动 Blender（{error}）"))
        })();
        match launch {
            Ok(child) => Ok(Self { child, root, folder, directory, revision: 0, digest: String::new() }),
            Err(error) => {
                cleanup_directory(&directory);
                Err(error)
            }
        }
    }

    pub fn poll(&mut self) -> (bool, Result<Option<String>, String>) {
        let status = match self.child.try_wait() {
            Ok(status) => status,
            Err(error) => return (true, Err(format!("无法查询 Blender 进程（{error}）"))),
        };
        let exited = status.is_some();
        let result = self.read_result(exited).and_then(|result| {
            if result.is_some() || status.is_none_or(|status| status.success()) {
                return Ok(result);
            }
            let log = log_tail(&self.directory);
            Err(if log.is_empty() { "Blender 异常退出".into() } else { log })
        });
        (exited, result)
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn cleanup(self) {
        cleanup_directory(&self.directory);
    }

    fn read_result(&mut self, exited: bool) -> Result<Option<String>, String> {
        let receipt = match read_receipt(&self.directory) {
            Ok(receipt) => receipt,
            Err(error) if error.kind() == io::ErrorKind::NotFound && !exited => return Ok(None),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let log = log_tail(&self.directory);
                return Err(if log.is_empty() { "Blender 尚未创建工作模型".into() } else { log });
            }
            Err(error) => return Err(error.to_string()),
        };
        let mut parts = receipt.lines();
        let revision = parts.next().and_then(|value| value.parse::<u64>().ok())
            .ok_or("Blender 保存版本无效")?;
        let state = parts.next().ok_or("Blender 保存回执不完整")?;
        let baseline = parts.next().ok_or("Blender 保存回执缺少初始摘要")?;
        if self.digest.is_empty() {
            self.digest = baseline.to_owned();
        }
        if state == "writing" {
            return if exited { Err("Blender 在导出期间退出".into()) } else { Ok(None) };
        }
        if state == "error" && (revision > self.revision || exited) {
            self.revision = revision;
            return Err(parts.next().unwrap_or("Blender 导出失败").to_owned());
        }
        if revision <= self.revision {
            return Ok(None);
        }
        if state != "ready" {
            return Err("Blender 保存回执状态无效".into());
        }

        let mut bytes = Vec::new();
        let path = self.directory.join(RESULT);
        open_glb(&path, GLB_FORMAT_MAX_BYTES)?.take(GLB_FORMAT_MAX_BYTES + 1).read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        // 两次回执锁定一次完整替换。连续按保存时，跨版本读取直接留给下一次轮询。
        if read_receipt(&self.directory).map_err(|error| error.to_string())? != receipt {
            return Ok(None);
        }
        if bytes.len() as u64 > GLB_FORMAT_MAX_BYTES || !bytes.starts_with(b"glTF") {
            return Err("Blender 回传文件超出 GLB 格式长度范围或缺少 GLB 文件头".into());
        }
        let digest = bytes_hash(&bytes);
        if digest == self.digest {
            self.revision = revision;
            return Ok(None);
        }
        let relative = format!("{}/models/blender-{digest}.glb", self.folder);
        let destination = safe_content_path(&self.root, &relative)?;
        let tag = self.directory.file_name().ok_or("Blender 会话目录名称无效")?.to_string_lossy();
        publish_model(&destination, &tag, &digest, &bytes)?;
        self.digest = digest;
        self.revision = revision;
        Ok(Some(relative))
    }
}

fn input_glb(root: &Path, directory: &Path, record: &ScenePlacementRecord) -> Result<PathBuf, String> {
    if record.asset_target.is_builtin() {
        // 现有场景 GLB 写入器烘焙作者网格和材质。实例摆放由当前场景保留。
        let local = ScenePlacementRecord {
            translation: [0.0; 3],
            yaw_degrees: 0.0,
            scale: [1.0; 3],
            ..record.clone()
        };
        let scene = ScenePlacementsAsset { placements: vec![local], ..Default::default() };
        let bytes = write_scene_placements_glb(&scene, "input.glb", None)?;
        let input = directory.join("input.glb");
        fs::write(&input, bytes).map_err(|error| error.to_string())?;
        return Ok(input);
    }
    if !record.asset_target.is_scene0()
        || !Path::new(&record.asset_path).extension().is_some_and(|value| value.eq_ignore_ascii_case("glb"))
    {
        return Err("在 Blender 编辑需要内置网格或完整 GLB 模型".into());
    }
    let path = safe_content_path(root, &record.asset_path)?;
    open_glb(&path, GLB_FORMAT_MAX_BYTES)?;
    Ok(path)
}

fn read_receipt(directory: &Path) -> io::Result<String> {
    let mut text = String::new();
    File::open(directory.join(VERSION))?.take(4096).read_to_string(&mut text)?;
    Ok(text)
}

fn publish_model(path: &Path, tag: &str, digest: &str, bytes: &[u8]) -> Result<(), String> {
    let same_content = || -> Result<(), String> {
        let metadata = ordinary(path)?;
        if metadata.is_file() && metadata.len() == bytes.len() as u64 && file_hash(path)? == digest {
            return Ok(());
        }
        Err(format!("Blender 生成路径已被不同内容占用：{}", path.display()))
    };
    match fs::symlink_metadata(path) {
        Ok(_) => return same_content(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    let parent = path.parent().ok_or("Blender 模型路径缺少父目录")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let staged = parent.join(format!(".blender-{tag}.tmp"));
    let mut file = OpenOptions::new().write(true).create_new(true).open(&staged)
        .map_err(|error| error.to_string())?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    // 同目录硬链接沿用不可变体积载体的发布方式，保证即使另一个作者并发生成也不替换目标。
    let result = written.and_then(|()| fs::hard_link(&staged, path));
    let _ = fs::remove_file(&staged);
    match result {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => same_content(),
        Err(error) => Err(format!("无法发布 Blender 模型（{error}）")),
    }
}

fn log_tail(directory: &Path) -> String {
    let mut message = Vec::new();
    if let Ok(mut file) = File::open(directory.join("stderr.log")) {
        let length = file.metadata().map_or(0, |metadata| metadata.len());
        let _ = file.seek(SeekFrom::Start(length.saturating_sub(1024)));
        let _ = file.take(1024).read_to_end(&mut message);
    }
    String::from_utf8_lossy(&message).trim().to_owned()
}

fn cleanup_directory(directory: &Path) {
    if !ordinary(directory).is_ok_and(|metadata| metadata.is_dir()) {
        return;
    }
    let Ok(entries) = fs::read_dir(directory) else { return; };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        let backup = name.strip_prefix("model.blend")
            .is_some_and(|suffix| !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit()));
        let exporter_stage = name.strip_prefix(".model.").and_then(|value| value.strip_suffix(".exporting.glb"))
            .is_some_and(|pid| !pid.is_empty() && pid.bytes().all(|byte| byte.is_ascii_digit()));
        if backup || exporter_stage || matches!(name.as_ref(), "model.blend" | RESULT | "input.glb"
            | "bridge.py" | EXPORTER_FILENAME | VERSION | ".save.version.tmp" | "stdout.log" | "stderr.log" | "quit.blend")
        {
            let _ = fs::remove_file(entry.path());
        }
    }
    // Blender 中主动另存的其他文件不属于自动清理范围。
    let _ = fs::remove_dir(directory);
}
