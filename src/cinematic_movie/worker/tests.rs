//! 正式宿主后台从可移动工程构建游戏、拍摄、编码和发布。只接受本轮新建的夹具工程。
use super::*;
#[test]
#[ignore = "显式指定自建摄影夹具、Cargo 缓存和解码器，启动真实 GPU 子进程"]
fn render_saved_project_and_publish_playable_mp4() -> Result<(), String> {
    let env = |key| {
        std::env::var_os(key)
            .map(PathBuf::from)
            .ok_or_else(|| format!("缺少 {key}"))
    };
    let project_root = env("FEATHRISE_MOVIE_TEST_PROJECT")?;
    let evidence = env("FEATHRISE_MOVIE_TEST_EVIDENCE")?;
    if fs::read_to_string(project_root.join(".movie-validation"))
        .ok()
        .as_deref()
        != Some("owned-cinematic-test")
        || !evidence.is_dir()
    {
        return Err("需要本轮独立创建的摄影夹具".into());
    }
    let game = crate::project::open(&project_root)?;
    let root = game.context.asset_roots[0].clone();
    let control = BuildControl::default();
    let embedded = std::env::var("FEATHRISE_MOVIE_TEST_EMBEDDED").as_deref() == Ok("1");
    let (ffmpeg, decoder, cache) = if embedded {
        let decoder = env("FEATHRISE_MOVIE_TEST_DECODER")?;
        let cache = crate::writable_paths::user_cache_dir()?.join("tools/embedded-ffmpeg-v1");
        let local_app_data = env("LOCALAPPDATA")?
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let expected = std::env::var("FEATHRISE_MOVIE_TEST_ENCODER_SHA256")
            .map_err(|_| "缺少内嵌编码器输入的 SHA-256")?
            .to_ascii_lowercase();
        if !cfg!(windows)
            || std::env::var_os("FEATHRISE_FFMPEG").is_some()
            || !decoder.is_absolute()
            || !decoder.is_file()
            || expected.len() != 64
            || !expected.bytes().all(|b| b.is_ascii_hexdigit())
            || !local_app_data.starts_with(evidence.canonicalize().map_err(|e| e.to_string())?)
            || cache.exists()
        {
            return Err("内嵌编码验收须使用本轮独占的 LOCALAPPDATA、全新编码器缓存、合法编码器摘要、绝对解码器路径且没有 FFmpeg 覆盖".into());
        }
        (
            None,
            decoder,
            Some((cache.join(&expected).join("ready/ffmpeg.exe"), expected)),
        )
    } else {
        let ffmpeg = env("FEATHRISE_FFMPEG")?;
        (Some(ffmpeg.clone()), ffmpeg, None)
    };
    let output = evidence.join("production.mp4");
    let sound = evidence.join("sound.wav");
    let options = || Options {
        root: root.clone(),
        asset: root.join("cinematics/studio.cinematic.json"),
        output: output.clone(),
        scene: None,
        settings: MovieSettings {
            width: 640,
            height: 360,
            fps: 12,
            seconds_per_line: 0.5,
            ..Default::default()
        },
        ffmpeg: ffmpeg.clone(),
        sound: sound.is_file().then(|| sound.clone()),
        replace: false,
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let target = env("FEATHRISE_MOVIE_TEST_TARGET")?;
    let worker = std::thread::spawn(move || {
        for phase in rx {
            eprintln!("{phase:?}")
        }
    });
    let path = export(game.clone(), target.clone(), options(), &control, tx)?;
    worker.join().map_err(|_| "进度线程退出")?;
    if let Some((actual, expected)) = cache {
        // 只读取真实导出留下的缓存，不在核验阶段重新调用解析器或释放编码器。
        let (digest, _) = crate::build_artifacts::hash_file(&actual, &control)?;
        if digest != expected {
            return Err(format!(
                "导出没有使用本轮释放的内嵌编码器：{}",
                actual.display()
            ));
        }
        eprintln!("内嵌 FFmpeg 已验证，SHA-256 {digest}");
    }
    let baseline = crate::build_artifacts::digest(&fs::read(&path).map_err(|e| e.to_string())?);
    assert!(
        export(
            game.clone(),
            target.clone(),
            options(),
            &control,
            std::sync::mpsc::channel().0
        )
        .unwrap_err()
        .contains("已存在")
    );
    let cancelled = BuildControl::default();
    let capture_control = cancelled.clone();
    let (events, phases) = std::sync::mpsc::channel();
    // 在包准备完、摄影即将开始时取消一次明确允许覆盖的导出；同名合格影片仍需逐字节保留。
    let stop = std::thread::spawn(move || {
        for phase in phases {
            if matches!(phase, Feedback::Status(ref text) if text.contains("逐帧摄影")) {
                capture_control.cancel();
                break;
            }
        }
    });
    let replace = Options {
        replace: true,
        ..options()
    };
    let cancelled_result = export(game, target, replace, &cancelled, events);
    stop.join().map_err(|_| "取消线程退出")?;
    assert!(cancelled_result.unwrap_err().contains("取消"));
    assert_eq!(
        baseline,
        crate::build_artifacts::digest(&fs::read(&path).map_err(|e| e.to_string())?)
    );
    let mut decode = Command::new(&decoder);
    decode
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-i"])
        .arg(&path)
        .args(["-map", "0:v:0", "-f", "framemd5", "-"]);
    let decoded = build_process::run(&decode, &control, None, build_process::read_diagnostics)?;
    assert_eq!(decoded.exit_code, 0, "{}", decoded.stderr);
    let frames: Vec<_> = decoded
        .stdout
        .lines()
        .filter(|line| line.starts_with("0,"))
        .collect();
    let expected_frames = if embedded { 18 } else { 42 };
    assert_eq!(frames.len(), expected_frames, "{}", decoded.stdout);
    assert!(
        decoded.stdout.contains("#dimensions 0: 640x360"),
        "{}",
        decoded.stdout
    );
    assert_ne!(
        frames[0].rsplit(',').next(),
        frames[if embedded { 9 } else { 32 }].rsplit(',').next(),
        "切镜后必须有不同像素"
    );
    if sound.is_file() {
        let pcm = evidence.join("decoded-audio.s16le");
        let mut audio = Command::new(decoder);
        audio
            .args(["-loglevel", "error", "-nostdin", "-n", "-i"])
            .arg(&path)
            .args(["-map", "0:a:0", "-ar", "44100", "-ac", "1", "-f", "s16le"])
            .arg(&pcm);
        let decoded = build_process::run(&audio, &control, None, build_process::read_diagnostics)?;
        assert_eq!(decoded.exit_code, 0, "{}", decoded.stderr);
        let samples = fs::metadata(pcm).map_err(|e| e.to_string())?.len() / 2;
        assert!(
            samples.abs_diff(44_100 * expected_frames as u64 / 12) <= 1024,
            "整片配乐应只差 AAC 编码器的一帧，实际 {samples} 个采样"
        );
    }
    Ok(())
}
