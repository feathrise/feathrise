//! 新建空场景只校验资料；旧地形生成入口仍独立校验网格尺寸和采样容量。

use crate::constants::{
    MAX_NEW_SCENE_EXTENT_METERS, MAX_TERRAIN_CELL_METERS, MIN_NEW_SCENE_EXTENT_METERS,
    MIN_TERRAIN_CELL_METERS,
}; // UI确认与后台创建共享同一成本边界。
use crate::types::{SceneCreationRequest, NewSceneTerrainMode}; // 冻结作者确认时的名称、尺寸和路线，不受保存期间输入影响。

const MAX_SCENE_NAME_CHARS: usize = 48; // 控制UI、目录和JSON诊断长度。

/// 一次新场景创建使用的受检世界尺寸和有界高度场规格。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SceneDimensions {
    pub(crate) width_meters: f32, // 世界X轴东西宽度，供创建预览算实际单元宽度。
    pub(crate) length_meters: f32, // 世界Z轴南北长度，供创建预览算实际单元长度。
    pub(super) world_min: [f32; 2], // 以原点为中心的X/Z最小端点。
    pub(super) world_max: [f32; 2], // 以原点为中心的X/Z最大端点。
    pub(crate) terrain_mesh_resolution: [u32; 2], // 原始路线表示GLB顶点数，Clipmap路线表示稀疏作者逻辑格点。
    pub(crate) terrain_height_resolution: [u32; 2], // 原始路线固定513²，Clipmap路线记录完整作者逻辑规格。
}

impl SceneDimensions {
    /// 稀疏路线的便捷入口；纯规格调用默认不建立完整 GLB。
    #[cfg(test)]
    pub(crate) fn new(
        width_meters: f32,        // 场景世界X跨度。
        length_meters: f32,       // 场景世界Z跨度。
        terrain_cell_meters: f32, // 作者选择的每轴最大逻辑格距。
    ) -> Result<Self, String> {
        Self::for_mode(
            NewSceneTerrainMode::SparseClipmap,
            width_meters,
            length_meters,
            terrain_cell_meters,
        )
    }

    /// 按所选作者路线校验容量，再计算居中世界范围与实际格点规格。
    pub(crate) fn for_mode(
        terrain_mode: NewSceneTerrainMode,
        width_meters: f32,
        length_meters: f32,
        terrain_cell_meters: f32,
    ) -> Result<Self, String> {
        if terrain_mode == NewSceneTerrainMode::Empty {
            // 空容器没有尺寸。此单位矩形只兼容旧清单字段，不产生地形、编辑边界或取景范围。
            return Ok(Self { width_meters: 1.0, length_meters: 1.0,
                world_min: [-0.5, -0.5],
                world_max: [0.5, 0.5],
                terrain_mesh_resolution: [0, 0], terrain_height_resolution: [0, 0] });
        }
        validate_scene_extent(width_meters, "东西宽度X")?;
        validate_scene_extent(length_meters, "南北长度Z")?;
        validate_terrain_cell_meters(terrain_cell_meters)?; // 即使绕过UI直接进入后台也拒绝非法间距。
        let terrain_mesh_resolution = [
            terrain_axis_resolution(width_meters, terrain_cell_meters, "X")?, // X轴严格按目标边长计算，不向上取二次幂。
            terrain_axis_resolution(length_meters, terrain_cell_meters, "Z")?, // Z轴独立按目标边长计算，矩形场景仍完整覆盖。
        ];

        let half_width = width_meters * 0.5;
        let half_length = length_meters * 0.5;
        let terrain_height_resolution = match terrain_mode {
            NewSceneTerrainMode::Empty => [0, 0],
            NewSceneTerrainMode::SparseClipmap => terrain_mesh_resolution, // 稀疏清单记录逻辑规格，但绝不据此分配整图数组。
        };
        Ok(Self {
            width_meters,
            length_meters,
            world_min: [-half_width, -half_length],
            world_max: [half_width, half_length],
            terrain_mesh_resolution, // 预览与后台生成使用同一格点规格，不暗中放大作者格距。
            terrain_height_resolution,
        })
    }
}

/// 在用户确认帧完成全部输入校验并返回不可变请求快照。
pub(super) fn validated_creation_request(
    display_name: &str,              // 用户输入的可见场景名称。
    width: &str,                     // X轴米制文本。
    length: &str,                    // Z轴米制文本。
    terrain_cell: &str,              // 所选路线的目标格距文本。
    terrain_mode: NewSceneTerrainMode, // 点击确认时的显式地形方案。
) -> Result<SceneCreationRequest, String> {
    let display_name = normalize_display_name(display_name)?;
    let (width_meters, length_meters) = if terrain_mode == NewSceneTerrainMode::Empty {
        (1.0, 1.0) // 隐藏的旧输入即使为空或无效，也不能阻止创建只有名称和备注的容器。
    } else {
        (parse_scene_extent(width, "东西宽度X")?, parse_scene_extent(length, "南北长度Z")?)
    };
    let terrain_cell_meters = if terrain_mode == NewSceneTerrainMode::Empty { crate::constants::TARGET_TERRAIN_CELL_METERS }
        else { parse_terrain_cell_meters(terrain_cell)? }; // 空容器没有作者格距，隐藏控件不能成为创建门。
    SceneDimensions::for_mode(
        terrain_mode,
        width_meters,
        length_meters,
        terrain_cell_meters,
    )?;
    Ok(SceneCreationRequest {
        display_name,
        notes: String::new(), // 入口调用方在确认帧补入已提交的可选备注快照。
        width_meters,
        length_meters,
        terrain_mode,
        collision_lods: Vec::new(), // 此处只校验尺寸；稀疏场景确认入口随后冻结碰撞草稿。
        display_lods: None,         // 稀疏确认入口取得真实SparseHeightSpec后生成或冻结显式列表。
        terrain_cell_meters,        // 当前场景保存期间不再读取可能变化的弹框输入。
        folder_id: None,            // UI请求系统在完整校验后冻结当前虚拟目录。
        folder_source_bytes: None,  // UI请求系统同时冻结精确归类文件基线。
    })
}

/// 校验用户可见名称并去掉首尾空白。
pub(super) fn normalize_display_name(raw: &str) -> Result<String, String> {
    let name = raw.trim();
    let count = name.chars().count();
    if count == 0 || count > MAX_SCENE_NAME_CHARS {
        return Err(format!("场景名称必须为1至{MAX_SCENE_NAME_CHARS}个字符"));
    }
    if name.chars().any(|character| character.is_control()) {
        return Err("场景名称不能包含控制字符".to_owned());
    }
    Ok(name.to_owned())
}

/// 把一个米制轴长度限制在作者工具可安全生成的范围内。
fn validate_scene_extent(value: f32, label: &str) -> Result<(), String> {
    if !value.is_finite() {
        return Err(format!("{label}必须是有限数字"));
    }
    if !(MIN_NEW_SCENE_EXTENT_METERS..=MAX_NEW_SCENE_EXTENT_METERS).contains(&value) {
        return Err(format!(
            "{label}必须在{MIN_NEW_SCENE_EXTENT_METERS}至{MAX_NEW_SCENE_EXTENT_METERS}米之间"
        ));
    }
    Ok(())
}

/// 解析一个尺寸输入框；允许小数米，但拒绝空值、单位文字和非有限结果。
fn parse_scene_extent(raw: &str, label: &str) -> Result<f32, String> {
    let value = raw
        .trim()
        .parse::<f32>()
        .map_err(|_| format!("{label}必须填写数字，单位为米"))?;
    validate_scene_extent(value, label)?;
    Ok(value)
}

/// 目标间距范围由两条路线共用，具体容量在路线校验阶段决定。
fn validate_terrain_cell_meters(value: f32) -> Result<(), String> {
    if !value.is_finite() || !(MIN_TERRAIN_CELL_METERS..=MAX_TERRAIN_CELL_METERS).contains(&value) {
        return Err(format!(
            "地形网格间距必须在{MIN_TERRAIN_CELL_METERS}至{MAX_TERRAIN_CELL_METERS}米之间"
        )); // 非有限输入或越界配置不得进入任何创建任务。
    }
    Ok(())
}

/// 解析作者指定的实际地形格距，拒绝空文本、非法单位和非有限数字。
fn parse_terrain_cell_meters(raw: &str) -> Result<f32, String> {
    let value = raw
        .trim()
        .parse::<f32>()
        .map_err(|_| "地形网格间距必须填写数字，单位为米".to_owned())?;
    validate_terrain_cell_meters(value)?;
    Ok(value)
}

/// 直接按目标格距向上取整；只防止轴格点字段溢出，不依据场景尺寸提高最小格距。
fn terrain_axis_resolution(
    extent_meters: f32,       // 当前X或Z轴的真实米制跨度。
    terrain_cell_meters: f32, // 作者要求的最大目标格距。
    axis: &str,               // 错误消息指出需要调整的轴。
) -> Result<u32, String> {
    let desired_cells = (f64::from(extent_meters) / f64::from(terrain_cell_meters)).ceil(); // 用高精度商避免边界附近过早截断。
    if desired_cells > f64::from(u32::MAX - 1) {
        return Err(format!(
            "{axis}轴需{desired_cells:.0}格，超过逻辑格点字段可表示的范围"
        )); // 即使以后放宽场景尺寸，也不得发生浮点转整数的静默饱和或格点加一溢出。
    }
    Ok(desired_cells as u32 + 1) // 顶点数始终是单元数加一，并完整覆盖两端。
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_scene_ignores_legacy_size_and_sampling_inputs() {
        let request = validated_creation_request("  空场景  ", "", "NaN", "无效", NewSceneTerrainMode::Empty).unwrap();
        assert_eq!(request.display_name, "空场景");
        let dimensions = SceneDimensions::for_mode(NewSceneTerrainMode::Empty, f32::NAN, -1.0, 0.0).unwrap();
        assert_eq!(dimensions.terrain_mesh_resolution, [0, 0]);
        assert_eq!(dimensions.terrain_height_resolution, [0, 0]);
    }

    #[test]
    fn creation_request_freezes_trimmed_chinese_name_and_dimensions() {
        let request = validated_creation_request(
            "  太行山麓  ",
            "2048",
            "512",
            "2",
            NewSceneTerrainMode::SparseClipmap,
        )
        .unwrap();
        assert_eq!(request.display_name, "太行山麓");
        assert_eq!(request.width_meters, 2_048.0);
        assert_eq!(request.length_meters, 512.0);
        assert_eq!(request.terrain_mode, NewSceneTerrainMode::SparseClipmap);
        assert_eq!(request.terrain_cell_meters, 2.0); // 作者指定的稀疏高度格距与其他表单输入一并冻结。
        assert!(
            validated_creation_request(
                "坏\n名字",
                "1024",
                "1024",
                "2",
                NewSceneTerrainMode::SparseClipmap,
            )
            .is_err()
        );
        assert!(
            validated_creation_request(
                "新图",
                "1024",
                "1024",
                "NaN",
                NewSceneTerrainMode::SparseClipmap,
            )
            .is_err()
        ); // 非有限间距不得进入后台拓扑分配。
        assert!(
            validated_creation_request(
                "新图",
                "1024",
                "1024",
                "2米",
                NewSceneTerrainMode::SparseClipmap,
            )
            .is_err()
        ); // 单位文字不进入数值快照。
    }

    #[test]
    fn eight_meter_target_and_non_square_dimensions_remain_centered() {
        let default = SceneDimensions::new(1_024.0, 1_024.0, 8.0).unwrap(); // 整除尺寸按目标格距生成精确128格。
        assert_eq!(default.world_min, [-512.0, -512.0]);
        assert_eq!(default.world_max, [512.0, 512.0]);
        assert_eq!(default.terrain_mesh_resolution, [129, 129]);
        assert_eq!(default.terrain_height_resolution, [129, 129]); // 清单记录作者格点数，不再固定派生513²。
        let rectangular = SceneDimensions::new(2_048.0, 512.0, 8.0).unwrap();
        assert_eq!(rectangular.world_min, [-1_024.0, -256.0]);
        assert_eq!(rectangular.world_max, [1_024.0, 256.0]);
        assert_eq!(rectangular.terrain_mesh_resolution, [257, 65]);
        assert_eq!(rectangular.terrain_height_resolution, [257, 65]); // 两轴按实际格距分别取整。
    }

    #[test]
    fn selected_cell_size_changes_sparse_author_resolution() {
        let coarse = SceneDimensions::new(262.0, 243.0, 8.0).unwrap(); // 粗作者格距产生较少逻辑格点。
        let fine = SceneDimensions::new(262.0, 243.0, 2.0).unwrap(); // 用户指定两米时两轴分别增加逻辑格点。
        assert_eq!(coarse.terrain_mesh_resolution, [34, 32]); // 262/8与243/8各自向上取整，不再无故翻倍。
        assert_eq!(fine.terrain_mesh_resolution, [132, 123]); // 两米目标形成131×122个作者单元。
        assert_eq!(fine.terrain_height_resolution, [132, 123]); // 清单与作者逻辑格点保持一致。
    }

    #[test]
    fn screenshot_dimensions_keep_requested_cell_size() {
        let dimensions = SceneDimensions::new(836.0, 836.0, 1.625).unwrap(); // 截图中的参数超过512格仅三格，不应被静默放粗。
        assert_eq!(dimensions.terrain_mesh_resolution, [516, 516]); // 两轴515个作者单元，不截成512格。
        assert_eq!(dimensions.terrain_height_resolution, [516, 516]); // 清单完整保留作者格距。
        assert!(836.0 / 515.0 <= 1.625); // 实际边长满足作者目标值。
    }

    #[test]
    fn requested_eighth_meter_grid_is_allowed_on_571_by_592_meter_scene() {
        let dimensions = SceneDimensions::new(571.0, 592.0, 0.125).unwrap(); // 回归用户截图：尺寸增长不能触发1024格人为上限。
        assert_eq!(dimensions.terrain_mesh_resolution, [4_569, 4_737]); // 两轴完整保持每格0.125米。
        assert_eq!(dimensions.terrain_height_resolution, [4_569, 4_737]); // 稀疏清单可表达全部逻辑格点。
    }

    #[test]
    fn scene_dimensions_allow_maximum_scene_at_eighth_meter_spacing() {
        assert!(SceneDimensions::new(f32::NAN, 1_024.0, 2.0).is_err());
        assert!(SceneDimensions::new(0.009, 1_024.0, 2.0).is_err());
        assert!(SceneDimensions::new(1_024.0, 8_193.0, 2.0).is_err());
        assert!(SceneDimensions::new(1_024.0, 1_024.0, f32::INFINITY).is_err()); // 非有限目标间距无法稳定计算网格数量。
        assert!(SceneDimensions::new(1_024.0, 1_024.0, 0.0).is_err()); // 零间距不得导致无限数量请求。
        assert_eq!(
            SceneDimensions::new(8_192.0, 8_192.0, 2.0)
                .unwrap()
                .terrain_mesh_resolution,
            [4_097, 4_097]
        ); // 默认两米格距保持完整逻辑坐标。
        assert_eq!(
            SceneDimensions::new(8_192.0, 8_192.0, 0.125)
                .unwrap()
                .terrain_height_resolution,
            [65_537, 65_537]
        ); // 最大图的最细逻辑格点只记入清单，不能据此分配整图数组。
        assert_eq!(
            SceneDimensions::new(8_192.0, 8_192.0, 8.0)
                .unwrap()
                .terrain_mesh_resolution,
            [1_025, 1_025]
        ); // 较粗目标仍可创建，格点数不会暗中固定为513²。
        assert_eq!(
            SceneDimensions::new(64.0, 64.0, 2.0)
                .unwrap()
                .terrain_height_resolution,
            [33, 33]
        ); // 旧64米规格仍严格采用请求的两米作者格距。
    }

    #[test]
    fn clipmap_scene_creation_accepts_centimeter_extent_and_preserves_source_endpoints() {
        // 【功能设计】保留的后台场景创建与新地形面板遵守同一下限；逐层核对真实来源，
        // 不能只放宽文本输入而让清单、显示或碰撞随后拒绝厘米级／不足一格的地形。
        for [width, length] in [[0.01_f32, 0.01], [0.01, 0.37], [1.0, 63.999]] {
            let request = validated_creation_request(
                "厘米地形",
                &width.to_string(),
                &length.to_string(),
                "0.125",
                NewSceneTerrainMode::SparseClipmap,
            ).unwrap();
            let dimensions = SceneDimensions::for_mode(
                request.terrain_mode, request.width_meters, request.length_meters,
                request.terrain_cell_meters,
            ).unwrap();
            let spec = feathrise_scene_content::SparseHeightSpec::new(
                dimensions.world_min, dimensions.world_max, request.terrain_cell_meters,
                super::super::create::DEFAULT_BASE_HEIGHT, super::super::create::DEFAULT_HEIGHT_SCALE,
            ).unwrap();
            assert_eq!(dimensions.terrain_height_resolution, spec.cells.map(|cells| cells as u32 + 1));
            let restored = feathrise_scene_content::SparseHeightSpec::from_resolution(
                dimensions.world_min, dimensions.world_max, dimensions.terrain_height_resolution,
                spec.base_height, spec.height_scale,
            ).unwrap();
            assert_eq!(restored.cells, spec.cells);
            let endpoint = restored.grid_point_world(restored.cells[0], restored.cells[1]).unwrap();
            for axis in 0..2 {
                assert!((endpoint[axis] - f64::from(dimensions.world_max[axis])).abs() < 1e-9);
            }
            let plan = feathrise_scene_content::compile_display_lod_plan(&restored, &[
                feathrise_scene_content::SceneTerrainDisplayLod {
                    outer_half_extent_meters: None, spacing_multiplier: 1,
                },
            ]).unwrap();
            assert_eq!(plan.bands.len(), 1);
            assert_eq!(plan.bands[0].outer_half_cells.map(u64::from), restored.cells);
            let collision = feathrise_scene_content::default_collision_lods_for_resolution(
                dimensions.terrain_height_resolution,
            );
            feathrise_scene::validate_layered_collision_settings(&restored, &collision).unwrap();
        }
        let error = validated_creation_request(
            "过小地形", "0.009", "1", "0.125", NewSceneTerrainMode::SparseClipmap,
        ).unwrap_err();
        assert!(error.contains("0.01至8192"), "{error}");
    }


}
