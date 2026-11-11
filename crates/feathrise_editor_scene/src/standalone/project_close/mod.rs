//! 工程关闭聚合原作者所有者；只调度原保存，不实现第二套写盘、丢弃或撤销算法。

pub(super) mod authors;
mod scenes;
mod presets;
mod resources;
mod snapshot;
#[cfg(test)]
mod tests;
pub(crate) mod types;

pub use resources::{ProjectAuthorAdapters, ProjectCloseSaveAdapters};
pub use types::{
    ProjectAuthorAdapter, ProjectAuthorDocument, ProjectAuthorSnapshot, ProjectCloseSnapshot,
    ProjectSaveProgress,
};

use crate::ui;
use bevy::prelude::*;
use resources::{ActiveSave, ProjectSaveState, SaveStage};
use types::{SaveItem, SaveTarget};

pub(super) fn install(app: &mut App) {
    if !app.is_plugin_added::<ProjectSavePlugin>() {
        app.add_plugins(ProjectSavePlugin);
    }
}

/// 普通宿主与原游戏作者共用唯一调度器，避免注册第二条写盘/成功判断管线。
pub struct ProjectSavePlugin;

impl Plugin for ProjectSavePlugin {
    fn build(&self, app: &mut App) {
        // Last发生在原输入采集、作者Tasks和Presentation之后：一个请求至少经过一整帧原任务驱动。
        app.init_resource::<ProjectSaveState>()
            .add_systems(Last, advance_project_save);
    }
}

pub fn project_close_snapshot(world: &World) -> ProjectCloseSnapshot {
    snapshot::collect(world).0
}

/// 保留原F12语义：场景/环境草稿进入玩法；独立文档与预设样品仍保留草稿，不强制保存。
/// 只在一次 Play 意图时调用：复用原所有者的轻量文档枚举，不在稳定帧扫描全部标签。
pub fn embedded_play_blocker(world: &World) -> Option<String> {
    let (snapshot, items) = snapshot::collect(world);
    if let Some(reason) = snapshot.blocked.or(snapshot.pending) {
        return Some(format!("工程预览暂不可用：{reason}"));
    }
    let draft = world
        .get_resource::<crate::authoring::EditorAuthoringSession>()
        .and_then(|session| session.draft.as_ref());
    if draft.is_some_and(|draft| draft.domain_dirty()) {
        return Some("当前场景含尚未接入工程内存来源的外部领域草稿，请先在原作者工具保存后再预览".into());
    }
    items
        .into_iter()
        .find(|item| requires_external_projection(&item.target))
        .map(|item| {
            format!(
                "{} 尚未接入当前场景的内存快照，请先在原作者工具中保存后再预览",
                item.label
            )
        })
}

fn requires_external_projection(target: &SaveTarget) -> bool {
    match target {
        SaveTarget::Domain(_, _) => true, // 未声明的应用作者不能被通用宿主猜成隔离样品。
        SaveTarget::Scene(_) | SaveTarget::GlobalEnvironment
        | SaveTarget::Documents | SaveTarget::Animation(_) | SaveTarget::Weights(_)
        | SaveTarget::Surface(_) | SaveTarget::Vegetation(_)
        | SaveTarget::Water(_) | SaveTarget::Building(_) => false,
        // 停放场景不在当前玩法World；原预设/文本/动画编辑也只作用其表单或独立RTT，保存后才改变正式资产。
    }
}

pub fn project_save_progress(world: &World) -> ProjectSaveProgress {
    world
        .get_resource::<ProjectSaveState>()
        .map_or(ProjectSaveProgress::Idle, |state| state.progress.clone())
}

/// 显式“保存全部”只登记有限当前队列；即刻返回不表示任何文档已成功。
pub fn request_save_all(world: &mut World) -> Result<(), String> {
    if matches!(
        project_save_progress(world),
        ProjectSaveProgress::Running { .. }
    ) {
        return Err("保存全部仍在执行，请等待或取消后续排队".into());
    }
    let (snapshot, queue) = snapshot::collect(world);
    if let Some(reason) = snapshot.blocked.or(snapshot.pending) {
        return Err(reason);
    }
    for item in &queue {
        match item.target {
            SaveTarget::Domain(domain, _) => {
                authors::find(world, domain)?;
            }
            SaveTarget::Surface(_) if !world.contains_resource::<ProjectCloseSaveAdapters>() => {
                return Err("地表作者保存适配未安装，已保留全部草稿".into());
            }
            _ => (),
        }
    }
    // 已写盘但正式采用失败时dirty可能已清；重试仍必须重验原回执，不能用空队列冒充成功。
    let retry = world
        .get_resource::<ProjectSaveState>()
        .and_then(|state| state.failed_active.clone())
        .filter(|failed| !queue.iter().any(|item| item.target == failed.item.target))
        .map(|mut failed| {
            // 失败后可能已有手工保存/外部换代；复验旧回执不再拥有推进其他标签hash的信用。
            failed.catalog_before = None;
            failed
        });
    world.insert_resource(ProjectSaveState {
        progress: ProjectSaveProgress::Running {
            current: "准备保存全部原作者文档".into(),
        },
        queue: queue.into(),
        active: retry,
        failed_active: None,
        cancel_after_active: false,
    });
    Ok(())
}

/// 取消不撤回已提交文件，也不drop原任务；只停止后续队列，当前实际工作仍由原所有者收口。
pub fn cancel_project_save(world: &mut World) {
    if let Some(mut state) = world.get_resource_mut::<ProjectSaveState>() {
        state.queue.clear();
        state.cancel_after_active = true;
        // 取消只停止后续队列；写盘后采用失败的真实回执必须保留，供再次关闭与显式重试复验。
        if state.active.is_none() && !matches!(state.progress, ProjectSaveProgress::Failed { .. }) {
            state.progress = ProjectSaveProgress::Idle;
        }
    }
}

fn advance_project_save(world: &mut World) {
    if !matches!(
        project_save_progress(world),
        ProjectSaveProgress::Running { .. }
    ) {
        return;
    }
    world.resource_scope(|world, mut state: Mut<ProjectSaveState>| advance(world, &mut state));
}

fn advance(world: &mut World, state: &mut ProjectSaveState) {
    if let Some(mut active) = state.active.take() {
        let result = match active.stage {
            SaveStage::SelectingScene => match &active.item.target {
                SaveTarget::Scene(map) => scenes::selected(world, map).and_then(|ready| {
                    // 原切换场景一经受理仍须完整收口，但取消后不能再为恢复的场景启动新的文件写入。
                    if ready && state.cancel_after_active {
                        return Ok(true);
                    }
                    if ready {
                        active.stage = scenes::begin(world, Some(map))?;
                    }
                    Ok(false)
                }),
                _ => Err("场景恢复队列身份不匹配，作者草稿保留".into()),
            },
            SaveStage::Submitted { scene_sequence } => poll(world, &active.item.target, scene_sequence),
        };
        match result {
            Ok(false) => {
                state.active = Some(active);
                return;
            }
            Err(error) => {
                state.queue.clear();
                state.progress = ProjectSaveProgress::Failed {
                    detail: format!("{}：{error}", active.item.label),
                };
                state.failed_active = Some(active);
                return; // 不自动重试、不继续保存其他文档，已成功文件保持，失败草稿仍在原所有者。
            }
            Ok(true) => presets::rebase_own_write(
                world,
                &active.item.target,
                active.catalog_before.as_deref(),
            ),
        }
    }
    if state.cancel_after_active {
        if let Some(mut editor) = world.get_resource_mut::<crate::resources::SceneEditorState>() {
            let map = editor
                .selected_scene_id
                .as_ref()
                .map_or_else(|| "空工作台".into(), ToString::to_string);
            editor.status =
                format!("已取消后续保存；当前停留在{map}，全部草稿、场景标签和撤销历史仍保留");
        }
        state.progress = ProjectSaveProgress::Idle;
        return;
    }
    if let Some(item) = state.queue.pop_front() {
        state.progress = ProjectSaveProgress::Running {
            current: item.label.clone(),
        };
        let before = presets::catalog_hash(world, &item.target);
        match start(world, &item) {
            Ok(stage) => {
                state.active = Some(ActiveSave {
                    item,
                    stage,
                    catalog_before: before,
                })
            }
            Err(error) => {
                state.queue.clear();
                state.progress = ProjectSaveProgress::Failed {
                    detail: format!("{}：{error}", item.label),
                };
            }
        }
        return;
    }
    // 队列已结束还要重查全部原所有者：新脏内容、部分失败或未覆盖owner绝不冒充成功。
    let snapshot = project_close_snapshot(world);
    if let Some(reason) = snapshot.blocked {
        state.progress = ProjectSaveProgress::Failed { detail: reason };
    } else if let Some(reason) = snapshot.pending {
        state.progress = ProjectSaveProgress::Running { current: reason };
    } else if !snapshot.unsaved.is_empty() {
        state.progress = ProjectSaveProgress::Failed {
            detail: format!(
                "仍有未保存内容，工程和历史保留：{}",
                snapshot.unsaved.join("；")
            ),
        };
    } else {
        state.progress = ProjectSaveProgress::Succeeded;
    }
}

fn start(world: &mut World, item: &SaveItem) -> Result<SaveStage, String> {
    match &item.target {
        SaveTarget::Scene(map) => return scenes::begin(world, Some(map)),
        SaveTarget::GlobalEnvironment => return scenes::begin(world, None),
        SaveTarget::Documents => ui::asset_preview::documents::project_close::request(world)?,
        SaveTarget::Animation(path) => {
            ui::asset_preview::animation::project_close::request(world, path)?
        }
        SaveTarget::Weights(path) => ui::asset_preview::weights::project_close::request(world, path)?,
        SaveTarget::Vegetation(id) => {
            ui::vegetation_preset_editor::project_close::request(world, *id)?
        }
        SaveTarget::Water(id) => ui::water_preset_editor::project_close::request(world, *id)?,
        SaveTarget::Building(id) => ui::building_presets::project_close::request(world, *id)?,
        target => presets::request(world, target)?,
    }
    Ok(SaveStage::Submitted { scene_sequence: None })
}

fn poll(world: &World, target: &SaveTarget, before: Option<u64>) -> Result<bool, String> {
    match target {
        SaveTarget::Scene(map) => scenes::poll(world, Some(map), before),
        SaveTarget::GlobalEnvironment => scenes::poll(world, None, before),
        SaveTarget::Documents => ui::asset_preview::documents::project_close::poll(world),
        SaveTarget::Animation(path) => {
            ui::asset_preview::animation::project_close::poll(world, path)
        }
        SaveTarget::Weights(path) => ui::asset_preview::weights::project_close::poll(world, path),
        SaveTarget::Vegetation(id) => ui::vegetation_preset_editor::project_close::poll(world, *id),
        SaveTarget::Water(id) => ui::water_preset_editor::project_close::poll(world, *id),
        SaveTarget::Building(id) => ui::building_presets::project_close::poll(world, *id),
        target => presets::poll(world, target),
    }
}
