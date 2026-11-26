//! 作用域和标签只改变侧栏投影。编辑、保存与撤销仍交给同一份规则文档。
use super::{Studio, types::*, view::{Action, OPTIONS, ROWS}};
use bevy::{ecs::system::SystemParam, input_focus::InputFocus, prelude::*};
use feathrise_scene_content::{SceneCatalogAsset, SceneCatalogEntry, SceneContentRegistry};

#[derive(SystemParam)]
pub(super) struct SceneOptions<'w> {
    registry: Option<Res<'w, SceneContentRegistry>>,
    catalogs: Option<Res<'w, Assets<SceneCatalogAsset>>>,
    workbench: Option<Res<'w, crate::workbench::WorkbenchModel>>,
}
impl SceneOptions<'_> {
    pub fn changed(&self) -> bool {
        self.registry.as_ref().is_some_and(Res::is_changed)
            || self.catalogs.as_ref().is_some_and(Res::is_changed)
            || self.workbench.as_ref().is_some_and(Res::is_changed)
    }
    pub fn entries(&self) -> &[SceneCatalogEntry] {
        let catalog = self.registry.as_ref()
            .and_then(|registry| self.catalogs.as_ref()?.get(&registry.catalog));
        catalog.map(|catalog| catalog.scenes.as_slice())
            .or_else(|| self.workbench.as_ref().map(|model| model.scenes.as_slice()))
            .unwrap_or_default()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ListPicker {
    Scenes,
    Tags,
}

impl Studio {
    pub(super) fn scene(&self, tab: Tab) -> Option<&str> {
        self.scenes[tab as usize].as_deref()
    }
    pub(super) fn tag_filter(&self, tab: Tab) -> &TagFilter {
        &self.tags[tab as usize]
    }
    pub(super) fn rows<'a>(&self, sheet: &'a Sheet, tab: Tab) -> &'a [usize] {
        sheet.visible_rows(tab, self.scene(tab), self.tag_filter(tab))
    }
    pub(super) fn list_option<'a>(
        &self,
        sheet: &'a Sheet,
        scenes: &'a [SceneCatalogEntry],
        index: usize,
    ) -> Option<&'a str> {
        let (picker, base) = self.picker?;
        match (picker, base + index) {
            (ListPicker::Scenes, i) => Some(scenes.get(i)?.id.as_str()),
            (ListPicker::Tags, 0) => Some("全部标签"),
            (ListPicker::Tags, 1) => Some("未标记"),
            (ListPicker::Tags, i) => sheet.filter_tags(self.sel.tab, self.scene(self.sel.tab))
                .get(i - 2).map(String::as_str),
        }
    }
    pub(super) fn list_control(
        &mut self,
        action: Action,
        sheet: &Sheet,
        scenes: &[SceneCatalogEntry],
        active_scene: Option<&str>,
        focus: &mut InputFocus,
    ) -> bool {
        let tab = self.sel.tab;
        if self.editing_item || tab == Tab::Locations {
            return false;
        }
        match action {
            Action::ScopeProject => self.change_scope(None, sheet, focus),
            Action::ScopeScene => {
                let scene = self.scene(tab).or(active_scene).or_else(|| {
                    scenes.first().map(|c| c.id.as_str())
                }).map(str::to_owned);
                if scene.is_some() {
                    self.change_scope(scene, sheet, focus);
                } else {
                    self.notice = "请先新建场景。".into();
                }
            }
            Action::ScopeScenePicker | Action::ListTagPicker => {
                let picker = if action == Action::ScopeScenePicker { ListPicker::Scenes } else { ListPicker::Tags };
                self.picker = (self.picker.is_none_or(|(existing, _)| existing != picker)).then_some((picker, 0));
            }
            Action::ListOption(i) => {
                if let Some((picker, base)) = self.picker
                    && let Some(option) = self.list_option(sheet, scenes, i)
                {
                    match picker {
                        ListPicker::Scenes => self.change_scope(Some(option.to_owned()), sheet, focus),
                        ListPicker::Tags => {
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
                    }
                }
                self.picker = None;
            }
            Action::ListOptionsPrevious | Action::ListOptionsNext => {
                if let Some((picker, base)) = self.picker {
                    let next = if action == Action::ListOptionsPrevious {
                        base.saturating_sub(OPTIONS)
                    } else {
                        base + OPTIONS
                    };
                    self.picker = Some((picker, next));
                    if self.list_option(sheet, scenes, 0).is_none() {
                        self.picker = Some((picker, base));
                    }
                }
            }
            Action::ListOptionsClose => self.picker = None,
            _ => return false,
        }
        true
    }
    fn change_scope(&mut self, scene: Option<String>, sheet: &Sheet, focus: &mut InputFocus) {
        let tab = self.sel.tab;
        if self.scene(tab) != scene.as_deref() {
            if self.has_unsaved_input() {
                self.notice = "请先保存或取消本次修改。".into();
                return;
            }
            self.scenes[tab as usize] = scene;
            self.tags[tab as usize] = TagFilter::All;
            let selection = Selection { tab, item: self.rows(sheet, tab).first().copied(), ..default() };
            self.select(selection, false, None, focus);
        }
        self.picker = None;
    }
}

/// 分页使用可见行号，实际变更仍使用当前作用域里的原始索引。
pub(super) fn page(selection: Selection, rows: &[usize]) -> usize {
    selection.item.and_then(|i| rows.binary_search(&i).ok()).unwrap_or(0) / ROWS
}

pub(super) fn neighbor(rows: &[usize], item: usize) -> Option<usize> {
    let at = rows.binary_search(&item).ok()?;
    rows.get(at + 1)
        .or_else(|| at.checked_sub(1).and_then(|i| rows.get(i)))
        .copied()
}
