//! F4 动态地图监控的分页与低频刷新会话资源。

use bevy::prelude::{Resource, Timer, TimerMode};
use feathrise_fps_display::{LodControlContent, LodControlPresetBindings};

/// F4 保留 F3 的五列顶层布局，但按动态地图正式数据域重新分组。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditorMapMonitorGroup {
    Overview,
    TerrainSurface,
    Vegetation,
    Water,
    SceneGameplay,
}

impl EditorMapMonitorGroup {
    pub(crate) const ALL: [Self; 5] = [
        Self::Overview,
        Self::TerrainSurface,
        Self::Vegetation,
        Self::Water,
        Self::SceneGameplay,
    ];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Overview => "总览",
            Self::TerrainSurface => "地形与地表",
            Self::Vegetation => "植被",
            Self::Water => "水域",
            Self::SceneGameplay => "场景与交互",
        }
    }

    pub(crate) const fn default_page(self) -> EditorMapMonitorPage {
        match self {
            Self::Overview => EditorMapMonitorPage::Overview,
            Self::TerrainSurface => EditorMapMonitorPage::Terrain,
            Self::Vegetation => EditorMapMonitorPage::OrdinaryGrass,
            Self::Water => EditorMapMonitorPage::StillWater,
            Self::SceneGameplay => EditorMapMonitorPage::Placements,
        }
    }

    pub(crate) const fn has_subtabs(self) -> bool {
        match self {
            Self::Overview => false, // 总览本身就是唯一叶子页，不重复显示二级按钮。
            Self::TerrainSurface | Self::Vegetation | Self::Water | Self::SceneGameplay => true, // 其余数据域均由二级页签细分。
        }
    }
}

/// 动态地图全部作者数据域的互斥叶子页；实体在场景建立时固定创建。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditorMapMonitorPage {
    Overview,
    Terrain,
    Surface,
    OrdinaryGrass,
    Wheat,
    MixedGrain,
    Trees,
    Shrubs,
    Rocks,
    StillWater,
    Rivers,
    Waterfalls,
    MeshWater,
    Placements,
    Portals,
    Boundaries,
    Environment,
}

impl EditorMapMonitorPage {
    /// 保留固定兼容槽，但没有工程映射时只开放两种通用后端，不展示游戏分类。
    pub(crate) fn available(self, bindings: Option<&LodControlPresetBindings>) -> bool {
        self.content().is_none_or(|content| {
            matches!(self, Self::OrdinaryGrass | Self::Trees)
                || bindings.and_then(|bindings| bindings.profile(content)).is_some()
        })
    }

    pub(crate) fn display_label(self, bindings: Option<&LodControlPresetBindings>) -> &'static str {
        if self.content().and_then(|content| bindings.and_then(|bindings| bindings.profile(content))).is_none() {
            match self {
                Self::OrdinaryGrass => return "叶片植被",
                Self::Trees => return "网格植被",
                _ => {}
            }
        }
        self.label()
    }

    pub(crate) const fn content(self) -> Option<LodControlContent> {
        match self {
            Self::OrdinaryGrass => Some(LodControlContent::OrdinaryGrass),
            Self::Wheat => Some(LodControlContent::Wheat),
            Self::MixedGrain => Some(LodControlContent::MixedGrain),
            Self::Trees => Some(LodControlContent::Trees),
            Self::Shrubs => Some(LodControlContent::Shrubs),
            Self::Rocks => Some(LodControlContent::Rocks),
            _ => None,
        }
    }

    pub(crate) const ALL: [Self; 17] = [
        Self::Overview,
        Self::Terrain,
        Self::Surface,
        Self::OrdinaryGrass,
        Self::Wheat,
        Self::MixedGrain,
        Self::Trees,
        Self::Shrubs,
        Self::Rocks,
        Self::StillWater,
        Self::Rivers,
        Self::Waterfalls,
        Self::MeshWater,
        Self::Placements,
        Self::Portals,
        Self::Boundaries,
        Self::Environment,
    ];

    /// 总览没有重复的二级按钮，其余十六页按所属数据域在同一固定按钮池中切换。
    pub(crate) const SUBTABS: [Self; 16] = [
        Self::Terrain,
        Self::Surface,
        Self::OrdinaryGrass,
        Self::Wheat,
        Self::MixedGrain,
        Self::Trees,
        Self::Shrubs,
        Self::Rocks,
        Self::StillWater,
        Self::Rivers,
        Self::Waterfalls,
        Self::MeshWater,
        Self::Placements,
        Self::Portals,
        Self::Boundaries,
        Self::Environment,
    ];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Overview => "总览",
            Self::Terrain => "地形",
            Self::Surface => "地表",
            Self::OrdinaryGrass => "普通草",
            Self::Wheat => "麦田",
            Self::MixedGrain => "杂粮田",
            Self::Trees => "树木",
            Self::Shrubs => "灌木",
            Self::Rocks => "岩石",
            Self::StillWater => "静水",
            Self::Rivers => "河流",
            Self::Waterfalls => "瀑布",
            Self::MeshWater => "网格水体",
            Self::Placements => "场景摆放",
            Self::Portals => "传送点",
            Self::Boundaries => "区域边界",
            Self::Environment => "环境",
        }
    }

    pub(crate) const fn group(self) -> EditorMapMonitorGroup {
        match self {
            Self::Overview => EditorMapMonitorGroup::Overview,
            Self::Terrain | Self::Surface => EditorMapMonitorGroup::TerrainSurface,
            Self::OrdinaryGrass
            | Self::Wheat
            | Self::MixedGrain
            | Self::Trees
            | Self::Shrubs
            | Self::Rocks => EditorMapMonitorGroup::Vegetation,
            Self::StillWater | Self::Rivers | Self::Waterfalls | Self::MeshWater => {
                EditorMapMonitorGroup::Water
            }
            Self::Placements | Self::Portals | Self::Boundaries | Self::Environment => {
                EditorMapMonitorGroup::SceneGameplay
            }
        }
    }

    /// 只有六种正式动态植被输出需要密度与候选后端报告。
    #[cfg(test)] // 生产分派使用穷尽match；该辅助只为结构回归测试提供清晰断言。
    pub(crate) const fn is_vegetation(self) -> bool {
        matches!(
            self,
            Self::OrdinaryGrass
                | Self::Wheat
                | Self::MixedGrain
                | Self::Trees
                | Self::Shrubs
                | Self::Rocks
        )
    }
}

/// F4 只保存在当前进程内的面板状态；默认从全图总览开始。
#[derive(Resource)]
pub(crate) struct EditorMapMonitorState {
    pub(crate) visible: bool,
    pub(crate) force_refresh: bool,
    pub(crate) group: EditorMapMonitorGroup,
    pub(crate) page: EditorMapMonitorPage,
    pub(crate) refresh_timer: Timer,
}

impl Default for EditorMapMonitorState {
    fn default() -> Self {
        Self {
            visible: false,
            force_refresh: false,
            group: EditorMapMonitorGroup::Overview,
            page: EditorMapMonitorPage::Overview,
            refresh_timer: Timer::from_seconds(0.5, TimerMode::Repeating),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_full_map_overview() {
        let state = EditorMapMonitorState::default();
        assert_eq!(state.group, EditorMapMonitorGroup::Overview);
        assert_eq!(state.page, EditorMapMonitorPage::Overview);
    }

    #[test]
    fn every_dynamic_map_page_has_one_consistent_group() {
        for page in EditorMapMonitorPage::ALL {
            assert!(EditorMapMonitorGroup::ALL.contains(&page.group()));
        }
        for group in EditorMapMonitorGroup::ALL {
            assert_eq!(group.default_page().group(), group);
        }
        assert_eq!(EditorMapMonitorPage::SUBTABS.len(), 16);
        assert!(
            EditorMapMonitorGroup::ALL
                .iter()
                .all(|group| group.label() != "故道植被")
        ); // F4不再复制旧专图分类，统一面向当前动态地图草稿。
        assert_eq!(
            EditorMapMonitorPage::ALL
                .iter()
                .filter(|page| page.is_vegetation())
                .count(),
            6
        ); // 普通草、两种作物和三种Mesh景观构成当前完整正式植被集合。
    }
}
