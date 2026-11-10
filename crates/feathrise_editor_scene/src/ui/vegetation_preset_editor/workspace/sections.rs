//! 活动植被预设标签的分区路由与公共基础字段。

mod blade;
mod mesh;

use super::super::types::{PresetDraft, PresetEditorSection, PresetField};
use super::notes::spawn_notes; // 自由备注使用有限多行视区，不复用单行资产路径控件。
use super::widgets::{spawn_field, spawn_group_title, spawn_readonly};
use bevy::prelude::*;

/// 只构造活动标签当前分区，避免预设、LOD 或部件总量扩大稳定帧 ECS 成本。
pub(super) fn spawn_section_body(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    draft: &PresetDraft,
) {
    match draft.section {
        PresetEditorSection::Basics => spawn_basics(parent, font, draft),
        PresetEditorSection::BladeLod => blade::spawn_lod(parent, font, draft),
        PresetEditorSection::BladeAppearance => blade::spawn_appearance(parent, font, draft),
        PresetEditorSection::BladeView => blade::spawn_view(parent, font, draft),
        PresetEditorSection::BladeMotion => blade::spawn_motion(parent, font, draft),
        PresetEditorSection::MeshLod => mesh::spawn_lod(parent, font, draft),
        PresetEditorSection::MeshPart => mesh::spawn_part(parent, font, draft),
        PresetEditorSection::MeshCollision => mesh::spawn_collision(parent, font, draft),
    }
}

/// 基础字段同时适用于完整数据 Blade 与 Mesh。
fn spawn_basics(parent: &mut ChildSpawnerCommands, font: &Handle<Font>, draft: &PresetDraft) {
    spawn_group_title(
        parent,
        font,
        "散布与画布",
        "稳定 ID 与后端锁定；作者内容参数可编辑",
    );
    let descriptor = &draft.descriptor;
    spawn_field(parent, font, descriptor, "显示名称", PresetField::Label);
    spawn_field(
        parent,
        font,
        descriptor,
        "画布宽度/px",
        PresetField::ResolutionX,
    );
    spawn_field(
        parent,
        font,
        descriptor,
        "画布高度/px",
        PresetField::ResolutionY,
    );
    spawn_field(
        parent,
        font,
        descriptor,
        "满密候选间距/m",
        PresetField::Spacing,
    );
    spawn_field(
        parent,
        font,
        descriptor,
        "随机缩放下限",
        PresetField::ScaleMin,
    );
    spawn_field(
        parent,
        font,
        descriptor,
        "随机缩放上限",
        PresetField::ScaleMax,
    );
    spawn_field(
        parent,
        font,
        descriptor,
        "散布种子(留空自动)",
        PresetField::Seed,
    );
    spawn_group_title(
        parent,
        font,
        "标识与来源",
        "技术身份只读；改名不迁移场景引用或运行时身份",
    );
    spawn_readonly(parent, font, "稳定 ID", &descriptor.id);
    spawn_readonly(parent, font, "正式输出 ID", &descriptor.output_id);
    spawn_readonly(
        parent,
        font,
        "运行时身份",
        &format!(
            "layer={} · prototype={}",
            descriptor
                .runtime_layer_id
                .map(|value| value.to_string())
                .unwrap_or_else(|| "自动派生".to_owned()),
            descriptor
                .prototype_id
                .map(|value| value.to_string())
                .unwrap_or_else(|| "自动派生/无".to_owned()),
        ),
    );
    spawn_readonly(
        parent,
        font,
        "历史别名",
        &if descriptor.aliases.is_empty() {
            "无".to_owned()
        } else {
            descriptor.aliases.join("、")
        },
    );
    spawn_readonly(parent, font, "作者来源（只读）", &descriptor.author_asset_path);
    spawn_notes(parent, font, descriptor); // 保留可选多行输入和六行内部滚动；不参与资源引用。
}
