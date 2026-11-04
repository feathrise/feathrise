//! 旧SDK依赖的只读诊断；只报告已失效的本地旧入口，不替作者改Cargo或控制源码。

use crate::{build_process::BuildControl, project::Project};
use std::{fs::File, io::Read, path::Path};
use toml::Value;

const MANIFEST_LIMIT: usize = 64 * 1024;

pub(super) fn verify(project: &Project, control: &BuildControl) -> Result<(), String> {
    control.check()?;
    let manifest = read_manifest(&project.manifest)?;
    let base = project.manifest.parent().ok_or("工程Cargo清单缺少目录")?;
    validate(&manifest, base, control)
}

fn read_manifest(path: &Path) -> Result<Value, String> {
    let mut text = String::new();
    File::open(path)
        .map_err(|error| format!("工程SDK依赖清单不可读：{}：{error}", path.display()))?
        .take((MANIFEST_LIMIT + 1) as u64)
        .read_to_string(&mut text)
        .map_err(|error| error.to_string())?;
    if text.len() > MANIFEST_LIMIT {
        return Err("工程SDK依赖清单超过64KiB预算".into());
    }
    toml::from_str(&text).map_err(|error| format!("工程SDK依赖清单无效：{error}"))
}

fn validate(manifest: &Value, base: &Path, control: &BuildControl) -> Result<(), String> {
    // 只诊断已知旧模板的直接依赖。自定义目标、registry/Git依赖和workspace解析仍交给Cargo。
    for kind in ["dependencies", "build-dependencies"] {
        let Some(dependencies) = manifest.get(kind).and_then(Value::as_table) else {
            continue;
        };
        for (alias, dependency) in dependencies {
            control.check()?;
            let package = dependency
                .get("package")
                .and_then(Value::as_str)
                .unwrap_or(alias);
            if package != "new_editor_game_runtime" {
                continue;
            }
            let Some(path) = dependency.get("path").and_then(Value::as_str) else {
                continue;
            };
            let path = base.join(path).join("Cargo.toml");
            let renamed = if path.is_file() {
                let dependency = read_manifest(&path)?;
                matches!(
                    dependency
                        .get("package")
                        .and_then(|package| package.get("name"))
                        .and_then(Value::as_str),
                    Some("feathrise_runtime" | "bevy_feathrise" | "feathrise_project_runtime" | "bevy_feathrise_project")
                )
            } else {
                true
            };
            if renamed {
                return Err(format!(
                    "旧SDK依赖{alias}仍指向new_editor_game_runtime，但本地路径已失效或包已更名：{}。请显式把Cargo依赖及源码导入更新为feathrise_runtime，并核对工程清单里的第三方补丁路径；也可使用package字段保留源码别名。编辑器不会覆写工程源码",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(text: &str) -> Value {
        toml::from_str(text).unwrap()
    }

    #[test]
    fn broken_legacy_path_is_reported_without_rewriting_custom_or_aliased_dependencies() {
        let base = std::env::temp_dir().join(format!(
            "feathrise-legacy-sdk-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        assert!(!base.exists());
        let old =
            manifest("[dependencies]\nnew_editor_game_runtime={path='missing-runtime/game'}\n");
        let error = validate(&old, &base, &BuildControl::default()).unwrap_err();
        assert!(error.contains("new_editor_game_runtime"));
        assert!(error.contains("feathrise_runtime"));
        assert!(error.contains("不会覆写工程源码"));
        for allowed in [
            "[dependencies]\nnew_editor_game_runtime={package='feathrise_runtime',path='../feathrise_runtime'}\n",
            "[dependencies]\nnew_editor_game_runtime={package='bevy_feathrise',path='../bevy_feathrise'}\n",
            "[dependencies]\nnew_editor_game_runtime={package='feathrise_project_runtime',path='../bevy_feathrise/game'}\n",
            "[dependencies]\nnew_editor_game_runtime={git='https://example.invalid/legacy-sdk.git',rev='abc123'}\n",
            "[dependencies]\ncustom_engine={path='../my-custom-engine'}\n",
        ] {
            validate(&manifest(allowed), &base, &BuildControl::default()).unwrap();
        }
        assert!(!base.exists()); // 诊断过程不创建、更改或迁移作者目录。
    }
}
