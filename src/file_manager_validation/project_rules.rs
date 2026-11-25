//! 两次独立作者进程复用桌面夹具隔离，退出后独立解析主资产根的正式文档。
use super::*;
pub(super) const ENV: &str = "FEATHRISE_PROJECT_RULES_VALIDATION";

pub(super) fn exercise(target: &Path, work: &Path, proof: &mut Value) -> Result<(), String> {
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
    let sample = include_str!(
        "../../crates/feathrise_editor_documents_model/src/project_rules/example.json"
    );
    let mut expected: Value = serde_json::from_str(sample).map_err(|e| e.to_string())?;
    expected["rules"][0]["id"] = json!("smith_route_edited");
    for phase in ["edit", "reopen"] {
        command.env(ENV, phase);
        let child = build_process::run(
            &command,
            &BuildControl::default(),
            Some(Duration::from_secs(150)),
            tail,
        )?;
        write_new(
            &work.join(format!("rules-{phase}.log")),
            format!("{}\n{}", child.stdout, child.stderr).as_bytes(),
        )?;
        let window = read_json(&work.join(format!("rules-{phase}.json")))?;
        proof[phase] = window.clone();
        if child.exit_code != 0 || window["passed"] != true {
            return Err(format!("规则窗口 {phase} 未通过：{}", window["error"]));
        }
        if read_json(&path)? != expected {
            return Err(format!("{phase} 退出后，独立回读的规则内容与编辑结果不符"));
        }
        let png = fs::read(work.join(format!("rules-{phase}.png"))).map_err(|e| e.to_string())?;
        if !png.starts_with(b"\x89PNG\r\n\x1a\n") || window["screenshot"]["sha256"] != sha256(&png)
        {
            return Err("规则窗口截图校验不符".into());
        }
    }
    proof["disk_reread_verified"] = json!(true);
    Ok(())
}
