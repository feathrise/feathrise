//! 删除前逐帧检查内存水文档：只读层的预设身份，不复制 R8、地形或整份草稿。

use super::*;
use feathrise_map_content::{MapId, TerrainId};
use std::ops::Bound::{Excluded, Unbounded};
use crate::ui::map_documents::MapDocuments;

/// 每次只检查固定数量的元数据槽；空水文档也计入预算，不能靠大量空网格绕过上限。
const CHECKS_PER_FRAME: usize = 128;

#[derive(PartialEq, Eq)]
struct SourceIdentity {
    map: Option<MapId>,
    author_revision: u64,
    water_revision: u64,
    terrain_revision: u64,
    active_terrain: Option<TerrainId>,
}

impl SourceIdentity {
    fn current(session: &EditorAuthoringSession) -> Self {
        Self {
            map: session.draft.as_ref().map(|draft| draft.map_id.clone()),
            author_revision: session.revision,
            water_revision: session.water_revision,
            terrain_revision: session.terrain_revision,
            active_terrain: session.draft.as_ref().and_then(|draft|
                draft.terrain_documents.active.as_ref().map(|active| active.definition.id.clone())),
        }
    }
}

pub(in super::super) struct WaterDeletePreflight {
    id: String,
    catalog_revision: u64,
    source: SourceIdentity,
    // 0 当前画布，1 停放根水，2 停放网格，3/4 撤销和重做中的水定义快照。
    source_slot: u8,
    layer: usize,
    terrain: Option<TerrainId>,
    history_command: usize,
    history_after: bool,
    active_complete: bool,
    parked_map: Option<MapId>,
    parked_count: Option<usize>,
}

impl WaterDeletePreflight {
    pub(in super::super) fn new(session: &EditorAuthoringSession, registry: &WaterPresetRegistry, id: String) -> Self {
        Self {
            id, catalog_revision: registry.revision, source: SourceIdentity::current(session),
            source_slot: 0, layer: 0, terrain: None,
            history_command: 0, history_after: false,
            active_complete: false, parked_map: None, parked_count: None,
        }
    }

    fn advance(&mut self, session: &EditorAuthoringSession, registry: &WaterPresetRegistry, documents: Option<&MapDocuments>, budget: usize) -> Result<bool, String> {
        if self.source != SourceIdentity::current(session) || registry.revision != self.catalog_revision
            || registry.last_error.is_some() || registry.resolve(&self.id).is_none()
        {
            return Err("删除检查期间地图或预设目录已变化；未写入文件，请重新确认删除".into());
        }
        let parked_count = documents.map_or(0, |documents| documents.parked.len());
        if self.parked_count.replace(parked_count).is_some_and(|before| before != parked_count)
            || documents.is_some_and(|documents| documents.history_task.is_some()
                || documents.pending_history.is_some() || documents.preparing_target.is_some()
                || documents.close_requested.is_some() || documents.closing.is_some())
        {
            return Err("地图标签正在切换或关闭，未删除水体预设；请等待完成后重试".into());
        }
        for _ in 0..budget {
            let current = if self.active_complete {
                if self.parked_map.is_none() {
                    self.parked_map = documents.and_then(|documents| documents.parked.first_key_value()).map(|(map, _)| map.clone());
                }
                let Some(map) = &self.parked_map else { return Ok(true); };
                &documents.and_then(|documents| documents.parked.get(map))
                    .ok_or("停放地图已变化，未删除水体预设")?.session
            } else { session };
            let Some(draft) = &current.draft else {
                if self.next_map(documents) { return Ok(true); }
                continue;
            };
            if draft.terrain_documents.loading {
                return Err("作者画布正在切换，未删除水体预设；请等待完成后重试".into());
            }
            let water = match self.source_slot {
                0 => draft.water.as_ref(),
                1 => draft.terrain_documents.root_water.as_ref().and_then(|root| root.water.as_ref()),
                2 => {
                    if self.terrain.is_none() {
                        self.terrain = draft.terrain_documents.values.first_key_value().map(|(id, _)| id.clone());
                    }
                    let Some(id) = &self.terrain else {
                        self.source_slot = 3;
                        continue;
                    };
                    draft.terrain_documents.values.get(id).and_then(|canvas| canvas.water.as_ref())
                }
                _ => {
                    let history = if self.source_slot == 3 { &current.undo } else { &current.redo };
                    let Some(command) = history.get(self.history_command) else {
                        self.history_command = 0;
                        self.history_after = false;
                        if self.source_slot == 3 { self.source_slot = 4; }
                        else if self.next_map(documents) { return Ok(true); }
                        continue;
                    };
                    // 历史恢复也会重新引入引用；只借用前后水定义，不读取整笔R8或高度快照。
                    match command.unscoped() {
                        crate::authoring::EditorCommand::WaterDefinitionsChanged { before, after } => {
                            Some(if self.history_after { after } else { before })
                        }
                        crate::authoring::EditorCommand::WaterLayersChanged(change) => {
                            Some(if self.history_after { &change.after } else { &change.before })
                        }
                        _ => None,
                    }
                }
            };
            if let Some(layer) = water.and_then(|water| water.layers.get(self.layer)) {
                // 规范 ID 和历史别名使用与运行时相同的解析器；空旧值是内联水材质，不引用目录。
                if registry.resolve(&layer.preset_id).is_some_and(|preset| preset.id == self.id) {
                    if self.source_slot >= 3 {
                        return Err("当前或停放地图的撤销/重做历史仍引用该水体预设；请保存并关闭相关地图后重试".into());
                    }
                    return Err("当前或停放的未保存地图仍引用该水体预设；请先替换对应静水体".into());
                }
                self.layer += 1;
                continue;
            }
            self.layer = 0;
            if self.source_slot < 2 {
                self.source_slot += 1;
            } else if self.source_slot == 2 {
                let id = self.terrain.as_ref().expect("上方确认当前停放槽存在");
                self.terrain = draft.terrain_documents.values.range((Excluded(id), Unbounded))
                    .next().map(|(next, _)| next.clone());
                if self.terrain.is_none() { self.source_slot = 3; }
            } else if self.history_after {
                self.history_command += 1;
                self.history_after = false;
            } else {
                self.history_after = true;
            }
        }
        Ok(false)
    }

    fn next_map(&mut self, documents: Option<&MapDocuments>) -> bool {
        self.source_slot = 0;
        self.layer = 0;
        self.terrain = None;
        self.history_command = 0;
        self.history_after = false;
        if !self.active_complete {
            self.active_complete = true;
            self.parked_map = documents.and_then(|documents| documents.parked.first_key_value()).map(|(map, _)| map.clone());
        } else if let Some(map) = &self.parked_map {
            self.parked_map = documents.and_then(|documents| documents.parked.range((Excluded(map), Unbounded)).next()).map(|(map, _)| map.clone());
        }
        self.parked_map.is_none()
    }
}

pub(in super::super) fn advance_delete_preflight(
    mut state: ResMut<WaterPresetEditorState>,
    registry: Option<Res<WaterPresetRegistry>>,
    mut session: ResMut<EditorAuthoringSession>,
    documents: Option<Res<MapDocuments>>,
    mut editor: ResMut<MapEditorState>,
) {
    let Some(mut pending) = state.delete_preflight.take() else { return; };
    if !editor.open || editor.close_requested {
        session.water_preset_delete_pending = false;
        state.status = "已取消尚未写入的水体预设删除检查".into();
        return;
    }
    let result = registry.as_deref().ok_or_else(|| "水体目录已失效，未删除任何文件".to_owned())
        .and_then(|registry| pending.advance(&session, registry, documents.as_deref(), CHECKS_PER_FRAME));
    match result {
        Ok(false) => state.delete_preflight = Some(pending),
        Err(error) => { session.water_preset_delete_pending = false; editor.status = error.clone(); state.status = error; }
        Ok(true) => {
            let registry = registry.as_deref().expect("完成检查需要有效目录");
            let baseline = baseline_catalog(registry);
            let baseline_revision = pending.catalog_revision;
            let mutation = WaterCatalogMutation::Delete { id: pending.id };
            let root = workspace_asset_root();
            state.status = "正在后台扫描全部地图水域引用；确认无引用后才会删除…".into();
            state.write_task = Some(IoTaskPool::get().spawn(async move {
                write_catalog_transaction(root, baseline, mutation, baseline_revision, None)
            }));
        }
    }
}

#[cfg(test)]
mod tests;
