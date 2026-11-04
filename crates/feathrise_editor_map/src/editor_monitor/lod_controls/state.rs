//! 把 F4 六个植被叶子页映射到 F3/F4 共享的四档 LOD 内容身份。
//! 【功能设计】F4 不再保存第二份掩码；两块面板操作同一资源，切换面板不会把已经关闭的档位重新打开。

use feathrise_fps_display::LodControlContent; // 共享身份统一叶片和Mesh的档位名称、掩码与渲染应用。

use crate::monitor_resources::EditorMapMonitorPage; // F4页面只负责选择当前要操作的动态地图输出。

/// 返回当前 F4 页面对应的共享植被内容；非植被对象没有伪造的 LOD 控件。
pub(super) const fn content_for_editor_page(
    page: EditorMapMonitorPage, // 当前F4互斥叶子页。
) -> Option<LodControlContent> {
    match page {
        EditorMapMonitorPage::OrdinaryGrass => Some(LodControlContent::OrdinaryGrass), // GPU普通草四档。
        EditorMapMonitorPage::Wheat => Some(LodControlContent::Wheat), // GPU麦田四档。
        EditorMapMonitorPage::MixedGrain => Some(LodControlContent::MixedGrain), // GPU杂粮四档。
        EditorMapMonitorPage::Trees => Some(LodControlContent::Trees), // Mesh树木四档。
        EditorMapMonitorPage::Shrubs => Some(LodControlContent::Shrubs), // Mesh灌木四档。
        EditorMapMonitorPage::Rocks => Some(LodControlContent::Rocks), // Mesh岩石四档。
        _ => None, // 地形、水体、摆放和环境没有统一四档LOD，必须隐藏控件。
    } // 完成十七页穷尽映射。
} // 返回可选共享内容身份。

#[cfg(test)] // 纯映射回归不启动窗口或渲染器。
mod tests {
    use super::*; // 验证生产页面映射。

    #[test] // F4只为六种正式动态植被输出提供逐档能力。
    fn exactly_six_vegetation_pages_map_to_shared_lod_contents() {
        let mappings = EditorMapMonitorPage::ALL
            .into_iter()
            .filter_map(|page| content_for_editor_page(page).map(|content| (page, content)))
            .collect::<Vec<_>>(); // 收集固定十七页中的全部受控页面。
        assert_eq!(mappings.len(), 6); // 不给没有LOD契约的其他对象显示无效按钮。
        assert_eq!(
            mappings,
            vec![
                (
                    EditorMapMonitorPage::OrdinaryGrass,
                    LodControlContent::OrdinaryGrass
                ), // 普通草。
                (EditorMapMonitorPage::Wheat, LodControlContent::Wheat), // 麦田。
                (
                    EditorMapMonitorPage::MixedGrain,
                    LodControlContent::MixedGrain
                ), // 杂粮田。
                (EditorMapMonitorPage::Trees, LodControlContent::Trees), // 树木。
                (EditorMapMonitorPage::Shrubs, LodControlContent::Shrubs), // 灌木。
                (EditorMapMonitorPage::Rocks, LodControlContent::Rocks), // 岩石。
            ],
        ); // 锁定F4与共享状态的一一对应关系。
    } // 完成六页映射回归。
} // 结束F4页面映射测试。
