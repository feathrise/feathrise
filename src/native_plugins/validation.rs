//! 通用的外部插件验收接入点；工程自己的场景由 validation 构建显式提供。

use bevy::prelude::App;
use std::path::PathBuf;

// 默认公开构建没有外部源码依赖；仅显式 validation 和适配器配置共同启用时编译该模块。
#[cfg(feathrise_external_validation)]
include!(concat!(env!("OUT_DIR"), "/plugin_validation_extension.rs"));

pub(crate) fn run(target: PathBuf, report: PathBuf, package: PathBuf) -> Result<(), String> {
    #[cfg(feathrise_external_validation)]
    { extension::run(target, report, package) }
    #[cfg(not(feathrise_external_validation))]
    {
        let _ = (target, report, package);
        Err("插件场景验收需要在 validation 构建时显式配置 FEATHRISE_VALIDATION_EXTENSION；通用回归见 docs/开发与验收.md".into())
    }
}

pub(super) fn install(app: &mut App, project: &crate::project::Project) {
    #[cfg(feathrise_external_validation)]
    extension::install(app, project);
    #[cfg(not(feathrise_external_validation))]
    let _ = (app, project);
}
