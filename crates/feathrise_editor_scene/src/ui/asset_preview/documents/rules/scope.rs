//! 列表只投影入口确定的作用域，筛选标签和分页继续使用原条目索引。
use super::{Studio, types::*, view::{Action, OPTIONS, ROWS}};
use bevy::{input_focus::InputFocus, prelude::*};

impl Studio {
    pub(super) fn scene(&self, tab: Tab) -> Option<&str> {
        let scene = self.scene_scope.as_ref()?;
        (tab != Tab::Locations).then_some(scene.as_str())
    }
    pub(super) fn tag_filter(&self, tab: Tab) -> &TagFilter {
        &self.tags[tab as usize]
    }
    pub(super) fn rows<'a>(&self, sheet: &'a Sheet, tab: Tab) -> &'a [usize] {
        sheet.visible_rows(tab, self.scene(tab), self.tag_filter(tab))
    }
    pub(super) fn list_option<'a>(&self, sheet: &'a Sheet, index: usize) -> Option<&'a str> {
        match self.picker? + index {
            0 => Some("全部标签"),
            1 => Some("未标记"),
            i => sheet.filter_tags(self.sel.tab, self.scene(self.sel.tab))
                .get(i - 2).map(String::as_str),
        }
    }
    pub(super) fn list_control(&mut self, action: Action, sheet: &Sheet, focus: &mut InputFocus) -> bool {
        let tab = self.sel.tab;
        if self.editing_item || tab == Tab::Locations {
            return false;
        }
        match action {
            Action::ListTagPicker => self.picker = self.picker.is_none().then_some(0),
            Action::ListOption(i) => {
                if let Some(base) = self.picker
                    && let Some(option) = self.list_option(sheet, i)
                {
                    if self.has_unsaved_input() {
                        self.notice = "请先保存或取消本次修改。".into();
                        return true;
                    }
                    self.tags[tab as usize] = match base + i {
                        0 => TagFilter::All,
                        1 => TagFilter::Untagged,
                        _ => TagFilter::Tag(option.to_owned()),
                    };
                    let selection = Selection { tab, item: self.rows(sheet, tab).first().copied(), ..default() };
                    self.select(selection, false, None, focus);
                }
                self.picker = None;
            }
            Action::ListOptionsPrevious => {
                self.picker = self.picker.map(|base| base.saturating_sub(OPTIONS));
            }
            Action::ListOptionsNext => {
                if self.list_option(sheet, OPTIONS).is_some() {
                    self.picker = self.picker.map(|base| base + OPTIONS);
                }
            }
            Action::ListOptionsClose => self.picker = None,
            _ => return false,
        }
        true
    }
}

/// 变量与规则的页号独立于正在编辑的行。删除最后一页的最后一条时收回到新的末页。
pub(super) fn page(selection: Selection, rows: &[usize]) -> usize {
    if selection.tab == Tab::Locations {
        // 地点表单仍嵌在行内，沿用由当前编辑行定位的分页。
        selection.item.and_then(|i| rows.binary_search(&i).ok()).unwrap_or(0) / ROWS
    } else {
        selection.list_page.min(rows.len().saturating_sub(1) / ROWS)
    }
}

pub(super) fn neighbor(rows: &[usize], item: usize) -> Option<usize> {
    let at = rows.binary_search(&item).ok()?;
    rows.get(at + 1)
        .or_else(|| at.checked_sub(1).and_then(|i| rows.get(i)))
        .copied()
}
