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
    // 预置一条不同的事件，使“打开规则页”与“定位新绑定”在真窗口中可区分。
    let existing_rule = json!({"id":"existing_watch","when":{"event":"quest.completed","target":"old_watch"},
        "actions":[{"request":"quest.start","target":"next_watch"}]});
    let rules_path = root.join("assets/project_rules.json");
    write_new(
        &rules_path,
        json!({"version":1,"variables":{},"rules":[existing_rule]})
            .to_string()
            .as_bytes(),
    )?;
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
            || !f.dialogue[0].wait
        {
            return Err("独立回读的四轨属性与实际作者输入不一致".into());
        }
        let policy = json!({"repeat":f.repeat,"conditions":f.conditions,"on_finish":f.on_finish});
        if policy != json!({
            "repeat":{"once_per_quest":{"quest":"guard_delivery"}},
            "conditions":[
                {"kind":"quest","quest":"guard_delivery","status":"active","stage":1},
                {"kind":"has_item","item":"name:守印剑"},
                {"kind":"seen","cinematic":"unseal","seen":true}
            ],
            "on_finish":[
                {"kind":"set_variable","key":"gate_open","value":3.0},
                {"kind":"set_variable","key":"cinematic_done","value":true},
                {"kind":"set_variable","key":"sword_route","value":"guard"},
                {"kind":"quest_objective","quest":"guard_delivery","from":1,"to":2}
            ]
        }) {
            return Err("独立回读的播放次数、条件或收尾动作与可视输入不符".into());
        }
        let selected_id = proof["edit"]["saved_rule_from_form"]
            .as_str()
            .filter(|id| id.starts_with("play_"))
            .ok_or("窗口未记录显式保存的规则 ID")?;
        let file = read_json(&rules_path)?;
        let Some([old, added]) = file["rules"].as_array().map(Vec::as_slice) else {
            return Err("独立回读应包含原有规则和一条新绑定".into());
        };
        if old != &existing_rule
            || added["id"] != selected_id
            || added["when"] != json!({"event":"location.entered","target":"east_gate"})
            || added["actions"] != json!([{"request":"cinematic.play","target":f.id}])
        {
            return Err("工程规则回读与窗口选中的绑定或原有规则不一致".into());
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
    let settings = fs::read(work.join("cinematic-settings.png")).map_err(|e| e.to_string())?;
    if proof["edit"]["policy_input_checked"] != true
        || proof["edit"]["policy_screenshot"]["sha256"] != sha256(&settings)
        || !settings.starts_with(b"\x89PNG\r\n\x1a\n")
    {
        return Err("演出设置的真实操作或截图证据缺失".into());
    }
    let export = fs::read(work.join("cinematic-export.png")).map_err(|e| e.to_string())?;
    if proof["reopen"]["export_input_checked"] != true
        || proof["reopen"]["export_screenshot"]["sha256"] != sha256(&export)
        || !export.starts_with(b"\x89PNG\r\n\x1a\n")
        || root.join("交付验收.mp4").exists()
    {
        return Err("视频面板截图、输入校验或失败后输出保护不符".into());
    }
    proof["disk_reread_verified"] = json!(true);
    Ok(())
}
