//! 网格创建的轻量规格预检；公共输入下限与后端格式约束分开，按钮和后台读取同一规格。
use feathrise_scene_content::{SceneTerrainKind, SparseHeightSpec};
use super::{components::Field, resources::TerrainObjectsEditorState, types::{CreationSource, CreationPlacementMode}};

pub(super) fn field_visible(field: Field, state: &TerrainObjectsEditorState) -> bool {
    if !state.creation_open { return false; }
    match field {
        Field::CuboidCreation(_) => state.creation_source == CreationSource::Cuboid,
        Field::Creation(index) => match index {
            0..=2 => state.placement_mode == CreationPlacementMode::Coordinates,
            3 | 5 => matches!(state.creation_source, CreationSource::Clipmap | CreationSource::Voxel),
            4 => state.creation_source == CreationSource::Voxel,
            6 => state.creation_source != CreationSource::Cuboid,
            7 => matches!(state.creation_source, CreationSource::Clipmap | CreationSource::Voxel),
            _ => false,
        },
        _ => false,
    }
}

pub(super) fn cuboid_target(state: &TerrainObjectsEditorState) -> Result<feathrise_scene_content::ScenePlacementAssetTarget, String> {
    let values = state.cuboid_creation;
    if values.iter().any(|v| !v.is_finite()) || values[..3].iter().any(|v| !(0.001..=10_000.0).contains(v))
        || values[3..6].iter().any(|v| !(0.0..=255.0).contains(v)) || !(0.0..=1.0).contains(&values[6]) {
        return Err("立方体尺寸、颜色或粗糙度无效".into());
    }
    let target = feathrise_scene_content::ScenePlacementAssetTarget::Cuboid {
        size_mm: [values[0], values[1], values[2]].map(|v| (v * 1000.0).round() as u32),
        color_srgb: [values[3], values[4], values[5]].map(|v| v.round() as u8),
        roughness_milli: (values[6] * 1000.0).round() as u16,
        collision: state.cuboid_collision,
    };
    target.validate()?;
    Ok(target)
}

pub(super) fn validate_selected(state: &TerrainObjectsEditorState) -> Result<(), String> {
    let mut values = state.creation;
    if state.placement_mode == CreationPlacementMode::Coordinates {
        if values[..3].iter().any(|v| !v.is_finite()) { return Err("指定坐标必须有限".into()); }
    } else {
        values[..3].fill(0.0); // 点放的坐标来自之后的真实点击，隐藏的旧坐标记忆不参与预检。
    }
    if state.creation_source == CreationSource::ImportedGlb {
        // 模型保持资产尺寸；只校验实际使用的摆放坐标和朝向。
        return if values[6].is_finite() { Ok(()) } else { Err("模型朝向必须有限".into()) };
    }
    match state.creation_source.kind() {
        Some(kind) => validate_input(kind, &values).map(|_| ()),
        None => cuboid_target(state).map(|_| ()),
    }
}

/// 正文保留一条当前来源的可用性和资产反馈；完整后端说明仍放在问号帮助中。
pub(super) fn sync_feedback(state: bevy::prelude::Res<TerrainObjectsEditorState>,
    editor: bevy::prelude::Res<crate::resources::SceneEditorState>,
    locale: Option<bevy::prelude::Res<feathrise_editor_ui::localization::EditorLocale>>,
    mut texts: bevy::prelude::Query<&mut bevy::prelude::Text, bevy::prelude::With<super::components::CreationFeedback>>) {
    let label = match state.creation_source {
        CreationSource::Cuboid => "立方体", CreationSource::ImportedGlb => "原始 GLB",
        CreationSource::Clipmap => "高度地形", CreationSource::Voxel => "体素地形",
    };
    let result = validate_selected(&state).and_then(|_| {
        if state.creation_source == CreationSource::ImportedGlb
            && !editor.selected_asset.as_ref().is_some_and(|path| path.to_ascii_lowercase().ends_with(".glb")) {
            Err("请先选择GLB资产".into())
        } else { Ok(()) }
    });
    let locale = locale.as_deref().copied().unwrap_or_default();
    let mut value = locale.format("mesh.creation.result", &[
        ("kind", locale.editor_label(label).into()),
        ("result", match result {
            Ok(()) => locale.text("mesh.creation.available").into(),
            Err(error) => locale.editor_notice(&error).into_owned(),
        }),
    ]);
    if state.creation_source == CreationSource::ImportedGlb {
        if let Some(path) = editor.selected_asset.as_ref() {
            value.push_str(&locale.format("mesh.creation.asset", &[("path", path.clone())]));
        }
    }
    for mut text in &mut texts { if text.0 != value { text.0 = value.clone(); } }
}

// 添加网格公共输入从0.125m起；既有体素源文件仍保留自身0.010m格式下限。
pub(super) const MIN_SPACING: f32 = 0.125;
pub(super) const MAX_SPACING: f32 = 1_000_000.0;
// 0.125m是Clipmap合法下界，不能被公共数值框的0.01m量化成0.13m。
pub(super) const SPACING_STEP: f32 = 0.001;

pub(super) struct CreationSpec {
    pub min: [f32; 2],
    pub max: [f32; 2],
    pub resolution: [u32; 2],
}

/// Clipmap按米制保存高度，不再把隐藏的旧高度域记忆当成创建规格或后端限制。
pub(super) fn normalized(kind: SceneTerrainKind, mut values: [f32; 8]) -> [f32; 8] {
    if kind == SceneTerrainKind::SparseClipmap { values[4] = 16.0; }
    values
}

/// 仅计算标量和格式容量，不构建Mesh、作者块或碰撞布局，可供输入帧即时判定。
pub(super) fn validate(kind: SceneTerrainKind, values: &[f32; 8]) -> Result<CreationSpec, String> {
    let normalized = normalized(kind, *values);
    let values = &normalized;
    if values[..7].iter().any(|value| !value.is_finite())
        || values[3..6].iter().any(|value| *value <= 0.0)
    {
        return Err("创建网格的位置和尺寸必须有限，尺寸必须大于零".into());
    }
    let min = [-values[3] * 0.5, -values[5] * 0.5];
    let max = [values[3] * 0.5, values[5] * 0.5];
    // 导入GLB沿用原资产几何；未使用的生成格距不得成为导入门槛。
    if kind == SceneTerrainKind::Gltf {
        return Ok(CreationSpec {
            min,
            max,
            resolution: [0, 0],
        });
    }
    if !values[7].is_finite() || values[7] <= 0.0 {
        return Err("源格距必须有限且大于零".into());
    }
    let cells = [values[3], values[5]].map(|extent| (extent / values[7]).ceil());
    if cells
        .iter()
        .any(|count| !count.is_finite() || *count >= u32::MAX as f32)
    {
        return Err("网格源分辨率超过安全整数范围".into());
    }
    let mut resolution = cells.map(|count| count as u32 + 1);
    match kind {
        SceneTerrainKind::SparseClipmap => {
            let spec = SparseHeightSpec::new(min, max, values[7], -values[4] * 0.5, values[4])?;
            resolution = spec.cells.map(|count| count as u32 + 1);
            // 清单按两轴格点数恢复同一拓扑；不整除边长的实际格距可略低于作者请求下限。
            let restored = SparseHeightSpec::from_resolution(
                min,
                max,
                resolution,
                -values[4] * 0.5,
                values[4],
            )?;
            if restored.cells.map(|count| count as u32 + 1) != resolution {
                return Err("Clipmap两轴分辨率与源范围、格距不一致".into());
            }
        }
        SceneTerrainKind::Voxel => {
            if !(0.01..=0.5).contains(&values[7]) {
                return Err("体素源格距必须在 0.010 至 0.500 米之间".into());
            }
            if min
                .into_iter()
                .chain(max)
                .chain([-values[4], 0.0])
                .any(|coordinate| (coordinate / values[7]).abs() > 32_000_000.0)
            {
                return Err("体素源坐标超出安全整数格点范围".into());
            }
        }
        _ => return Err("添加操作只接受高度地形或体素地形".into()),
    }
    Ok(CreationSpec {
        min,
        max,
        resolution,
    })
}

/// 公共添加入口只呈现实际可输入的范围；后端格式仍可读取既有厘米格距作者源。
pub(super) fn validate_input(kind: SceneTerrainKind, values: &[f32; 8]) -> Result<CreationSpec, String> {
    if kind != SceneTerrainKind::Gltf {
        if !values[7].is_finite() || !(MIN_SPACING..=MAX_SPACING).contains(&values[7]) {
            return Err("本入口源格距必须从 0.125 米起，且不超过 1000000 米".into());
        }
        if kind == SceneTerrainKind::Voxel && values[7] > 0.5 {
            return Err("本入口体素源格距必须在 0.125 至 0.500 米之间".into());
        }
    }
    validate(kind, values)
}

/// 创建字段服从公共数值框范围；各生成器额外限制仍由规格预检解释。
pub(super) fn set_field(values: &mut [f32; 8], index: usize, value: f32) -> Result<(), String> {
    let minimum = if index == 7 {
        MIN_SPACING
    } else if (3..6).contains(&index) {
        0.01
    } else {
        -1_000_000.0
    };
    if index >= values.len() || !value.is_finite() || !(minimum..=MAX_SPACING).contains(&value) {
        return Err("初始规格超出数值框允许范围".into());
    }
    values[index] = value;
    Ok(())
}

pub(super) fn summary(values: &[f32; 8], asset: Option<&str>) -> String {
    let row = |kind, name, limits| match validate_input(kind, values) {
        Ok(_) => format!("{name}：可创建（{limits}）"),
        Err(error) => format!("{name}：不可创建 · {error}"),
    };
    format!(
        "公共源格距最小0.125m，精度0.001m。\n{}\n{}\n高度地形的米制高度不受初始规格限制；体素地形的高度为初始实心盒高。网格使用资产几何，不使用源格距。{}",
        row(
            SceneTerrainKind::SparseClipmap,
            "高度地形",
            "格距0.125～64m，边长0.01～8192m"
        ),
        row(SceneTerrainKind::Voxel, "体素地形", "本入口格距0.125～0.500m"),
        asset.map_or_else(
            || "请先在左侧资产页选择 GLB。".to_owned(),
            |path| format!("所选资产：{path}")
        ),
    )
}

#[cfg(test)]
#[path = "creation_tests.rs"]
mod tests;
