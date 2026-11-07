//! 本轮代码迭代的真实窗口验收；仅显式validation和带独占归属标记的中性工程允许自动操作。

use bevy::prelude::*;
use serde_json::{Value, json};
use std::{io::Read, path::{Path, PathBuf}};

mod driver;
mod evidence;
mod fixture;
mod resources;
mod close_cancel;
mod function_hotpatch;
pub(crate) use fixture::run;
use resources::Plan;

const OWNER_FILE: &str = ".code-iteration-validation-owner.json";
const PURPOSE: &str = "code-iteration-first-stage";

/// 已核验自有专项只延迟最后退出，不写后台完成或恢复状态；普通编辑器没有此资源。
pub(super) fn close_drain_held(world: &World) -> bool {
    close_cancel::held(world) || crate::locale_validation::close_drain_held(world)
}

pub(super) fn install(app: &mut App, project: &crate::project::Project, target: &Path) {
    let Some(report) = std::env::var_os("FEATHRISE_CODE_ITERATION_REPORT").map(PathBuf::from) else { return; };
    let owner = (|| -> Result<(), String> {
        if !report.is_absolute() || report.exists() { return Err("代码迭代验收要求本轮未存在的绝对报告路径".into()); }
        if crate::embedded_viewport::validation::code_iteration_owner(&project.root)? != *project {
            return Err("代码迭代验收中性工程来源不一致".into());
        }
        let runtime = std::env::var_os("FEATHRISE_CODE_ITERATION_RUNTIME_DIR").map(PathBuf::from);
        if runtime.as_deref() != report.parent().map(|parent| parent.join("runtime")).as_deref() {
            return Err("实际运行取证目录必须属于本轮独占报告目录".into());
        }
        let marker = project.root.join(OWNER_FILE);
        let mut bytes = Vec::new();
        std::fs::File::open(&marker).map_err(|error| format!("代码迭代验收拒绝无归属工程：{error}"))?
            .take(16 * 1024 + 1).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
        if bytes.len() > 16 * 1024 { return Err("代码迭代验收owner超过固定预算".into()); }
        let owner: Value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        if owner["schema"] != 1 || owner["purpose"] != PURPOSE
            || owner["project_id"] != project.id || owner["project_root"] != json!(project.root)
            || owner["report"] != json!(report)
        { return Err("代码迭代验收owner与当前工程或报告不一致，未安装自动操作".into()); }
        Ok(())
    })();
    if let Err(error) = owner {
        eprintln!("FEATHRISE_CODE_ITERATION_VALIDATION_REFUSED {error}");
        app.add_systems(Startup, |mut exit: MessageWriter<AppExit>| { exit.write(AppExit::error()); });
        return;
    }
    feathrise_editor_map::standalone::embedded_validation::install(app);
    if std::env::var_os("FEATHRISE_CODE_ITERATION_CLOSE_CANCEL")
        .is_some_and(|value| value == "1")
    {
        app.init_resource::<close_cancel::CloseCancelCheck>();
    }
    println!("FEATHRISE_CODE_ITERATION_VALIDATION_START {}", json!({"project_id":project.id,
        "project_root":project.root,"report":report,"target":target}));
    app.insert_resource(Plan::new(project.clone(), report, target.to_path_buf()))
        .add_systems(Last, driver::drive);
}
