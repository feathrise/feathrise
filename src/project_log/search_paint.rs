//! 从已排版的当前终端页读取字形簇位置，为 Bevy 文字层提供搜索背景及横向定位。

use super::{
    components::{LogBody, LogDisplay, LogScroll},
    resources::ProjectLogState,
    search::LogSearch,
};
use bevy::{
    prelude::*,
    text::{ComputedTextBlock, RunGeometry, TextBackgroundColor, TextLayoutInfo},
};
use parley::{Affinity, Cursor, Selection};
use std::ops::Range;

#[derive(Component)]
pub(super) struct LogSearchBrush;

pub(super) const MATCH_BACKGROUND: Color = Color::srgba(0.63, 0.42, 0.06, 0.46);
pub(super) const ACTIVE_BACKGROUND: Color = Color::srgba(0.10, 0.44, 0.76, 0.76);

impl LogSearchBrush {
    pub const COUNT: usize = 2;

    pub(super) fn spawn(parent: &mut ChildSpawnerCommands) {
        // 空段只登记背景样式。实际命中来自当前页面的字形几何，
        // 一个字符的高频查询也不创建成千上万个 TextSpan 或重排正文。
        for background in [MATCH_BACKGROUND, ACTIVE_BACKGROUND] {
            parent.spawn((Self, TextSpan::default(), TextColor(Color::NONE), TextBackgroundColor(background)));
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct PaintKey {
    page: Option<u64>,
    origin: u64,
    revision: u64,
    query: u64,
    active: Option<u64>,
    left: f32,
    right: f32,
}

pub(super) fn paint(
    state: Res<ProjectLogState>,
    mut search: ResMut<LogSearch>,
    mut displays: Query<(&LogDisplay, &ComputedTextBlock, &mut TextLayoutInfo), Without<LogBody>>,
    mut panes: Query<(&ComputedNode, &mut ScrollPosition), With<LogScroll>>,
    mut previous: Local<Option<PaintKey>>,
) {
    let (Ok((page, computed, mut info)), Ok((pane, mut scroll))) = (displays.single_mut(), panes.single_mut())
    else { return; };
    if !state.open || pane.is_empty() { return; }
    // Bevy 的文字提取按段编号读取背景。根段及两个固定空段位于 ANSI 动态段之前。
    if computed.entities().len() <= LogSearchBrush::COUNT { return; }
    let match_section = 1;
    let active_section = 2;
    let ready = page.revision == state.content_revision && search.ready(&state);
    let active = ready.then(|| search.active_offset(&state)).flatten();
    let scale = pane.inverse_scale_factor().recip();
    let width = pane.content_box().width();
    if let Some(target) = search.horizontal_jump {
        if state.wrap || Some(target) != active {
            search.horizontal_jump = None;
        } else if let Some(range) = reveal_range(computed, page.origin, target, search.query_bytes()) {
            let left = scroll.x * scale;
            let desired = if range.start < left || range.end - range.start > width {
                (range.start - 8.).max(0.)
            } else if range.end > left + width { range.end - width + 8. }
            else { left };
            // 新长行需要先由既有测宽系统扩展滚动占位，才能采用其横向坐标。
            if desired == left || info.size.x <= pane.content_size().x + 1. {
                let max = (pane.content_size().x - (pane.size().x - pane.scrollbar_size.x)).max(0.);
                scroll.x = desired.min(max) / scale;
                search.horizontal_jump = None;
            }
        }
    }

    let left = scroll.x * scale;
    let right = left + width;
    let key = (ready && search.count() > 0).then_some(PaintKey {
        page: page.fingerprint, origin: page.origin, revision: page.revision,
        query: search.paint_revision, active, left, right,
    });
    if *previous == key && (key.is_none() || !info.is_changed()) { return; }
    *previous = key;
    info.run_geometry.retain(|run| run.section_index != match_section && run.section_index != active_section);
    if key.is_none() { return; }

    let current = active.map(|start| start..start + search.query_bytes());
    for line in computed.buffer().lines() {
        let range = line.text_range();
        if !search.intersects(page.origin + range.start as u64..page.origin + range.end as u64) {
            continue;
        }
        let metrics = line.metrics();
        let mut x = metrics.offset + metrics.inline_min_coord;
        // 按实际视觉簇遍历，支持中英文混排及软换行。相邻同色簇合并成一块。
        // 未换行时也只为当前水平视口内的字形提供背景。
        'runs: for run in line.runs() {
            for cluster in run.visual_clusters() {
                if x >= right { break 'runs; }
                let start = x;
                x += cluster.advance();
                if x <= left || x <= start { continue; }
                let bytes = cluster.text_range();
                let bounds = page.origin + bytes.start as u64..page.origin + bytes.end as u64;
                let section_index = if current.as_ref().is_some_and(|range|
                    range.start < bounds.end && bounds.start < range.end)
                { active_section } else if search.intersects(bounds) { match_section } else { continue; };
                let rect = Rect::new(start.max(left), metrics.block_min_coord, x.min(right), metrics.block_max_coord);
                if let Some(last) = info.run_geometry.last_mut()
                    && last.section_index == section_index && last.bounds.min.y == rect.min.y
                    && (last.bounds.max.x - rect.min.x).abs() < 0.1
                {
                    last.bounds.max.x = rect.max.x;
                } else {
                    info.run_geometry.push(RunGeometry { section_index, bounds: rect, ..default() });
                }
            }
        }
    }
}

fn reveal_range(computed: &ComputedTextBlock, origin: u64, start: u64, len: u64) -> Option<Range<f32>> {
    let from = start.checked_sub(origin)? as usize;
    let to = from + len as usize;
    let layout = computed.buffer();
    let selection = Selection::new(
        Cursor::from_byte_index(layout, from, Affinity::Downstream),
        Cursor::from_byte_index(layout, to, Affinity::Upstream),
    );
    let mut range = f32::INFINITY..f32::NEG_INFINITY;
    selection.geometry_with(layout, |bounds, _| {
        range.start = range.start.min(bounds.x0 as f32);
        range.end = range.end.max(bounds.x1 as f32);
    });
    (range.start < range.end).then_some(range)
}
