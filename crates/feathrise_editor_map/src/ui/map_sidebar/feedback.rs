//! 只记录本页原按钮真正产生的诊断，身份或事务阶段换代后拒绝旧回显。

use super::{resources::MapSidebarFeedback, types::FeedbackContext};
use crate::resources::{MapBrowserState, MapEditorState};
use feathrise_map_content::MapContentRegistry;

impl MapSidebarFeedback {
    pub(crate) fn record(
        &mut self,
        editor: &MapEditorState,
        browser: &MapBrowserState,
        registry: Option<&MapContentRegistry>,
        deletion_busy: bool,
    ) {
        self.context = Some(FeedbackContext {
            selected: browser.selected_item.clone(),
            selection_revision: browser.selection.revision,
            current_map: editor.selected_map_id.clone(),
            pending_map: editor.pending_map_transition.clone(),
            folder_revision: browser.document_revision,
            catalog_revision: registry.map_or(0, |registry| registry.revision),
            map_dialog: editor.new_map_dialog_open,
            folder_dialog: browser.dialog.is_some(),
            switch_confirmation: editor.map_switch_confirmation.is_some(),
            deletion_busy,
            dirty: editor.dirty,
            transitioning: editor.map_transition_in_progress,
        });
        self.message = editor.status.clone(); // 原入口的真实结果，不从后来底栏文字反向推测成功或失败。
    }

    pub(super) fn current_message<'a>(
        &'a self,
        editor: &MapEditorState,
        browser: &MapBrowserState,
        registry: Option<&MapContentRegistry>,
        deletion_busy: bool,
    ) -> &'a str {
        self.context
            .as_ref()
            .filter(|context| {
                context.selected == browser.selected_item
                    && context.selection_revision == browser.selection.revision
                    && context.current_map == editor.selected_map_id
                    && context.pending_map == editor.pending_map_transition
                    && context.folder_revision == browser.document_revision
                    && context.catalog_revision == registry.map_or(0, |registry| registry.revision)
                    && context.map_dialog == editor.new_map_dialog_open
                    && context.folder_dialog == browser.dialog.is_some()
                    && context.switch_confirmation == editor.map_switch_confirmation.is_some()
                    && context.deletion_busy == deletion_busy
                    && context.dirty == editor.dirty
                    && context.transitioning == editor.map_transition_in_progress
            })
            .map_or("", |_| self.message.as_str())
    }
}
