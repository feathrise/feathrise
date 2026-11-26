//! 空工程检验首个变量的添加，示例工程检验编辑和重开；父进程独立核对各窗口截图及正式文档。
use super::*;
pub(super) const ENV: &str = "FEATHRISE_PROJECT_RULES_VALIDATION";

fn fixture(target: &Path, work: &Path, with_scenes: bool) -> Result<Command, String> {
    let root = crate::project_template::create_with_control(
        &work.join("project"),
        "工程规则验收",
        &crate::project_template_root(),
        &BuildControl::default(),
    )?;
    let project = crate::project::open(&root)?;
    if project.root != work.join("project") || project.context.asset_roots != [root.join("assets")]
    {
        return Err("规则验收工程归属无效".into());
    }
    let path = root.join("assets/project_rules.json");
    if path.exists() {
        return Err("新建夹具已存在工程规则".into());
    }
    write_new(
        &work.join(OWNER),
        &serde_json::to_vec(&owner(&project, work)).map_err(|e| e.to_string())?,
    )?;
    write_new(&work.join(".project-rules-fixture"), b"project-rules-v1")?;
    if with_scenes {
        // 复用带归属校验的场景夹具，供场景工具切换和地点视口取点使用。
        write_new(&work.join(".feathrise-native-validation-owner.json"),
            &serde_json::to_vec(&json!({"schema":1,"purpose":"ordinary-author-boundary-validation","work_root":work}))
                .map_err(|e| e.to_string())?)?;
        crate::native_validation::prepare_workspace_fixture(&root, "交互工具验收")?;
        write_new(
            &root.join("assets/inline_context.md"),
            "# 独立文档\n\n切换右侧交互工具时保留本页。\n".as_bytes(),
        )?;
    }
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
    Ok(command)
}

fn screenshot(work: &Path, name: &str, record: &Value) -> Result<(), String> {
    let png = fs::read(work.join(format!("rules-{name}.png"))).map_err(|e| e.to_string())?;
    if !png.starts_with(b"\x89PNG\r\n\x1a\n") || record["sha256"] != sha256(&png) {
        return Err(format!("规则窗口 {name} 截图校验不符"));
    }
    Ok(())
}

fn verify_window(
    command: &mut Command,
    work: &Path,
    phase: &str,
    proof: &mut Value,
    expected: &Value,
) -> Result<(), String> {
    command.env(ENV, phase);
    let child = build_process::run(
        command,
        &BuildControl::default(),
        Some(Duration::from_secs(150)),
        tail,
    )?;
    write_new(
        &work.join(format!("rules-{phase}.log")),
        format!("{}\n{}", child.stdout, child.stderr).as_bytes(),
    )?;
    let window = read_json(&work.join(format!("rules-{phase}.json"))).map_err(|error| {
        let detail = child
            .stderr
            .lines()
            .rfind(|line| !line.trim().is_empty())
            .unwrap_or("请查看本轮窗口日志");
        format!(
            "规则窗口 {phase} 退出码 {}，{detail}；{error}。日志 rules-{phase}.log",
            child.exit_code
        )
    })?;
    proof[phase] = window.clone();
    if child.exit_code != 0 || window["passed"] != true {
        return Err(format!("规则窗口 {phase} 未通过：{}", window["error"]));
    }
    let disk = read_json(&work.join("project/assets/project_rules.json"))?;
    let mut expected = expected.clone();
    if matches!(phase, "empty" | "draw-reopen") {
        // 第一窗已用原相机投影的已知坐标核验范围；在新进程的表单和两个进程退出后的磁盘分别再核对。
        let range = &proof["empty"]["layout"]["location-fields"]["range"];
        if range["scene"] != "M1-SAMPLE" || range["min"][1] != 0.5 || range["max"][1] != 3.5 {
            return Err("没有真实视口取点和垂直高度回填的证据".into());
        }
        expected["locations"]["drawn_gate"] = range.clone();
    }
    if disk != expected {
        return Err(format!(
            "{phase} 退出后，独立回读的规则内容与编辑结果不符：{disk}"
        ));
    }
    screenshot(work, phase, &window["screenshot"])?;
    let samples: &[&str] = match phase {
        "empty" => &[
            "catalog-portal-default",
            "catalog-portal",
            "catalog-boundary",
            "catalog-motion-trigger",
            "catalog-return",
            "catalog-narrow",
            "catalog-context",
            "empty-variable",
            "first-variable",
            "false-variable",
            "number-variable",
            "number-reread",
            "location",
            "location-drawing",
        ],
        "reopen" => &["rule-wide", "rule-narrow", "rule-narrow-scrolled"],
        _ => &[],
    };
    for name in samples {
        screenshot(work, name, &window["layout"][*name]["screenshot"])?;
    }
    if phase == "empty" {
        let layout = &window["layout"];
        let context = &layout["catalog-context"];
        let tools = context["tools"]
            .as_array()
            .ok_or("缺少中央文档的六项直达核对")?;
        let expected = [
            "传送点",
            "空气墙",
            "动画触发",
            "地点区域",
            "工程变量",
            "触发规则",
        ];
        if layout["variable-dialog-blocked-tool-switch"] != true
            || layout["plain-text-no-options"] != true
            || layout["boolean-value-buttons"] != json!(["false", "true"])
            || layout["variable-type-buttons"] != json!(["number", "bool", "text", "enum"])
            || layout["compact-variable-scroll"]["scroll_y"].as_f64() != Some(0.)
            || layout["variable-deleted-from-list"] != "delete_me"
            || context["center"]["tab"] != "Document"
            || !context["center"]["document"]
                .as_str()
                .is_some_and(|path| path.contains("inline_context.md"))
            || tools.len() != expected.len()
            || tools.iter().zip(expected).any(|(tool, name)| {
                tool["tool"] != name || tool["directory"]["row_counts"] != json!([4, 2])
            })
            || layout["location-drawing"]["center"] != layout["catalog-narrow"]["center"]
            || layout["catalog-narrow"]["center"]["tab"] != "Scene"
        {
            return Err("六项直达、变量对话框或同一场景视口的保留证据不完整".into());
        }
    }
    if phase == "edit" {
        screenshot(work, "source", &window["source_screenshot"])?;
        if window["layout"]["rule-created-in-dialog"] != true
            || window["layout"]["rule-edited-in-dialog"] != true
            || window["layout"]["rule-deleted-from-list"] != "delete_rule"
        {
            return Err("触发规则弹窗或列表直接删除证据不完整".into());
        }
    }
    if (phase == "edit"
        && (window["direct_saves"]["delivery"]["saved"] != expected["variables"]["delivery"]
            || window["layout"]["enum-editor"] != expected["variables"]["delivery"]))
        || (phase == "reopen"
            && window["layout"]["enum-reread"] != expected["variables"]["delivery"])
    {
        return Err("枚举默认值和选项的保存或独立窗口重开证据不完整".into());
    }
    Ok(())
}

pub(super) fn exercise(target: &Path, work: &Path, proof: &mut Value) -> Result<(), String> {
    // 空工程由作者入口新建；示例只由验收宿主写入独占夹具，后续均走真实的打开与编辑入口。
    let blank = work.join("first-variable");
    fs::create_dir(&blank).map_err(|e| e.to_string())?;
    let mut first = fixture(target, &blank, true)?;
    let first_doc = json!({"version":1,"variables":{"gate_open":{"default":true},"a":{"default":3}},
        "locations":{"east_gate":{"scene":"river","min":[0,0,0],"max":[4,3,6]}},"rules":[]});
    verify_window(&mut first, &blank, "empty", proof, &first_doc)?;
    verify_window(&mut first, &blank, "draw-reopen", proof, &first_doc)?;

    let mut command = fixture(target, work, false)?;
    let sample = include_str!(
        "../../crates/feathrise_editor_documents_model/src/project_rules/example.json"
    );
    write_new(
        &work.join("project/assets/project_rules.json"),
        sample.as_bytes(),
    )?;
    let mut expected: Value = serde_json::from_str(sample).map_err(|e| e.to_string())?;
    expected["rules"][0]["id"] = json!("smith_route_edited");
    expected["variables"]["dock_ready"] = json!({"default":false});
    expected["variables"]["a"] = json!({"default":0});
    expected["variables"]["delivery"] = json!({"default":"harbor","choices":["harbor","mentor"]});
    expected["rules"].as_array_mut().unwrap().push(json!({
        "id":"guard_freed","when":{"event":"quest.completed","target":"dock_guard"},
        "if":[{"variable":"dock_ready","equals":false}],"once":true,
        "actions":[{"set":"dock_ready","value":true},{"request":"cinematic.play","target":"guard-unseal"}]
    }));
    expected["rules"].as_array_mut().unwrap().push(json!({
        "id":"draw_at_three","when":{"changed":"a","to":3},
        "actions":[{"request":"cinematic.play","target":"draw-river-sword"}]
    }));
    for phase in ["edit", "reopen"] {
        verify_window(&mut command, work, phase, proof, &expected)?;
    }
    proof["disk_reread_verified"] = json!(true);
    Ok(())
}
