//! 资产菜单的纯身份校验、缩放坐标换算和剪贴板反馈，稳定帧不做文件IO。

use bevy::clipboard::ClipboardError; // 复制失败必须给出真实结果，不伪报成功。
use bevy::prelude::{Rect, Vec2}; // 所有边界计算统一使用UI逻辑像素。
use std::path::{Component, Path, PathBuf};
use super::{constants::MAX_COPY_PATH_BYTES, messages::CopyAssetPath, types::AssetPathCopyKind};

use super::resources::AssetContextMenuState; // 只读所有权由统一编辑器鼠标门消费。
use super::types::AssetMenuTarget; // 冻结路径、页索引和版本。
use crate::resources::{EditorAssetCatalog, MapEditorState}; // 校验只借用现有目录，不改变作者数据。

impl AssetContextMenuState {
    /// 菜单活动及其关闭帧都独占鼠标，避免复制按钮同时绘制地图。
    pub(crate) fn owns_mouse(&self) -> bool {
        self.target.is_some() || self.captured_this_frame // 松键隔离继续交给既有公共门。
    }
}

/// 刷新、可见窗口换绑、关闭或切图立即取消旧菜单，不等待下标恰好重用。
pub(super) fn target_is_current(
    target: &AssetMenuTarget,     // 右键按下时的稳定快照。
    catalog: &EditorAssetCatalog, // 当前已公开目录。
    editor: &MapEditorState,      // 当前F12会话。
) -> bool {
    editor.open // 隐藏编辑器绝不能留下悬浮菜单。
        && !editor.close_requested // 安全关闭等待也结束浏览操作。
        && editor.pending_map_transition.is_none() // 切图请求已经使旧菜单失效。
        && editor.selected_map_id == target.map_id // 不把另一张图的会话当作同一来源。
        && catalog.requested_revision == target.revision // 手动刷新尚未完成也立即取消。
        && catalog.applied_revision == target.revision // 后台换代后拒绝旧行身份。
        && catalog.viewport_revision == target.viewport_revision
        && target.index >= catalog.visible_start
        && target.index < catalog.visible_start.saturating_add(catalog.visible_count)
        && catalog.entries.get(target.index).is_some_and(|entry| entry.asset_path == target.path) // O(1)复核完整路径，文件和目录都可复制。
}

/// 物理鼠标位置只除一次最终UI倍率，支持系统DPI与游戏UiScale叠加。
pub(crate) fn logical_anchor(
    cursor: Vec2,       // Window::physical_cursor_position提供的物理像素。
    root: Rect,         // F12根的实际物理矩形。
    inverse_scale: f32, // ComputedNode提供的完整倍率倒数。
) -> Option<Vec2> {
    (cursor.is_finite()
        && root.min.is_finite()
        && root.max.is_finite()
        && root.size().min_element() > 0.0
        && inverse_scale.is_finite()
        && inverse_scale > 0.0)
        .then(|| (cursor - root.min) * inverse_scale) // 保留根偏移，不能只用窗口逻辑坐标估算。
}

/// 菜单保持紧凑，靠近右下角时向窗口内收，不改变资产列表布局。
#[cfg(test)]
pub(crate) fn menu_rect(anchor: Vec2, viewport: Vec2) -> Rect {
    sized_menu_rect(anchor, viewport, Vec2::new(240.0, 164.0))
}

/// 不同条目数共用根内锚点换算，按可见项适配尺寸，标签组空白保持紧凑。
pub(crate) fn sized_menu_rect(anchor: Vec2, viewport: Vec2, desired: Vec2) -> Rect {
    let viewport = viewport.max(Vec2::ZERO); // 防御尚未完成或极小的布局尺寸。
    let size = desired.min(viewport); // 极小窗口也不超出根范围。
    let maximum = (viewport - size - Vec2::splat(4.0)).max(Vec2::ZERO); // 右下边缘留四逻辑像素。
    let position = (anchor + Vec2::splat(4.0)).clamp(Vec2::ZERO, maximum); // 鼠标右下展开，空间不足时自动避让。
    Rect::from_corners(position, position + size) // 位置与尺寸共用同一逻辑坐标系。
}

/// 注入写入函数便于测试，生产路径只调用一次系统剪贴板。
pub(super) fn copy_path_status(
    request: &CopyAssetPath,
    root: &Path,
    write: impl FnOnce(&str) -> Result<(), ClipboardError>, // 不自动重试或访问任意其他剪贴板内容。
) -> String {
    let description = request.kind.description();
    match path_to_copy(root, &request.path, request.kind) {
        Ok(path) => copy_asset_text_status(description, &path, write),
        Err(error) => format!("复制{description}失败：{error}"), // 路径无效时不覆盖用户原剪贴板。
    }
}

/// 相对复制以当前项目为根；绝对复制对原始assets根做词法归一，源文件删除或目录为链接时仍可复制地址。
pub(super) fn path_to_copy(root: &Path, relative: &str, kind: AssetPathCopyKind) -> Result<String, String> {
    if relative.is_empty() || relative.len() > MAX_COPY_PATH_BYTES || relative.contains('\\')
        || Path::new(relative).components().any(|part| !matches!(part, Component::Normal(_))) {
        return Err("只能复制assets内的资产相对路径".into());
    }
    // 浏览器内部身份仍相对assets；只在复制输出处加项目内目录前缀，不改资产引用或追踪目录链接。
    if kind == AssetPathCopyKind::Relative { return Ok(format!("assets/{relative}")); }
    if !root.is_absolute() || root.as_os_str().len() > MAX_COPY_PATH_BYTES {
        return Err("无法确定当前assets的绝对路径".into());
    }
    let mut absolute = PathBuf::new();
    for part in root.components() {
        match part {
            Component::CurDir => {},
            Component::ParentDir => { if !absolute.pop() { return Err("assets根路径无效".into()); } },
            _ => absolute.push(part.as_os_str()),
        }
    }
    for part in Path::new(relative).components() { absolute.push(part.as_os_str()); } // Windows统一原生分隔符，相对复制仍保留原正斜杠。
    crate::ui::external_files::platform_path(&absolute).into_os_string().into_string()
        .map_err(|_| "绝对路径包含无法复制的字符".into()) // 保留真实Unicode，不用有损转换生成错误地址。
}

/// 文件路径和预览完整引用共用一次平台写入与一致的成功/失败语义。
pub(crate) fn copy_asset_text_status(
    description: &str, // 状态栏明确区分资产路径、网格引用和材质引用。
    value: &str,       // 调用方已经验证的最终文本，不在这里隐式改写。
    write: impl FnOnce(&str) -> Result<(), ClipboardError>, // 只执行用户本次明确授权的一次写入。
) -> String {
    match write(value) {
        Ok(()) => format!("已复制{description}：{value}"), // 只有系统剪贴板真实成功才提示完成。
        Err(error) => format!("复制{description}失败：{error}；请重新点击复制按钮"), // 失败不改变选择、预览或作者数据。
    } // 两个入口不维护互相矛盾的反馈格式。
}
