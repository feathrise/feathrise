//! 场景编辑器侧栏仅在当前进程内保存的页签、帮助和上下文UI类型。

use bevy::prelude::*; // Resource、Component与编辑器工具枚举派生。

use crate::types::EditorTool; // 工具真实变化用于一次性定位相关左栏页。

/// 顶部创建与工具模块；沿用原身份，工具正文统一挂在右栏。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditorLeftSidebarTab {
    #[default]
    Scene, // 兼容原视口模块记忆；场景目录现由独立导航状态控制。
    Meshes,     // 创建网格与所选属性独立成页；完整对象列表位于左侧场景树。
    Terrain,    // 雕刻与地表权重集中在同一页；选择模式位于视口。
    Domain(&'static str), // 应用自己的工具页；内容由应用挂载。
    Vegetation, // 植被正式输出、作者层与编译参数集中在同一页。
    Water,      // 静水、河流与瀑布共享统一对象页。
    Building,   // 建筑预设、地基语义与精确摆放集中在独立页。
    Gameplay,   // 摆放、传送与区域边界归入玩法对象页。
    Environment, // 天空、天气与全局环境参数由同一个顶部模块打开。
}

impl EditorLeftSidebarTab {
    /// 右侧独立工具页签使用当前模块的真实名称，不再借用“属性”标题。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Scene => "工具",
            Self::Meshes => "网格",
            Self::Terrain => "塑形",
            Self::Domain(_) => "扩展",
            Self::Vegetation => "植被",
            Self::Water => "水域",
            Self::Building => "建筑",
            Self::Gameplay => "交互",
            Self::Environment => "环境",
        }
    }

    /// 环境沿用原唯一参数树及输入门，其余模块继续复用工具正文宿主。
    pub const fn work_page(self) -> EditorRightSidebarTab {
        match self {
            Self::Environment => EditorRightSidebarTab::Environment,
            _ => EditorRightSidebarTab::Tools,
        }
    }
}

/// 左侧只承担导航，切换目录/场景树不改变当前工具或所选对象。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditorNavigatorTab {
    #[default]
    Scenes,
    SceneTree,
    Assets, // 文件浏览独立于右侧当前属性目标。
}

/// 属性的来源由最后一次显式点选决定；保留文件选择不代表一直抢占场景属性。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditorInspectorTarget {
    #[default]
    Scene,
    Asset,
    SceneEntry, // 场景目录的显式选择；允许与当前已进入场景不同。
}

/// 右侧完整高度的属性、工具与环境正文；资产仅是属性页的内部来源身份。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditorRightSidebarTab {
    #[default]
    Scene, // 对象属性与独立预设预览复用原唯一控件，不再与场景树分割高度。
    Tools,       // 顶部选中模块的创建与工具正文。
    Assets,      // 所选文件属性与唯一预览；界面统一显示在“属性”页签下。
    SceneEntry,         // 所选目录场景的只读属性，编辑继续使用原正式事务。
    Environment, // 顶部环境模块的工具正文；天空作者参数与天气调试独立，气候只读。
    Rigging,     // 当前模型的绑骨与外部动作，占用既有动态工具槽。
}

/// 一个静态页签按钮或页面容器的完整左右身份。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorSidebarTab {
    Navigator(EditorNavigatorTab), // 左侧场景目录/场景树。
    Left(EditorLeftSidebarTab),   // 顶部模块按钮与右侧工具页面共用原模块身份。
    Right(EditorRightSidebarTab), // 右栏按钮和页面共享右页枚举。
}

impl Default for EditorSidebarTab {
    fn default() -> Self {
        Self::Navigator(EditorNavigatorTab::Scenes) // BSN模板身份随后由页面或按钮实参覆盖。
    }
}

/// 兼容既有静态/动态正文的帮助类别，唯一根浮层显露，不参与场景作者数据。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditorSidebarHelp {
    #[default]
    Vegetation, // 植被组规则、覆盖预览和笔刷手势说明。
    Water,          // 静水笔刷或连续水网的上下文说明。
    PlacementWater, // Blender整网格水数值与UV约定说明。
    Assets,         // GLB、植被和KTX2资产用途说明。
    SceneBrowser,     // 当前/所选场景的完整身份及目录操作说明。
    Inspector,      // 对象来源与完整ID，常规属性保持直接可见。
    MeshLods,       // LOD范围、倍率及末层边界的详细解释。
    Placement,      // 摆放与洞口的长操作说明。
    Terrain,        // 塑形、三维雕刻与积水工具的长说明。
}

impl EditorSidebarHelp {
    /// 把固定帮助类别映射到资源中的常数槽位。
    const fn index(self) -> usize {
        match self {
            Self::Vegetation => 0,     // 植被帮助使用第一个独立布尔槽。
            Self::Water => 1,          // 水域帮助使用第二个独立布尔槽。
            Self::PlacementWater => 2, // 网格水帮助使用第三个独立布尔槽。
            Self::Assets => 3,         // 资产格式帮助使用第四个独立布尔槽。
            Self::SceneBrowser => 4,
            Self::Inspector => 5,
            Self::MeshLods => 6,
            Self::Placement => 7,
            Self::Terrain => 8,
        }
    }

    /// 完整说明显示在根浮层标题，正文中的入口统一只用问号。
    pub fn title(self) -> &'static str {
        match self {
            Self::SceneBrowser => "场景目录 · 详情与操作说明",
            Self::Inspector => "对象来源与完整身份",
            _ => "操作说明",
        }
    }
}

/// 侧栏纯会话状态；关闭F12后仍保留，但不写场景文件或settings.json。
#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub struct SceneEditorSidebarState {
    pub navigator: EditorNavigatorTab, // 左侧导航与工具选择互不抢页。
    pub left: EditorLeftSidebarTab,   // 顶部选中的工具模块，切属性时仍记忆。
    pub right: EditorRightSidebarTab, // 当前唯一可见右栏页。
    pub(crate) rigging_available: bool, // 中央活动绑骨或已蒙皮模型覆盖动态工具槽。
    pub inspector: EditorInspectorTarget, // 切换工具后返回属性，恢复最后一次显式检查来源。
    pub observed_tool: EditorTool,    // 仅工具真正变化时执行一次自动定位。
    /// 新建/编辑场景弹框打开前的左栏页；关闭弹框时恢复，避免模态期间误切到地形。
    pub tab_before_new_scene_dialog: Option<EditorLeftSidebarTab>,
    expanded_help: [bool; 9], // 兼容旧帮助请求；同时只允许一个根帮助浮层。
}

impl Default for SceneEditorSidebarState {
    fn default() -> Self {
        Self {
            navigator: EditorNavigatorTab::Scenes, // 首次打开从场景目录开始。
            left: EditorLeftSidebarTab::Meshes,   // 打开工具页时有明确的创建入口。
            right: EditorRightSidebarTab::Scene, // 首次打开显示所选属性或空选择提示。
            rigging_available: false,
            inspector: EditorInspectorTarget::Scene,
            observed_tool: EditorTool::Inspect,  // 默认查看工具不把首次左页自动改成地形。
            tab_before_new_scene_dialog: None,     // 无弹框时不保留恢复目标。
            expanded_help: [false; 9],           // 默认正文完全隐藏，问号不占正文高度。
        }
    }
}

impl SceneEditorSidebarState {
    /// 返回指定左右页签是否为当前活动页。
    pub fn tab_active(&self, tab: EditorSidebarTab) -> bool {
        match tab {
            EditorSidebarTab::Navigator(navigator) => self.navigator == navigator,
            EditorSidebarTab::Left(left) => self.left == left, // 返回属性或资产后仍记忆上一次工具模块。
            EditorSidebarTab::Right(EditorRightSidebarTab::Tools) => {
                if self.rigging_available {
                    self.right == EditorRightSidebarTab::Rigging
                } else {
                    matches!(self.right, EditorRightSidebarTab::Tools | EditorRightSidebarTab::Environment)
                }
            }
            EditorSidebarTab::Right(EditorRightSidebarTab::Scene) => matches!(self.right, EditorRightSidebarTab::Scene | EditorRightSidebarTab::Assets | EditorRightSidebarTab::SceneEntry),
            EditorSidebarTab::Right(right) => self.right == right,
        }
    }

    pub(crate) fn dynamic_tool_page(&self) -> EditorRightSidebarTab {
        if self.rigging_available {
            EditorRightSidebarTab::Rigging
        } else {
            self.left.work_page()
        }
    }

    /// 工具页只有右侧“工具”和顶部模块同时匹配时才可见；按钮仍记忆模块高亮。
    pub fn tool_page_visible(&self, tab: EditorLeftSidebarTab) -> bool {
        self.right == tab.work_page() && self.left == tab
    }

    pub fn scene_browser_visible(&self) -> bool {
        self.navigator == EditorNavigatorTab::Scenes
    }

    pub fn scene_tree_visible(&self) -> bool {
        self.navigator == EditorNavigatorTab::SceneTree
    }

    pub fn assets_browser_visible(&self) -> bool {
        self.navigator == EditorNavigatorTab::Assets
    }

    /// 文件/对象点选显式切换来源；隐藏列表中的旧选择不得持续夺回属性。
    pub fn reveal_asset_properties(&mut self) {
        self.inspector = EditorInspectorTarget::Asset;
        self.right = EditorRightSidebarTab::Assets;
    }

    pub fn reveal_scene_properties(&mut self) {
        self.inspector = EditorInspectorTarget::Scene;
        self.right = EditorRightSidebarTab::Scene;
    }

    pub fn reveal_scene_entry_properties(&mut self) {
        self.inspector = EditorInspectorTarget::SceneEntry;
        self.right = EditorRightSidebarTab::SceneEntry;
    }

    pub fn reveal_properties(&mut self) {
        self.right = match self.inspector {
            EditorInspectorTarget::Scene => EditorRightSidebarTab::Scene,
            EditorInspectorTarget::Asset => EditorRightSidebarTab::Assets,
            EditorInspectorTarget::SceneEntry => EditorRightSidebarTab::SceneEntry,
        };
    }

    pub fn page_visible(&self, tab: EditorSidebarTab) -> bool {
        match tab {
            EditorSidebarTab::Left(tab) => self.tool_page_visible(tab),
            // 属性和工具宿主严格分离；工具页签的活动样式不决定属性正文的可见性。
            EditorSidebarTab::Right(right) => self.right == right,
            _ => self.tab_active(tab),
        }
    }

    /// 返回一类操作说明当前是否展开。
    pub const fn help_expanded(&self, help: EditorSidebarHelp) -> bool {
        self.expanded_help[help.index()] // 固定槽无哈希和动态分配。
    }

    /// 旧工厂只发布帮助来源；唯一根浮层接管原正文，不再就地展开。
    pub fn toggle_help(&mut self, help: EditorSidebarHelp) {
        let index = help.index(); // 一次取得固定槽位避免重复匹配。
        let next = !self.expanded_help[index];
        self.expanded_help = [false; 9];
        self.expanded_help[index] = next;
    }

    pub fn active_help(&self) -> Option<EditorSidebarHelp> {
        [EditorSidebarHelp::Vegetation, EditorSidebarHelp::Water, EditorSidebarHelp::PlacementWater,
            EditorSidebarHelp::Assets, EditorSidebarHelp::SceneBrowser, EditorSidebarHelp::Inspector,
            EditorSidebarHelp::MeshLods, EditorSidebarHelp::Placement, EditorSidebarHelp::Terrain]
            .into_iter().find(|help| self.help_expanded(*help))
    }

    pub fn close_help(&mut self) {
        if self.expanded_help.iter().any(|expanded| *expanded) { self.expanded_help = [false; 9]; }
    }

    /// 打开新建/资料弹框时冻结当前左栏页，关闭时原样恢复。
    pub fn freeze_left_for_new_scene_dialog(&mut self) {
        if self.tab_before_new_scene_dialog.is_none() {
            self.tab_before_new_scene_dialog = Some(self.left); // 只记录首次打开前的模块，嵌套打开不覆盖。
        }
    }

    /// 弹框关闭后恢复打开前的左栏页；无冻结目标时保持现状。
    pub fn restore_left_after_new_scene_dialog(&mut self) {
        if let Some(tab) = self.tab_before_new_scene_dialog.take() {
            self.left = tab; // 取消、保存和创建成功都回到用户原先模块。
        }
    }
}

/// 页签按钮携带目标页，不扩大全局作者动作枚举。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SceneEditorSidebarTabButton(pub EditorSidebarTab);

/// 页签按钮文本由专用Presentation系统更新活动色。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SceneEditorSidebarTabText;

/// 一次创建的页面容器通过Display切换，不在页签交互中增删实体。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SceneEditorSidebarPage(pub EditorSidebarTab);

/// 绝对定位的小问号携带原帮助类别，不占正文高度。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SceneEditorSidebarHelpButton(pub EditorSidebarHelp);

/// 问号文字保持短标签，完整标题放在根帮助浮层。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SceneEditorSidebarHelpButtonText;

/// 原帮助正文默认退出布局，打开时由唯一根浮层借用，关闭后原实体归位。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SceneEditorSidebarHelpBody(pub EditorSidebarHelp);

/// 水域列表存在对象时才显示分页与删除所选操作行。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SceneEditorWaterListActions;

/// 当前真正选中静水体时显示复制与水位属性。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SceneEditorStillWaterProperties;

/// 当前真正选中河流或瀑布时显示控制点与连接属性。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SceneEditorWaterNetworkProperties;

/// 任一有效水域选择存在时显示状态与默认折叠操作说明。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SceneEditorWaterSelectionDetails;

/// 植被列表非空时显示分页与删除所选操作行。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SceneEditorVegetationListActions;

/// 当前存在有效作者层时显示复制、排序、规则和数值属性。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SceneEditorVegetationSelectionProperties;

/// 场景树存在网格或失效绑定选择时才显示完整网格水属性。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SceneEditorPlacementSelectionProperties;

/// 资产页纯分页行只在当前目录确有多页时显示。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SceneEditorAssetPagination;

#[cfg(test)]
mod tests {
    use super::*; // 直接验证默认页与各类详情/帮助互不串扰。

    #[test]
    fn sidebar_defaults_to_scene_scene_and_collapsed_help() {
        let state = SceneEditorSidebarState::default(); // 构造正式首次打开状态。
        assert_eq!(state.navigator, EditorNavigatorTab::Scenes); // 左侧先呈现场景目录。
        assert_eq!(state.left, EditorLeftSidebarTab::Meshes); // 顶部保留网格创建模块。
        assert_eq!(state.right, EditorRightSidebarTab::Scene); // 右侧先呈现对象属性。
        assert!(!state.help_expanded(EditorSidebarHelp::Water)); // 长水域说明默认折叠。
    }

    #[test]
    fn help_requests_keep_one_source_for_the_shared_popup() {
        let mut state = SceneEditorSidebarState::default(); // 从全部折叠开始。
        state.toggle_help(EditorSidebarHelp::Water); // 只展开水域说明。
        assert!(state.help_expanded(EditorSidebarHelp::Water)); // 水域状态已经展开。
        assert!(!state.help_expanded(EditorSidebarHelp::Assets)); // 资产说明不得被串联展开。
        state.toggle_help(EditorSidebarHelp::Assets);
        assert!(!state.help_expanded(EditorSidebarHelp::Water));
        assert_eq!(state.active_help(), Some(EditorSidebarHelp::Assets));
        state.close_help();
        assert!(state.active_help().is_none());
    }

    #[test]
    fn file_and_object_properties_follow_explicit_selection_and_survive_tools() {
        let mut state = SceneEditorSidebarState::default();
        state.navigator = EditorNavigatorTab::Assets;
        state.left = EditorLeftSidebarTab::Environment;
        state.reveal_asset_properties();
        assert!(state.assets_browser_visible());
        assert!(state.tab_active(EditorSidebarTab::Right(EditorRightSidebarTab::Scene)));
        assert!(!state.page_visible(EditorSidebarTab::Right(EditorRightSidebarTab::Scene)));
        assert!(state.page_visible(EditorSidebarTab::Right(EditorRightSidebarTab::Assets)));
        state.right = EditorRightSidebarTab::Environment;
        assert!(state.assets_browser_visible(), "右侧工具不能藏起左侧文件列表");
        state.reveal_properties();
        assert_eq!(state.right, EditorRightSidebarTab::Assets);
        state.reveal_scene_properties();
        assert!(state.assets_browser_visible(), "点击场景对象仍保留资产目录位置");
        state.right = EditorRightSidebarTab::Tools;
        state.reveal_properties();
        assert_eq!(state.right, EditorRightSidebarTab::Scene);
        assert_eq!(state.left, EditorLeftSidebarTab::Environment);
    }

    #[test]
    fn selected_scene_properties_survive_tools_and_yield_to_files_and_objects() {
        let mut state = SceneEditorSidebarState::default();
        state.reveal_scene_entry_properties();
        assert!(state.tab_active(EditorSidebarTab::Right(EditorRightSidebarTab::Scene)));
        assert!(!state.page_visible(EditorSidebarTab::Right(EditorRightSidebarTab::Scene)));
        state.right = EditorRightSidebarTab::Tools;
        state.reveal_properties();
        assert_eq!(state.right, EditorRightSidebarTab::SceneEntry);
        state.reveal_asset_properties();
        state.right = EditorRightSidebarTab::Tools;
        state.reveal_properties();
        assert_eq!(state.right, EditorRightSidebarTab::Assets);
        state.reveal_scene_entry_properties();
        state.reveal_scene_properties();
        state.right = EditorRightSidebarTab::Tools;
        state.reveal_properties();
        assert_eq!(state.right, EditorRightSidebarTab::Scene);
    }

    #[test]
    fn navigator_tool_memory_and_visible_work_page_are_independent() {
        let mut state = SceneEditorSidebarState::default();
        state.left = EditorLeftSidebarTab::Water;
        state.right = EditorRightSidebarTab::Tools;
        assert!(state.tool_page_visible(EditorLeftSidebarTab::Water));
        state.navigator = EditorNavigatorTab::SceneTree;
        assert!(state.scene_tree_visible());
        assert!(state.tool_page_visible(EditorLeftSidebarTab::Water));
        state.right = EditorRightSidebarTab::Scene;
        assert!(!state.tool_page_visible(EditorLeftSidebarTab::Water));
        assert!(state.tab_active(EditorSidebarTab::Left(EditorLeftSidebarTab::Water)));
        assert!(state.scene_tree_visible());
        state.left = EditorLeftSidebarTab::Environment;
        state.right = EditorRightSidebarTab::Environment;
        assert!(state.tab_active(EditorSidebarTab::Right(EditorRightSidebarTab::Environment)));
        assert!(state.tab_active(EditorSidebarTab::Right(EditorRightSidebarTab::Tools)));
        assert!(!state.tab_active(EditorSidebarTab::Right(EditorRightSidebarTab::Scene)));
        assert!(!state.tab_active(EditorSidebarTab::Left(EditorLeftSidebarTab::Water)));
        assert!(!state.page_visible(EditorSidebarTab::Right(EditorRightSidebarTab::Scene)));
        assert!(state.tool_page_visible(EditorLeftSidebarTab::Environment));
        state.right = EditorRightSidebarTab::Scene;
        assert!(state.tab_active(EditorSidebarTab::Left(EditorLeftSidebarTab::Environment)));
        assert!(!state.tool_page_visible(EditorLeftSidebarTab::Environment));
    }
}
