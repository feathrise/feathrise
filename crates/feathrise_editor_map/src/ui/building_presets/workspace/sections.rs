//! 建筑工作台四分区的原字段清单；只构造当前连接点，不展开全部模块参数。

use super::super::{components::BuildingButtonKind, types::*};
use super::widgets::{button, field, fields, group, readonly, row};
use bevy::prelude::*;
use feathrise_map_content::MAX_BUILDING_CONNECTION_POINTS;

pub(super) fn spawn(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    state: &BuildingPresetEditorState,
    draft: &BuildingDraft,
) {
    match draft.section {
        BuildingPresetSection::Basics => basics(parent, font, state, draft),
        BuildingPresetSection::Footprint => footprint(parent, font, state, draft),
        BuildingPresetSection::Entrance => entrance(parent, font, state, draft),
        BuildingPresetSection::Connections => connections(parent, font, state, draft),
    }
}

fn basics(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    state: &BuildingPresetEditorState,
    draft: &BuildingDraft,
) {
    let descriptor = &draft.descriptor;
    group(
        parent,
        font,
        "基础资料",
        "名称用于显示；分类用于浏览，不改变建筑的摆放几何",
    );
    field(
        parent,
        font,
        state,
        descriptor,
        "显示名称",
        BuildingPresetField::Label,
    );
    row(parent, |actions| {
        button(
            actions,
            font,
            &format!("分类：{}（切换）", descriptor.category.label()),
            BuildingPresetAction::CycleCategory,
            BuildingButtonKind::Ordinary,
            false,
        );
        button(
            actions,
            font,
            &format!("室内：{}（切换）", descriptor.interior.label()),
            BuildingPresetAction::CycleInterior,
            BuildingButtonKind::Ordinary,
            false,
        );
    });
    readonly(
        parent,
        font,
        "室内声明",
        "不自动生成门洞、碰撞或传送内容；实际可达能力由模型和地图配置决定",
    );
    group(
        parent,
        font,
        "标识与模型来源",
        "以下是只读来源，改显示名称不会迁移地图引用身份",
    );
    readonly(parent, font, "稳定 ID", &descriptor.id);
    readonly(parent, font, "模型路径", &descriptor.asset_path);
    readonly(
        parent,
        font,
        "模型目标",
        &format!("{:?}", descriptor.asset_target),
    );
}

fn footprint(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    state: &BuildingPresetEditorState,
    draft: &BuildingDraft,
) {
    group(
        parent,
        font,
        "占地矩形",
        "模型局部X/Z，单位米；不包含门前净空，重叠和地图边界检查使用旋转后的真实四边形",
    );
    fields(
        parent,
        font,
        state,
        &draft.descriptor,
        &[
            ("占地中心 X/m", BuildingPresetField::FootprintCenterX),
            ("占地中心 Z/m", BuildingPresetField::FootprintCenterZ),
            ("占地半宽 X/m", BuildingPresetField::FootprintHalfX),
            ("占地半深 Z/m", BuildingPresetField::FootprintHalfZ),
        ],
    );
    group(
        parent,
        font,
        "地基与坡差",
        "落点采用占地中心与四角五点采样；建筑保持竖直，以最高地面和模型基准Y计算根高度",
    );
    fields(
        parent,
        font,
        state,
        &draft.descriptor,
        &[
            ("模型地基基准 Y/m", BuildingPresetField::FoundationBaseY),
            ("最大允许坡差/m", BuildingPresetField::FoundationMaxDelta),
        ],
    );
}

fn entrance(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    state: &BuildingPresetEditorState,
    draft: &BuildingDraft,
) {
    group(
        parent,
        font,
        "正门与门前空间",
        "模型局部坐标；零度朝局部+Z，门前净空从门槛向外延伸，不作为墙体实体碰撞",
    );
    fields(
        parent,
        font,
        state,
        &draft.descriptor,
        &[
            ("门槛位置 X/m", BuildingPresetField::EntranceX),
            ("门槛位置 Y/m", BuildingPresetField::EntranceY),
            ("门槛位置 Z/m", BuildingPresetField::EntranceZ),
            ("正门朝向/度", BuildingPresetField::EntranceYaw),
            ("门前净空半宽/m", BuildingPresetField::EntranceHalfWidth),
            ("门前净空半深/m", BuildingPresetField::EntranceHalfDepth),
        ],
    );
    group(
        parent,
        font,
        "植被净空",
        "模型局部X/Z，单位米；只影响独立派生覆盖，不改写地图原始R8作者图",
    );
    fields(
        parent,
        font,
        state,
        &draft.descriptor,
        &[
            ("净空中心 X/m", BuildingPresetField::VegetationCenterX),
            ("净空中心 Z/m", BuildingPresetField::VegetationCenterZ),
            ("净空半宽 X/m", BuildingPresetField::VegetationHalfX),
            ("净空半深 Z/m", BuildingPresetField::VegetationHalfZ),
        ],
    );
}

fn connections(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    state: &BuildingPresetEditorState,
    draft: &BuildingDraft,
) {
    let entries = &draft.descriptor.connections;
    let selected = draft
        .selected_connection
        .min(entries.len().saturating_sub(1));
    group(
        parent,
        font,
        "模块连接点",
        "只编辑当前选中点；兼容类型须完全相同，吸附时方向相反并自动采用90°旋转",
    );
    row(parent, |selectors| {
        for (index, connection) in entries.iter().enumerate() {
            button(
                selectors,
                font,
                &format!("{} · {}", index + 1, connection.id),
                BuildingPresetAction::SelectConnection(index),
                BuildingButtonKind::Ordinary,
                selected == index,
            );
        }
    }); // 选择器数量沿用正式32点上限，不为每个点创建完整输入组。
    row(parent, |actions| {
        button(
            actions,
            font,
            "新增连接点",
            BuildingPresetAction::AddConnectionPoint,
            BuildingButtonKind::Ordinary,
            false,
        );
        if !entries.is_empty() {
            button(
                actions,
                font,
                "删除当前点",
                BuildingPresetAction::RemoveConnectionPoint(selected),
                BuildingButtonKind::Ordinary,
                false,
            );
        }
    });
    readonly(
        parent,
        font,
        "连接点数量",
        &format!(
            "{} / {MAX_BUILDING_CONNECTION_POINTS} · 旧预设可保持为空",
            entries.len()
        ),
    );
    let Some(connection) = entries.get(selected) else {
        readonly(
            parent,
            font,
            "当前点",
            "尚无连接点；点击新增后设置ID、兼容类型与地基平面位置",
        );
        return;
    };
    group(
        parent,
        font,
        &format!("当前点 {} · {}", selected + 1, connection.id),
        "地基平面局部X/Z；柱顶、梁端和楼层连接不在本协议内",
    );
    fields(
        parent,
        font,
        state,
        &draft.descriptor,
        &[
            ("连接点 ID", BuildingPresetField::ConnectionId(selected)),
            ("兼容类型", BuildingPresetField::ConnectionKind(selected)),
            ("位置 X/m", BuildingPresetField::ConnectionX(selected)),
            ("位置 Z/m", BuildingPresetField::ConnectionZ(selected)),
            ("朝外角度/度", BuildingPresetField::ConnectionYaw(selected)),
        ],
    );
}
