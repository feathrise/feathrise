//! 虚拟场景树的分帧输入快照、后台整理与展开投影。
//!
//! 目录或catalog变化只分帧冻结一次完整输入；会话展开变化只在后台过滤共享树，稳定帧不扫描全部项目。

use std::sync::Arc; // 完整树由后台投影任务共享，展开点击不会在主线程深复制全部行。

use bevy::prelude::*; // ECS资源读取与系统参数。
use bevy::tasks::{AsyncComputeTaskPool, futures::check_ready}; // 树整理与可见过滤均在计算池完成，主线程只非阻塞采用。
use feathrise_scene_content::{SceneCatalogAsset, SceneContentRegistry}; // 公共catalog仍是场景发现与作者顺序的唯一来源。

use crate::constants::SCENE_BROWSER_INDEX_ITEMS_PER_FRAME; // 每帧冻结文件夹与场景记录的共享硬预算。
use crate::resources::{SceneBrowserIndexState, SceneBrowserState}; // 会话展开集合和双阶段索引状态。
use crate::types::{
    SceneBrowserEntry, SceneBrowserIndexBuild, SceneBrowserIndexSignature, SceneBrowserItem,
}; // 后台任务只拥有普通快照类型。

use super::helpers::{build_browser_tree, project_visible_browser_tree}; // 纯树整理与展开投影共享确定性规则。

/// 返回当前公共场景目录及其已应用修订。
fn catalog_snapshot<'a>(
    registry: Option<&'a SceneContentRegistry>, // 运行时目录注册表可能尚未建立。
    catalogs: &'a Assets<SceneCatalogAsset>,    // 原子发布的catalog资产集合。
) -> (u64, &'a [feathrise_scene_content::SceneCatalogEntry]) {
    let Some(registry) = registry else {
        return (0, &[]); // 独立编辑器测试或首载前显示空场景部分。
    };
    let entries = catalogs
        .get(&registry.catalog)
        .map(|catalog| catalog.scenes.as_slice())
        .unwrap_or_default(); // 句柄尚未完成时保持空切片并等待注册表后续修订。
    (registry.revision, entries) // catalog作者顺序不得在主线程或后台重新排序。
}

/// 输入双修订变化时建立一份新的有界完整树快照草稿。
fn begin_source_build(
    desired: SceneBrowserIndexSignature, // 本轮需要完整整理的归类与catalog身份。
    index: &mut SceneBrowserIndexState,  // 原地换代分帧草稿并保留上一公开树。
) {
    index.desired_signature = Some(desired.clone()); // 后台结果必须逐字段匹配该身份。
    index.build = Some(SceneBrowserIndexBuild {
        signature: desired,  // 草稿从第一帧起冻结完整双修订。
        folder_cursor: 0,    // 文件夹数组从头按预算复制。
        scene_cursor: 0,       // 文件夹完成后按catalog作者顺序复制场景。
        folders: Vec::new(), // 最多512条受检普通记录。
        scenes: Vec::new(),    // 最多catalog预算内的轻量场景行。
    });
    index.desired_projection = None; // 新完整树尚未完成，旧展开身份不能用于它。
    index.rebuilding = true; // 旧公开树仍可显示，但摘要明确提示正在整理。
}

/// 按固定记录预算冻结完整树输入；完成后只把纯数据整理交给唯一后台任务。
fn advance_source_build(
    browser: &SceneBrowserState, // 当前受检归类文档和场景归属。
    catalog_entries: &[feathrise_scene_content::SceneCatalogEntry], // 本帧冻结的catalog作者数组。
    index: &mut SceneBrowserIndexState, // 分帧草稿、后台任务和公开完整树。
) {
    if index.task.is_some() {
        return; // 不可协作取消的旧整理任务真实结束前不启动第二份完整树任务。
    }
    let Some(mut build) = index.build.take() else {
        return; // 当前双修订已有完整树时不扫描任何输入记录。
    };
    if index.desired_signature.as_ref() != Some(&build.signature) {
        return; // 热更新换代后的旧草稿直接丢弃，下一帧会建立最新身份。
    }

    let mut remaining = SCENE_BROWSER_INDEX_ITEMS_PER_FRAME; // 两类输入共享同一帧硬预算。
    while remaining > 0 && build.folder_cursor < browser.document.folders.len() {
        build
            .folders
            .push(browser.document.folders[build.folder_cursor].clone()); // 每条文件夹只冻结一次短记录。
        build.folder_cursor += 1; // 无条件推进拥有型游标。
        remaining -= 1; // 最多512文件夹也不会在一帧全部复制。
    }
    while remaining > 0
        && build.folder_cursor >= browser.document.folders.len()
        && build.scene_cursor < catalog_entries.len()
    {
        let entry = &catalog_entries[build.scene_cursor]; // 严格按catalog作者数组顺序读取。
        let parent_folder_id = browser.document.scene_folders.get(entry.id.as_str()).cloned(); // 缺失归属键明确表示根目录。
        build.scenes.push(SceneBrowserEntry {
            item: SceneBrowserItem::Scene(entry.id.clone()), // 正式SceneId继续贯穿进入、删除和拖拽链。
            display_name: entry.display_name.clone(),    // 后台拥有短作者名称快照。
            published: entry.published,                  // 保留正式目录发布状态供后续显示扩展。
            depth: 0,                                    // 后台按真实父树统一写入最终深度。
            parent_folder_id,                            // 场景分组无需再次查归类文档。
        });
        build.scene_cursor += 1; // 每张catalog场景只冻结一次。
        remaining -= 1; // 大目录跨帧完成，不在Update做无界完整复制。
    }

    let complete = build.folder_cursor >= browser.document.folders.len()
        && build.scene_cursor >= catalog_entries.len(); // 两类输入完整后才能开始后台树整理。
    if !complete {
        index.build = Some(build); // 游标和已经冻结的前缀跨帧继续。
        return;
    }

    let signature = build.signature; // 后台结果携带完整双修订身份。
    let folders = build.folders; // 所有权直接移入计算池，不在主线程再复制。
    let scenes = build.scenes; // catalog顺序已经在分帧冻结阶段确定。
    index.task = Some(AsyncComputeTaskPool::get().spawn(async move {
        let entries = Arc::new(build_browser_tree(folders, scenes)); // 后台完成同级排序、分组和深度优先展开。
        (signature, entries) // 主线程只在身份仍匹配时原子采用Arc。
    }));
}

/// 非阻塞采用完整树任务；迟到结果只释放槽位，不覆盖当前双修订。
fn poll_source_task(index: &mut SceneBrowserIndexState) {
    let Some(task) = index.task.as_mut() else {
        return; // 没有完整树任务时稳定帧立即返回。
    };
    let Some((signature, entries)) = check_ready(task) else {
        return; // 计算未完成时绝不等待。
    };
    index.task = None; // 不可取消任务真实完成后释放唯一槽。
    if index.desired_signature.as_ref() != Some(&signature) {
        return; // 归类或catalog已经换代，保留新草稿等待下一轮整理。
    }
    index.full_entries = entries; // 完整树以O(1)共享句柄一次替换。
    index.applied_signature = Some(signature); // 后续投影只允许读取该双修订。
    index.applied_projection = None; // 新完整树尚未产生任何可见投影。
    index.desired_projection = None; // 本帧稍后按当前展开修订建立新目标。
}

/// 推进会话展开集合的唯一后台可见投影，并在完整身份匹配时原子替换公开行。
fn advance_projection(
    browser: &SceneBrowserState,                 // 当前展开集合和修订。
    desired_source: &SceneBrowserIndexSignature, // 当前必须使用的完整树双修订。
    index: &mut SceneBrowserIndexState,          // 共享完整树、投影任务和公开可见行。
) {
    if index.applied_signature.as_ref() != Some(desired_source) {
        index.rebuilding = true; // 完整树未就绪时继续显示上一结果和整理提示。
        return;
    }
    let desired_projection = (desired_source.clone(), browser.expansion_revision); // 快速连点只推进轻量会话身份。
    if index.desired_projection.as_ref() != Some(&desired_projection) {
        index.desired_projection = Some(desired_projection.clone()); // 旧任务完成后会被明确拒收。
        index.rebuilding = true; // 展开结果采用前不暴露半棵树。
    }

    if let Some(task) = index.projection_task.as_mut()
        && let Some((signature, expansion_revision, entries)) = check_ready(task)
    {
        index.projection_task = None; // 投影真实完成后才释放single-flight槽。
        let completed_identity = (signature, expansion_revision); // 与最新展开点击逐字段比较。
        if index.desired_projection.as_ref() == Some(&completed_identity)
            && index.applied_signature.as_ref() == Some(&completed_identity.0)
        {
            index.entries = entries; // 完整可见树一次原子替换，滚动池下一阶段读取。
            index.total_entries = index.full_entries.len(); // 总数与同一完整树投影在本系统边界一起公开。
            index.applied_projection = Some(completed_identity); // 稳定帧无需再次投影。
            index.rebuilding = false; // 摘要移除整理提示。
        }
    }

    if index.projection_task.is_some()
        || index.applied_projection.as_ref() == Some(&desired_projection)
    {
        return; // 当前投影正在执行或已经完整采用时不创建第二个任务。
    }
    let full_entries = Arc::clone(&index.full_entries); // 主线程O(1)共享完整树，不深复制最多4608行。
    let expanded = browser.expanded_folder_ids.clone(); // 最多512个短稳定ID，作为不可变会话快照。
    let (signature, expansion_revision) = desired_projection; // 任务结果携带完整拒收身份。
    index.projection_task = Some(AsyncComputeTaskPool::get().spawn(async move {
        let entries = project_visible_browser_tree(full_entries.as_slice(), &expanded); // 后台跳过全部折叠后代。
        (signature, expansion_revision, Arc::new(entries)) // 公开投影供后台连选共享，不在按下帧克隆整棵树。
    }));
}

/// 驱动场景树完整索引与可见投影；稳定状态只进行常数次身份比较和空任务检查。
pub(crate) fn advance_scene_browser_index(
    browser: Res<SceneBrowserState>, // 当前有效归类文档与会话展开状态。
    registry: Option<Res<SceneContentRegistry>>, // 公共场景目录身份与修订。
    catalogs: Res<Assets<SceneCatalogAsset>>, // 当前catalog作者数组。
    mut index: ResMut<SceneBrowserIndexState>, // 两阶段任务与原子公开结果。
) {
    let (catalog_revision, catalog_entries) = catalog_snapshot(registry.as_deref(), &catalogs); // 一帧内固定同一目录视图。
    let desired_source = SceneBrowserIndexSignature {
        folder_revision: browser.document_revision, // 结构、归属或回滚变化都会重建完整树。
        catalog_revision,                           // 新建、删除和catalog热更新都会换代。
    };
    let desired_projection = (desired_source.clone(), browser.expansion_revision); // 稳定帧只比较小型身份，不触碰完整树。
    let stable = index.desired_signature.as_ref() == Some(&desired_source)
        && index.applied_signature.as_ref() == Some(&desired_source)
        && index.applied_projection.as_ref() == Some(&desired_projection)
        && index.build.is_none()
        && index.task.is_none()
        && index.projection_task.is_none()
        && !index.rebuilding; // 完整树和当前展开投影都已原子采用。
    if stable {
        return; // 不取得ResMut可变解引用，稳定帧不会误触资源变化或扫描任何树行。
    }
    if index.desired_signature.as_ref() != Some(&desired_source) {
        begin_source_build(desired_source.clone(), &mut index); // 只换代草稿，上一公开树继续可见。
    }

    poll_source_task(&mut index); // 先释放可能已经完成的旧任务槽。
    advance_source_build(&browser, catalog_entries, &mut index); // 再按固定预算推进最新输入。
    advance_projection(&browser, &desired_source, &mut index); // 完整树就绪后才生成会话可见投影。
}
