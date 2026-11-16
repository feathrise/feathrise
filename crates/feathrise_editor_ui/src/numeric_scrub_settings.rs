//! 作者数值框统一恢复工程速度，右键松开后只修改一次设置，宿主沿原后台写槽持久化。

use bevy::prelude::*;
use feathrise_editor_core::EditorSettings;
use feathrise_ui_widgets::{
    ActiveNumericInputScrub, NumericInputScrubConfig, NumericInputScrubSensitivityCommitted,
    NumericInputScrubSettingsKey, UiWidgetsSystemSet,
};

pub(super) fn install(app: &mut App) {
    app.add_systems(Update, (restore_speeds, restore_new_speeds).chain().before(UiWidgetsSystemSet::NumericInput))
        .add_systems(Update, save_speeds.after(UiWidgetsSystemSet::NumericInput));
}

fn restore_new_speeds(
    settings: Option<Res<EditorSettings>>,
    mut fields: Query<(&NumericInputScrubSettingsKey, &mut NumericInputScrubConfig), Added<NumericInputScrubSettingsKey>>,
) {
    let Some(settings) = settings else { return; };
    // 设置未变化也可能重建面板；只处理新登记的字段，不遍历稳定帧的整棵UI。
    for (key, mut scrub) in &mut fields {
        if let Some(speed) = settings.numeric_scrub_speeds.get(&key.0) {
            scrub.sensitivity = *speed;
        }
    }
}

fn restore_speeds(
    settings: Option<Res<EditorSettings>>,
    active: Res<ActiveNumericInputScrub>,
    mut fields: Query<(&NumericInputScrubSettingsKey, &mut NumericInputScrubConfig)>,
) {
    let Some(settings) = settings else { return; };
    if !settings.is_changed() || active.entity.is_some() { return; }
    for (key, mut scrub) in &mut fields {
        // 捕获期间的即时速度归手势拥有，设置同步仅作用于下一笔。
        if let Some(speed) = settings.numeric_scrub_speeds.get(&key.0)
            && scrub.sensitivity != *speed
        {
            scrub.sensitivity = *speed;
        }
    }
}

fn save_speeds(
    mut settings: Option<ResMut<EditorSettings>>,
    mut commits: MessageReader<NumericInputScrubSensitivityCommitted>,
    fields: Query<&NumericInputScrubSettingsKey>,
) {
    for commit in commits.read() {
        let Some(settings) = settings.as_mut() else { continue; };
        let Ok(key) = fields.get(commit.entity) else { continue; };
        if !commit.after.is_finite() || commit.after <= 0.0 || commit.before == commit.after { continue; }
        if key.0.len() > 512 || settings.numeric_scrub_speeds.len() >= 512
            && !settings.numeric_scrub_speeds.contains_key(&key.0)
        {
            warn!("数值框速度设置超过容量，本次速度未保存");
            continue;
        }
        let speed = commit.after.clamp(0.01, 100.0);
        if settings.numeric_scrub_speeds.get(&key.0) != Some(&speed) {
            settings.numeric_scrub_speeds.insert(key.0.clone(), speed);
        }
    }
}
