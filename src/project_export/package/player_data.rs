//! 识别已运行的旧 RPG 包。清单外仅允许旧运行端确实写入的路径，发布前保全原字节。

use super::{
    files::{self, Tree},
    paths,
    transaction::Stage,
};
use crate::{build_artifacts, build_process::BuildControl};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

pub(super) const DIRECTORY_PREFIX: &str = ".feathrise-export-data-";
pub(super) const EXTRA_CONTENT: &str = "导出目录含清单之外的文件或目录，拒绝覆盖并保留原内容";

#[derive(Debug, PartialEq, Eq)]
pub(super) struct LegacyFile {
    source: PathBuf,
    destination: PathBuf,
    digest: (String, u64),
}

pub(super) fn legacy_files(
    root: &Path,
    descriptor_name: &str,
    actual: &Tree,
    expected_files: &BTreeSet<PathBuf>,
    expected_directories: &BTreeSet<PathBuf>,
    control: &BuildControl,
) -> Result<Vec<LegacyFile>, String> {
    let descriptor = files::read_bounded(&root.join(descriptor_name), 64 * 1024, control)?;
    let descriptor: serde_json::Value =
        crate::project::parse_descriptor(descriptor_name, &descriptor)?;
    if descriptor["rpg_content"] != "sword-and-wanderer" {
        return Err(EXTRA_CONTENT.into());
    }

    // 用旧包清单解释玩家曾经写到哪里，即使新工程已经更改作者数据目录也能识别。
    let author_data = descriptor["editor_data"].as_str().unwrap_or(".feathrise");
    let save_directory = if author_data != ".feathrise" {
        Path::new(author_data).join("game-saves")
    } else if actual
        .directories
        .contains(Path::new("saves/feathrise-play"))
    {
        "saves/feathrise-play".into()
    } else {
        ".feathrise/game-saves".into()
    };
    paths::relative(&save_directory)?;
    let save_directory = if actual.directories.contains(&save_directory) {
        save_directory
    } else {
        "bevy_learn5_3d/saves".into() // 旧独立入口在缺少用户数据目录时的回退路径。
    };
    let mut known_directories = BTreeSet::new();
    let mut legacy = Vec::new();
    for relative in actual.files.difference(expected_files) {
        control.check()?;
        let source = root.join(relative);
        let name = relative
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(EXTRA_CONTENT)?;
        let (destination, digest) = if relative == Path::new("settings.json") {
            let bytes = files::read_bounded(&source, 4 * 1024 * 1024, control)?;
            let valid = serde_json::from_slice::<serde_json::Value>(&bytes).is_ok_and(|settings| {
                settings["graphics"].is_object()
                    && settings["control"].is_object()
                    && settings["general_keybindings"].is_object()
                    && settings["skill_keybindings"].is_array()
            });
            if !valid {
                return Err(EXTRA_CONTENT.into());
            }
            let digest = (
                feathrise_scene_content::authoring_io::bytes_hash(&bytes),
                bytes.len() as u64,
            );
            (relative.clone(), digest)
        } else if relative.parent() == Some(save_directory.as_path())
            && matches!(
                name,
                "index.json"
                    | "save_1.json"
                    | "save_2.json"
                    | "save_3.json"
                    | "save_4.json"
                    | "save_5.json"
            )
        {
            for parent in save_directory
                .ancestors()
                .take_while(|parent| !parent.as_os_str().is_empty())
            {
                if !expected_directories.contains(parent) {
                    known_directories.insert(parent.to_owned());
                }
            }
            let digest = build_artifacts::hash_file(&paths::checked(&source, false)?, control)?;
            (Path::new("saves").join(name), digest)
        } else {
            return Err(EXTRA_CONTENT.into());
        };
        legacy.push(LegacyFile {
            source: relative.clone(),
            destination,
            digest,
        });
    }
    if !actual
        .directories
        .difference(expected_directories)
        .eq(known_directories.iter())
    {
        return Err(EXTRA_CONTENT.into());
    }
    Ok(legacy)
}

pub(super) fn preserve(
    backup: &Path,
    project_id: &str,
    legacy: &[LegacyFile],
    control: &BuildControl,
) -> Result<(), String> {
    if legacy.is_empty() {
        return Ok(());
    }
    let key = feathrise_scene_content::authoring_io::bytes_hash(project_id.as_bytes());
    let directory = backup.with_file_name(format!("{DIRECTORY_PREFIX}{key}"));
    match fs::symlink_metadata(&directory) {
        Ok(_) => {
            // 上次复制后若发布失败，可用完整且字节相同的副本重试。冲突时保留两份。
            paths::checked(&directory, true)?;
            for file in legacy {
                let target = directory.join(&file.destination);
                if !target.try_exists().map_err(|error| error.to_string())?
                    || !matches_digest(&target, file, control)?
                {
                    return Err(format!(
                        "旧玩家数据无法与现有目录自动合并，旧包和现有数据均保留：{}",
                        target.display()
                    ));
                }
            }
            return Ok(());
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(error) => return Err(format!("无法检查玩家数据目录：{error}")),
    }
    let stage = Stage::create(&directory)?;
    if legacy
        .iter()
        .any(|file| file.destination.starts_with("saves"))
    {
        fs::create_dir(stage.path.join("saves")).map_err(|error| error.to_string())?;
    }
    for file in legacy {
        let source = paths::checked(&backup.join(&file.source), false)?;
        let target = stage.path.join(&file.destination);
        files::copy_file(&source, &target, file.digest.1, control)?;
        if !matches_digest(&target, file, control)? {
            return Err(format!(
                "旧玩家数据在复制时变化，保留旧包：{}",
                source.display()
            ));
        }
    }
    control.check()?;
    stage.publish_player_data(&directory)
}

fn matches_digest(
    path: &Path,
    legacy: &LegacyFile,
    control: &BuildControl,
) -> Result<bool, String> {
    let path = paths::checked(path, false)?;
    Ok(build_artifacts::hash_file(&path, control)? == legacy.digest)
}
