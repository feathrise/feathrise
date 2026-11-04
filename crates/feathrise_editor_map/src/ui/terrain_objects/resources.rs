//! 当前网格选择与单飞后台事务；集合复制、源文件生成和校验不进入输入帧。
use super::types::Pending;
use bevy::prelude::*;
use feathrise_map_content::{MapId, MapTerrainObject, TerrainId};
use std::sync::Arc;
use super::components::MeshPropertySection;

#[derive(bevy::ecs::system::SystemParam)]
pub(in crate::ui) struct PanelWorkspaceGate<'w> {
    workspace: Option<Res<'w, crate::ui::vegetation_preset_editor::VegetationPresetEditorState>>,
    browser: Option<Res<'w, crate::resources::MapBrowserState>>,
    settings: Option<Res<'w, crate::resources::MapEditorSettingsState>>,
    mouse: Option<Res<'w, crate::resources::MapEditorNumericScrubMouseGuard>>,
    text: Option<Res<'w, crate::ui::text_input::EditorTextInputGuard>>,
}
impl PanelWorkspaceGate<'_> {
    pub(super) fn visible(&self) -> bool {
        self.workspace.as_ref().is_none_or(|state| {
            state.active_tab == crate::ui::vegetation_preset_editor::PresetWorkspaceTab::Map
        }) && self.creation_visible()
    }
    /// 网格工具的创建始终绑定当前主地图，中央文档和预览不改变创建参数或按钮的用途。
    pub(super) fn creation_visible(&self) -> bool {
        self.browser.as_ref().is_none_or(|state| state.dialog.is_none())
            && self.settings.as_ref().is_none_or(|state| !state.open)
    }
    pub(in crate::ui) fn buttons_allowed(&self) -> bool {
        self.visible() && self.creation_buttons_allowed()
    }
    pub(super) fn creation_buttons_allowed(&self) -> bool {
        self.creation_context_valid()
            && self.mouse.as_ref().is_none_or(|state| !state.blocked())
    }
    pub(super) fn creation_context_valid(&self) -> bool {
        self.creation_visible() && self.text.as_ref().is_none_or(|state| !state.block_shortcuts)
    }
}

#[derive(Resource, Default)]
pub(super) struct NameInputBinding {
    /// 文本框只能提交给最初绑定的地图与对象，切换列表行后旧中文缓冲不串写。
    pub target: Option<(MapId, TerrainId)>,
    pub pending_focus: Option<(Entity, MapId, TerrainId)>,
    pub observed_name: String,
}
#[derive(Resource)]
pub(crate) struct TerrainObjectsEditorState {
    pub map: Option<MapId>,
    pub(super) legacy: Option<Arc<MapTerrainObject>>,
    pub(super) observed_revision: Option<u64>,
    pub selected: Option<TerrainId>,
    pub selected_index: Option<usize>,
    /// 当前滚动窗口首行；仅用于固定行池换绑，不再有分页状态。
    pub list_start: usize,
    /// 最后聚焦的LOD组，仅决定需要揭示哪组选层，不参与字段或动作的用途路由。
    pub display_lods: bool,
    /// 显示与碰撞分别维护连续窗口和真实选层，不共享可见行绑定。
    pub(super) lods: [LodListState; 2],
    /// 添加区和所选属性是两套常驻控件，创建规格不会因场景选择改变语义。
    pub creation: [f32; 8],
    pub(crate) creation_open: bool,
    pub(crate) creation_source: super::types::CreationSource,
    pub(crate) placement_mode: super::types::CreationPlacementMode,
    /// 尺寸XYZ、sRGB三色和粗糙度；定点单位在请求边界归一为毫米与千分比。
    pub(super) cuboid_creation: [f32; 7],
    pub(super) cuboid_collision: bool,
    pub(super) point_placement: Option<super::creation_placement::PointPlacement>,
    pub(super) point_query: Option<super::creation_placement::PointQuery>,
    pub(super) placement_mouse_capture: bool,
    pub(super) placement_ticket: u64,
    pub(super) start_requested: bool,
    pub(super) cancel_requested: bool,
    /// 属性按参考图纵向并列，折叠互不排斥；不再用互斥页签隐藏其他参数。
    pub(super) folded_sections: [bool; 6],
    pub(super) pending: Option<Pending>,
    pub(super) stitch_preview:Option<super::types::StitchPreview>,
    pub(super) stitch_apply:bool,
    pub(super) stitch_cancel:bool,
    pub(super) stitch_release:Option<bevy::tasks::Task<()>>,
    pub(crate) transforming: bool,
    pub(crate) brushing: bool,
    pub(crate) documents_loading: bool,
    pub(super) brush_history: Option<(u64, usize)>,
    pub locked: std::collections::BTreeSet<TerrainId>,
    /// 编辑器显隐仅影响查看与拾取，与持久 enabled 的地图参与状态分开。
    pub hidden: std::collections::BTreeSet<TerrainId>,
}
impl Default for TerrainObjectsEditorState {
    fn default() -> Self {
        Self {
            map: None,
            legacy: None,
            observed_revision: None,
            selected: None,
            selected_index: None,
            list_start: 0,
            display_lods: true,
            lods: Default::default(),
            creation: [0.0, 0.0, 0.0, 64.0, 16.0, 64.0, 0.0, 0.25],
            creation_open: false,
            creation_source: Default::default(),
            placement_mode: Default::default(),
            cuboid_creation: [2.0, 2.0, 2.0, 160.0, 160.0, 160.0, 0.7],
            cuboid_collision: true,
            point_placement: None,
            point_query: None,
            placement_mouse_capture: false,
            placement_ticket: 0,
            start_requested: false,
            cancel_requested: false,
            folded_sections: [false, false, false, false, true, true],
            pending: None,
            stitch_preview:None,
            stitch_apply:false,
            stitch_cancel:false,
            stitch_release:None,
            transforming: false,
            brushing: false,
            documents_loading: false,
            brush_history: None,
            locked: Default::default(),
            hidden: Default::default(),
        }
    }
}
#[derive(Clone, Copy, Default)]
pub(super) struct LodListState {
    pub start: usize,
    pub selected: usize,
}
impl TerrainObjectsEditorState {
    pub(crate) fn owns_mouse(&self) -> bool {
        self.transforming || self.placement_mouse_capture
    }
    pub(super) fn section_open(&self, section: MeshPropertySection) -> bool {
        !self.folded_sections[section as usize]
    }
    pub(super) fn set_section_open(&mut self, section: MeshPropertySection, open: bool) {
        self.folded_sections[section as usize] = !open;
    }
    pub(super) fn lod_section(display: bool) -> MeshPropertySection {
        if display { MeshPropertySection::DisplayLods } else { MeshPropertySection::CollisionLods }
    }
    pub(super) fn lod(&self, display: bool) -> &LodListState {
        &self.lods[usize::from(display)]
    }
    pub(super) fn lod_mut(&mut self, display: bool) -> &mut LodListState {
        &mut self.lods[usize::from(display)]
    }
    /// 视口与场景列表切换网格时共用重置，不能带着上一网格的连续窗口和层号打开属性。
    pub(crate) fn reset_lod_selection(&mut self) {
        self.lods = Default::default();
    }
    pub(crate) fn preparing(&self) -> bool {
        self.pending.is_some()||self.stitch_preview.is_some()||self.stitch_release.is_some()
    }
    pub(crate) fn busy(&self) -> bool {
        self.preparing() || self.transforming || self.brushing || self.documents_loading
    }
}
