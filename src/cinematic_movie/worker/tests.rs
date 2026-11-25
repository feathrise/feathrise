//! 正式宿主后台从可移动工程构建游戏、拍摄、编码和发布。只接受本轮新建的夹具工程。
use super::*;
#[test]
#[ignore = "显式指定自建摄影夹具、Cargo 缓存和 FFmpeg，启动真实 GPU 子进程"]
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
    let ffmpeg = env("FEATHRISE_FFMPEG")?;
    let output = evidence.join("production.mp4");
    let control = BuildControl::default();
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
        ffmpeg: Some(ffmpeg.clone()),
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
    let mut decode = Command::new(ffmpeg);
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
    assert_eq!(frames.len(), 42, "{}", decoded.stdout);
    assert_ne!(
        frames[0].rsplit(',').next(),
        frames[32].rsplit(',').next(),
        "切镜后必须有不同像素"
    );
    if sound.is_file() {
        let pcm = evidence.join("decoded-audio.s16le");
        let mut audio = Command::new(env("FEATHRISE_FFMPEG")?);
        audio
            .args(["-loglevel", "error", "-nostdin", "-n", "-i"])
            .arg(&path)
            .args(["-map", "0:a:0", "-ar", "44100", "-ac", "1", "-f", "s16le"])
            .arg(&pcm);
        let decoded = build_process::run(&audio, &control, None, build_process::read_diagnostics)?;
        assert_eq!(decoded.exit_code, 0, "{}", decoded.stderr);
        let samples = fs::metadata(pcm).map_err(|e| e.to_string())?.len() / 2;
        assert!(
            samples.abs_diff(154_350) <= 1024,
            "全片 3.5 秒的配乐应只差 AAC 编码器的一帧，实际 {samples} 个采样"
        );
    }
    Ok(())
}
