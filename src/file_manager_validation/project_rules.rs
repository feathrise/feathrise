//! 独占工程检验首次添加、编辑重开和分页；父进程独立核对各窗口截图及正式文档。
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
    // 首窗包含六项工具、地点取点、两种作用域的保存、场景切换及两种中央页取证。
    let timeout = if phase == "empty" { 240 } else { 150 };
    let child = build_process::run(
        command,
        &BuildControl::default(),
        Some(Duration::from_secs(timeout)),
        tail,
    )
    .map_err(|error| format!("规则窗口 {phase}：{error}"))?;
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
            "variable-column-resize",
            "number-variable",
            "number-reread",
            "location",
            "location-drawing",
            "scene-preview-property",
            "scene-variable-property",
            "scene-trigger-property",
            "scene-alternate-property",
            "scene-property-on-document",
        ],
        "edit" => &["rule-column-resize"],
        "reopen" => &["rule-wide", "rule-narrow", "rule-narrow-scrolled"],
        "paging" => &["variable-pager-first", "rule-pager-last"],
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
        if layout["scene-config-tree"]["labels"] != json!(["场景预览", "场景变量", "场景触发"])
            || layout["scene-preview-controls"] != json!(["平滑", "平直", "显示三角线", "显示碰撞"])
            || layout["scene-preview-property"]["center"]["tab"] != "Scene"
            || layout["scene-variable-property"]["title"] != "场景变量"
            || layout["scene-trigger-property"]["title"] != "场景触发"
            || layout["scene-variable-property"]["scene"] != "场景 · M1-SAMPLE"
            || layout["scene-trigger-property"]["scene"] != "场景 · M1-SAMPLE"
            || layout["scene-alternate-property"]["scene"] != "场景 · M1-EMPTY"
            || layout["scene-property-on-document"]["center"] != context["center"]
        {
            return Err("场景树配置、属性正文或中央文档保留证据不完整".into());
        }
    }
    let resize = match phase {
        "empty" => Some(("variable-column-resize", 0, "删除变量")),
        "edit" => Some(("rule-column-resize", 1, "删除规则")),
        _ => None,
    };
    if let Some((sample, separator, operation)) = resize {
        let evidence = &window["layout"][sample];
        if evidence["separator"] != separator
            || evidence["geometry"]["headings"][3]["text"] != "操作列"
            || evidence["geometry"]["footer"]["count"].as_array().map(Vec::len) != Some(4)
            || evidence["delta"].as_f64().is_none_or(|delta| delta.abs() <= 2.)
            || evidence["operations_width"].as_f64().is_none_or(|width| width <= 0.)
        {
            return Err(format!("缺少拖动列宽后{operation}的实窗表格证据"));
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
    if phase == "paging" {
        for id in ["variable-pager", "rule-pager"] {
            let pager = &window["layout"][id];
            if pager["initial"]["pages"] != json!([1, 2, 3, 4])
                || pager["initial"]["previous_disabled"] != true
                || pager["next"]["current"] != 2
                || pager["previous"]["current"] != 1
                || pager["direct"]["current"] != 4
                || pager["last"]["pages"] != json!([2, 3, 4, 5])
                || pager["last"]["next_disabled"] != true
                || pager["filtered"]["pager"] != false
                || pager["filtered"]["total"] != 2
                || pager["reset"]["current"] != 1
                || pager["reset"]["total"] != 35
            {
                return Err(format!("{id} 缺少分页与筛选证据"));
            }
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
    let hook = |value: u32, tag: &str| json!({
        "id":"scene_hook","tag":tag,"when":{"event":"quest.completed","target":"harbor_entry"},
        "actions":[{"set":"a","value":value}]
    });
    let first_doc = json!({
        "version":1,
        "variables":{"gate_open":{"default":true},"a":{"default":3,"tag":"World"}},
        "rules":[hook(3, "World")],
        "locations":{"east_gate":{"scene":"river","min":[0,0,0],"max":[4,3,6]}},
        "scenes":{"M1-SAMPLE":{
            "variables":{"a":{"default":7,"tag":"Harbor"},"dock_flag":{"default":false,"tag":"Harbor"}},
            "rules":[hook(7, "Harbor")]
        }}
    });
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
    let pagination_work = work.join("pagination");
    fs::create_dir(&pagination_work).map_err(|e| e.to_string())?;
    let mut pagination = fixture(target, &pagination_work, true)?;
    let paginated = pagination_fixture();
    let bytes = serde_json::to_vec_pretty(&paginated).map_err(|e| e.to_string())?;
    let source = pagination_work.join("project/assets/project_rules.json");
    write_new(&source, &bytes)?;
    verify_window(&mut pagination, &pagination_work, "paging", proof, &paginated)?;
    if fs::read(source).map_err(|e| e.to_string())? != bytes {
        return Err("分页或筛选不应重写工程规则文件".into());
    }
    proof["disk_reread_verified"] = json!(true);
    Ok(())
}

fn pagination_fixture() -> Value {
    let mut variables = serde_json::Map::new();
    let mut rules = Vec::new();
    // 35 条产生五页和三条末页；标签横跨首尾，切换时必须重新计算当前页和总数。
    for number in 1..=35 {
        let id = format!("p{number:02}");
        let mut variable = json!({"default":number});
        let mut rule = json!({
            "id":format!("r{number:02}"),
            "when":{"event":"quest.completed","target":"paging"},
            "actions":[{"set":id,"value":0}]
        });
        if matches!(number, 1 | 35) {
            variable["tag"] = json!("Harbor");
            rule["tag"] = json!("Harbor");
        }
        variables.insert(id, variable);
        rules.push(rule);
    }
    json!({"version":1,"variables":variables,"rules":rules})
}
