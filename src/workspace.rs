//! 同一主窗口的工程生命周期；原作者App按工程隔离，作为主窗口内的完整工作台呈现。
//!
//! 工程命令与状态使用有界低频通道；草稿、历史、视口和F12仍由原作者实例本地处理。
mod author;
mod host;
mod process;
mod resources;
mod types;
#[cfg(all(feature = "validation", windows))]
mod validation;

pub(crate) use author::install as install_author;
pub(crate) use host::run;

use crate::{user_preferences::Preferences, workspace_ui::*};

pub(crate) fn preferences() -> Result<(Preferences, Option<String>), String> {
    // 验收配置显式隔离，不触碰作者自己的最近工程和默认目录。
    let override_file = std::env::var_os("FEATHRISE_PREFERENCES_FILE");
    let home = crate::user_preferences::user_home()?;
    let loaded = match &override_file {
        Some(file) => Preferences::load_at(file, &home),
        None => Preferences::load(),
    };
    match loaded {
        Ok(value) => Ok((value, None)),
        Err(error) => {
            // 坏配置只显示诊断并保留原文件；隔离验收的兜底位置也必须保持隔离。
            let fallback = match override_file {
                Some(file) => Preferences::fallback_at(file, home)?,
                None => Preferences::defaults()?,
            };
            Ok((fallback, Some(error)))
        }
    }
}

fn model(preferences: &Preferences, project: Option<&crate::project::Project>) -> WorkspaceModel {
    WorkspaceModel {
        project: project.map(|project| WorkspaceProject {
            name: project.name.clone(),
            root: project.root.clone(),
        }),
        default_directory: preferences
            .default_project_directory
            .to_string_lossy()
            .into_owned(),
        recent: preferences
            .recent_projects
            .iter()
            .map(|entry| RecentEntry {
                name: entry.name.clone(),
                path: entry.path.clone(),
                last_opened: recent_time(entry.last_opened_unix_seconds),
            })
            .collect(),
        busy: false,
        cancellable: false,
        status: if project.is_some() {
            "工程已打开"
        } else {
            "未打开工程"
        }
        .into(),
        revision: 1,
    }
}

fn recent_time(timestamp: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(timestamp);
    match now.saturating_sub(timestamp) {
        0..60 => "刚刚".into(),
        seconds @ 60..3600 => format!("{} 分钟前", seconds / 60),
        seconds @ 3600..86400 => format!("{} 小时前", seconds / 3600),
        seconds => format!("{} 天前", seconds / 86400),
    }
}
