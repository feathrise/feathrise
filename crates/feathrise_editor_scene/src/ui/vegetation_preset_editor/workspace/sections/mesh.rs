//! 数据 Mesh 的任意距离档、部件、材质、风和近场碰撞分区。

use bevy::prelude::*;
use feathrise_scene_content::{
    MeshPresetAlphaMode, MeshPresetMeshDerivation, MeshPresetWindPart, VegetationPresetBillboard,
    VegetationPresetCollision, VegetationPresetRuntime,
};

use super::super::super::types::{
    CollisionField, MeshLodField, MeshPartField, PresetDraft, PresetEditorAction, PresetField,
};
use super::super::widgets::{
    spawn_action_row, spawn_field, spawn_group_title, spawn_readonly, spawn_selector_row,
};

/// 任意数量距离档的完整范围与结构动作。
pub(super) fn spawn_lod(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    draft: &PresetDraft,
) {
    let VegetationPresetRuntime::Mesh { lods, .. } = &draft.descriptor.runtime else {
        spawn_wrong_backend(parent, font, "当前预设没有可编辑 Mesh LOD 数据");
        return;
    };
    spawn_group_title(
        parent,
        font,
        "Mesh 每层 LOD",
        "逐档编辑距离；整层替换会覆盖本档全部部件，单部件请在模型部件页设置引用",
    );
    spawn_selector_row(
        parent,
        font,
        lods.iter().enumerate().map(|(index, lod)| {
            (
                format!("LOD{} · {}部件", lod.level, lod.parts.len()),
                PresetEditorAction::SelectLod(index),
                index == draft.selected_lod,
            )
        }),
    );
    spawn_action_row(
        parent,
        font,
        &[
            (
                "选中GLB替换当前档全部部件",
                PresetEditorAction::ApplySelectedAsset,
                210.0, // 明确完整替换范围，避免和路径行的单字段赋值混淆。
            ),
            (
                "从选中GLB新增LOD",
                PresetEditorAction::AppendMeshLod,
                180.0,
            ),
            ("复制当前档", PresetEditorAction::DuplicateMeshLod, 112.0),
            ("删除当前档", PresetEditorAction::RemoveMeshLod, 112.0),
        ],
    );
    let index = draft.selected_lod.min(lods.len().saturating_sub(1));
    let Some(lod) = lods.get(index) else {
        spawn_wrong_backend(parent, font, "Mesh LOD 列表为空");
        return;
    };
    let field = |field| PresetField::MeshLod { lod: index, field };
    spawn_field(
        parent,
        font,
        &draft.descriptor,
        "淡入起点/m",
        field(MeshLodField::StartBegin),
    );
    spawn_field(
        parent,
        font,
        &draft.descriptor,
        "完全可见/m",
        field(MeshLodField::StartFull),
    );
    spawn_field(
        parent,
        font,
        &draft.descriptor,
        "淡出起点/m",
        field(MeshLodField::EndBegin),
    );
    spawn_field(
        parent,
        font,
        &draft.descriptor,
        "完全隐藏/m",
        field(MeshLodField::EndHidden),
    );
    spawn_readonly(parent, font, "当前档部件数", &lod.parts.len().to_string());
}

/// 当前档当前部件的模型、材质、变换、派生与风参数。
pub(super) fn spawn_part(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    draft: &PresetDraft,
) {
    let VegetationPresetRuntime::Mesh { lods, .. } = &draft.descriptor.runtime else {
        spawn_wrong_backend(parent, font, "当前预设没有可编辑 Mesh 部件数据");
        return;
    };
    let lod_index = draft.selected_lod.min(lods.len().saturating_sub(1));
    let Some(lod) = lods.get(lod_index) else {
        spawn_wrong_backend(parent, font, "Mesh LOD 列表为空");
        return;
    };
    spawn_group_title(
        parent,
        font,
        "Mesh 模型部件",
        "每档可含任意数量树干、树冠、花叶或岩石部件",
    );
    spawn_selector_row(
        parent,
        font,
        lods.iter().enumerate().map(|(index, lod)| {
            (
                format!("LOD{} · {}部件", lod.level, lod.parts.len()),
                PresetEditorAction::SelectLod(index),
                index == draft.selected_lod,
            )
        }),
    );
    spawn_readonly(
        parent,
        font,
        "当前距离档",
        &format!("LOD{} · {}部件", lod.level, lod.parts.len()),
    );
    spawn_selector_row(
        parent,
        font,
        lod.parts.iter().enumerate().map(|(index, part)| {
            (
                format!("部件 {}", part.part_id),
                PresetEditorAction::SelectPart(index),
                index == draft.selected_part,
            )
        }),
    );
    spawn_action_row(
        parent,
        font,
        &[
            ("复制为新部件", PresetEditorAction::DuplicateMeshPart, 105.0),
            ("删除当前部件", PresetEditorAction::RemoveMeshPart, 105.0),
        ],
    ); // 先选择距离档和部件，再执行结构动作；不把增删藏在风参数中。
    let part_index = draft.selected_part.min(lod.parts.len().saturating_sub(1));
    let Some(part) = lod.parts.get(part_index) else {
        spawn_wrong_backend(parent, font, "当前 LOD 没有部件");
        return;
    };
    let field = |field| PresetField::MeshPart {
        lod: lod_index,
        part: part_index,
        field,
    };
    spawn_field(
        parent,
        font,
        &draft.descriptor,
        "稳定部件 ID",
        field(MeshPartField::PartId),
    );
    spawn_field(
        parent,
        font,
        &draft.descriptor,
        "Mesh 子资产路径",
        field(MeshPartField::MeshPath),
    );
    spawn_field(
        parent,
        font,
        &draft.descriptor,
        "材质子资产路径",
        field(MeshPartField::MaterialPath),
    );
    spawn_group_title(parent, font, "局部变换", "只编辑当前距离档的当前部件");
    for (label, field_name) in [
        ("局部平移 X/m", MeshPartField::TranslationX),
        ("局部平移 Y/m", MeshPartField::TranslationY),
        ("局部平移 Z/m", MeshPartField::TranslationZ),
        ("局部 Y 旋转/°", MeshPartField::YawDegrees),
        ("局部缩放 X", MeshPartField::ScaleX),
        ("局部缩放 Y", MeshPartField::ScaleY),
        ("局部缩放 Z", MeshPartField::ScaleZ),
    ] {
        spawn_field(parent, font, &draft.descriptor, label, field(field_name));
    }
    spawn_group_title(
        parent,
        font,
        "几何与材质",
        "派生、材质覆盖和公告板仍遵循正式预设约束",
    );
    spawn_readonly(
        parent,
        font,
        "网格取得方式",
        derivation_name(&part.mesh_derivation),
    );
    if !matches!(part.mesh_derivation, MeshPresetMeshDerivation::Direct) {
        spawn_field(
            parent,
            font,
            &draft.descriptor,
            "派生几何预算",
            field(MeshPartField::DerivationBudget),
        );
    }
    spawn_readonly(
        parent,
        font,
        "Alpha 覆盖",
        alpha_name(part.material_overrides.alpha_mode),
    );
    if matches!(
        part.material_overrides.alpha_mode,
        Some(MeshPresetAlphaMode::Mask { .. })
    ) {
        spawn_field(
            parent,
            font,
            &draft.descriptor,
            "Alpha Mask 阈值",
            field(MeshPartField::AlphaCutoff),
        );
    }
    spawn_readonly(
        parent,
        font,
        "双面覆盖",
        match part.material_overrides.double_sided {
            None => "继承源材质",
            Some(true) => "强制双面",
            Some(false) => "强制单面",
        },
    );
    spawn_readonly(
        parent,
        font,
        "方向光阴影",
        if part.casts_shadow {
            "开启"
        } else {
            "关闭"
        },
    );
    spawn_readonly(parent, font, "公告板", billboard_name(part.billboard));
    spawn_action_row(
        parent,
        font,
        &[
            (
                "切换网格取得",
                PresetEditorAction::CycleMeshDerivation,
                130.0,
            ),
            ("切换 Alpha", PresetEditorAction::CycleAlphaMode, 112.0),
            ("切换双面", PresetEditorAction::ToggleDoubleSided, 112.0),
            ("切换阴影", PresetEditorAction::ToggleShadow, 112.0),
            ("切换公告板", PresetEditorAction::CycleBillboard, 120.0),
        ],
    );
    spawn_group_title(parent, font, "部件风", "角色及高度权重独立于部件增删");
    spawn_readonly(
        parent,
        font,
        "高度权重风",
        if part.wind.is_some() {
            "开启"
        } else {
            "关闭"
        },
    );
    spawn_action_row(
        parent,
        font,
        &[
            ("开关部件风", PresetEditorAction::ToggleWind, 120.0),
        ],
    );
    if let Some(wind) = part.wind.as_ref() {
        spawn_readonly(parent, font, "风部件角色", wind_part_name(wind.part));
        spawn_action_row(
            parent,
            font,
            &[(
                "切换树干 / 叶簇角色",
                PresetEditorAction::CycleWindPart,
                185.0,
            )],
        );
        for (label, field_name) in [
            ("风根高度/m", MeshPartField::WindRootHeight),
            ("风梢高度/m", MeshPartField::WindTipHeight),
            ("风强度倍率", MeshPartField::WindStrength),
            ("风频率倍率", MeshPartField::WindFrequency),
        ] {
            spawn_field(parent, font, &draft.descriptor, label, field(field_name));
        }
    }
}

/// 地表下扎与圆柱/长方体碰撞完整参数。
pub(super) fn spawn_collision(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    draft: &PresetDraft,
) {
    let VegetationPresetRuntime::Mesh {
        collision,
        ground_embed_depth,
        ..
    } = &draft.descriptor.runtime
    else {
        spawn_wrong_backend(parent, font, "当前预设没有可编辑 Mesh 碰撞数据");
        return;
    };
    spawn_group_title(
        parent,
        font,
        "Mesh 贴地与碰撞",
        "碰撞与渲染 LOD 正交，按近场区域距离驻留",
    );
    spawn_field(
        parent,
        font,
        &draft.descriptor,
        "地表下扎深度/m",
        PresetField::MeshGroundEmbed,
    );
    spawn_readonly(parent, font, "当前碰撞策略", collision_name(collision));
    spawn_readonly(parent, font, "当前下扎值", &ground_embed_depth.to_string());
    spawn_action_row(
        parent,
        font,
        &[
            ("循环碰撞策略", PresetEditorAction::CycleCollision, 140.0),
            (
                "切换随实例缩放",
                PresetEditorAction::ToggleCollisionScale,
                150.0,
            ),
        ],
    );
    match collision {
        VegetationPresetCollision::None => {}
        VegetationPresetCollision::Cylinder {
            scale_with_instance,
            ..
        } => {
            spawn_field(
                parent,
                font,
                &draft.descriptor,
                "圆柱半径/m",
                PresetField::Collision(CollisionField::CylinderRadius),
            );
            spawn_field(
                parent,
                font,
                &draft.descriptor,
                "圆柱完整高度/m",
                PresetField::Collision(CollisionField::CylinderHeight),
            );
            spawn_field(
                parent,
                font,
                &draft.descriptor,
                "圆柱中心 Y/m",
                PresetField::Collision(CollisionField::CylinderCenterY),
            );
            spawn_collision_common(parent, font, draft, *scale_with_instance);
        }
        VegetationPresetCollision::Cuboid {
            scale_with_instance,
            ..
        } => {
            for (label, field) in [
                ("长方体尺寸 X/m", CollisionField::CuboidSizeX),
                ("长方体尺寸 Y/m", CollisionField::CuboidSizeY),
                ("长方体尺寸 Z/m", CollisionField::CuboidSizeZ),
                ("中心偏移 X/m", CollisionField::CuboidCenterX),
                ("中心偏移 Y/m", CollisionField::CuboidCenterY),
                ("中心偏移 Z/m", CollisionField::CuboidCenterZ),
            ] {
                spawn_field(
                    parent,
                    font,
                    &draft.descriptor,
                    label,
                    PresetField::Collision(field),
                );
            }
            spawn_collision_common(parent, font, draft, *scale_with_instance);
        }
    }
}

fn spawn_collision_common(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    draft: &PresetDraft,
    scale_with_instance: bool,
) {
    spawn_readonly(
        parent,
        font,
        "尺寸随实例缩放",
        if scale_with_instance { "是" } else { "否" },
    );
    spawn_field(
        parent,
        font,
        &draft.descriptor,
        "进入创建距离/m",
        PresetField::Collision(CollisionField::EnterDistance),
    );
    spawn_field(
        parent,
        font,
        &draft.descriptor,
        "离开回收距离/m",
        PresetField::Collision(CollisionField::ExitDistance),
    );
}

fn spawn_wrong_backend(parent: &mut ChildSpawnerCommands, font: &Handle<Font>, message: &str) {
    spawn_readonly(parent, font, "不可编辑", message);
}

fn derivation_name(value: &MeshPresetMeshDerivation) -> &'static str {
    match value {
        MeshPresetMeshDerivation::Direct => "直接使用源网格",
        MeshPresetMeshDerivation::DispersedTriangles { .. } => "分散真实三角形 · 薄片植物超远档",
        MeshPresetMeshDerivation::ConvexHull { .. } => "封闭凸包 · 岩石超远档",
    }
}

fn alpha_name(value: Option<MeshPresetAlphaMode>) -> &'static str {
    match value {
        None => "继承源材质",
        Some(MeshPresetAlphaMode::Opaque) => "强制不透明",
        Some(MeshPresetAlphaMode::Mask { .. }) => "Alpha Mask",
        Some(MeshPresetAlphaMode::Blend) => "透明混合",
    }
}

fn billboard_name(value: VegetationPresetBillboard) -> &'static str {
    match value {
        VegetationPresetBillboard::Fixed => "固定作者姿态",
        VegetationPresetBillboard::Cylindrical => "绕世界 Y 轴朝向视图",
        VegetationPresetBillboard::Spherical => "三维朝向视图",
    }
}

fn wind_part_name(value: MeshPresetWindPart) -> &'static str {
    match value {
        MeshPresetWindPart::Stem => "树干 / 刚性茎干",
        MeshPresetWindPart::Foliage => "树冠 / 灌木 / 花叶",
    }
}

fn collision_name(value: &VegetationPresetCollision) -> &'static str {
    match value {
        VegetationPresetCollision::None => "无碰撞",
        VegetationPresetCollision::Cylinder { .. } => "圆柱近场碰撞",
        VegetationPresetCollision::Cuboid { .. } => "长方体近场碰撞",
    }
}
