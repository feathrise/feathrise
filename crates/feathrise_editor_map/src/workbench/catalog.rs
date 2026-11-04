//! 将工程目录快照转换成原地图树投影；单飞后台任务防止大工程在UI帧整理整张目录。
use super::{WorkbenchModel, resources::CatalogJob, types::CatalogResult};
use crate::{
    resources::{MapBrowserIndexState, MapBrowserState},
    types::{MapBrowserEntry, MapBrowserIndexSignature, MapBrowserItem},
};
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, futures::check_ready},
};
use std::sync::Arc;

pub(super) fn advance(
    model: Res<WorkbenchModel>,
    mut job: ResMut<CatalogJob>,
    mut index: ResMut<MapBrowserIndexState>,
    mut browser: ResMut<MapBrowserState>,
) {
    if let Some(task) = job.retirement.as_mut() {
        if check_ready(task).is_none() {
            return;
        }
        job.retirement = None;
    }
    if let Some(result) = job.task.as_mut().and_then(check_ready) {
        job.task = None;
        // 切工程和同工程目录热更都验证来源；迟到投影不能把同名地图绑定到新工程。
        if result.context == model.context && Arc::ptr_eq(&result.source, &model.maps) {
            let signature = MapBrowserIndexSignature {
                folder_revision: model.context,
                catalog_revision: 0,
            };
            let retired = (
                std::mem::replace(&mut index.entries, result.entries.clone()),
                std::mem::replace(&mut index.full_entries, result.entries),
                std::mem::replace(&mut job.indices, result.indices),
                job.adopted.take(),
            );
            index.total_entries = index.entries.len();
            index.applied_signature = Some(signature.clone());
            index.applied_projection = Some((signature.clone(), 0));
            index.desired_projection = Some((signature, 0));
            index.rebuilding = false;
            browser.document_revision = model.context;
            browser.initial_load_resolved = true;
            job.adopted = Some((model.context, result.source));
            job.retirement = Some(AsyncComputeTaskPool::get().spawn(async move {
                drop(retired);
            }));
        } else {
            job.retirement = Some(AsyncComputeTaskPool::get().spawn(async move {
                drop(result);
            }));
        }
    }
    if job.task.is_some()
        || job.adopted.as_ref().is_some_and(|(context, source)| {
            *context == model.context && Arc::ptr_eq(source, &model.maps)
        })
    {
        return;
    }
    let source = model.maps.clone();
    let context = model.context;
    index.rebuilding = true;
    job.task = Some(AsyncComputeTaskPool::get().spawn(async move {
        let entries = source
            .iter()
            .map(|entry| MapBrowserEntry {
                item: MapBrowserItem::Map(entry.id.clone()),
                display_name: entry.display_name.clone(),
                published: entry.published,
                depth: 1,
                parent_folder_id: None,
            })
            .collect();
        let indices = source
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.id.0.clone(), index))
            .collect();
        CatalogResult {
            context,
            source,
            entries: Arc::new(entries),
            indices,
        }
    }));
}
