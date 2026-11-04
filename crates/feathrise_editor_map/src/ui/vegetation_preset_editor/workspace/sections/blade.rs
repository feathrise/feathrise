//! 数据 Blade 的逐档 LOD、外观、视图与运动分区。

use bevy::prelude::*;
use feathrise_map_content::{
    BladePresetMotion, BladePresetUltraLodBudget, VegetationPresetRuntime,
};

use super::super::super::types::{
    BladeAppearanceGroup, BladeBoundsField, BladeLodField, BladeMotionField, BladeViewGroup,
    PresetDraft, PresetEditorAction, PresetField,
};
use super::super::widgets::{
    spawn_action_row, spawn_field, spawn_group_title, spawn_readonly, spawn_selector_row,
};

/// 当前档完整候选窗口、模型路径、半径和淡变。
pub(super) fn spawn_lod(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    draft: &PresetDraft,
) {
    let VegetationPresetRuntime::Blade { lods, .. } = &draft.descriptor.runtime else {
        spawn_wrong_backend(parent, font, "当前预设没有可编辑 Blade LOD 数据");
        return;
    };
    spawn_group_title(
        parent,
        font,
        "Blade 每层 LOD",
        "可增删复制档位；逐档编辑模型、候选网格、半径与淡变",
    );
    spawn_selector_row(
        parent,
        font,
        lods.iter().enumerate().map(|(index, lod)| {
            (
                format!("LOD{} · {}", lod.level, lod.name),
                PresetEditorAction::SelectLod(index),
                index == draft.selected_lod,
            )
        }),
    );
    spawn_action_row(
        parent,
        font,
        &[
            ("新增层", PresetEditorAction::AppendBladeLod, 96.0),
            ("复制当前层", PresetEditorAction::DuplicateBladeLod, 112.0),
            ("删除当前层", PresetEditorAction::RemoveBladeLod, 112.0),
        ],
    ); // 结构动作紧跟选择区，仍由原事务校验1..=8层，不增设UI档数上限。
    let index = draft.selected_lod.min(lods.len().saturating_sub(1));
    let Some(lod) = lods.get(index) else {
        spawn_wrong_backend(parent, font, "Blade LOD 数组为空");
        return;
    };
    let field = |field| PresetField::BladeLod { lod: index, field };
    spawn_field(
        parent,
        font,
        &draft.descriptor,
        "档位名称",
        field(BladeLodField::Name),
    );
    spawn_field(
        parent,
        font,
        &draft.descriptor,
        "Mesh 子资产路径",
        field(BladeLodField::MeshPath),
    );
    spawn_field(
        parent,
        font,
        &draft.descriptor,
        "候选网格单边数",
        field(BladeLodField::GridSide),
    );
    spawn_field(
        parent,
        font,
        &draft.descriptor,
        "世界格距/m",
        field(BladeLodField::Spacing),
    );
    spawn_field(
        parent,
        font,
        &draft.descriptor,
        "内半径/m",
        field(BladeLodField::InnerRadius),
    );
    spawn_field(
        parent,
        font,
        &draft.descriptor,
        "外半径/m",
        field(BladeLodField::OuterRadius),
    );
    spawn_field(
        parent,
        font,
        &draft.descriptor,
        "内淡变宽度/m",
        field(BladeLodField::InnerFade),
    );
    spawn_field(
        parent,
        font,
        &draft.descriptor,
        "外淡变宽度/m",
        field(BladeLodField::OuterFade),
    );
    spawn_field(
        parent,
        font,
        &draft.descriptor,
        "档位种子",
        field(BladeLodField::Seed),
    );
    spawn_field(
        parent,
        font,
        &draft.descriptor,
        "吸附格数",
        field(BladeLodField::SnapCells),
    );
    spawn_readonly(
        parent,
        font,
        "候选窗口边长/m",
        &lod.patch_extent().to_string(),
    );
    // 网格路径行现在自带“定位 / 使用选中”；多网格GLB在同一内嵌面板显式选择，不再走隐式第一项入口。
}

/// 综合色、株形、包围、纹理和固定超远预算。
pub(super) fn spawn_appearance(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    draft: &PresetDraft,
) {
    let VegetationPresetRuntime::Blade {
        ultra_lod_budget, ..
    } = &draft.descriptor.runtime
    else {
        spawn_wrong_backend(parent, font, "当前预设没有可编辑 Blade 外观数据");
        return;
    };
    spawn_group_title(
        parent,
        font,
        "Blade 外观与包围",
        "纹理、综合色、株形、动态边界与超远固定预算",
    );
    spawn_field(
        parent,
        font,
        &draft.descriptor,
        "综合色纹理(留空白图)",
        PresetField::BladeBaseColorPath,
    );
    spawn_field(
        parent,
        font,
        &draft.descriptor,
        "密度图概率增益",
        PresetField::BladeDensityGain,
    );
    spawn_field(
        parent,
        font,
        &draft.descriptor,
        "超远额外水平覆盖/m",
        PresetField::BladeUltraSweep,
    );
    spawn_readonly(
        parent,
        font,
        "超远固定预算",
        ultra_budget_name(*ultra_lod_budget),
    );
    spawn_action_row(
        parent,
        font,
        &[("循环超远预算", PresetEditorAction::CycleUltraBudget, 130.0)],
    );
    for (label, field) in [
        ("最高叶尖/m", BladeBoundsField::MaxHeight),
        ("最大水平扫掠/m", BladeBoundsField::HorizontalSweep),
        ("屏幕参考株高/m", BladeBoundsField::ReferenceHeight),
        ("根部下扎/m", BladeBoundsField::RootPenetration),
    ] {
        spawn_field(
            parent,
            font,
            &draft.descriptor,
            label,
            PresetField::BladeBounds(field),
        );
    }
    for (group, label) in [
        (BladeAppearanceGroup::Geometry, "几何(xy株高/z根深/w倾斜)"),
        (
            BladeAppearanceGroup::WidthFlexibility,
            "宽度柔韧(xy宽/zw柔韧)",
        ),
        (BladeAppearanceGroup::Bending, "弯曲(xy上限/zw响应)"),
        (BladeAppearanceGroup::AtlasUv, "图集 UV(xy缩放/zw偏移)"),
        (BladeAppearanceGroup::RootColor, "根部综合色 RGBA"),
        (BladeAppearanceGroup::TipColor, "尖端综合色 RGBA"),
        (BladeAppearanceGroup::CoolTint, "冷色扰动 RGBA"),
        (BladeAppearanceGroup::WarmTint, "暖色扰动 RGBA"),
    ] {
        spawn_group_title(parent, font, label, "");
        for component in 0..4 {
            spawn_field(
                parent,
                font,
                &draft.descriptor,
                &format!("分量 {}", ["x", "y", "z", "w"][component]),
                PresetField::BladeAppearance { group, component },
            );
        }
    }
}

/// 按实际档数构建屏幕空间阈值和常驻范围，单档不展示不存在的交接。
pub(super) fn spawn_view(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    draft: &PresetDraft,
) {
    let VegetationPresetRuntime::Blade { view_lod, .. } = &draft.descriptor.runtime else {
        spawn_wrong_backend(parent, font, "当前预设没有可编辑 Blade 视图 LOD 数据");
        return;
    };
    spawn_group_title(
        parent,
        font,
        "Blade 视图 LOD",
        "投影像素阈值、镜头避让、常驻半径与交接宽度",
    );
    for index in 0..view_lod.pixel_thresholds.len() {
        spawn_view_pair(
            parent,
            font,
            draft,
            &format!("LOD{}→LOD{index} 像素", index + 1),
            BladeViewGroup::PixelThresholds(index),
            ["开始", "结束"],
        );
    }
    spawn_view_pair(
        parent,
        font,
        draft,
        "镜头避让/m",
        BladeViewGroup::CameraFadeDistances,
        ["隐藏", "恢复"],
    );
    spawn_view_scalar(
        parent,
        font,
        draft,
        "参考草高/m",
        BladeViewGroup::ReferenceGrassHeight,
    );
    spawn_view_scalar(
        parent,
        font,
        draft,
        "最细世界格距/m",
        BladeViewGroup::FinestGridSpacing,
    );
    for component in 0..view_lod.residency_radii.len() {
        spawn_field(
            parent,
            font,
            &draft.descriptor,
            &format!("常驻半径 LOD{component}/m"),
            PresetField::BladeView {
                group: BladeViewGroup::ResidencyRadii,
                component,
            },
        );
    }
    spawn_view_scalar(
        parent,
        font,
        draft,
        "常驻安全边距/m",
        BladeViewGroup::ResidencyMargin,
    );
    for component in 0..view_lod.residency_transition_widths.len() {
        spawn_field(
            parent,
            font,
            &draft.descriptor,
            &format!("祖先交接宽度 {component}/m"),
            PresetField::BladeView {
                group: BladeViewGroup::ResidencyTransitionWidths,
                component,
            },
        );
    }
}

/// 风/水流驱动和水流标量。
pub(super) fn spawn_motion(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    draft: &PresetDraft,
) {
    let VegetationPresetRuntime::Blade { motion, .. } = &draft.descriptor.runtime else {
        spawn_wrong_backend(parent, font, "当前预设没有可编辑 Blade 运动数据");
        return;
    };
    spawn_group_title(
        parent,
        font,
        "Blade 运动",
        "普通草响应世界风；水草可使用独立水流波形",
    );
    spawn_readonly(parent, font, "当前驱动", motion_name(motion));
    spawn_action_row(
        parent,
        font,
        &[("切换风 / 水流", PresetEditorAction::ToggleMotion, 150.0)],
    );
    if matches!(motion, BladePresetMotion::WaterFlow { .. }) {
        for (label, field) in [
            ("水流方向 X", BladeMotionField::DirectionX),
            ("水流方向 Z", BladeMotionField::DirectionZ),
            ("动画速度倍率", BladeMotionField::Speed),
            ("草尖位移/m", BladeMotionField::TipStrength),
            ("空间频率倍率", BladeMotionField::SpatialScale),
        ] {
            spawn_field(
                parent,
                font,
                &draft.descriptor,
                label,
                PresetField::BladeMotion(field),
            );
        }
    }
}

fn spawn_view_pair(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    draft: &PresetDraft,
    label: &str,
    group: BladeViewGroup,
    suffixes: [&str; 2],
) {
    for (component, suffix) in suffixes.into_iter().enumerate() {
        spawn_field(
            parent,
            font,
            &draft.descriptor,
            &format!("{label} {suffix}"),
            PresetField::BladeView { group, component },
        );
    }
}

fn spawn_view_scalar(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    draft: &PresetDraft,
    label: &str,
    group: BladeViewGroup,
) {
    spawn_field(
        parent,
        font,
        &draft.descriptor,
        label,
        PresetField::BladeView {
            group,
            component: 0,
        },
    );
}

fn spawn_wrong_backend(parent: &mut ChildSpawnerCommands, font: &Handle<Font>, message: &str) {
    spawn_readonly(parent, font, "不可编辑", message);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::vegetation_preset_editor::{
        fields::field_value,
        types::{PresetDraftMode, PresetEditorFieldButton},
    };
    use bevy::ecs::world::CommandQueue;
    use feathrise_map_content::VegetationPresetCatalogAsset;

    #[test]
    fn blade_view_controls_cover_exactly_the_existing_levels() {
        let catalog: VegetationPresetCatalogAsset = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/vegetation_presets.vegetation_presets.json"
        )))
        .unwrap();
        let template = catalog
            .presets
            .into_iter()
            .find(|preset| preset.id == "hn01_ordinary_grass")
            .unwrap();
        for count in 1..=8 {
            let mut descriptor = template.clone();
            let VegetationPresetRuntime::Blade { view_lod, .. } = &mut descriptor.runtime else {
                unreachable!()
            };
            view_lod.pixel_thresholds = vec![[1.0, 2.0]; count - 1];
            view_lod.residency_radii = vec![40.0; count];
            view_lod.residency_transition_widths = vec![4.0; count - 1];
            let draft = PresetDraft::new(PresetDraftMode::Create, descriptor);
            let mut world = World::new();
            let mut queue = CommandQueue::default();
            Commands::new(&mut queue, &world)
                .spawn_empty()
                .with_children(|parent| {
                    spawn_view(parent, &Handle::default(), &draft);
                });
            queue.apply(&mut world);
            let fields = world
                .query::<&PresetEditorFieldButton>()
                .iter(&world)
                .map(|button| button.0)
                .collect::<Vec<_>>();
            assert_eq!(fields.len(), 4 * count + 2); // 2(N-1)阈值、N半径、N-1宽度及五个共有标量。
            for field in &fields {
                field_value(&draft.descriptor, *field).expect("已显示控件必须指向存在的参数");
            }
            for index in 0..count - 1 {
                for component in 0..2 {
                    assert!(fields.contains(&PresetField::BladeView {
                        group: BladeViewGroup::PixelThresholds(index),
                        component
                    }));
                }
                assert!(fields.contains(&PresetField::BladeView {
                    group: BladeViewGroup::ResidencyTransitionWidths,
                    component: index
                }));
            }
            for component in 0..count {
                assert!(fields.contains(&PresetField::BladeView {
                    group: BladeViewGroup::ResidencyRadii,
                    component
                }));
            }
        }
    }
}

fn ultra_budget_name(value: BladePresetUltraLodBudget) -> &'static str {
    match value {
        BladePresetUltraLodBudget::High => "High · 2×2 父格",
        BladePresetUltraLodBudget::Dense => "Dense · 3×3 父格",
        BladePresetUltraLodBudget::Medium => "Medium · 4×4 父格",
        BladePresetUltraLodBudget::Low => "Low · 8×8 父格",
    }
}

fn motion_name(value: &BladePresetMotion) -> &'static str {
    match value {
        BladePresetMotion::Wind => "世界统一风 + 近场交互",
        BladePresetMotion::WaterFlow { .. } => "独立水平水流 + 近场交互",
    }
}
