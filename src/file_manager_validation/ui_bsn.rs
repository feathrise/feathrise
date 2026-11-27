//! 通过独立 Feathrise 的两个实际进程验收原生 Rust 界面制作、源码往返及双击重开。

use super::*;

pub(super) const PHASE_ENV: &str = "FEATHRISE_UI_BSN_VALIDATION";

pub(super) fn exercise(target: &Path, work: &Path, proof: &mut Value) -> Result<(), String> {
    let root = crate::project_template::create_with_control(
        &work.join("project"),
        "界面布局验收",
        &crate::project_template_root(),
        &BuildControl::default(),
    )?;
    let project = crate::project::open(&root)?;
    if project.root != work.join("project") || project.context.asset_roots != [root.join("assets")]
    {
        return Err("界面布局验收工程归属无效".into());
    }
    fs::create_dir(root.join("assets/ui")).map_err(|error| error.to_string())?;
    write_new(
        &work.join(OWNER),
        &serde_json::to_vec(&owner(&project, work)).map_err(|e| e.to_string())?,
    )?;
    write_new(&work.join(".ui-bsn-fixture"), b"feathrise-ui-bsn-v1")?;
    let mut command = Command::new(std::env::current_exe().map_err(|error| error.to_string())?);
    for (key, _) in std::env::vars_os() {
        let name = key.to_string_lossy();
        if name.contains("_VALIDATION")
            || name.contains("_SMOKE_")
            || name.starts_with("FEATHRISE_WORKSPACE_")
        {
            command.env_remove(key);
        }
    }
    command
        .args(["--native-editor", "--project"])
        .arg(&root)
        .arg("--target-dir")
        .arg(target)
        .env(ENV, work);
    crate::native_validation::isolate_author_command(&mut command, work)?;

    let asset = root.join("assets/ui/character_stats.rs");
    let mut saved = None;
    for phase in ["edit", "reopen"] {
        command.env(PHASE_ENV, phase);
        let child = build_process::run(
            &command,
            &BuildControl::default(),
            Some(Duration::from_secs(200)),
            tail,
        )?;
        write_new(
            &work.join(format!("ui-bsn-{phase}.log")),
            format!("{}\n{}", child.stdout, child.stderr).as_bytes(),
        )?;
        let window = read_json(&work.join(format!("ui-bsn-{phase}.json")))?;
        proof[phase] = window.clone();
        if child.exit_code != 0 || window["passed"] != true {
            return Err(format!("界面布局窗口 {phase} 未通过，{}", window["error"]));
        }
        let data = fs::read(&asset).map_err(|error| error.to_string())?;
        let source = std::str::from_utf8(&data).map_err(|error| error.to_string())?;
        if !source.starts_with("// feathrise-ui: editable\n")
            || !source.contains("bsn!")
            || !source.contains(r#""physical_attack""#)
            || !source.contains(r#""health""#)
            || !source.contains("position_type: PositionType::Absolute")
            || !source.ends_with("\npub const EDITOR_USER_CODE: &str = \"manual Rust is retained\";\n")
        {
            return Err("实际落盘的 Rust 界面缺少原生宏、绑定、拖动坐标或宏外手写代码".into());
        }
        let hash = sha256(&data);
        if saved.as_ref().is_some_and(|before| before != &hash) {
            return Err("双击重开改变了界面资产原文件".into());
        }
        saved = Some(hash);
        let png = fs::read(work.join(format!("ui-bsn-{phase}.png")))
            .map_err(|error| error.to_string())?;
        if !png.starts_with(b"\x89PNG\r\n\x1a\n") || window["screenshot"]["sha256"] != sha256(&png)
        {
            return Err("界面布局截图的实际字节与窗口回执不符".into());
        }
    }
    proof["asset"] = json!(asset);
    proof["asset_sha256"] = json!(saved);
    Ok(())
}
