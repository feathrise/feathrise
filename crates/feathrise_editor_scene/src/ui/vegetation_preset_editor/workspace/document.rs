//! 植被文档的页头、状态和顶部导航；共享标签条和预设事务仍由原工作区管理。

use bevy::prelude::*;
use feathrise_scene_content::VegetationPresetRuntime;

use super::super::components::{
    PresetButtonKind, PresetDocumentBadge, PresetDocumentHeader, PresetEditorForm,
};
use super::super::types::{
    PresetDraft, PresetDraftMode, PresetEditorAction, PresetEditorSection, PresetEditorStatusText,
    PresetReferenceOwner, PresetWorkspaceDynamicRoot, VegetationPresetEditorState,
};
use super::sections::spawn_section_body;
use super::widgets::{spawn_button, spawn_row, spawn_section_nav, text_font};
use crate::ui::theme as colors;

/// 状态区分资源操作、写盘和热采用；新建且尚未编辑也不能显示为“已保存”。
pub(in super::super) fn draft_badge(
    draft: &PresetDraft,
    state: &VegetationPresetEditorState,
) -> &'static str {
    if state.write_task.is_some() {
        "保存中…"
    } else if state.adoption.is_some() {
        "等待热采用…"
    } else if state.asset_reference.is_some() {
        "资产引用处理中"
    } else if state.pending_inspection.is_some() || state.inspection_work.is_some() {
        "资产检查中…"
    } else if state.active_field.is_some() {
        "字段输入中"
    } else if matches!(draft.mode, PresetDraftMode::Create) {
        "新建 · 未保存"
    } else if draft.dirty() {
        "未保存修改"
    } else {
        "已保存"
    }
}

pub(super) fn spawn_editor(
    commands: &mut Commands,
    host: Entity,
    font: &Handle<Font>,
    draft: &PresetDraft,
    state: &VegetationPresetEditorState,
    gltfs: &Assets<bevy::gltf::Gltf>,
    gltf_meshes: &Assets<bevy::gltf::GltfMesh>,
) {
    commands.entity(host).with_children(|parent| {
        parent
            .spawn((
                PresetWorkspaceDynamicRoot,
                PresetReferenceOwner(state.active_preset_tab_id().expect("只构造活动植被草稿")),
                Node {
                    width: Val::Percent(100.0),
                    min_width: Val::Px(0.0),
                    max_width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    flex_shrink: 0.0,
                    row_gap: Val::Px(6.0),
                    ..default()
                }, // 只限制文档阅读宽度，不增加相机、场景预览或第二套滚动宿主。
            ))
            .with_children(|root| {
                // 沿用首部非模态选择区与原滚动恢复：矮窗口也能操作冻结目标的候选，右侧资产不被遮挡。
                super::super::asset_reference::spawn_picker(root, font, state, gltfs, gltf_meshes);
                root.spawn((
                    PresetDocumentHeader,
                    Node {
                        width: Val::Percent(100.0),
                        min_width: Val::Px(0.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(6.0),
                        ..default()
                    },
                ))
                .with_children(|header| {
                    spawn_row(header, |row| {
                        row.spawn((
                            Text::new(&draft.descriptor.label),
                            text_font(font, 18.0),
                            TextColor(colors::TEXT_PRIMARY),
                            TextLayout::new(Justify::Left, LineBreak::AnyCharacter),
                            Node {
                                flex_basis: Val::Px(260.0),
                                flex_grow: 1.0,
                                min_width: Val::Px(0.0),
                                max_width: Val::Percent(100.0),
                                ..default()
                            },
                        ));
                        spawn_button(
                            row,
                            font,
                            "保存预设",
                            PresetEditorAction::Save,
                            PresetButtonKind::Primary,
                            false,
                        );
                        spawn_button(
                            row,
                            font,
                            "关闭标签",
                            PresetEditorAction::Cancel,
                            PresetButtonKind::Ordinary,
                            false,
                        );
                    });
                    header.spawn((
                        PresetDocumentBadge,
                        Text::new(draft_badge(draft, state)),
                        text_font(font, 11.0),
                        TextColor(colors::TEXT_MUTED),
                    ));
                    header.spawn((
                        PresetEditorStatusText,
                        crate::ui::status_bar::PanelStatusText("植被预设"),
                        feathrise_editor_ui::feedback::GlobalPanelStatus,
                        Text::new(&state.status),
                        text_font(font, 11.0),
                        TextColor(colors::TEXT_ACCENT),
                        TextLayout::new(Justify::Left, LineBreak::AnyCharacter),
                        Node {
                            width: Val::Percent(100.0),
                            min_width: Val::Px(0.0),
                            ..default()
                        },
                    )); // 无效字段、资源检查和保存失败在当前文档直接可见，仍与侧栏及底栏共享同一结果。
                });
                spawn_navigation(root, font, draft);
                root.spawn((
                    PresetEditorForm,
                    Node {
                        width: Val::Percent(100.0),
                        min_width: Val::Px(0.0),
                        max_width: Val::Percent(100.0),
                        flex_wrap: FlexWrap::Wrap,
                        column_gap: Val::Px(6.0),
                        row_gap: Val::Px(5.0),
                        align_items: AlignItems::Start,
                        flex_shrink: 0.0,
                        ..default()
                    },
                ))
                .with_children(|form| spawn_section_body(form, font, draft));
            });
    });
}

fn spawn_navigation(parent: &mut ChildSpawnerCommands, font: &Handle<Font>, draft: &PresetDraft) {
    let sections: &[(PresetEditorSection, &str)] = match &draft.descriptor.runtime {
        VegetationPresetRuntime::Blade { .. } => &[
            (PresetEditorSection::Basics, "基础与散布"),
            (PresetEditorSection::BladeLod, "每层 LOD"),
            (PresetEditorSection::BladeAppearance, "外观与包围"),
            (PresetEditorSection::BladeView, "视图 LOD"),
            (PresetEditorSection::BladeMotion, "运动"),
        ],
        VegetationPresetRuntime::Mesh { .. } => &[
            (PresetEditorSection::Basics, "基础与散布"),
            (PresetEditorSection::MeshLod, "每层 LOD"),
            (PresetEditorSection::MeshPart, "模型部件"),
            (PresetEditorSection::MeshCollision, "贴地与碰撞"),
        ],
    };
    spawn_section_nav(parent, font, sections, draft.section);
}
