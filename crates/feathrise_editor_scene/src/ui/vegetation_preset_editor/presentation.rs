//! 预设管理内嵌面板的增量文字、显隐、焦点和按钮反馈。

use feathrise_editor_ui::UiInteraction;
use bevy::prelude::*; // Query、Text与UI颜色组件。
use feathrise_editor_ui::localization::EditorLocale;
use feathrise_scene_content::{
    BladePresetMotion, VegetationPresetCollision, VegetationPresetRegistry, VegetationPresetRuntime,
}; // 摘要直接读取正式运行时枚举。
use crate::ui::theme as colors; // 聚焦与交互沿用F12统一颜色。

use super::fields::field_value; // 未聚焦详细字段直接读取活动强类型草稿。
use super::components::{PresetButtonKind, PresetDocumentBadge, PresetPathValue};
use super::types::{
    PresetDraft, PresetDraftMode, PresetEditorAction, PresetEditorButton, PresetEditorFieldButton,
    PresetEditorFieldText, PresetEditorStatusText, PresetEditorSummaryText, PresetField,
    PresetWorkspaceTab, VegetationPresetEditorState,
}; // 固定静态壳的marker与会话值。
use crate::resources::SceneEditorState; // 当前目录选择和右侧Asset摘要。

/// 把最终状态同步到固定数量UI实体，不重建列表或面板。
#[allow(clippy::too_many_arguments)] // 每个Query只负责一种固定marker，保持BSN壳无字符串实体查找。
pub(super) fn sync_panel(
    state: Res<VegetationPresetEditorState>,
    editor: Res<SceneEditorState>,
    registry: Option<Res<VegetationPresetRegistry>>,
    locale: Option<Res<EditorLocale>>,
    mut summaries: Query<&mut Text, With<PresetEditorSummaryText>>,
    mut statuses: Query<
        &mut Text,
        (
            With<PresetEditorStatusText>,
            Without<PresetEditorSummaryText>,
        ),
    >,
    mut field_texts: Query<
        (&PresetEditorFieldText, &mut Text, &mut TextColor), // 备注全选沿用强调色回显，不改变其字符内容。
        (
            Without<PresetEditorStatusText>,
            Without<PresetEditorSummaryText>,
            Without<bevy::text::EditableText>, // 官方字段自行渲染，禁止覆盖Text。
        ),
    >, // 字段文字同时排除状态和摘要marker，向Bevy证明三个可变Text查询两两互斥。
    mut field_borders: Query<(&PresetEditorFieldButton, &mut BorderColor)>,
    mut path_texts: Query<
        (&PresetPathValue, &mut Text),
        (
            Without<PresetEditorFieldText>,
            Without<PresetEditorStatusText>,
            Without<PresetEditorSummaryText>,
        ),
    >,
) {
    let locale = locale.as_deref().copied().unwrap_or_default();
    let summary = panel_summary(&state, &editor, registry.as_deref(), locale);
    for mut text in &mut summaries {
        if **text != summary {
            **text = summary.clone();
        }
    }
    let status = locale.editor_notice(&state.status);
    for mut text in &mut statuses {
        if text.as_str() != status.as_ref() {
            **text = status.to_string();
        }
    }
    if state.is_changed() {
        // 完整路径回显只在输入/草稿变化时生成；不能反写官方输入实体或触发整页重建。
        for (marker, mut text) in &mut path_texts {
            let value = if state.active_field == Some(marker.0) {
                state.input_buffer.clone()
            } else {
                state
                    .active_draft()
                    .and_then(|draft| field_value(&draft.descriptor, marker.0).ok())
                    .unwrap_or_default()
            };
            let value = if value.is_empty() { "未指定".to_owned() } else { value };
            if **text != value {
                **text = value;
            }
        }
        // 稳定帧不复制任意长度备注，也不反复生成多行排版输入。
        for (marker, mut text, mut color) in &mut field_texts {
            // 只同步活动表单的有限字段。
            let value = field_display(&state, marker.0); // 唯一读映射继续保留作者空备注。
            if **text != value {
                **text = value;
            } // 无实际文本变化不触发重新排版。
            let next_color = colors::TEXT_VALUE; // 选区高亮由官方绘制。
            if color.0 != next_color {
                color.0 = next_color;
            } // 普通字段颜色保持原样。
        }
    }
    for (marker, mut border) in &mut field_borders {
        let color = if state.active_field == Some(marker.0) {
            colors::INPUT_ACTIVE_BORDER
        } else {
            colors::COPPER_BORDER_WEAK
        };
        if border.top != color {
            *border = BorderColor::all(color);
        }
    }
}

/// 返回当前草稿或目录选择的紧凑身份摘要。
fn panel_summary(
    state: &VegetationPresetEditorState,
    editor: &SceneEditorState,
    registry: Option<&VegetationPresetRegistry>,
    locale: EditorLocale,
) -> String {
    let selected_asset = editor
        .selected_asset
        .as_deref()
        .map(|path| shorten(path, 34))
        .unwrap_or_else(|| locale.text("vegetation.selection.no_asset").to_owned());
    if let Some(draft) = state.active_draft() {
        let mode = match &draft.mode {
            PresetDraftMode::Create => "vegetation.selection.new",
            PresetDraftMode::Edit { .. } => "label.edit",
        };
        return locale.format("vegetation.selection.draft", &[
            ("mode", locale.text(mode).to_owned()),
            ("name", draft.descriptor.label.clone()),
            ("dirty", if draft.dirty() { locale.text("vegetation.selection.unsaved").to_owned() } else { String::new() }),
            ("type", runtime_summary(&draft.descriptor.runtime, locale)),
            ("asset", selected_asset),
        ]);
    }
    let Some(id) = editor.vegetation_selected_preset_id.as_deref() else {
        return locale.format("vegetation.selection.empty", &[("asset", selected_asset)]);
    };
    match registry.and_then(|registry| registry.resolve(id)) {
        Some(preset) => locale.format("vegetation.selection.selected", &[
            ("name", preset.label.clone()),
            ("type", runtime_summary(&preset.runtime, locale)),
            ("asset", selected_asset),
        ]),
        None => locale.format("vegetation.selection.unavailable", &[("id", id.to_owned())]),
    }
}

/// 返回Blade运动或Mesh碰撞的后端摘要。
fn runtime_summary(runtime: &VegetationPresetRuntime, locale: EditorLocale) -> String {
    match runtime {
        VegetationPresetRuntime::Blade { motion, .. } => {
            locale.format("vegetation.type.blade", &[("motion", locale.editor_label(motion_name(motion)).to_owned())])
        }
        VegetationPresetRuntime::Mesh {
            lods, collision, ..
        } => locale.format("vegetation.type.mesh", &[
            ("count", lods.len().to_string()),
            ("collision", locale.editor_label(collision_name(collision)).to_owned()),
        ]),
    }
}

/// 返回运动来源短标签。
fn motion_name(motion: &BladePresetMotion) -> &'static str {
    match motion {
        BladePresetMotion::Wind => "随风摆动", // 说明可观察的运动效果。
        BladePresetMotion::WaterFlow { .. } => "水流",
    }
}

/// 返回碰撞来源短标签。
fn collision_name(collision: &VegetationPresetCollision) -> &'static str {
    match collision {
        VegetationPresetCollision::None => "无碰撞",
        VegetationPresetCollision::Cylinder { .. } => "圆柱碰撞",
        VegetationPresetCollision::Cuboid { .. } => "长方体碰撞",
    }
}

/// 返回表单字段值，并只在当前焦点后追加IME候选与光标。
fn field_display(state: &VegetationPresetEditorState, field: PresetField) -> String {
    let Some(draft) = state.active_draft() else {
        return "—".to_owned();
    };
    if state.active_field == Some(field) {
        format!("{}|", state.input_buffer) // 本查询只包含旧数值字段；中文显示完全属于EditableText。
    } else {
        field_value(&draft.descriptor, field).unwrap_or_else(|_| "—".to_owned())
    }
}

/// 按Unicode字符安全裁剪过长Asset路径，保留末尾文件身份。
fn shorten(value: &str, max_chars: usize) -> String {
    let count = value.chars().count();
    if count <= max_chars {
        return value.to_owned();
    }
    let tail = value
        .chars()
        .skip(count - max_chars + 1)
        .collect::<String>();
    format!("…{tail}")
}

#[derive(Default)]
pub(super) struct PresetStyleCache {
    initialized: bool,
    revision: u64,
    catalog_revision: u64,
    selected_id: Option<String>,
}

type PresetButtonStyle<'a> = (
    &'a UiInteraction,
    &'a PresetEditorButton,
    Option<&'a PresetButtonKind>,
    &'a mut BackgroundColor,
    &'a mut BorderColor,
);

/// 修订/目录选择变化才完整回显；稳定帧只处理Changed交互，不反复解析全部LOD/部件选择按钮。
pub(super) fn sync_button_styles(
    state: Res<VegetationPresetEditorState>,
    editor: Res<SceneEditorState>,
    registry: Option<Res<VegetationPresetRegistry>>,
    mut cache: Local<PresetStyleCache>,
    mut buttons: ParamSet<(
        Query<PresetButtonStyle<'_>, (Changed<UiInteraction>, Without<PresetEditorFieldButton>, Without<feathrise_editor_ui::workspace_tabs::WorkspaceTabButton>)>,
        Query<PresetButtonStyle<'_>, (Without<PresetEditorFieldButton>, Without<feathrise_editor_ui::workspace_tabs::WorkspaceTabButton>)>,
    )>,
) {
    let catalog_revision = registry.as_deref().map_or(0, |registry| registry.revision);
    let full_sync = !cache.initialized
        || cache.revision != state.ui_revision
        || cache.catalog_revision != catalog_revision
        || cache.selected_id.as_deref() != editor.vegetation_selected_preset_id.as_deref();
    if !full_sync && buttons.p0().is_empty() {
        return;
    }
    let active_draft = state.active_draft();
    let close_confirmation = state
        .tabs
        .iter()
        .find(|tab| tab.close_confirmation)
        .map(|tab| tab.id);
    if full_sync {
        cache.initialized = true;
        cache.revision = state.ui_revision;
        cache.catalog_revision = catalog_revision;
        cache.selected_id = editor.vegetation_selected_preset_id.clone();
        for (interaction, marker, kind, mut background, mut border) in &mut buttons.p1() {
            let active = action_active(
                marker.0,
                state.active_tab,
                active_draft,
                close_confirmation,
                &editor,
                registry.as_deref(),
            );
            apply_button_style(
                interaction,
                kind.copied().unwrap_or_default(),
                active,
                &mut background,
                &mut border,
            );
        }
    } else {
        for (interaction, marker, kind, mut background, mut border) in &mut buttons.p0() {
            let active = action_active(
                marker.0,
                state.active_tab,
                active_draft,
                close_confirmation,
                &editor,
                registry.as_deref(),
            );
            apply_button_style(
                interaction,
                kind.copied().unwrap_or_default(),
                active,
                &mut background,
                &mut border,
            );
        }
    }
}

fn apply_button_style(
    interaction: &UiInteraction,
    kind: PresetButtonKind,
    active: bool,
    background: &mut BackgroundColor,
    border: &mut BorderColor,
) {
    let color = match (kind, interaction) {
        (PresetButtonKind::Primary, UiInteraction::None) => colors::SELECT_BG,
        (PresetButtonKind::Primary, _) => colors::SELECT_BG_HOVER,
        (PresetButtonKind::Navigation, UiInteraction::None) if !active => Color::NONE,
        (PresetButtonKind::Navigation, _) if active => colors::SELECT_BG,
        (_, UiInteraction::Pressed) => colors::BUTTON_DEV_SMALL_PRESSED,
        (_, UiInteraction::Hovered) => colors::BUTTON_DEV_SMALL_HOVERED,
        (_, UiInteraction::None) if active => colors::BUTTON_DEV_SMALL_PRESSED,
        (_, UiInteraction::None) => colors::BUTTON_DEV_SMALL_NORMAL,
    };
    if background.0 != color {
        background.0 = color;
    }
    let border_color = if active || kind == PresetButtonKind::Primary {
        colors::COPPER_BORDER
    } else if kind == PresetButtonKind::Navigation {
        Color::NONE
    } else {
        colors::COPPER_BORDER_WEAK
    };
    if border.top != border_color {
        *border = BorderColor::all(border_color);
    }
}

/// 正文换代、输入拥有者和忙碌阶段变化时才更新状态；稳定帧不复制预设或路径。
pub(super) fn sync_document_badge(
    state: Res<VegetationPresetEditorState>,
    mut seen: Local<Option<(Entity, u64, Option<PresetField>, u8)>>,
    mut badges: Query<(Entity, &mut Text, &mut TextColor), With<PresetDocumentBadge>>,
) {
    let Ok((entity, mut text, mut color)) = badges.single_mut() else {
        return;
    };
    let phase = u8::from(state.write_task.is_some())
        | (u8::from(state.adoption.is_some()) << 1)
        | (u8::from(state.asset_reference.is_some()) << 2)
        | (u8::from(state.pending_inspection.is_some() || state.inspection_work.is_some()) << 3);
    let signature = (entity, state.ui_revision, state.active_field, phase);
    if *seen == Some(signature) {
        return;
    }
    let Some(draft) = state.active_draft() else {
        return;
    };
    *seen = Some(signature);
    let label = super::workspace::draft_badge(draft, &state);
    if text.as_str() != label {
        **text = label.to_owned();
    }
    color.0 = if draft.dirty()
        || state.active_field.is_some()
        || matches!(draft.mode, PresetDraftMode::Create)
    {
        colors::TEXT_ACCENT
    } else {
        colors::TEXT_MUTED
    };
}

/// 判断一个选择型动作是否代表当前导航状态；普通命令按钮永不保持按下色。
fn action_active(
    action: PresetEditorAction,
    active_tab: PresetWorkspaceTab,
    active_draft: Option<&PresetDraft>,
    close_confirmation: Option<u64>,
    editor: &SceneEditorState,
    registry: Option<&VegetationPresetRegistry>,
) -> bool {
    match action {
        PresetEditorAction::SelectCatalogPreset(index) => registry
            .and_then(|registry| registry.presets().get(index))
            .is_some_and(|preset| {
                editor.vegetation_selected_preset_id.as_deref() == Some(preset.id.as_str())
            }), // 目录重排后按规范ID重新求选中态，绝不持久化易漂移的数组索引。
        PresetEditorAction::ActivateSceneTab => active_tab == PresetWorkspaceTab::Scene,
        PresetEditorAction::ActivatePresetTab(tab_id) => {
            active_tab == PresetWorkspaceTab::Vegetation(tab_id)
        }
        PresetEditorAction::ClosePresetTab(tab_id) => close_confirmation == Some(tab_id),
        PresetEditorAction::SelectSection(section) => {
            active_draft.is_some_and(|draft| draft.section == section)
        }
        PresetEditorAction::SelectLod(index) => {
            active_draft.is_some_and(|draft| draft.selected_lod == index)
        }
        PresetEditorAction::SelectPart(index) => {
            active_draft.is_some_and(|draft| draft.selected_part == index)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::{IntoSystem, System}; // 初始化正式系统参数才能覆盖Bevy运行时B0001检查。
    use bevy::prelude::World; // 空测试World只用于初始化生产系统的访问元数据。

    use super::{shorten, sync_panel}; // 同时验证纯路径裁剪和正式面板同步系统。

    #[test] // cargo check不会初始化QueryState，因此必须保留这个真实系统回归门。
    fn sync_panel_system_initializes_without_text_query_conflicts() {
        let mut world = World::new(); // 空World足以登记组件访问并执行查询冲突验证。
        let mut system = IntoSystem::into_system(sync_panel); // 使用生产函数签名，不复制一份弱化测试参数。
        system.initialize(&mut world); // 若任意两个可变Text查询可能重叠，本行会以B0001失败。
    }

    #[test]
    fn asset_summary_truncates_on_unicode_boundaries() {
        let shortened = shorten("gltf/blender/植被/白桦树/白桦树高模.glb", 12);
        assert!(shortened.starts_with('…'));
        assert!(shortened.ends_with("白桦树高模.glb"));
    }
}
