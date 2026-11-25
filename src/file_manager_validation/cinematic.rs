//! 中性工程和两个独立的真实编辑器分别验证作者制作与重开，再从文件系统独立核对数据和截图。
use super::*;

pub(super) fn exercise(target: &Path, work: &Path, proof: &mut Value) -> Result<(), String> {
    let root = crate::project_template::create_with_control(
        &work.join("project"),
        "剧情演出验收",
        &crate::project_template_root(),
        &BuildControl::default(),
    )?;
    let project = crate::project::open(&root)?;
    if project.root != work.join("project") || project.context.asset_roots != [root.join("assets")]
    {
        return Err("演出验收夹具归属无效".into());
    }
    for (filename, value) in [
        (OWNER, owner(&project, work)),
        (
            ".feathrise-native-validation-owner.json",
            json!({"schema":1,"purpose":"ordinary-author-boundary-validation","work_root":work}),
        ),
    ] {
        write_new(
            &work.join(filename),
            &serde_json::to_vec(&value).map_err(|e| e.to_string())?,
        )?;
    }
    write_new(
        &work.join(".cinematic-author-fixture"),
        b"cinematic-author-v1",
    )?;
    crate::native_validation::prepare_workspace_fixture(&root, "剧情演出验收")?;
    let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
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
        .env(super::ENV, work);
    crate::native_validation::isolate_author_command(&mut command, work)?;
    let asset = root.join("assets/交付验收.cinematic.json");
    for phase in ["edit", "reopen"] {
        command.env("FEATHRISE_CINEMATIC_AUTHOR_VALIDATION", phase);
        let child = build_process::run(
            &command,
            &BuildControl::default(),
            Some(Duration::from_secs(240)),
            tail,
        )?;
        write_new(
            &work.join(format!("cinematic-{phase}.log")),
            format!("{}\n{}", child.stdout, child.stderr).as_bytes(),
        )?;
        let window = read_json(&work.join(format!("cinematic-{phase}.json")))?;
        proof[phase] = window.clone();
        if child.exit_code != 0 || window["passed"] != true {
            return Err(format!("演出窗口 {phase} 未通过：{}", window["error"]));
        }
        let bytes = fs::read(&asset).map_err(|e| e.to_string())?;
        let f = feathrise_cinematic::Cinematic::from_slice(&bytes).map_err(|e| e.to_string())?;
        let actor = f.actors.first().filter(|t| {
            t.target == "guard_point#0"
                && t.keys.len() == 1
                && t.clips.len() == 1
                && t.clips[0].name == "Walk"
        });
        if f.camera.len() != 2
            || f.camera[1].time != 4.
            || f.actors.len() != 1
            || actor.is_none()
            || f.objects.len() != 1
            || f.objects[0].target != "sword_rack"
            || f.objects[0].keys.len() != 1
            || f.dialogue.len() != 2
            || f.dialogue[0].text.as_deref() != Some("守卫接过剑")
            || f.dialogue[1].yarn.as_deref() != Some("GuardDelivery")
        {
            return Err("独立回读的四轨属性与实际作者输入不一致".into());
        }
        let rules = read_json(&root.join("assets/project_rules.json"))?;
        if !rules["rules"].as_array().is_some_and(|rules| {
            rules.iter().any(|r| {
                r["when"] == json!({"event":"location.entered","target":"east_gate"})
                    && r["actions"] == json!([{"request":"cinematic.play","target":f.id}])
            })
        }) {
            return Err("独立回读未找到绑定到此演出稳定 ID 的地点进入规则".into());
        }
        let image =
            fs::read(work.join(format!("cinematic-{phase}.png"))).map_err(|e| e.to_string())?;
        if !image.starts_with(b"\x89PNG\r\n\x1a\n")
            || window["screenshot"]["sha256"] != sha256(&image)
        {
            return Err("四轨窗口截图校验不符".into());
        }
        proof["asset_id"] = json!(f.id);
        proof["asset_sha256"] = json!(sha256(&bytes));
    }
    proof["disk_reread_verified"] = json!(true);
    Ok(())
}
