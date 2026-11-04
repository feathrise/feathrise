//! F4动态地图各数据域报告的统一分派入口。

mod common; // 地图身份、计数格式与植被R8统计口径只维护一份。
mod map; // 总览、地形和地表读取作者草稿中的常数大小元数据。
mod scene; // 摆放、传送、边界和环境读取已加载作者文档。
mod vegetation; // 六种正式植被输出共享密度、候选和预览报告。
mod water; // 静水、河流、瀑布及GLB网格水读取水域缓存。

use feathrise_fps_display::LodControlPresetBindings;
use feathrise_atmosphere::{WeatherDebugOverride, WeatherState}; // 环境页只在可见刷新点读取现成天气小资源。

use crate::authoring::{EditorAuthoringSession, VegetationPreviewState, WaterPreviewState}; // 统一入口只接收现成作者缓存，不持有或创建后台任务。
use crate::monitor_resources::EditorMapMonitorPage; // 当前叶子页决定唯一报告分支。
use crate::resources::MapEditorState; // F12显隐、dirty与当前笔刷参数用于解释作者状态。

/// 只构建当前可见页报告，避免每次刷新为全部十七页分配字符串。
pub(super) fn build_editor_map_report(
    page: EditorMapMonitorPage,                  // 当前互斥F4叶子页。
    bindings: Option<&LodControlPresetBindings>,
    editor: &MapEditorState,                     // 当前F12会话和笔刷状态。
    session: &EditorAuthoringSession,            // 当前已加载动态地图草稿。
    vegetation_preview: &VegetationPreviewState, // Mesh散布的既有完成值与安装进度。
    water_preview: &WaterPreviewState,           // 水网既有任务、安装和采用状态。
    weather_state: &WeatherState,                // 自动天气连续过渡状态。
    weather_override: &WeatherDebugOverride,     // F12天气强制模式和预览覆盖。
) -> String {
    match page {
        EditorMapMonitorPage::Overview => {
            map::build_overview_report(editor, session, vegetation_preview, water_preview)
        }
        EditorMapMonitorPage::Terrain => map::build_terrain_report(editor, session),
        EditorMapMonitorPage::Surface => map::build_surface_report(editor, session),
        EditorMapMonitorPage::OrdinaryGrass
        | EditorMapMonitorPage::Wheat
        | EditorMapMonitorPage::MixedGrain
        | EditorMapMonitorPage::Trees
        | EditorMapMonitorPage::Shrubs
        | EditorMapMonitorPage::Rocks => {
            vegetation::build_vegetation_report(page, bindings, editor, session, vegetation_preview)
        }
        EditorMapMonitorPage::StillWater => {
            water::build_still_water_report(editor, session, water_preview)
        }
        EditorMapMonitorPage::Rivers => water::build_river_report(editor, session, water_preview),
        EditorMapMonitorPage::Waterfalls => {
            water::build_waterfall_report(editor, session, water_preview)
        }
        EditorMapMonitorPage::MeshWater => water::build_mesh_water_report(editor, session),
        EditorMapMonitorPage::Placements => scene::build_placements_report(editor, session),
        EditorMapMonitorPage::Portals => scene::build_portals_report(editor, session),
        EditorMapMonitorPage::Boundaries => scene::build_boundaries_report(editor, session),
        EditorMapMonitorPage::Environment => {
            scene::build_environment_report(editor, session, weather_state, weather_override)
        } // 环境页与F12天气滑块必须显示同一有效预览比例。
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_pages_are_covered_by_the_full_map_dispatch() {
        let editor = MapEditorState::default(); // 空会话足以验证每个分支都有明确正文。
        let session = EditorAuthoringSession::default(); // 不构造文件数据，避免测试依赖地图资产。
        let vegetation_preview = VegetationPreviewState::default(); // 无预览时应给出等待态。
        let water_preview = WaterPreviewState::default(); // 无水网时应给出等待态。
        let weather_state = WeatherState::default(); // 自动天气默认值足以覆盖环境页分派。
        let weather_override = WeatherDebugOverride::default(); // 默认无强制或手动预览覆盖。
        let mut project_bindings = LodControlPresetBindings::default();
        for content in feathrise_fps_display::LodControlContent::ALL {
            project_bindings.bind(content, format!("test_{content:?}"));
        }
        // 普通工程与显式分类工程各走完整十七分支；不因隐藏分类页而缩小分派覆盖。
        for bindings in [None, Some(&project_bindings)] {
            for page in EditorMapMonitorPage::ALL {
                let report = build_editor_map_report(
                    page,
                    bindings,
                    &editor,
                    &session,
                    &vegetation_preview,
                    &water_preview,
                    &weather_state,
                    &weather_override,
                );
                let expected = match (bindings.is_none(), page) {
                    (true, EditorMapMonitorPage::OrdinaryGrass) => "叶片植被",
                    (true, EditorMapMonitorPage::Trees) => "网格植被",
                    _ => page.label(),
                };
                assert!(report.starts_with(expected), "{page:?}应显示{expected}，实际为{report}");
                assert!(!report.contains("尚未接入")); // 完整监控不再保留旧树木首版占位文案。
            }
        }
    }
}
