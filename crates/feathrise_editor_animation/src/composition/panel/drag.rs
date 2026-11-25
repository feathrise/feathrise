//! 内容行和工作区页签共用编辑器按钮的实际指针位置，松开到另一个 GLB 才交给原编辑命令。

use super::*;
use feathrise_editor_animation_model::request::{GlbContentDropTarget, checked_clip_path};

pub(super) fn install(app: &mut App) {
    app.add_systems(Update, drop_onto_file.before(SceneEditorSystemSet::Input));
}

fn drop_onto_file(
    context: AnimationContext,
    preview: Res<AssetPreviewState>,
    media: Res<AssetPreviewWorkspace>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    mut panel: ResMut<State>,
    mut state: ResMut<AnimationEditorState>,
    buttons: Query<(
        &Button,
        &RelativeCursorPosition,
        &InheritedVisibility,
        &ComputedNode,
    )>,
    mut tabs: Query<(
        &GlbContentDropTarget,
        &RelativeCursorPosition,
        &InheritedVisibility,
        &mut EditorButtonClick,
    )>,
) {
    let Some(mouse) = mouse.as_deref() else {
        return;
    };
    let hovered_slot = || buttons.iter().find_map(|(b, c, v, n)| match b {
        Button::Slot(row) if c.cursor_over() && v.get() && !n.is_empty() => Some(*row), _ => None,
    });
    if !context.available(&preview, &media)
        || !media.active(context.host.tab)
        || context.host.weights_own_viewport
        || context.guard.blocked()
        || !panel.open
        || state.busy()
    {
        if panel.drag.is_some() {
            panel.drag = None;
        }
        if panel.slot_drag.is_some() { panel.slot_drag = None; panel.slot_hover = None; }
        return;
    }
    if mouse.just_pressed(MouseButton::Left) && (panel.kind == ContentKind::Animation || panel.slots_open && panel.kind == ContentKind::Material) {
        let Some(row) = buttons
            .iter()
            .find_map(|(b, cursor, visibility, node)| match b {
                Button::Row(i) if cursor.cursor_over() && visibility.get() && !node.is_empty() => {
                    Some(panel.page * ROWS + i)
                }
                _ => None,
            })
        else {
            return;
        };
        if panel.kind == ContentKind::Material {
            panel.slot_drag = panel.shown.as_ref().map(|doc| (Arc::downgrade(doc), row));
            return;
        }
        // 普通或 Ctrl 单击仍由行按钮处理。拖未选中的行时仅冻结它，不提前改写多选。
        let source = if selected(&panel.spans, panel.kind, row) {
            panel.freeze()
        } else {
            panel
                .shown
                .clone()
                .zip(panel.path.clone())
                .map(|(document, origin)| Copied {
                    document,
                    origin,
                    root: panel.root.clone(),
                    spans: Arc::from([ContentSpan {
                        kind: panel.kind,
                        first: row,
                        last: row,
                    }]),
                })
        };
        panel.drag = source;
    }
    if panel.slot_drag.is_some() && mouse.pressed(MouseButton::Left) {
        let hit = hovered_slot();
        if hit != panel.slot_hover { panel.slot_hover = hit; }
    }
    if !mouse.just_released(MouseButton::Left) { return; }
    if let Some((doc, material)) = panel.slot_drag.take() {
        panel.slot_hover = None;
        if panel.slots_open && panel.shown.as_ref().is_some_and(|shown| std::sync::Weak::ptr_eq(&doc, &Arc::downgrade(shown)))
            && let Some(slot) = hovered_slot() {
            material::drop_material(&mut panel, &mut state, material, slot);
        }
        return;
    }
    if panel.drag.is_none() {
        return;
    }
    let source = panel.drag.take().unwrap();
    if !panel
        .shown
        .as_ref()
        .is_some_and(|doc| Arc::ptr_eq(doc, &source.document))
        || source.root != context.host.asset_root
    {
        return;
    }
    let Some((tab, _, _, mut click)) = tabs
        .iter_mut()
        .find(|(_, cursor, visibility, _)| cursor.cursor_over() && visibility.get())
    else {
        return;
    };
    let Ok(path) = checked_clip_path(&source.root, &tab.0) else {
        return;
    };
    if state.active.as_ref() == Some(&path) {
        return;
    }
    panel.pending_drop = Some((path, source));
    *click = EditorButtonClick::triggered();
    state.status = "正在打开目标 GLB 以导入所选动作".into();
    state.changed();
}
