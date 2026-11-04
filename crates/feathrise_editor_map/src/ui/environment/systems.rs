//! 按真实作用域与资源身份增量回显；不写草稿、运行时天气、相机或保存状态。

use bevy::prelude::*;
use feathrise_atmosphere::{
    AuthoredSkyboxPhase, AuthoredSkyboxStatus, WorldClock, WorldEnvironmentModifiers,
};
use feathrise_map_content::{MapEnvironmentDescriptor, MapSkyMode};

use super::components::{EnvironmentMapControls, EnvironmentSidebarText};
use super::helpers::current_ibl_values;
use super::types::{CurrentIblValues, EnvironmentObservation, EnvironmentTextField};
use crate::authoring::{
    EditorAuthoringSession, GlobalEnvironmentAuthoringState, GlobalEnvironmentSaveTask,
};
use crate::components::{MapEditorCamera, MapEditorEnvironmentText};
use crate::resources::MapEditorState;
use crate::ui::sidebars::{EditorRightSidebarTab, MapEditorSidebarState};
use crate::ui::theme as colors;

fn same_ibl(before: Option<&AuthoredSkyboxStatus>, current: Option<&AuthoredSkyboxStatus>) -> bool {
    match (before, current) {
        (Some(before), Some(current)) => {
            before.phase == current.phase
                && before.asset_path == current.asset_path
                && before.revision == current.revision
                && before.detail == current.detail
                && before.visible_skybox_active == current.visible_skybox_active
        }
        (None, None) => true,
        _ => false,
    }
}

fn same_source(
    before: Option<&MapEnvironmentDescriptor>,
    current: Option<&MapEnvironmentDescriptor>,
) -> bool {
    match (before, current) {
        (Some(before), Some(current)) => {
            before.sky_mode == current.sky_mode
                && before.skybox_path == current.skybox_path
                && before.weather == current.weather
        }
        (None, None) => true,
        _ => false,
    }
} // 数值及天体覆盖由原字段持有；拖动它们不复制静态来源路径或重做概览。

/// 缓存用户实际看到的整数计算值，时钟亚秒变化不反复重排文本或复制静态来源。
fn displayed_values(values: CurrentIblValues) -> CurrentIblValues {
    CurrentIblValues {
        day_night_factor: (values.day_night_factor * 100.0).round() / 100.0,
        diffuse: values.diffuse.round(),
        specular: values.specular.round(),
        skybox: values.skybox.round(),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn sync_editor_environment_text(
    editor: Res<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    global: Res<GlobalEnvironmentAuthoringState>,
    global_save: Res<GlobalEnvironmentSaveTask>,
    sidebar: Res<MapEditorSidebarState>,
    clock: Res<WorldClock>,
    modifiers: Res<WorldEnvironmentModifiers>,
    ibl: Query<Option<&AuthoredSkyboxStatus>, With<MapEditorCamera>>,
    added: Query<(), Added<MapEditorEnvironmentText>>,
    mut previous: Local<Option<EnvironmentObservation>>,
    mut texts: Query<
        (
            &EnvironmentSidebarText,
            &mut Text,
            &mut Node,
            &mut TextColor,
        ),
        Without<EnvironmentMapControls>,
    >,
    mut map_controls: Query<
        &mut Node,
        (
            With<EnvironmentMapControls>,
            Without<EnvironmentSidebarText>,
        ),
    >,
) {
    if !editor.open || sidebar.right != EditorRightSidebarTab::Environment {
        return;
    }
    let draft = session.draft.as_ref().filter(|draft| {
        editor.selected_map_id.as_ref() == Some(&draft.map_id) && !editor.map_transition_in_progress
    }); // 同名地图不构成身份；切图空窗不能呈现旧草稿参数。
    let dynamic = draft.filter(|draft| draft.manifest.is_data_driven());
    let environment = dynamic.map(|draft| &draft.manifest.environment);
    let current = environment
        .map(|environment| displayed_values(current_ibl_values(environment, &clock, &modifiers)));
    let draft_map_id = session.draft.as_ref().map(|draft| &draft.map_id);
    let map_name = draft.map(|draft| &draft.manifest.display_name);
    let runtime_scene = draft.map(|draft| &draft.manifest.runtime_scene);
    let ibl_status = ibl.single().ok().flatten();
    let dirty = dynamic.is_some_and(|draft| draft.environment_dirty);
    let global_saving = global_save.task.is_some() || (session.saving && global.dirty);
    let first = previous.is_none() || !added.is_empty();
    let old = previous.get_or_insert_with(EnvironmentObservation::default);
    let map_changed = first
        || old.map_id.as_ref() != editor.selected_map_id.as_ref()
        || old.draft_map_id.as_ref() != draft_map_id
        || old.map_name.as_ref() != map_name
        || old.runtime_scene.as_ref() != runtime_scene
        || !same_source(old.environment.as_ref(), environment)
        || old.loading != editor.map_transition_in_progress;
    let values_changed = map_changed || old.current_values != current;
    let feedback_changed = map_changed
        || old.environment_revision != session.environment_revision
        || old.environment_dirty != dirty
        || old.saving != session.saving
        || !same_ibl(old.ibl.as_ref(), ibl_status);
    let global_changed = first
        || old.global_initialized != global.initialized
        || old.global_dirty != global.dirty
        || old.global_saving != global_saving
        || old.global_error != global.load_error;
    if !map_changed && !values_changed && !feedback_changed && !global_changed {
        return;
    }

    for mut node in &mut map_controls {
        let display = if dynamic.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }

    let map_text = map_changed.then(|| {
        if editor.map_transition_in_progress {
            return (
                "地图加载中".into(),
                String::new(),
                "等待当前地图与作者草稿就绪，不显示旧地图参数。".into(),
            );
        }
        let Some(draft) = draft else {
            return if editor.selected_map_id.is_some() {
                (
                    "等待地图草稿".into(),
                    String::new(),
                    "当前地图草稿尚未就绪；世界调试与全局默认仍可使用。".into(),
                )
            } else {
                (
                    "未进入地图".into(),
                    String::new(),
                    "进入地图后编辑持久参数；空世界仍可调整世界调试与全局默认。".into(),
                )
            };
        };
        let name = draft.manifest.display_name.clone();
        let Some(environment) = environment else {
            return (
                name,
                format!("固定场景  {}", draft.manifest.runtime_scene),
                "固定场景沿用原环境系统，不提供动态地图参数或天体覆盖。".into(),
            );
        };
        let visible = match environment.sky_mode {
            MapSkyMode::Dynamic => "动态大气与云；天空盒亮度仅为切换保留",
            MapSkyMode::Skybox => "主Cube背景；关闭动态大气与云",
        };
        (
            name,
            format!(
                "天空模式  {}\n可见来源  {visible}\n天空资源  {}\n地图气候（只读）  {} · {}",
                environment.sky_mode.display_name(),
                environment.skybox_path.as_deref().unwrap_or("未设置"),
                environment.weather.region.display_name(),
                environment.weather.climate.display_name()
            ),
            "此区写入当前地图；太阳/月光未启用覆盖的属性跟随全局默认。".into(),
        )
    });
    let values_text = values_changed.then(|| current.map_or_else(String::new, |values| {
        let skybox = if environment.is_some_and(|environment| environment.sky_mode == MapSkyMode::Skybox) {
            format!("\n天空盒 {:.0} cd/m²", values.skybox)
        } else {
            String::new()
        };
        format!("昼夜与天气计算值\n补光 {:.0} · 反射 {:.0} cd/m²{skybox}\n环境昼夜系数 {:.0}%（不含天气）",
            values.diffuse, values.specular, values.day_night_factor * 100.0)
    })); // 日间基础值保留在原四个字段，避免在摘要重复；计算值不声明资源已采用。

    let current_status = environment.and_then(|environment| {
        let path = environment
            .skybox_path
            .as_deref()
            .map(str::trim)
            .filter(|path| !path.is_empty());
        ibl_status.filter(|status| {
            status.revision == session.environment_revision && status.asset_path.as_deref() == path
        })
    }); // 路径与作者修订同时匹配，旧Ready/Failed不能冒充当前请求结果。
    let map_feedback = feedback_changed.then(|| {
        let Some(environment) = environment else {
            return String::new();
        };
        let save = if session.saving {
            "正在保存地图参数"
        } else if dirty {
            "地图环境未保存 · 使用顶部保存"
        } else {
            "地图环境无未保存修改"
        };
        let phase = if let Some(status) = current_status {
            format!(
                "环境IBL  {}\n{}",
                status.phase.display_name(),
                status.detail
            )
        } else if environment
            .skybox_path
            .as_deref()
            .is_none_or(|path| path.trim().is_empty())
        {
            "环境IBL  未设置天空资源".into()
        } else {
            "环境IBL  等待当前请求状态".into()
        };
        format!("{save}\n{phase}")
    });
    let global_feedback = global_changed.then(|| {
        let save = if global_saving {
            "正在保存全局默认"
        } else if global.dirty {
            "全局默认未保存 · 使用顶部保存"
        } else {
            "全局默认无未保存修改"
        };
        let source = global
            .load_error
            .as_deref()
            .unwrap_or(if global.initialized {
                "全局来源已同步"
            } else {
                "等待全局来源同步，暂用安全默认值"
            });
        format!("来源  assets/config/world_environment.world_environment.json\n{save}\n{source}")
    });

    for (marker, mut text, mut node, mut color) in &mut texts {
        let value = match marker.0 {
            EnvironmentTextField::MapContext => map_text.as_ref().map(|value| &value.0),
            EnvironmentTextField::MapSource => map_text.as_ref().map(|value| &value.1),
            EnvironmentTextField::MapHint => map_text.as_ref().map(|value| &value.2),
            EnvironmentTextField::CurrentValues => values_text.as_ref(),
            EnvironmentTextField::MapFeedback => map_feedback.as_ref(),
            EnvironmentTextField::GlobalFeedback => global_feedback.as_ref(),
        };
        let Some(value) = value else { continue };
        if text.as_str() != value {
            **text = value.clone();
        }
        let display = if value.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
        if node.display != display {
            node.display = display;
        }
        let desired = match marker.0 {
            EnvironmentTextField::MapContext => colors::TEXT_VALUE,
            EnvironmentTextField::MapFeedback
                if current_status
                    .is_some_and(|status| status.phase == AuthoredSkyboxPhase::Failed) =>
            {
                colors::DANGER_TEXT
            }
            EnvironmentTextField::GlobalFeedback if global.load_error.is_some() => {
                colors::DANGER_TEXT
            }
            _ => colors::TEXT_HINT,
        };
        if color.0 != desired {
            color.0 = desired;
        }
    }
    if map_changed {
        old.map_id = editor.selected_map_id.clone();
        old.draft_map_id = draft_map_id.cloned();
        old.map_name = map_name.cloned();
        old.runtime_scene = runtime_scene.cloned();
        old.environment = environment.cloned();
        old.loading = editor.map_transition_in_progress;
    }
    if values_changed {
        old.current_values = current;
    }
    if feedback_changed {
        old.environment_revision = session.environment_revision;
        old.environment_dirty = dirty;
        old.saving = session.saving;
        old.ibl = ibl_status.cloned();
    }
    if global_changed {
        old.global_initialized = global.initialized;
        old.global_dirty = global.dirty;
        old.global_saving = global_saving;
        old.global_error = global.load_error.clone();
    }
}
