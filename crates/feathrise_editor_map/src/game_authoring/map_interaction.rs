//! 当前外部作者域的地图交互端口；普通地图只比较身份、呈现事实和转交动作，不解释游戏文档。

use bevy::prelude::*;
use std::sync::Arc;

#[cfg(test)]
pub(crate) mod fixtures;

/// 外部工具只冻结自己的小型输入身份；地面查询不解释应用的点放、路线或笔刷结构。
#[derive(Clone)]
pub struct DomainGroundTarget {
    pub domain: &'static str,
    pub identity: String,
    pub continuous: bool,
    pub click_qualified: bool,
    pub payload: Arc<dyn std::any::Any + Send + Sync>,
}

impl std::fmt::Debug for DomainGroundTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DomainGroundTarget").field("domain", &self.domain)
            .field("identity", &self.identity).field("continuous", &self.continuous)
            .field("click_qualified", &self.click_qualified).finish()
    }
}
impl PartialEq for DomainGroundTarget {
    fn eq(&self, other: &Self) -> bool {
        (self.domain, &self.identity, self.continuous, self.click_qualified)
            == (other.domain, &other.identity, other.continuous, other.click_qualified)
    }
}
impl Eq for DomainGroundTarget {}

#[derive(Component, Clone, Default)]
pub struct DomainHorizontalScroll;

/// 现有统一树只需区分作者层与层内对象；具体点位/区域/角色类型由实际游戏解释。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomainItemKind { Layer, Object }

#[derive(Clone, Debug)]
pub struct DomainTreeRow {
    pub id: String,
    pub name: String,
    pub label: String,
    pub hidden: bool,
    pub locked: bool,
    pub actions: u8, // Focus/Rename/Copy/Delete/Visibility/Lock按现有菜单顺序各占一位。
    pub copy_locked: bool,
}

#[derive(Clone, Debug)]
pub struct DomainSelectionInfo {
    pub layer_id: Option<String>,
    pub layer_index: Option<usize>,
    pub visible: bool,
    pub status: String,
}

pub type DomainPick = Arc<dyn Fn(Ray3d, Option<Vec3>, f32) -> Vec<(String, String, f32)> + Send + Sync>;
pub type DomainRow = Arc<dyn Fn(DomainItemKind, usize) -> Option<DomainTreeRow> + Send + Sync>;
pub type DomainSelection = Arc<dyn Fn(DomainItemKind, &str) -> Option<DomainSelectionInfo> + Send + Sync>;
pub type DomainBounds = Arc<dyn Fn(DomainItemKind, &str) -> Option<(Vec3, Vec3)> + Send + Sync>;

/// 四个回调捕获同一已采用不可变文档；命中与聚焦仍只在原后台槽调用，不能帧内扫描整图。
pub struct MapInteractionSnapshot {
    pub map: feathrise_map_content::MapId,
    pub document_domain: &'static str,
    pub domain: &'static str,
    pub label: &'static str,
    pub hash: String,
    pub layer_label: &'static str,
    pub object_label: &'static str,
    pub layer_count: usize,
    pub object_count: usize,
    pub pick: DomainPick,
    pub row: DomainRow,
    pub selection: DomainSelection,
    pub bounds: DomainBounds,
}

#[derive(Clone, Debug)]
pub enum MapInteractionSelection {
    Clear,
    Layer(String),
    Object(String),
}

/// 跨系统阶段的请求都冻结同一完整来源身份，防止不同地图的同名对象和同修订串用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapInteractionIdentity {
    pub map: feathrise_map_content::MapId,
    pub document_domain: &'static str,
    pub hash: String,
    pub generation: u64,
}

impl MapInteractionIdentity {
    pub fn current(&self, draft: &crate::authoring::EditorMapDraft, generation: u64) -> bool {
        self.map == draft.map_id && self.generation == generation
            && draft.domain.as_ref().is_some_and(|domain| domain.domain == self.document_domain && domain.hash() == self.hash)
    }
}

#[derive(Clone, Debug)]
pub struct MapInteractionRequest {
    pub identity: MapInteractionIdentity,
    pub action: MapInteractionSelection,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapInteractionClose {
    pub identity: MapInteractionIdentity,
    pub revision: u64,
}

/// 单个当前扩展槽；未装游戏时保持空值，既不创建角色状态也不显示游戏树行或工具。
#[derive(Resource, Default)]
pub struct MapInteractionState {
    pub enabled: bool,
    pub observed_generation: u64,
    pub snapshot: Option<Arc<MapInteractionSnapshot>>,
    pub selected_layer: Option<String>,
    pub selected: Option<String>,
    pub hover_layer: Option<String>,
    pub suppress_layer_highlight: bool,
    pub text_input_active: bool,
    pub unfinished_shape: bool,
    pub work_pending: bool,
    pub camera_mouse_owned: bool,
    pub precise_cursor: bool,
    pub cursor_target: Option<crate::viewport::vegetation_ground::GroundCursorTarget>,
    pub ground_cursor: Option<Vec3>,
    pub ground_cursor_on_object: bool,
    pub close_completed: Option<MapInteractionClose>,
    pub close_request: Option<MapInteractionClose>,
    pub status: String,
    pub selection_request: Option<MapInteractionRequest>, // 新的明确选择替换旧意图，不累积无界动作队列。
}

impl MapInteractionState {
    fn identity(&self) -> Option<MapInteractionIdentity> {
        let snapshot = self.snapshot.as_ref().filter(|_| self.enabled)?;
        Some(MapInteractionIdentity {
            map: snapshot.map.clone(), document_domain: snapshot.document_domain,
            hash: snapshot.hash.clone(), generation: self.observed_generation,
        })
    }
    pub fn close_completed_for(&self, session: &crate::authoring::EditorAuthoringSession) -> bool {
        session.draft.as_ref().is_some_and(|draft| self.snapshot_for(draft).is_some()
            && self.close_completed.as_ref().is_some_and(|completed| completed.revision == session.revision
                && completed.identity.current(draft, self.observed_generation)))
    }
    /// 收口只接受当前草稿的完整回执；旧回执失效后重新发起当前来源请求。
    pub fn prepare_close(&mut self, session: &crate::authoring::EditorAuthoringSession) -> bool {
        if self.close_completed_for(session) { return true; }
        self.close_completed = None;
        self.close_request = session.draft.as_ref().filter(|draft| self.snapshot_for(draft).is_some())
            .and_then(|_| self.identity()).map(|identity| MapInteractionClose { identity, revision: session.revision });
        false
    }
    pub fn snapshot_for(&self, draft: &crate::authoring::EditorMapDraft) -> Option<&Arc<MapInteractionSnapshot>> {
        let snapshot = self.snapshot.as_ref().filter(|_| self.enabled)?;
        (snapshot.map == draft.map_id && draft.domain.as_ref().is_some_and(|domain|
            domain.domain == snapshot.document_domain && domain.hash() == snapshot.hash)).then_some(snapshot)
    }
    fn request_selection(&mut self, action: MapInteractionSelection) {
        self.selection_request = self.identity().map(|identity| MapInteractionRequest { identity, action });
    }
    pub fn cancel_pick(&mut self) { self.request_selection(MapInteractionSelection::Clear); }
    pub fn domain(&self) -> &'static str { self.snapshot.as_ref().map_or("", |snapshot| snapshot.domain) }
    pub fn label(&self) -> &'static str { self.snapshot.as_ref().map_or("扩展", |snapshot| snapshot.label) }
    pub fn busy(&self) -> bool { self.enabled && self.work_pending }
    pub fn owns_mouse(&self) -> bool { self.enabled && self.camera_mouse_owned }
    pub fn select_layer(&mut self, id: &str) -> bool {
        let Some(info) = self.snapshot.as_ref().and_then(|snapshot| (snapshot.selection)(DomainItemKind::Layer, id)) else { return false; };
        if !info.visible { return false; }
        self.selected_layer = info.layer_id;
        self.selected = None;
        self.hover_layer = None;
        self.status = info.status;
        self.request_selection(MapInteractionSelection::Layer(id.to_owned()));
        true
    }
    pub fn select_object(&mut self, id: &str) -> bool {
        let Some(info) = self.snapshot.as_ref().and_then(|snapshot| (snapshot.selection)(DomainItemKind::Object, id)) else { return false; };
        if !info.visible { return false; }
        self.selected_layer = info.layer_id;
        self.selected = Some(id.to_owned());
        self.hover_layer = None;
        self.status = info.status;
        self.request_selection(MapInteractionSelection::Object(id.to_owned()));
        true
    }
    pub fn selection_info(&self, kind: DomainItemKind, id: &str) -> Option<DomainSelectionInfo> {
        (self.snapshot.as_ref()?.selection)(kind, id)
    }
}

/// 私有域采用作者结果/动作后重新发布事实；统一树在此之后才能继续校验候选。
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MapInteractionAdoption;

/// 私有域的大型列表额外滚动区只需声明此标记，普通滚轮判定不再引用私有UI组件。
#[derive(Component)]
pub struct DomainScrollViewport;
