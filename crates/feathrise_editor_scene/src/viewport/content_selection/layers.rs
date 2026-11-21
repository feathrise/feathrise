//! 跨类型作者层选择：一次在途后台查询、稳定身份、同点轮选及不写草稿的临时预高亮。

use super::super::modes::SelectionFilter;
use bevy::{prelude::*, tasks::Task}; // 会话数据与非阻塞任务句柄。
use feathrise_scene_content::SceneId; // 防止跨场景同名层串选。 // 与视口筛选按钮共用唯一类型。

#[derive(Clone, Debug, PartialEq, Eq)] // 作者ID而非预设ID构成单选身份。
pub(crate) enum LayerTarget {
    TerrainObject(String), // 独立地形与普通摆放、子场景使用不同稳定身份。
    Subscene(String), // 父图只选完整引用，内部成员保持只读。
    Portal(String),     // 传送点以作者稳定ID选择，绝不触发传送。
    Boundary(String),   // 边界选择整条路径而非开始拖动控制点。
    Placement(String),  // 单独摆放资产以整实例稳定ID参与统一轮选。
    Surface(String),    // 地表作者层。
    Vegetation(String), // 植被独立作者画布。
    Domain(String),  // 角色作者层。
    Water(crate::types::WaterAuthoringFeatureKind, String), // 水域类别与稳定作者ID共同构成身份。
    RootWater(crate::types::WaterAuthoringFeatureKind, String), // 根水候选始终冻结根来源；正式采用恢复根槽后转为原Water身份。
}

#[derive(Clone, Debug)] // 完整后台候选不会携带ECS借用。
pub(super) struct LayerCandidate {
    pub author_index: Option<usize>, // 后台解析的作者数组位置，覆盖侧常数访问。
    pub target: LayerTarget,         // 点击与预高亮共用稳定身份。
    pub label: String,               // 类型前缀与作者名称。
    pub rank: u8,                    // 仅同一深度时区分覆盖层：可见草、地表、承载网格。
    pub distance: f32,               // 真实射线深度优先，不能让类型优先级穿过前景。
}

#[derive(Clone, Debug, PartialEq)] // 每次请求及草稿换代都必须重新验证。
pub(super) struct Identity {
    pub scene_id: SceneId,              // 草稿场景。
    pub revision: u64,           // 联合作者修订。
    pub placement_derived_pose_revision: u64, // 同一作者修订内迟到的贴地仍会改变命中与距离顺序。
    pub domain_hash: String,  // 角色版本可独立于普通图层修订变化。
    pub generation: u64,         // 同图重进隔离。
    pub ticket: u64,             // 后来的点击淘汰旧结果。
    pub filter: SelectionFilter, // 切筛选不采用旧类型列表。
}

#[derive(Clone)] // 至多合并一项最新点击，无任务队列增长。
pub(super) struct Request {
    pub canvas_terrain: Option<feathrise_scene_content::TerrainId>, // 本次作者快照所属画布，不能借用另一网格同名层。
    pub canvas_definition: Option<std::sync::Arc<feathrise_scene_content::SceneTerrainObject>>, // 与候选同批冻结完整来源，换槽后不得借同ID的新定义续接。
    pub water_canvas_definition: Option<std::sync::Arc<feathrise_scene_content::SceneTerrainObject>>, // 水在输入时采集，可能早于所点地表画布恢复，来源不得随后台重采样改派。
    pub hit_terrain: Option<(feathrise_scene_content::TerrainId, usize)>, // 后台真实最近网格，驱动同次点击载入作者画布。
    pub terrain_ground: Option<std::sync::Arc<crate::authoring::terrain_ground::TerrainGroundSnapshot>>,
    pub terrain_objects: std::sync::Arc<[feathrise_scene_content::SceneTerrainObject]>,
    pub hidden_terrains: std::sync::Arc<std::collections::BTreeSet<feathrise_scene_content::TerrainId>>,
    pub subscenes: super::subscenes::Capture,
    pub additive: bool, // Shift在按下时冻结；异步结果不能读取松手后的修饰键。
    pub gameplay: super::gameplay::Snapshot, // 有界分帧冻结后交付同一个查询worker。
    pub identity: Identity,                  // 点击当帧冻结场景和代次，排队不能迁移到新图。
    pub ray: Ray3d,                          // 点击相机姿态已经冻结。
    pub world: Option<Vec3>,                 // 地形实际三角形命中。
    pub pixel: Vec2,                         // 同位置轮选使用RTT像素。
    pub radius: f32,                         // 角色标记的屏幕点击容差换算世界值。
    pub preferred: Option<u64>,              // 逐株Mesh射线强命中。
    pub vegetation_world: Option<Vec2>,      // 冠层命中以实际Mesh的XZ采样，避免斜视落到树后。
    pub preferred_distance: Option<f32>,     // 真实Mesh射线距离，与角色近远一致比较。
    pub water_candidates: Vec<LayerCandidate>, // 本次点击的全部可见水体，异步合并而不抢占其他图层。
    pub placement_candidates: Vec<LayerCandidate>, // 同一射线全部真实命中的独立资产。
    pub placement_hits: std::collections::HashMap<String, super::placement::PlacementViewportHit>, // 保留每实例最近网格供场景树同步。
    pub ticket: u64, // 请求替换淘汰旧任务。
}

pub(super) struct CanvasWait {
    pub request: Request,
    pub observed_loading: bool,
}

#[derive(Resource, Default)] // 只维护会话态，不进入文件或撤销历史。
pub struct LayerCandidateState {
    pub(crate) additive: bool, // 与完整候选同票据采用，候选列表确认也沿用这次增减语义。
    pub(super) placement_hits:
        std::collections::HashMap<String, super::placement::PlacementViewportHit>, // 与候选结果原子采用的精确网格身份。
    pub(super) selected_placement_hit: Option<super::placement::PlacementViewportHit>, // 正式确认只保留一项；新候选采用不能提前覆盖原子网格。
    pub(super) confirmed_viewport_selection: bool, // 正式来源独立于临时列表；派生失效后整实例选择不能降为场景树叶片。
    pub(crate) selected: Option<LayerTarget>, // 正式选中的作者层。
    pub(crate) selection_revision: u64, // 明确点击确认才递增，区分同一对象再次选择与稳定帧/悬停。
    pub(crate) hover: Option<LayerTarget>,    // 候选悬停仅覆盖视觉，不改正式选择。
    pub(crate) pointer_over_ui: bool,         // 点击候选控件不穿透视口。
    pub(crate) selected_author_index: Option<usize>, // 与正式选中ID一同发布。
    pub(crate) hover_author_index: Option<usize>, // 悬停不修改正式下标。
    pub(super) request: Option<Request>,      // single-flight忙时保留最后一次输入。
    pub(super) task: Option<Task<(Identity, Request, Vec<LayerCandidate>)>>, // 一个实际在途worker。
    pub(super) task_identity: Option<Identity>, // 只允许正式画布采用显式转接修订，其他作者换代仍拒收。
    pub(super) waiting_canvas: Option<CanvasWait>, // 同次点击等待最近网格的完整画布，不要求再点一次。
    pub(super) canvas_terrain: Option<feathrise_scene_content::TerrainId>, // 当前正式选择的作者画布归属。
    pub(super) candidate_canvas: Option<std::sync::Arc<feathrise_scene_content::SceneTerrainObject>>, // 同批普通作者候选的原画布；RootWater另属根，切源不改写本字段。
    pub(super) candidate_water_canvas: Option<std::sync::Arc<feathrise_scene_content::SceneTerrainObject>>, // 普通Water沿用输入时的私有源，不借地表查询后来采用的另一画布。
    pub(super) identity: Option<Identity>,    // 当前完整候选的身份。
    pub(super) candidates: Vec<LayerCandidate>, // 完整结果，UI只展示一页。
    pub(super) pixel: Option<Vec2>,           // 最近有效点击，用于同点轮选。
    pub(super) index: usize,                  // 当前候选序号。
    pub(super) page: usize,                   // 重叠很多层时固定八行分页。
    pub(super) ticket: u64,                   // 取消/新请求都递增。
    pub(super) ui_revision: u64,              // 只在列表内容变化时重建固定小树。
    pub(super) applied_ui_revision: u64,      // 显示侧签名缓存。
    pub(super) apply: Option<LayerTarget>,    // 查询和按钮共用一个正式选层出口。
    pub(super) apply_candidate_index: Option<usize>, // 待确认行与正式下标分离；拒绝切源时保留原目标、下标和选中行。
    pub(crate) clear_objects: bool,           // 空命中完成后才清除旧对象选择，等待期间保持画面。
}

impl LayerCandidateState {
    /// 实际外部消费者只需要判断所选类别，不开放普通场景候选、网格和水域内部格式。
    pub fn is_domain_selected(&self) -> bool { matches!(self.selected, Some(LayerTarget::Domain(_))) }
    /// 视口水位预览只续接自己的数值修订，真实换选票据保持不变，迟到候选仍由旧修订拒收。
    pub(crate) fn continue_water_level_revision(&mut self,
        session: &crate::authoring::EditorAuthoringSession, id: &str,
        before_revision: u64, selection_revision: u64) {
        let Some(draft) = session.draft.as_ref() else { return; };
        if self.selection_revision != selection_revision || !self.author_canvas_matches(draft)
            || !matches!(self.selected.as_ref(), Some(LayerTarget::Water(crate::types::WaterAuthoringFeatureKind::StillWater, selected)) if selected == id) {
            return;
        }
        if let Some(identity) = self.identity.as_mut().filter(|identity|
            identity.scene_id == draft.scene_id && identity.revision == before_revision
                && identity.domain_hash == draft.adopted_domain_hash()) {
            identity.revision = session.revision;
        }
    }

    /// 作者事务换代后保留正式类别；复制和删除采用写侧已经确认的新下标，不让属性栏变空。
    pub(crate) fn rebind_author_revision(&mut self, editor: &crate::resources::SceneEditorState,
        session: &crate::authoring::EditorAuthoringSession,
        domain: Option<&crate::game_authoring::scene_interaction::SceneInteractionState>, filter: SelectionFilter) {
        let Some(draft) = session.draft.as_ref() else { return; };
        if editor.selected_scene_id.as_ref() != Some(&draft.scene_id) || editor.close_requested || editor.scene_transition_in_progress { return; }
        let generation = domain.map_or(0, |state| state.observed_generation);
        let rebind = self.identity.as_ref().is_some_and(|old| old.scene_id == draft.scene_id && old.generation == generation && old.filter == filter
            && (old.revision != session.revision || old.domain_hash != draft.adopted_domain_hash()));
        if !rebind { return; } // 切换场景、玩法世代或筛选变化仍由原轮询完整取消。
        let next = match self.selected.as_ref() {
            Some(LayerTarget::Surface(_)) if self.author_canvas_matches(draft) => draft.surface.as_ref().and_then(|surface| {
                let index = editor.selected_surface_layer;
                surface.layers.get(index).map(|layer| (LayerTarget::Surface(layer.id.clone()), Some(index)))
            }),
            Some(LayerTarget::Portal(id)) => self.selected_author_index.and_then(|index| {
                draft.portals.portals.get(index).filter(|portal| portal.id == *id
                    && editor.selected_current_portal_id.as_ref() == Some(id))
                    .map(|portal| (LayerTarget::Portal(portal.id.clone()), Some(index)))
            }),
            Some(LayerTarget::Boundary(id)) => self.selected_author_index.and_then(|index| {
                draft.boundaries.as_ref()?.paths.get(index).filter(|path| path.id == *id
                    && session.selected_boundary_path.as_ref().is_some_and(|(map, selected)| map == &draft.scene_id && selected == id))
                    .map(|path| (LayerTarget::Boundary(path.id.clone()), Some(index)))
            }), // 属性提交只续接正式作者目标；改名和数值提交不能让原表单消失。
            Some(LayerTarget::Water(..)) if !self.author_canvas_matches(draft) => None, // 撤销恢复其他画布后，旧索引不能把属性目标续接成另一来源的同名水体。
            Some(LayerTarget::Water(kind, _)) => draft.water.as_ref().and_then(|water| {
                use crate::types::WaterAuthoringFeatureKind::*;
                let (id, index) = match kind {
                    StillWater => (water.layers.get(editor.selected_water_layer).map(|item| &item.id), editor.selected_water_layer),
                    River => (water.rivers.get(editor.selected_water_river).map(|item| &item.id), editor.selected_water_river),
                    Waterfall => (water.waterfalls.get(editor.selected_waterfall).map(|item| &item.id), editor.selected_waterfall),
                };
                id.map(|id| (LayerTarget::Water(*kind, id.clone()), Some(index)))
            }),
            Some(LayerTarget::Vegetation(_)) => draft.vegetation.as_ref().and_then(|vegetation| {
                let index = editor.selected_vegetation_layer;
                let id = vegetation.authoring.as_ref().map_or_else(|| vegetation.layers.get(index).map(|item| &item.id),
                    |authoring| authoring.layers.get(index).map(|item| &item.id));
                id.map(|id| (LayerTarget::Vegetation(id.clone()), Some(index)))
            }),
            Some(LayerTarget::Domain(_)) => domain.filter(|state| state.snapshot_for(draft).is_some()).and_then(|state| {
                if let Some(id) = state.selected_layer.as_ref() {
                    state.selection_info(crate::game_authoring::scene_interaction::DomainItemKind::Layer, id)
                        .filter(|info| info.visible).map(|info| (LayerTarget::Domain(id.clone()), info.layer_index))
                } else { state.selected.as_ref().and_then(|id| state.selection_info(crate::game_authoring::scene_interaction::DomainItemKind::Object, id)
                    .filter(|info| info.visible).map(|_| (LayerTarget::Domain(id.clone()), None))) }
            }),
            _ => return,
        };
        if let Some((target, index)) = next { self.confirm_tree_author(target, index, session, generation, filter); }
        else { self.clear_for_scene_tree(); }
    }
    /// 场景树直选作者对象时同步完整身份，旧视口查询不得覆盖新的属性目标。
    pub(crate) fn confirm_tree_author(&mut self, target: LayerTarget, index: Option<usize>,
        session: &crate::authoring::EditorAuthoringSession, generation: u64, filter: SelectionFilter) {
        self.clear();
        let Some(draft) = session.draft.as_ref() else { return; };
        self.identity = Some(Identity { scene_id: draft.scene_id.clone(), revision: session.revision,
            placement_derived_pose_revision: session.placement_derived_pose_revision,
            domain_hash: draft.adopted_domain_hash().to_owned(), generation, ticket: self.ticket, filter });
        self.selected = Some(target); self.selected_author_index = index;
        self.canvas_terrain = draft.terrain_documents.active.as_ref().map(|active| active.definition.id.clone());
        self.selection_revision = self.selection_revision.wrapping_add(1);
    }

    /// 只有同批候选的成功换槽可以续接修订；旧worker保留实际槽位，但永久失去采用资格。
    pub(super) fn continue_candidate_source(&mut self,
        session: &crate::authoring::EditorAuthoringSession) {
        let Some(draft) = session.draft.as_ref() else { return; };
        self.ticket = self.ticket.wrapping_add(1);
        self.request = None;
        self.waiting_canvas = None;
        self.task_identity = None;
        self.hover = None;
        self.hover_author_index = None;
        if let Some(identity) = self.identity.as_mut() {
            identity.revision = session.revision;
            identity.ticket = self.ticket;
        }
        self.canvas_terrain = draft.terrain_documents.active.as_ref().map(|active| active.definition.id.clone());
    } // 列表、原来源、命中点、精确资产命中和分页均属于原点击，不能随换槽被clear抹掉。
    pub(crate) fn confirm_terrain(&mut self, id: &feathrise_scene_content::TerrainId, index: usize) {
        self.clear(); self.selected = Some(LayerTarget::TerrainObject(id.as_str().to_owned()));
        self.selected_author_index = Some(index); self.selection_revision = self.selection_revision.wrapping_add(1);
    }
    /// 画布载入换代只淘汰未确认点击与旧候选，保留同一正式网格；后台载入不是新的选择确认。
    pub(crate) fn complete_terrain_canvas(&mut self, id: &feathrise_scene_content::TerrainId, index: usize,
        before_revision: u64, revision: u64) {
        // 只有作者画布成功采用的这条显式修订边可以续接原点击；编辑、切换场景、树选择仍通过票据失效。
        if let Some(request) = self.request.as_mut().filter(|request|
            request.ticket == self.ticket && request.identity.revision == before_revision) {
            request.identity.revision = revision;
        }
        if let Some(identity) = self.task_identity.as_mut().filter(|identity|
            identity.ticket == self.ticket && identity.revision == before_revision) {
            identity.revision = revision;
        }
        if self.waiting_canvas.as_ref().is_some_and(|wait| wait.request.ticket == self.ticket
            && wait.request.identity.revision == before_revision
            && wait.request.hit_terrain.as_ref().is_some_and(|(target, at)| target == id && *at == index)) {
            let mut request = self.waiting_canvas.take().unwrap().request;
            if let Some(identity) = self.identity.as_mut().filter(|identity|
                identity.scene_id == request.identity.scene_id && identity.revision == before_revision
                    && identity.domain_hash == request.identity.domain_hash
                    && identity.generation == request.identity.generation && identity.filter == request.identity.filter) {
                identity.revision = revision;
            } // 同点新点击为重采样恢复原私有槽时，保留上一完整列表才能比较顺序并继续轮选。
            request.identity.revision = revision;
            self.request = Some(request); // 保留原射线、落点和筛选，重新捕获刚完整采用的作者快照。
            return;
        }
        if self.request.is_some() || self.task_identity.as_ref().is_some_and(|key| key.ticket == self.ticket) {
            if matches!(self.selected.as_ref(), Some(LayerTarget::Portal(_) | LayerTarget::Boundary(_)
                | LayerTarget::Placement(_) | LayerTarget::Domain(_) | LayerTarget::Subscene(_)))
                && let Some(identity) = self.identity.as_mut().filter(|key| key.revision == before_revision) {
                identity.revision = revision;
            } else { self.identity = None; }
            // 场景级正式目标只续接作者修订，旧点击失效后表单仍有身份；派生代次留给原失效门核对。
            // 私有作者层不借此分支续接，避免跨画布的同名水、地表或植被获得编辑权限。
            self.candidates.clear();
            self.placement_hits.clear();
            self.candidate_canvas = None;
            self.candidate_water_canvas = None;
            self.hover = None;
            self.hover_author_index = None;
            self.pixel = None;
            self.apply = None;
            self.apply_candidate_index = None;
            self.clear_objects = false;
            self.index = 0;
            self.page = 0;
            self.pointer_over_ui = false;
            self.ui_revision = self.ui_revision.wrapping_add(1);
            // 仅淘汰旧候选表现与待采用副作用；不能用freeze换票据取消合法续接，或重盖旧请求的派生姿态。
            // 正式selected、独立精确命中与视口来源仍归上一确认所有，在途Task继续占用唯一槽。
            return; // 新单击仍在查询时，旧画布完成不得取消它；结果必须再次核对实际作者画布。
        }
        if !matches!(self.selected.as_ref(), Some(LayerTarget::TerrainObject(selected)) if selected == id.as_str()) {
            if matches!(self.selected, Some(LayerTarget::Surface(_) | LayerTarget::Vegetation(_) | LayerTarget::Water(..))) {
                self.clear(); // 非本次点击触发的换槽，不能把旧网格同名草、表层或水域留在新画布上。
            } else if let Some(identity) = self.identity.as_mut().filter(|key| key.revision == before_revision) {
                identity.revision = revision; // 场景级角色/资产的明确选择不被无关画布完成抢走。
                self.candidates.clear();
                self.ui_revision = self.ui_revision.wrapping_add(1);
            }
            return; // 用户已经改选其他对象时，旧网格画布不能抢回属性栏。
        }
        self.rebind_terrain_canvas(id, index);
    }
    /// 雕刻等源换代不具备继续旧点击的权限，保留原先的严格失效语义。
    pub(crate) fn rebind_terrain_canvas(&mut self, id: &feathrise_scene_content::TerrainId, index: usize) {
        if !matches!(self.selected.as_ref(), Some(LayerTarget::TerrainObject(selected)) if selected == id.as_str()) { return; }
        let selected = self.selected.take();
        self.clear(); // 旧查询继续占用原单槽，完成后按新票据拒收，不能重新清空选择。
        self.selected = selected;
        self.selected_author_index = Some(index);
    }
    pub(crate) fn confirm_subscene(&mut self, id: &feathrise_scene_content::SceneInstanceId,
        session: &crate::authoring::EditorAuthoringSession, generation: u64, filter: SelectionFilter) {
        self.clear(); // 目录点击也换票据，迟到的空白点击不能取消刚确认的子场景。
        let Some(draft) = session.draft.as_ref().filter(|draft| draft.manifest.subscenes.iter().any(|record| &record.id == id)) else { return; };
        self.identity = Some(Identity { scene_id: draft.scene_id.clone(), revision: session.revision,
            placement_derived_pose_revision: session.placement_derived_pose_revision,
            domain_hash: draft.adopted_domain_hash().to_owned(), generation, ticket: self.ticket, filter });
        self.apply = Some(LayerTarget::Subscene(id.to_string()));
        self.additive = false; self.selected_author_index = None;
    }
    pub(crate) fn restore_subscene(&mut self, id: &feathrise_scene_content::SceneInstanceId,
        session: &crate::authoring::EditorAuthoringSession, generation: u64, filter: SelectionFilter) {
        self.confirm_subscene(id, session, generation, filter);
        self.selected = self.apply.take(); // 文档恢复是已确认选择，移动/旋转模式的输入锁不应丢弃它。
        self.selection_revision = self.selection_revision.wrapping_add(1);
    }
    pub(crate) fn portal_index(&self, id: &str) -> Option<usize> {
        self.author_index(&LayerTarget::Portal(id.to_owned())) // 列表悬停按后台缓存常数访问。
    }
    pub(crate) fn has_viewport_candidates(&self) -> bool {
        !self.candidates.is_empty()
    }
    /// 候选预览和已确认的视口选择共用整实例语义，列表失效不改变正式来源。
    pub(crate) fn has_viewport_selection(&self) -> bool {
        self.has_viewport_candidates() || self.confirmed_viewport_selection
    }
    pub(crate) fn clear_for_scene_tree(&mut self) {
        self.clear(); // 明确的场景树点击即使身份相同也接管选择。
    }
    pub(crate) fn placement_mesh_selection(
        &self,
        id: &str,
    ) -> Option<&crate::ui::gltf_browser::EditorGltfMeshSelection> {
        let confirmed = self.selected_placement_hit.as_ref().filter(|hit| {
            matches!(self.selected.as_ref(), Some(LayerTarget::Placement(selected)) if selected == id)
                && !matches!(self.hover.as_ref(), Some(LayerTarget::Placement(hover)) if hover == id)
                && match hit {
                    super::placement::PlacementViewportHit::SceneMesh(selection) => selection.placement_id == id,
                    super::placement::PlacementViewportHit::Primitive { placement_id, .. } => placement_id == id,
                }
        }); // 悬停读取本次候选；正式选择读取独立确认值，不能借同ID的新子网格覆盖原路径。
        match confirmed.or_else(|| self.placement_hits.get(id))? {
            super::placement::PlacementViewportHit::SceneMesh(selection) => Some(selection), // 悬停借用同一完整结果的网格身份。
            super::placement::PlacementViewportHit::Primitive { .. } => None, // 子资产不能伪造Scene0节点路径。
        }
    }
    pub(crate) fn surface_index(&self, id: &str) -> Option<usize> {
        // 覆盖不用扫描完整层表。
        self.author_index(&LayerTarget::Surface(id.to_owned())) // 同一稳定ID校验。
    }
    pub(crate) fn vegetation_index(&self, id: &str) -> Option<usize> {
        // 植被覆盖复用后台结果。
        self.author_index(&LayerTarget::Vegetation(id.to_owned())) // 不按预设合并。
    }
    pub(crate) fn author_canvas_matches(&self, draft: &crate::authoring::EditorSceneDraft) -> bool {
        self.canvas_terrain.as_ref() == draft.terrain_documents.active.as_ref().map(|active| &active.definition.id)
    }
    pub(crate) fn selected_matches_session(&self, session: &crate::authoring::EditorAuthoringSession) -> bool {
        session.draft.as_ref().is_some_and(|draft| self.identity.as_ref().is_some_and(|identity|
            identity.scene_id == draft.scene_id && identity.revision == session.revision
                && identity.domain_hash == draft.adopted_domain_hash()))
    } // 检查器移挂原表单也必须拒绝旧场景或旧作者修订，不能只凭相同ID显示输入。
    fn author_index(&self, target: &LayerTarget) -> Option<usize> {
        // 两个常数候选缓存。
        if self.hover.as_ref() == Some(target) {
            self.hover_author_index
        } else if self.selected.as_ref() == Some(target) {
            self.selected_author_index
        } else {
            None
        } // 非当前目标不得复用旧索引。
    }
    pub(super) fn request(
        &mut self,
        mut identity: Identity,
        ray: Ray3d,
        world: Option<Vec3>,
        pixel: Vec2,
        _size: Vec2, // 侧栏卡片不再依赖视口尺寸。
        radius: f32,
        preferred: Option<(u64, Vec3, f32)>,
    ) {
        // 输入只记录常数大小快照。
        self.ticket = self.ticket.wrapping_add(1); // 旧在途查询保留槽位但失去采用资格。
        identity.ticket = self.ticket; // 同一请求使用唯一票据。
        self.hover = None; // 新点击不继承候选悬停。
        self.waiting_canvas = None; // 新点击永久取消旧画布等待，不借后续载入重新获权。
        self.request = Some(Request {
            canvas_terrain: None, canvas_definition: None, water_canvas_definition: None, hit_terrain: None,
            terrain_ground: None, terrain_objects: Default::default(), hidden_terrains: Default::default(),
            subscenes: Default::default(),
            additive: false,
            gameplay: Default::default(), // 只冻结本次票据的玩法作者数据。
            identity,                     // 启动后台之前再核对。
            ray,
            world,
            pixel,
            radius,
            preferred: preferred.map(|hit| hit.0),
            vegetation_world: preferred
                .map(|hit| Vec2::new(hit.1.x, hit.1.z))
                .or(world.map(|p| Vec2::new(p.x, p.z))),
            preferred_distance: preferred.map(|hit| hit.2),
            water_candidates: Vec::new(), // 调用侧完成当前可见水面采样后一次性交付。
            placement_candidates: Vec::new(), // 调用侧仅在选择模式交付完整资产命中。
            placement_hits: Default::default(), // 不继承上次点击的精确网格。
            ticket: self.ticket,
        }); // 不克隆场景。
    }

    /// 进入变换时只取消未确认的点击/悬停；保留正式目标、作者下标和精确网格缓存。
    pub(crate) fn freeze_selection(&mut self) {
        self.ticket = self.ticket.wrapping_add(1);
        if let Some(identity) = self.identity.as_mut() {
            identity.ticket = self.ticket;
        } // 回到选择后旧worker仍无采用资格，已确认的选择不用重建或重新猜测。
        self.request = None;
        self.waiting_canvas = None;
        self.task_identity = None;
        self.apply = None;
        self.apply_candidate_index = None;
        self.clear_objects = false;
        self.hover = None;
        self.hover_author_index = None;
        self.pointer_over_ui = false;
        self.pixel = None;
    } // 不丢弃在途句柄或遍历候选/网格缓存；原单槽继续非阻塞回收完成结果。

    /// 仅派生姿态换代淘汰命中缓存，正式选择及其唯一精确网格仍归原确认所有。
    pub(super) fn invalidate_derived_pose(
        &mut self,
        session: &crate::authoring::EditorAuthoringSession,
        generation: u64,
        filter: SelectionFilter,
    ) {
        let Some(draft) = session.draft.as_ref() else { return; };
        let same_context = |identity: &Identity| identity.scene_id == draft.scene_id
            && identity.revision == session.revision
            && identity.domain_hash == draft.adopted_domain_hash()
            && identity.generation == generation && identity.filter == filter;
        if !self.identity.as_ref().is_some_and(|identity| same_context(identity)
            && identity.placement_derived_pose_revision != session.placement_derived_pose_revision) {
            return;
        }
        let pending = self.request.take().filter(|request| same_context(&request.identity)
            && request.ticket == self.ticket && request.identity.ticket == self.ticket
            && request.identity.placement_derived_pose_revision == session.placement_derived_pose_revision);
        let explicit_apply = self.apply_candidate_index.is_none().then(|| self.apply.take()).flatten();
        self.freeze_selection(); // 在途Task仍占原槽；旧候选、空命中和后台结果一并失去采用票据。
        self.apply = explicit_apply; // 场景树等无候选行的明确目标不依赖旧距离快照。
        self.candidates.clear();
        self.placement_hits.clear(); // 候选命中表可以全部失效，正式精确身份由独立单项保留。
        self.candidate_canvas = None;
        self.candidate_water_canvas = None;
        self.index = 0;
        self.page = 0;
        if let Some(identity) = self.identity.as_mut() {
            identity.placement_derived_pose_revision = session.placement_derived_pose_revision;
        }
        self.ui_revision = self.ui_revision.wrapping_add(1);
        if let Some(mut request) = pending {
            request.ticket = self.ticket;
            request.identity.ticket = self.ticket;
            self.request = Some(request);
        } // 同帧在新姿态上发生的点击继续使用自己的完整输入，只换取消后的票据。
    }

    pub(super) fn clear(&mut self) {
        // 不丢弃运行中的CPU任务句柄。
        self.ticket = self.ticket.wrapping_add(1); // 取消保证迟到结果无副作用。
        self.request = None; // 清理未启动输入。
        self.waiting_canvas = None;
        self.task_identity = None;
        self.canvas_terrain = None;
        self.candidate_canvas = None;
        self.candidate_water_canvas = None;
        self.identity = None; // 相同名字也不能跨代继承。
        self.selected = None; // 退出模式清理覆盖所有权。
        self.hover = None; // 恢复正常模块覆盖。
        self.apply = None; // 取消未发布点击。
        self.apply_candidate_index = None;
        self.clear_objects = false; // 模式退出不额外取消用户既有对象选择。
        self.candidates.clear(); // 清空候选列表。
        self.placement_hits.clear(); // 网格身份不能跨场景或候选代次沿用。
        self.selected_placement_hit = None;
        self.confirmed_viewport_selection = false;
        self.pixel = None; // 下次必须从第一项开始。
        self.pointer_over_ui = false; // 隐藏UI不阻挡绘制。
        self.ui_revision = self.ui_revision.wrapping_add(1); // 通知显示侧收起。
    }
}
