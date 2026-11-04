//! 植被库、空白画布、完整编译和密度发布的身份化回显；不扫描R8、运行时实例或目录。

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use feathrise_map_content::VegetationPresetRegistry;
use feathrise_scene::DynamicBladeDraft;

use crate::authoring::{
    EditorAuthoringSession, EditorMapDraft, VegetationBrushStroke, VegetationCompileState,
};
use crate::resources::MapEditorState;
use crate::types::EditorTool;
use crate::ui::sidebars::{EditorLeftSidebarTab, MapEditorSidebarState};
use crate::ui::theme as colors;
use crate::ui::vegetation_layers::{VegetationBlankLayerAllocation, vegetation_canvas_unavailable};
use crate::viewport::modes::{ViewportMode, ViewportModeState};

use super::{VegetationSidebarFeedbackKind, VegetationSidebarFeedbackText};

#[derive(SystemParam)]
pub(super) struct VegetationSidebarFeedback<'w> {
    session: Res<'w, EditorAuthoringSession>,
    registry: Option<Res<'w, VegetationPresetRegistry>>,
    allocation: Option<Res<'w, VegetationBlankLayerAllocation>>,
    compile: Option<Res<'w, VegetationCompileState>>,
    published: Option<Res<'w, DynamicBladeDraft>>,
    stroke: Res<'w, VegetationBrushStroke>,
}

impl VegetationSidebarFeedback<'_> {
    fn current_draft(&self, editor: &MapEditorState) -> Option<&EditorMapDraft> {
        self.session
            .draft
            .as_ref()
            .filter(|draft| Some(&draft.map_id) == editor.selected_map_id.as_ref())
    }

    fn creation_target(&self, editor: &MapEditorState) -> String {
        if let Some(id) = editor.vegetation_selected_preset_id.as_deref() {
            return match self.registry.as_deref() {
                Some(registry) => match registry.resolve(id) {
                    Some(preset) => format!(
                        "新建层预设：{}\n只在当前地图创建空白作者画布，不修改全局预设。",
                        preset.label
                    ),
                    None => format!("新建层预设暂不可用：{id}；请重新选择。"),
                },
                None => format!("新建层预设：{id}；等待目录加载，已有正式输出仍按原规则复用。"),
            }; // 永远读取地图建层选择；中央正在编辑的另一份草稿不能冒充它。
        }
        let layer = self
            .current_draft(editor)
            .and_then(|draft| draft.vegetation.as_ref())
            .and_then(|vegetation| vegetation.authoring.as_ref())
            .and_then(|authoring| {
                authoring.layers.get(
                    editor
                        .selected_vegetation_layer
                        .min(authoring.layers.len().saturating_sub(1)),
                )
            });
        layer.map_or_else(
            || "新建层预设：尚未选择；先在上方全局库选择，再新建空白层。".to_owned(),
            |layer| {
                format!(
                    "新建层预设：沿用“{}”的正式输出；新建仍为空白画布。",
                    layer.label
                )
            },
        ) // 原创建事务允许沿用所选作者层；这里仅O(1)取作者描述，不解析或创建正式输出。
    }

    fn availability(&self, editor: &MapEditorState, mode: &ViewportModeState) -> String {
        let Some(draft) = self.current_draft(editor) else {
            return "等待当前地图作者数据；全局植被预设仍可管理。".into();
        };
        if self.session.saving {
            return "正在保存当前地图；等待完整作者事务完成。".into();
        }
        if let Some(message) = vegetation_canvas_unavailable(editor, draft) {
            return message.into();
        }
        if self.allocation.as_ref().is_some_and(|allocation| {
            allocation.pending_for(editor, &self.session, self.registry.as_deref())
        }) {
            return "正在后台准备当前预设的空白R8画布；完整采用后才新增图层和撤销记录。".into();
        }
        if self.stroke.active {
            return "当前植被笔划正在进行或收尾；松开后再修改图层结构。".into();
        }
        if draft
            .vegetation
            .as_ref()
            .and_then(|vegetation| vegetation.authoring.as_ref())
            .is_some_and(|authoring| authoring.layers.is_empty())
        {
            return "当前网格尚无植被作者层；可用所选预设创建第一张空白画布。".into();
        }
        if mode.mode == ViewportMode::Edit && editor.tool == EditorTool::Vegetation {
            "视口：植被编辑；隐藏、锁定或无有效作者层时，原笔刷继续拒绝落笔。".into()
        } else {
            "视口：未启用植被笔刷；选择模式不落笔，图层按钮及属性仍走原作者事务。".into()
        }
    }

    fn compilation(&self, editor: &MapEditorState) -> (String, Color) {
        let waiting = |message: &str| (message.to_owned(), colors::TEXT_MUTED);
        let Some(draft) = self.current_draft(editor) else {
            return waiting("编译：等待当前地图植被作者数据。");
        };
        if let Some(message) = vegetation_canvas_unavailable(editor, draft) {
            return (format!("编译：{message}"), colors::TEXT_MUTED);
        }
        if self.compile.as_ref().is_some_and(|state| {
            state.failed_map_id.as_ref() == Some(&draft.map_id)
                && state.failed_revision == Some(self.session.vegetation_revision)
        }) {
            return (
                "编译：当前修订失败，保留上一完整正式结果；原因见最近操作。".into(),
                colors::DANGER_TEXT,
            );
        }
        if self.session.vegetation_compiled_revision == self.session.vegetation_revision {
            return (
                format!(
                    "编译：作者与正式输出修订{}一致；不代表GPU已经显示。",
                    self.session.vegetation_revision
                ),
                colors::TEXT_VALUE,
            );
        }
        if self.stroke.active {
            return waiting("编译：笔划期间合并最新修订，松开后再编译完整结果。");
        }
        if self.compile.as_ref().is_some_and(|state| {
            state.task.is_some()
                && state.desired_map_id.as_ref() == Some(&draft.map_id)
                && state.desired_revision == self.session.vegetation_revision
        }) {
            return waiting("编译：后台任务处理中；新修订可能仍在等待旧任务归还唯一槽。");
        }
        waiting("编译：等待当前完整作者修订，继续保留上一完整结果。")
    }

    fn preview(&self, editor: &MapEditorState) -> String {
        let Some(draft) = self.current_draft(editor) else {
            return "预览：等待当前地图植被数据。".into();
        };
        if let Some(message) = vegetation_canvas_unavailable(editor, draft) {
            return format!("预览：{message}");
        }
        if !draft.terrain_manifest().is_data_driven() {
            return format!(
                "覆盖预览：{}；固定场景草叶与模型沿用原接管链，编译完成不等于画面就绪。",
                editor.vegetation_preview_mode.label()
            );
        }
        let current = self.session.vegetation_compiled_revision == self.session.vegetation_revision
            && self
                .published
                .as_ref()
                .and_then(|published| published.for_map(&draft.map_id))
                .is_some_and(|snapshot| {
                    snapshot.source_revision == self.session.vegetation_revision
                });
        if current {
            "预览：当前完整密度已发布；草叶GPU、模型及高度仍按原加载链就绪，空白层不显示植被。"
                .into()
        } else {
            "预览：等待当前完整密度发布；上一版画面不代表本次完成，空白层不显示植被。".into()
        } // 只验证同图同修订CPU发布回执，绝不把它冒充RenderApp或真实可见性回执。
    }
}

pub(super) fn sync_feedback(
    editor: Res<MapEditorState>,
    sidebar: Res<MapEditorSidebarState>,
    mode: Res<ViewportModeState>,
    feedback: VegetationSidebarFeedback,
    mut texts: Query<(&VegetationSidebarFeedbackText, &mut Text, &mut TextColor)>,
) {
    if !editor.open || !sidebar.tool_page_visible(EditorLeftSidebarTab::Vegetation) {
        return;
    }
    let creation = feedback.creation_target(&editor);
    let availability = feedback.availability(&editor, &mode);
    let (compilation, compilation_color) = feedback.compilation(&editor);
    let preview = feedback.preview(&editor);
    let catalog = feedback
        .registry
        .as_ref()
        .and_then(|registry| registry.last_error.as_deref());
    let catalog_message = catalog.map_or_else(String::new, |error| {
        format!("全局库更新失败：{error}；保留上一完整目录。")
    });
    for (marker, mut text, mut color) in &mut texts {
        let (value, tint) = match marker.0 {
            VegetationSidebarFeedbackKind::CreationTarget => {
                (creation.as_str(), colors::TEXT_VALUE)
            }
            VegetationSidebarFeedbackKind::Availability => {
                (availability.as_str(), colors::TEXT_MUTED)
            }
            VegetationSidebarFeedbackKind::Compile => (compilation.as_str(), compilation_color),
            VegetationSidebarFeedbackKind::Preview => (preview.as_str(), colors::TEXT_MUTED),
            VegetationSidebarFeedbackKind::Catalog => {
                (catalog_message.as_str(), colors::DANGER_TEXT)
            }
            VegetationSidebarFeedbackKind::Message => (editor.status.as_str(), colors::TEXT_MUTED),
        };
        if text.as_str() != value {
            **text = value.to_owned();
        }
        if color.0 != tint {
            color.0 = tint;
        }
    } // 六个常驻节点只在实际文字或颜色变化时更新，不生成实体或扫描图片与场景。
}
