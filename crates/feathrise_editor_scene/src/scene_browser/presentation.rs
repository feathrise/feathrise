//! 虚拟文件夹模态框、拖拽浮层和固定根目录落点的增量回显。

use feathrise_editor_ui::UiInteraction;
use bevy::prelude::*; // UI节点、文本、交互和资源查询。
use bevy::ui::FocusPolicy; // 模态显隐同步鼠标点透策略。
use bevy::window::PrimaryWindow; // 鼠标物理位置与系统DPI只从实际主窗口读取。

use crate::components::{
    SceneBrowserDragPreview, SceneBrowserDragPreviewText, SceneEditorSceneRootRow, SceneEditorRoot,
    SceneFolderDialog, SceneFolderDialogNameGroup, SceneFolderDialogStatusText, SceneFolderDialogTitle,
}; // 静态BSN实体通过稳定marker增量更新。
use crate::resources::{SceneBrowserDragState, SceneBrowserState, SceneEditorState}; // 模态和拖拽权威状态。
use crate::types::{SceneBrowserDropTarget, SceneBrowserItem, SceneFolderDialogMode}; // 模式标题、根选择和拖拽落点身份。
use crate::ui::management_dialogs::components::{
    FolderConfirmText, FolderConsequencesText, FolderTargetText,
};

/// 同步文件夹共用模态的显隐、命中、标题、输入和诊断。
#[allow(clippy::type_complexity)] // 五个固定文字共用带过滤的查询，避免多组可变Text参数冲突。
pub(crate) fn sync_scene_folder_dialog(
    editor: Res<SceneEditorState>,   // F12关闭时模态必须同步隐藏。
    browser: Res<SceneBrowserState>, // dialog和last_error是唯一回显来源。
    mut roots: Query<
        (Entity, &mut Node, &mut UiInteraction, &mut FocusPolicy),
        With<SceneFolderDialog>,
    >, // 唯一全屏模态根。
    children: Query<&Children>,    // 隐藏时清理固定小层级的旧按钮Interaction。
    mut descendant_interactions: Query<&mut UiInteraction, Without<SceneFolderDialog>>, // 与根查询结构互斥。
    mut name_groups: Query<&mut Node, (With<SceneFolderDialogNameGroup>, Without<SceneFolderDialog>)>, // 显式与可变模态根查询互斥；删除确认隐藏名称输入组。
    mut texts: Query<
        (
            &mut Text,
            Option<&SceneFolderDialogTitle>,
            Option<&SceneFolderDialogStatusText>,
            Option<&FolderTargetText>,
            Option<&FolderConsequencesText>,
            Option<&FolderConfirmText>,
        ),
        Or<(
            With<SceneFolderDialogTitle>,
            With<SceneFolderDialogStatusText>,
            With<FolderTargetText>,
            With<FolderConsequencesText>,
            With<FolderConfirmText>,
        )>,
    >, // 只查询五个固定文字，错误不覆盖目标或影响范围。
) {
    if !(editor.is_changed() || browser.is_changed()) {
        return; // 稳定帧不重写模态UI组件。
    }
    let visible = editor.open && browser.dialog.is_some(); // 只有F12内冻结操作时捕获输入。
    for (entity, mut node, mut interaction, mut focus) in &mut roots {
        let display = if visible {
            Display::Flex // 显示遮罩和居中面板。
        } else {
            Display::None // 完全退出布局。
        };
        if node.display != display {
            node.display = display;
        }
        let policy = if visible {
            FocusPolicy::Block // 可见时阻止所有底层点透。
        } else {
            FocusPolicy::Pass // 隐藏当帧立即释放鼠标命中。
        };
        focus.set_if_neq(policy);
        if !visible {
            *interaction = UiInteraction::None; // 下一次打开从无根按压状态开始。
            for descendant in children.iter_descendants(entity) {
                if let Ok(mut descendant_interaction) = descendant_interactions.get_mut(descendant)
                {
                    *descendant_interaction = UiInteraction::None; // 确认、取消和输入按钮不保留旧Pressed。
                }
            }
        }
    }

    let Some(dialog) = browser.dialog.as_ref() else {
        return; // 隐藏时无需更新占位文本。
    };
    let input_visible = dialog.mode != SceneFolderDialogMode::Delete; // 删除只需要二次确认说明。
    for mut node in &mut name_groups {
        let display = if input_visible {
            Display::Flex // 创建和重命名显示中文输入框。
        } else {
            Display::None // 删除确认不接受名称编辑。
        };
        if node.display != display {
            node.display = display;
        }
    }
    let title = match dialog.mode {
        SceneFolderDialogMode::Create => "新建虚拟场景文件夹",
        SceneFolderDialogMode::Rename => "重命名虚拟场景文件夹",
        SceneFolderDialogMode::Delete => "删除空虚拟场景文件夹",
    }; // 标题明确本操作不会移动真实资产目录。
    let target = match dialog.mode {
        SceneFolderDialogMode::Create => dialog.target_folder_id.as_deref().map_or_else(
            || "创建位置：虚拟根目录".to_owned(),
            |id| format!("创建位置 · 父文件夹 ID：{id}"),
        ),
        SceneFolderDialogMode::Rename => format!(
            "目标文件夹 ID：{}",
            dialog
                .target_folder_id
                .as_deref()
                .unwrap_or("缺失；确认时会拒绝无效目标")
        ),
        SceneFolderDialogMode::Delete => format!(
            "删除目标：{}\n文件夹 ID：{}",
            dialog.name,
            dialog
                .target_folder_id
                .as_deref()
                .unwrap_or("缺失；确认时会拒绝无效目标")
        ),
    }; // 只使用模态冻结身份，不跟随后台目录高亮或把输入中的新名称冒充原目标。
    let consequences = match dialog.mode {
        SceneFolderDialogMode::Create => {
            "只创建编辑器归类记录，不创建磁盘目录，也不移动场景资产。名称最多48个Unicode字符，同级不可重名。"
        }
        SceneFolderDialogMode::Rename => {
            "只改变显示名；稳定 ID、父级、子文件夹及场景归属保持不变，不移动磁盘目录。"
        }
        SceneFolderDialogMode::Delete => {
            "只允许删除空虚拟文件夹。确认时重新校验，不删除任何场景资产。"
        }
    };
    let confirm = match dialog.mode {
        SceneFolderDialogMode::Create => "新建文件夹",
        SceneFolderDialogMode::Rename => "保存名称",
        SceneFolderDialogMode::Delete => "删除空文件夹",
    };
    let status = browser
        .last_error
        .as_deref()
        .unwrap_or("待确认 · 尚未提交本次操作。");
    for (
        mut text,
        title_marker,
        status_marker,
        target_marker,
        consequences_marker,
        confirm_marker,
    ) in &mut texts
    {
        let value = if title_marker.is_some() {
            title
        } else if status_marker.is_some() {
            status
        } else if target_marker.is_some() {
            &target
        } else if consequences_marker.is_some() {
            consequences
        } else if confirm_marker.is_some() {
            confirm
        } else {
            continue;
        };
        if text.0 != value {
            text.0 = value.to_owned(); // 按内容差异写入，输入或错误变化不会重复标脏静态风险说明。
        }
    }
}

/// 同步穿透命中的场景/文件夹拖拽名称浮层及当前落点拒绝原因。
pub(crate) fn sync_scene_browser_drag_preview(
    editor: Res<SceneEditorState>,    // F12关闭时立即隐藏浮层。
    drag: Res<SceneBrowserDragState>, // 活动态、落点诊断和冻结名称。
    mut previews: Query<(&mut Node, &mut FocusPolicy), With<SceneBrowserDragPreview>>, // 唯一浮层根始终穿透命中。
    mut texts: Query<
        (&mut Text, &mut FocusPolicy),
        (
            With<SceneBrowserDragPreviewText>,
            Without<SceneBrowserDragPreview>,
        ),
    >, // 显式与可变浮层根查询互斥；名称子节点也必须穿透命中。
) {
    if !(editor.is_changed() || drag.is_changed()) {
        return; // 稳定帧不改正文；位置单独在布局后按物理鼠标同步。
    }
    let visible = editor.open && drag.active; // 阈值内普通单击不显示浮层。
    for (mut node, mut focus) in &mut previews {
        focus.set_if_neq(FocusPolicy::Pass);
        let display = if visible {
            Display::Flex // 超过六物理像素后显示。
        } else {
            Display::None // 取消、松开或关闭F12立即隐藏。
        };
        if node.display != display {
            node.display = display;
        }
    }
    let label = drag
        .press
        .as_ref()
        .map(|press| {
            let kind = if matches!(press.item, SceneBrowserItem::Folder(_)) {
                "文件夹"
            } else {
                "场景"
            };
            let error = drag
                .drop_error
                .as_deref()
                .map(|error| format!("\n无法放置：{error}"))
                .unwrap_or_default();
            if let Some(selection) = drag.selection.as_ref() {
                format!(
                    "移动 {} 个场景、{} 个文件夹{error}",
                    selection.scenes, selection.folders
                )
            } else {
                format!("拖动{kind}：{}{error}", press.display_name)
            }
        })
        .unwrap_or_else(|| "拖动目录项".to_owned()); // 防御性占位不会在正常活动态出现。
    for (mut text, mut focus) in &mut texts {
        focus.set_if_neq(FocusPolicy::Pass); // 子文本不能遮住光标右下方的文件夹或根目录落点。
        if text.0 != label {
            text.0 = label.clone(); // 冻结名称或落点诊断变化时才更新正文。
        }
    }
}

/// 使用本帧真实文本尺寸贴近鼠标；物理坐标计算避让，写UI平移时才除一次最终倍率。
pub(crate) fn position_scene_browser_drag_preview(
    windows: Query<&Window, With<PrimaryWindow>>,
    roots: Query<
        (&ComputedNode, &UiGlobalTransform),
        (
            With<SceneEditorRoot>,
            Without<SceneBrowserDragPreview>,
            Without<SceneBrowserDragPreviewText>,
        ),
    >,
    mut previews: Query<
        (
            &Node,
            &ComputedNode,
            &mut UiTransform,
            &mut UiGlobalTransform,
        ),
        (
            With<SceneBrowserDragPreview>,
            Without<SceneBrowserDragPreviewText>,
        ),
    >,
    mut texts: Query<
        &mut UiGlobalTransform,
        (
            With<SceneBrowserDragPreviewText>,
            Without<SceneBrowserDragPreview>,
        ),
    >,
) {
    let Ok((node, computed, mut transform, mut global)) = previews.single_mut() else {
        return;
    };
    if node.display == Display::None {
        return;
    }
    let (Ok(window), Ok((root, root_transform))) = (windows.single(), roots.single()) else {
        return;
    };
    let Some(cursor) = window.physical_cursor_position() else {
        return;
    };
    let inverse = root.inverse_scale_factor();
    if root.size().min_element() <= 0.0
        || computed.size().min_element() <= 0.0
        || !inverse.is_finite()
        || inverse <= 0.0
    {
        return; // 首次根布局尚未完成时不猜测UiScale或把窗口逻辑坐标当成编辑器坐标。
    }
    let bounds = Rect::from_center_size(root_transform.translation, root.size());
    let gap = Vec2::splat(12.0 * window.scale_factor()); // 与鼠标保持12窗口逻辑像素距离，不随编辑器倍率漂移。
    let size = computed.size(); // 已包含本帧中文换行和拒绝原因的实际物理尺寸。
    let mut position = cursor + gap;
    for axis in 0..2 {
        if position[axis] + size[axis] > bounds.max[axis] {
            position[axis] = cursor[axis] - gap[axis] - size[axis]; // 右/下空间不足就翻到鼠标另一侧。
        }
    }
    position = position.clamp(bounds.min, (bounds.max - size).max(bounds.min)); // 同时尊重F12根的实际偏移和窗口边界。
    let delta = position + size * 0.5 - global.translation;
    if delta.length_squared() <= 0.0001 {
        return;
    }
    let previous = match (transform.translation.x, transform.translation.y) {
        (Val::Px(x), Val::Px(y)) => Vec2::new(x, y),
        _ => Vec2::ZERO,
    };
    let translation = previous + delta * inverse;
    transform.translation = Val2::px(translation.x, translation.y);
    let mut affine = global.affine();
    affine.translation += delta;
    *global = affine.into();
    for mut text in &mut texts {
        let mut affine = text.affine();
        affine.translation += delta;
        *text = affine.into(); // 布局后同步两个固定节点，同帧文字与铜边一起移动。
    }
}

/// 固定根目录行按选择、点击或拖拽状态复用真实目录行视觉。
pub(crate) fn sync_scene_browser_root_row(
    browser: Res<SceneBrowserState>,  // 根目录可以成为明确的新建上下文选择。
    drag: Res<SceneBrowserDragState>, // 根目录落点在拖拽期间强化底色与铜边。
    mut rows: Query<
        (
            Ref<UiInteraction>,     // 只在悬停、按下或资源变化时重写视觉。
            &mut BackgroundColor, // 点击与有效落点使用现有水墨铜金反馈。
            &mut BorderColor,     // 有效拖拽落点使用强铜边。
        ),
        With<SceneEditorSceneRootRow>,
    >,
) {
    let state_changed = browser.is_changed() || drag.is_changed(); // 选择与落点变化都需要刷新固定行。
    let selected = browser.selected_item.as_ref() == Some(&SceneBrowserItem::Root); // 根高亮与文件夹、场景使用同一整行语义。
    let highlighted = matches!(drag.drop_target, Some(SceneBrowserDropTarget::Root)); // 场景与文件夹都可拖回根目录。
    for (interaction, mut background, mut border) in &mut rows {
        if !state_changed && !interaction.is_changed() {
            continue; // 稳定帧不重写单行Node或颜色组件。
        }
        let (background_color, border_color) =
            crate::ui::scene_sidebar::tree_row_colors(selected, highlighted, *interaction);
        background.set_if_neq(BackgroundColor(background_color));
        border.set_if_neq(border_color); // 根行与固定虚拟行池使用同一交互语义。
    }
}
