//! 工程日志查找按原文行建立有界字节索引。当前页折行、高亮和滚动都复用生产阅读层。

use super::{
    components::{LogSearchBar, LogSearchCount, LogSearchHint, LogSearchInput},
    resources::{FRAME_LINES, LINE_BYTES, LogPosition, ProjectLogState},
};
use bevy::{prelude::*, text::EditableText};
use feathrise_editor_ui::localization::EditorLocale;
use std::{borrow::Cow, collections::VecDeque, ops::Range};

// 单行最多 1 KiB。每帧新扫描预算大于生产每帧 32 条的采用量，
// 持续输出时仍能完成初次建索引。尾行槽数固定，且只在正文修订变化时扫描。
const SEARCH_FRAME_LINES: usize = FRAME_LINES * 2;
pub(super) const SEARCH_QUERY_CHARS: usize = LINE_BYTES;

#[derive(Clone, Copy)]
pub(super) enum SearchDirection { Previous, Next }

#[derive(Resource, Default)]
pub(super) struct LogSearch {
    pub shown: bool,
    needle: String,
    indexed_through: u64,
    clear_revision: u64,
    revision: Option<u64>,
    pub paint_revision: u64,
    // 非重叠匹配每字节至多一处，正文上限 256 KiB。不复制正文和每项匹配文本。
    committed: VecDeque<u64>,
    tails: Vec<u64>,
    active: Option<LogPosition>,
    first_match: bool,
    pending_jump: bool,
    pub horizontal_jump: Option<u64>,
}

impl LogSearch {
    fn reset(&mut self, clear_revision: u64) {
        self.clear_revision = clear_revision;
        self.paint_revision = self.paint_revision.wrapping_add(1);
        self.indexed_through = 0;
        self.revision = None;
        self.committed.clear();
        self.tails.clear();
        self.active = None;
        self.first_match = false;
        self.pending_jump = false;
        self.horizontal_jump = None;
    }

    pub(super) fn ready(&self, state: &ProjectLogState) -> bool {
        self.shown && !self.needle.is_empty() && self.revision == Some(state.content_revision)
    }

    fn index(&mut self, state: &ProjectLogState) {
        self.revision = None;
        let oldest = state.lines.front().map_or(state.next_offset, |line| line.start);
        let evicted = self.committed.partition_point(|start| *start < oldest);
        drop(self.committed.drain(..evicted));
        let skip = state.lines.partition_point(|line| line.start < self.indexed_through);
        for line in state.lines.iter().skip(skip).take(SEARCH_FRAME_LINES) {
            index_line(&self.needle, &line.text, line.start, &mut self.committed);
            self.indexed_through = line.start + line.text.len() as u64 + 1;
        }
        if self.indexed_through < state.next_offset { return; }

        // 完成行的绝对偏移不移动。临时行却会被改写或整体后移，按当前任务快照重新取它们。
        self.tails.clear();
        let mut origin = state.next_offset;
        for tail in state.current_tails() {
            index_line(&self.needle, &tail.text, origin, &mut self.tails);
            origin += tail.text.len() as u64 + 1;
        }
        self.revision = Some(state.content_revision);
        if self.active_index(state).is_none() {
            self.active = None;
            self.pending_jump = false;
        }
    }

    pub(super) fn count(&self) -> usize { self.committed.len() + self.tails.len() }

    fn before(&self, offset: u64) -> usize {
        self.committed.partition_point(|start| *start < offset)
            + self.tails.partition_point(|start| *start < offset)
    }

    fn at(&self, index: usize) -> Option<u64> {
        self.committed.get(index).or_else(||
            self.tails.get(index.saturating_sub(self.committed.len()))).copied()
    }

    fn active_index(&self, state: &ProjectLogState) -> Option<usize> {
        let offset = state.offset_of(self.active?)?;
        let index = self.before(offset);
        (self.at(index) == Some(offset)).then_some(index)
    }

    pub(super) fn active_offset(&self, state: &ProjectLogState) -> Option<u64> {
        if !self.ready(state) { return None; }
        self.at(self.active_index(state)?)
    }

    pub(super) fn query_bytes(&self) -> u64 { self.needle.len() as u64 }

    /// 字形簇可能横跨几个字节。高亮与该簇相交的原文范围。
    pub(super) fn intersects(&self, range: Range<u64>) -> bool {
        self.before(range.end).checked_sub(1)
            .and_then(|index| self.at(index))
            .is_some_and(|start| start + self.query_bytes() > range.start)
    }

    pub(super) fn navigate(&mut self, state: &ProjectLogState, direction: SearchDirection) {
        let count = self.count();
        if !self.ready(state) || count == 0 { return; }
        let active = self.active_index(state);
        let index = match (direction, active) {
            (SearchDirection::Previous, None | Some(0)) => count - 1,
            (SearchDirection::Previous, Some(index)) => index - 1,
            (SearchDirection::Next, None) => 0,
            (SearchDirection::Next, Some(index)) => (index + 1) % count,
        };
        self.select(state, index);
    }

    fn select(&mut self, state: &ProjectLogState, index: usize) {
        self.active = self.at(index).map(|offset| state.position_at(offset));
        self.first_match = false;
        self.pending_jump = self.active.is_some();
    }

    pub(super) fn pending_jump(&self) -> bool { self.pending_jump }

    pub(super) fn jump(&mut self, state: &mut ProjectLogState) {
        if !self.pending_jump || !self.ready(state)
            || (state.wrap && state.wrap_layout.revision != Some(state.content_revision)) { return; }
        self.pending_jump = false;
        let Some(offset) = self.active_offset(state) else { return; };
        let row = if state.wrap {
            state.wrap_layout.rows.partition_point(|row| row.start <= offset).saturating_sub(1)
        } else if offset < state.next_offset {
            state.lines.partition_point(|line| line.start <= offset).saturating_sub(1)
        } else {
            state.rows().take_while(|(start, _)| *start <= offset).count().saturating_sub(1)
        };
        // 从真实视觉行锚点定位，随后交回原滚动状态。尾部继续收到输出时不把读者拉走。
        let first = row.saturating_sub(state.visible_rows / 2);
        state.scroll_back = state.display_row_count().saturating_sub(first + state.visible_rows);
        state.follow = false;
        state.dirty = true;
        self.horizontal_jump = (!state.wrap).then_some(offset);
    }
}

fn index_line(needle: &str, line: &str, origin: u64, matches: &mut impl Extend<u64>) {
    // ASCII 折叠不改变 UTF-8 字节边界；中文、路径和 crate 名共享原文偏移。
    let folded = if line.bytes().any(|byte| byte.is_ascii_uppercase()) {
        Cow::Owned(line.to_ascii_lowercase())
    } else { Cow::Borrowed(line) };
    matches.extend(folded.match_indices(needle).map(|(start, _)| origin + start as u64));
}

pub(super) fn sync(
    state: Res<ProjectLogState>,
    mut search: ResMut<LogSearch>,
    fields: Query<&EditableText, With<LogSearchInput>>,
) {
    if !state.open || !search.shown { return; }
    if let Ok(field) = fields.single() {
        let needle = field.value().to_string().to_ascii_lowercase();
        if needle != search.needle {
            search.reset(state.clear_revision);
            search.needle = needle;
            search.first_match = true;
        }
    }
    if search.clear_revision != state.clear_revision {
        search.reset(state.clear_revision);
    }
    if search.needle.is_empty() || search.ready(&state) { return; }
    search.index(&state);
    if search.ready(&state) && search.first_match && search.count() > 0 {
        search.select(&state, 0);
    }
}

pub(super) fn present(
    state: Res<ProjectLogState>,
    search: Res<LogSearch>,
    locale: Option<Res<EditorLocale>>,
    mut bars: Query<&mut Node, With<LogSearchBar>>,
    mut hints: Query<&mut Visibility, With<LogSearchHint>>,
    mut counts: Query<&mut Text, With<LogSearchCount>>,
    fields: Query<&EditableText, With<LogSearchInput>>,
) {
    for mut bar in &mut bars {
        let display = if search.shown { Display::Flex } else { Display::None };
        if bar.display != display { bar.display = display; }
    }
    if !state.open || !search.shown { return; }
    let composing = fields.single().is_ok_and(|field| field.editor.is_composing());
    let hint_visibility = if search.needle.is_empty() && !composing { Visibility::Inherited } else { Visibility::Hidden };
    for mut hint in &mut hints { hint.set_if_neq(hint_visibility); }
    let locale = locale.as_deref().copied().unwrap_or_default();
    let status = if search.needle.is_empty() {
        String::new()
    } else if search.ready(&state) {
        locale.format("log.search.count", &[
            ("current", search.active_index(&state).map_or(0, |index| index + 1).to_string()),
            ("total", search.count().to_string()),
        ])
    } else { locale.text("log.search.pending").into() };
    for mut text in &mut counts {
        if text.0 != status { text.0.clone_from(&status); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_log::{
        resources::{CACHE_LINES, ProjectLogChannel},
        types::{LogEvent, LogEventKind, LogSource, LogTask},
    };

    #[test]
    fn only_complete_results_publish_and_eviction_retracts_the_active_match() {
        let (channel, _) = ProjectLogChannel::new("查找淘汰回归");
        let mut state = ProjectLogState::new(&channel, "查找淘汰回归");
        let mut sequence = 0;
        let mut append = |state: &mut ProjectLogState, text: &str| {
            sequence += 1;
            state.adopt(LogEvent {
                owner: state.owner.clone(),
                generation: state.generation,
                sequence,
                task: LogTask::default(),
                source: LogSource::Stderr,
                kind: LogEventKind::Line,
                text: text.into(),
                spans: Vec::new(),
            });
        };
        append(&mut state, "初始警告");
        for _ in 2..CACHE_LINES { append(&mut state, "编译输出"); }
        append(&mut state, "最后警告");
        let mut search = LogSearch { shown: true, needle: "警告".into(), ..default() };
        search.index(&state);
        assert!(!search.ready(&state));
        for _ in 0..CACHE_LINES / SEARCH_FRAME_LINES { search.index(&state); }
        assert!(search.ready(&state));
        assert_eq!(search.count(), 2);
        search.navigate(&state, SearchDirection::Next);
        assert_eq!(search.active_offset(&state), Some("初始".len() as u64));

        append(&mut state, "新输出");
        search.index(&state);
        assert_eq!(state.lines.len(), CACHE_LINES);
        assert_eq!(search.count(), 1);
        assert!(search.active_offset(&state).is_none());
        search.navigate(&state, SearchDirection::Previous);
        let offset = search.active_offset(&state).unwrap();
        assert!(state.rows().any(|(start, row)| row == "最后警告" && offset == start + "最后".len() as u64));
    }
}
