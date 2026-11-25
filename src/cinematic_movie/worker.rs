//! 一个独立工程进程产生已回读的 PNG 序列，再由 FFmpeg 编码。发布前任何失败都保留原影片。
#[cfg(test)]
mod tests;
use super::{Feedback, Options};
use crate::{
    build_process::{self, BuildControl},
    native_play::{ProjectBuildBackend, ProjectBuildFeatures, prepare_game_with_backend},
    project::Project,
};
use feathrise_cinematic::{Cinematic, movie::*};
use feathrise_editor_core::ProjectBuildProfile;
use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc::Sender,
};

struct Directory(PathBuf);
impl Directory {
    fn create(parent: &Path) -> Result<Self, String> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        for index in 0u64.. {
            let name = format!(".feathrise-movie-{}-{nonce:x}-{index}", std::process::id());
            let path = parent.join(name);
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("无法创建导出工作目录：{e}")),
            }
        }
        unreachable!()
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) fn export(
    project: Project,
    target: PathBuf,
    options: Options,
    control: &BuildControl,
    status: Sender<Feedback>,
) -> Result<PathBuf, String> {
    let project = project.refresh_for_session()?;
    let root = project
        .context
        .asset_roots
        .first()
        .ok_or("工程缺少主资产根")?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if options.root.canonicalize().map_err(|e| e.to_string())? != root {
        return Err("演出不属于当前工程主资产根".into());
    }
    let asset = options.asset.canonicalize().map_err(|e| e.to_string())?;
    let relative = asset
        .strip_prefix(&root)
        .map_err(|_| "演出路径已离开当前工程")?
        .to_owned();
    if !feathrise_cinematic::is_cinematic_path(&asset) {
        return Err("请选择已保存的 .cinematic.json 演出".into());
    }
    options.settings.validate()?;
    let bytes = fs::read(asset).map_err(|e| e.to_string())?;
    let doc = Cinematic::from_slice(&bytes)?;
    let output = project.root.join(&options.output);
    if !output
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("mp4"))
    {
        return Err("输出文件须使用 .mp4 扩展名".into());
    }
    let parent = output.parent().ok_or("输出文件缺少父目录")?;
    if !parent.is_dir() {
        return Err(format!("输出目录不存在：{}", parent.display()));
    }
    if output.exists() && !options.replace {
        return Err("输出文件已存在。请选择新文件名，或明确允许覆盖".into());
    }
    let work = Directory::create(&std::env::temp_dir())?;
    let frames = work.0.join("frames");
    fs::create_dir(&frames).map_err(|e| e.to_string())?;
    let job = MovieJob {
        version: PROTOCOL,
        project_id: project.id.clone(),
        asset: relative,
        cinematic: doc.id.clone(),
        asset_sha256: crate::build_artifacts::digest(&bytes),
        scene: options.scene.clone(),
        settings: options.settings,
    };
    let jobpath = work.0.join("job.json");
    fs::write(
        &jobpath,
        serde_json::to_vec(&job).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let ffmpeg = options.ffmpeg.unwrap_or_else(|| {
        std::env::var_os("FEATHRISE_FFMPEG")
            .map(PathBuf::from)
            .unwrap_or_else(|| "ffmpeg".into())
    });
    let mut check = Command::new(&ffmpeg);
    check.args(["-hide_banner", "-h", "encoder=libx264"]);
    let codec = build_process::run(&check, control, None, build_process::read_diagnostics)
        .map_err(|e| format!("无法启动 FFmpeg，请填写可执行文件路径：{e}"))?;
    if codec.exit_code != 0 || !codec.stdout.contains("Encoder libx264") {
        return Err(format!("FFmpeg 缺少 libx264 编码器：{}", codec.stderr));
    }
    if let Some(sound) = &options.sound
        && !sound.is_file()
    {
        return Err(format!("配乐文件不存在：{}", sound.display()));
    }
    let _ = status.send(Feedback::Status("正在准备工程运行程序".into()));
    // 离线摄影运行正式游戏进程，不需要 DX 热补丁工具链。
    let prepared = prepare_game_with_backend(
        &project,
        &target,
        job.scene.as_deref(),
        ProjectBuildProfile::Debug,
        ProjectBuildBackend::Native,
        &ProjectBuildFeatures::default(),
        control,
        None,
    )?;
    let mut command = prepared.embedded_command(&project, control)?;
    command
        .env(MOVIE_JOB_ENV, &jobpath)
        .env("EDITOR_SAVED_PLAY", "1");
    if let Some(scene) = &job.scene {
        command.env("EDITOR_SAVED_PLAY_SCENE", scene);
    }
    let _ = status.send(Feedback::Status("正在游戏场景中逐帧摄影".into()));
    let reader_control = control.clone();
    let unsupported = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let old = unsupported.clone();
    let frame_status = status.clone();
    let child = build_process::run(&command, control, None, move |file| {
        let mut protocol = false;
        for line in BufReader::new(file).lines() {
            let line = line.map_err(|e| e.to_string())?;
            if let Some(progress) = line.strip_prefix("FEATHRISE_MOVIE_FRAMES ") {
                let _ = frame_status.send(Feedback::Status(format!(
                    "正在游戏场景中摄影 · {progress} 帧"
                )));
            }
            if line.trim() == format!("FEATHRISE_MOVIE_PROTOCOL={PROTOCOL}") {
                protocol = true
            }
            if line.starts_with("NEW_EDITOR_SAVED_GAME_START ") && !protocol {
                old.store(true, std::sync::atomic::Ordering::Release);
                reader_control.cancel();
            }
        }
        Ok(())
    });
    if unsupported.load(std::sync::atomic::Ordering::Acquire) {
        return Err("当前工程 SDK 尚不支持影片摄影协议，请更新工程运行时".into());
    }
    let child = child?;
    let result = fs::read(work.0.join("result.json"))
        .map_err(|_| format!("工程退出但未交回摄影结果：{}", child.stderr))?;
    let receipt: MovieReceipt = serde_json::from_slice(&result).map_err(|e| e.to_string())?;
    if let Some(error) = receipt.error {
        if let Some(choice) = receipt.choice {
            let _ = status.send(Feedback::Choice(choice));
        }
        return Err(error);
    }
    if child.exit_code != 0
        || receipt.version != PROTOCOL
        || receipt.cinematic != doc.id
        || receipt.frames == 0
    {
        return Err(format!("摄影回执不完整：{}", child.stderr));
    }
    let staging = Directory::create(parent)?;
    let encoded = staging.0.join("movie.mp4");
    let _ = status.send(Feedback::Status(format!(
        "正在编码 {} 帧 H.264 视频",
        receipt.frames
    )));
    encode(
        &ffmpeg,
        &frames,
        &encoded,
        job.settings.fps,
        receipt.frames,
        options.sound.as_deref(),
        control,
    )?;
    control.check()?;
    feathrise_editor_assets::files::publish_staged_file(&encoded, &output, options.replace)
        .map_err(|e| format!("发布 {} 失败：{e}", output.display()))?;
    Ok(output)
}
fn encode(
    ffmpeg: &Path,
    frames: &Path,
    output: &Path,
    fps: u32,
    count: u64,
    sound: Option<&Path>,
    control: &BuildControl,
) -> Result<(), String> {
    let mut cmd = Command::new(ffmpeg);
    cmd.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostdin",
        "-n",
        "-nostats",
        "-progress",
        "pipe:1",
        "-framerate",
        &fps.to_string(),
        "-start_number",
        "0",
        "-i",
    ])
    .arg(frames.join("frame-%08d.png"));
    if let Some(sound) = sound {
        cmd.arg("-i").arg(sound).args([
            "-map", "0:v:0", "-map", "1:a:0", "-c:a", "aac", "-af", "apad",
        ]);
    }
    cmd.args([
        // PNG 输入天然在最后一帧结束。以整片时长裁两条流，避免 -frames:v 提前截断 AAC。
        "-t",
        &format!("{:.12}", count as f64 / fps as f64),
        "-c:v",
        "libx264",
        "-pix_fmt",
        "yuv420p",
        "-movflags",
        "+faststart",
        "-f",
        "mp4",
    ])
    .arg(output);
    let result = build_process::run(&cmd, control, None, build_process::read_diagnostics)?;
    if result.exit_code != 0 {
        return Err(format!("FFmpeg 编码失败：{}", result.stderr));
    }
    let actual = result
        .stdout
        .lines()
        .filter_map(|l| l.strip_prefix("frame="))
        .last()
        .and_then(|s| s.trim().parse::<u64>().ok());
    if actual != Some(count) || !result.stdout.lines().any(|l| l.trim() == "progress=end") {
        return Err(format!(
            "FFmpeg 实际编码 {actual:?} 帧，摄影回执要求 {count} 帧"
        ));
    }
    Ok(())
}
