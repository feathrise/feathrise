//! 工程数值拖动速度的有界格式与原子读写，复用作者偏好单飞IO槽和关闭回执。

use super::{publish_checked, read_optional, EditorSettingsData};
use crate::project::{Project, ProjectContext};
use feathrise_editor_core::SceneEditorScrubSensitivitySettings;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

const FILE_NAME: &str = "numeric-input.json";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    format: u32,
    project_id: String,
    #[serde(default)]
    scrub: SceneEditorScrubSensitivitySettings,
    #[serde(default)]
    fields: BTreeMap<String, f32>,
}

pub(super) struct ProjectStore {
    pub(super) personal: EditorSettingsData,
    context: ProjectContext,
    file: PathBuf,
    original: Option<Vec<u8>>,
    scrub: SceneEditorScrubSensitivitySettings,
    fields: BTreeMap<String, f32>,
    load_failed: bool,
}

impl ProjectStore {
    pub(super) fn load(project: &Project, personal: EditorSettingsData) -> (Self, Option<String>) {
        let mut store = Self {
            file: project.context.editor_data.join(FILE_NAME),
            context: project.context.clone(),
            original: None,
            scrub: personal.scrub.clone(),
            fields: personal.numeric_scrub_speeds.clone(),
            personal,
            load_failed: false,
        };
        let loaded = (|| {
            let file = store.context.checked_data_directory()?.join(FILE_NAME);
            let original = read_optional(&file)?;
            // 兼容旧工程的个人速度。原文身份始终记录新位置，首次保存也不会覆盖外部新文件。
            let inherited = if original.is_none()
                && let Some(directory) = store.context.checked_legacy_data_directory()?
            {
                read_optional(&directory.join(FILE_NAME))?
            } else {
                None
            };
            if let Some(bytes) = original.as_ref().or(inherited.as_ref()) {
                let data: Stored = serde_json::from_slice(bytes)
                    .map_err(|error| format!("工程数值输入设置损坏：{error}"))?;
                if data.format != 1 || data.project_id != store.context.id {
                    return Err("工程数值输入设置的版本或工程身份不匹配".into());
                }
                validate_fields(&data.fields)?;
                store.scrub = data.scrub;
                store.fields = data.fields;
            }
            store.file = file;
            store.original = original;
            Ok(())
        })();
        let error = loaded.err();
        store.load_failed = error.is_some();
        // 已有工程里的兼容槽也按公共设置规则规范化，但保留原文字节，读取本身不写盘。
        let mut normalized = store.personal.clone();
        store.apply(&mut normalized);
        let normalized = EditorSettingsData::from(&normalized.into_runtime());
        store.scrub = normalized.scrub;
        store.fields = normalized.numeric_scrub_speeds;
        (store, error)
    }

    pub(super) fn apply(&self, settings: &mut EditorSettingsData) {
        settings.scrub = self.scrub.clone();
        settings.numeric_scrub_speeds = self.fields.clone();
    }

    pub(super) fn save(&mut self, settings: &EditorSettingsData) -> Result<(), String> {
        if settings.scrub == self.scrub && settings.numeric_scrub_speeds == self.fields {
            return Ok(()); // 打开、左键改值或调整其他偏好均不重写工程速度文件。
        }
        if self.load_failed {
            return Err("工程数值输入设置读取失败，修复原文件并重开工程后可保存速度".into());
        }
        validate_fields(&settings.numeric_scrub_speeds)?;
        if self.context.ensure_data_directory()?.join(FILE_NAME) != self.file {
            return Err("工程数值输入设置路径已变化".into());
        }
        let mut bytes = serde_json::to_vec_pretty(&Stored {
            format: 1,
            project_id: self.context.id.clone(),
            scrub: settings.scrub.clone(),
            fields: settings.numeric_scrub_speeds.clone(),
        }).map_err(|error| format!("无法编码工程数值输入设置：{error}"))?;
        bytes.push(b'\n');
        publish_checked(&self.file, &self.original, &bytes)?;
        self.original = Some(bytes);
        self.scrub = settings.scrub.clone();
        self.fields = settings.numeric_scrub_speeds.clone();
        Ok(())
    }
}

fn validate_fields(fields: &BTreeMap<String, f32>) -> Result<(), String> {
    if fields.len() > 512 || fields.iter().any(|(key, value)|
        key.len() > 512 || !value.is_finite() || !(0.01..=100.0).contains(value))
    {
        return Err("工程数值输入速度超出容量或有效范围".into());
    }
    Ok(())
}
