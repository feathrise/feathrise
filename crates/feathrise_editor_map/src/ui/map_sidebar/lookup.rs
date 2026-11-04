//! 所选名称优先从原二十八槽绑定读取，离屏选择按固定预算定位同一完整投影。

use super::types::SelectionLookup;
use crate::resources::MapBrowserIndexState;
use crate::types::{MapBrowserEntry, MapBrowserItem};
use std::sync::Arc;

pub(super) const LOOKUP_STEPS: usize = 64;

pub(super) fn advance_lookup(
    lookup: &mut SelectionLookup,
    item: Option<&MapBrowserItem>,
    index: &MapBrowserIndexState,
    visible: Option<&MapBrowserEntry>,
) {
    if !lookup.initialized
        || lookup.item.as_ref() != item
        || lookup.projection != index.applied_projection
    {
        lookup.initialized = true;
        lookup.item = item.cloned();
        lookup.projection = index.applied_projection.clone();
        lookup.source = None;
        lookup.cursor = 0;
        lookup.complete = item.is_none() || matches!(item, Some(MapBrowserItem::Root));
        lookup.record = None;
        // 完整树已经换代但可见投影未采用时，不能用私有候选名称替换仍显示的旧目录。
        if !lookup.complete
            && index
                .applied_projection
                .as_ref()
                .is_some_and(|(signature, _)| index.applied_signature.as_ref() == Some(signature))
        {
            lookup.source = Some(Arc::clone(&index.full_entries));
        }
    }
    if let Some(visible) = visible.filter(|entry| Some(&entry.item) == item) {
        if lookup.record.as_ref() != Some(visible) {
            lookup.record = Some(visible.clone()); // 只拥有一个受检短记录，不复制完整树。
        }
        lookup.complete = true;
        lookup.source = None;
        return;
    }
    if lookup.complete {
        return;
    }
    let Some(source) = lookup.source.as_ref() else {
        return;
    };
    let end = lookup.cursor.saturating_add(LOOKUP_STEPS).min(source.len());
    for entry in &source[lookup.cursor..end] {
        if Some(&entry.item) == item {
            lookup.record = Some(entry.clone());
            lookup.complete = true;
            break;
        }
    }
    lookup.cursor = end;
    lookup.complete |= end == source.len(); // 没有找到必须明确失效，不能猜测列表首项或同名对象。
    if lookup.complete {
        lookup.source = None;
    } // 短记录已定位或确认为失效后立即释放旧树租约。
}
