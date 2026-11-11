//! 固定操作文案、真实快捷键回显及根内避让；不扫描场景或估算文字宽度。

use bevy::prelude::*;
use feathrise_editor_core::EditorSettings;
use feathrise_editor_core::EditorShortcutAction;

use super::types::{TooltipAction, TooltipCopy};
use crate::types::{EditorAction, EditorLayoutAction};
use crate::ui::action_availability::EditorActionAvailability;

pub(super) fn tooltip_copy(action: TooltipAction, mesh_edit: bool) -> TooltipCopy {
    let (title, description, shortcut, fixed_key) = match action {
        TooltipAction::Editor(EditorAction::Undo) => (
            "撤销",
            "撤销当前场景的上一次作者修改，不影响其他文档。",
            Some(EditorShortcutAction::Undo),
            None,
        ),
        TooltipAction::Editor(EditorAction::Redo) => (
            "重做",
            "恢复当前场景刚撤销的作者修改。",
            Some(EditorShortcutAction::Redo),
            None,
        ),
        TooltipAction::Editor(EditorAction::Save) => (
            "保存场景",
            "保存当前场景草稿；预设与资产文档使用各自的保存入口。",
            Some(EditorShortcutAction::Save),
            None,
        ),
        TooltipAction::Editor(EditorAction::NewScene) => (
            "新建场景",
            "填写场景名称和可选备注，在当前目录创建空白场景。",
            None,
            None,
        ),
        TooltipAction::Editor(EditorAction::AssetFolderUp) => (
            "上级目录", "返回当前目录的上一级。", None, None,
        ),
        TooltipAction::Editor(EditorAction::RefreshAssets) => (
            "刷新", "重新读取当前目录。", None, None,
        ),
        TooltipAction::Editor(EditorAction::Close) => (
            "启动",
            "收起编辑器并返回当前场景，不自动保存或丢弃草稿。",
            None,
            None,
        ),
        TooltipAction::Editor(EditorAction::ViewPerspective) => (
            "自由视图",
            "恢复自由观察方向；右键旋转，中键平移，滚轮缩放。",
            None,
            None,
        ),
        TooltipAction::Editor(EditorAction::ViewTop) => (
            "顶视",
            "从上方观察当前场景，保留当前位置与投影模式。",
            None,
            None,
        ),
        TooltipAction::Editor(EditorAction::ViewFront) => (
            "前视",
            "切换到前方观察方向，不修改场景或所选对象。",
            None,
            None,
        ),
        TooltipAction::Editor(EditorAction::ViewSide) => (
            "侧视",
            "切换到侧方观察方向，不修改场景或所选对象。",
            None,
            None,
        ),
        TooltipAction::Editor(EditorAction::ToggleOrthographic) => (
            "正交投影",
            "在正交与透视投影间切换，观察方向保持不变。",
            None,
            None,
        ),
        TooltipAction::Editor(EditorAction::FocusSelected) => (
            "聚焦所选",
            "对准已确认的资产、水体或传送点；不采用候选悬停选择。",
            Some(EditorShortcutAction::FocusSelected),
            None,
        ),
        TooltipAction::Editor(EditorAction::FrameScene) => (
            "查看全图",
            "按当前场景边界取景，保留观察方向与投影模式。",
            Some(EditorShortcutAction::FrameScene),
            None,
        ),
        TooltipAction::Layout(EditorLayoutAction::Content) => (
            "导航",
            "显示或收起左侧内容浏览器，保留首选宽度与未保存草稿。",
            None,
            None,
        ),
        TooltipAction::Layout(EditorLayoutAction::Inspector) => (
            "工作区",
            "显示或收起右侧检查器及预览，保留当前选择。",
            None,
            None,
        ),
        TooltipAction::Layout(EditorLayoutAction::Focus) => (
            "专注编辑 / 展开两侧",
            "隐藏两侧以扩大工作区；再次点击展开左右两侧，保留各自宽度。",
            None,
            None,
        ),
        TooltipAction::Settings => (
            "设置",
            "调整界面缩放、配色、布局、吸附及快捷键；不修改作者数据。",
            None,
            None,
        ),
        TooltipAction::TerrainParticipation => (
            "参与场景", "勾选表示参与场景；点击切换。会写入场景，可撤销；锁定时需先解锁。", None, None,
        ),
        TooltipAction::TerrainVisibility => (
            "编辑器显示", "眼睛表示显示，划线表示隐藏；只影响编辑器显示，保留场景参与状态和碰撞。", None, None,
        ),
        TooltipAction::TerrainLock => (
            "锁定 / 解锁", "闭锁时禁止修改网格，仍可选中查看；再次点击解锁。", None, None,
        ),
        TooltipAction::ViewportMode(mode) => {
            use crate::viewport::modes::ViewportMode;
            match (mesh_edit, mode) {
                (true, ViewportMode::Select) => ("选择元素", "单击选择点、边或面；Shift 增减选择，A 全选。", None, None),
                (true, ViewportMode::Move) => ("移动所选元素", "拖动世界轴手柄移动所选点、边或面；G 开始移动，XYZ 约束。", None, Some("G")),
                (true, ViewportMode::Rotate) => ("旋转所选元素", "绕所选元素中心旋转；R 开始旋转，XYZ 约束。单点绕自身中心旋转不改变位置。", None, Some("R")),
                (true, ViewportMode::Scale) => ("缩放所选元素", "围绕所选元素中心缩放；S 开始缩放，XYZ 约束。单点围绕自身中心缩放不改变位置。", None, Some("S")),
                (_, ViewportMode::Select) => ("选择", "选择场景中的对象并查看属性，不落笔修改场景。", None, None),
                (_, ViewportMode::Move) => ("移动", "拖动世界轴或XZ平面手柄移动当前所选对象。", Some(EditorShortcutAction::MoveSelected), None),
                (_, ViewportMode::Rotate) => ("旋转", "绕世界Y轴旋转所选对象；建筑沿用90°档位。", Some(EditorShortcutAction::RotateSelected), None),
                (_, ViewportMode::Scale) => ("缩放", "等比缩放所选普通资产，保持已有三轴比例。", Some(EditorShortcutAction::ScaleSelected), None),
                (_, ViewportMode::Edit) => ("编辑", "恢复当前工具面板上次使用的绘制工具。", None, None),
            } // 同一图标随对象/元素模式回显真实操作及键位，不能让 W/E/R 的对象说明误导网格编辑。
        },
        TooltipAction::ViewportOptions => ("筛选与操作", "展开选择类别与复制、粘贴、删除等对象操作。", None, None),
        TooltipAction::AssetThumbnails => ("缩略图", "在缩略图和列表视图间切换。", None, None),
        TooltipAction::AssetSearch => (
            "内容搜索", "查找文件内容，或展开当前的资产引用结果。", None, Some("Ctrl+Shift+F"),
        ),
        TooltipAction::AssetMissingCheck => (
            "缺失检查", "检查场景和配置引用的资产。再次点击收起结果。", None, None,
        ),
        TooltipAction::AssetFavorites => (
            "收藏", "右键文件或文件夹加入收藏。展开后可定位收藏项和检查路径。", None, None,
        ),
        TooltipAction::AssetRecent => (
            "最近访问", "查看最近打开的文件和目录。展开后可定位条目或清空记录。", None, None,
        ),
        TooltipAction::ViewShading(mode) => (mode.label(), match mode {
            crate::viewport::shading::types::ShadingMode::Wireframe => "显示真实网格三角边；下拉可开启穿透线框。",
            crate::viewport::shading::types::ShadingMode::Solid => "使用真实表面法线显示灰色实体；下拉可增强凹凸轮廓或显示法线颜色。",
            crate::viewport::shading::types::ShadingMode::MaterialPreview => "预览原始材质并使用当前相机专属工作室补光。",
            crate::viewport::shading::types::ShadingMode::Rendered => "显示场景原始材质、灯光和环境。",
        }, None, None),
        TooltipAction::OrientationAxis => ("沿轴观察", "点击轴端点切到对应正交视图；再次点击已对准的端点翻到反方向。", None, None),
        TooltipAction::OrientationProjection => ("切换投影", "在正交与透视间切换，保留当前观察角度和焦点。", None, None),
        _ => unreachable!("只有已登记的编辑器动作具有提示文案"),
    };
    TooltipCopy {
        title,
        description,
        shortcut,
        fixed_key,
    }
}

pub(super) fn tooltip_title(
    copy: &TooltipCopy,
    settings: &EditorSettings,
    locale: feathrise_editor_ui::localization::EditorLocale,
) -> String {
    let key = copy
        .shortcut
        .map(|action| {
            settings
                .editor_keybindings
                .get(action)
                .map_or_else(|| locale.editor_label("未绑定").to_owned(), |binding| binding.display())
        })
        .or_else(|| copy.fixed_key.map(str::to_owned));
    let title = locale.editor_label(copy.title);
    key.map_or_else(|| title.to_owned(), |key| format!("{title} · {key}"))
}

pub(super) fn unavailable_reason(
    action: TooltipAction,
    state: &EditorActionAvailability,
) -> Option<&'static str> {
    match action {
        TooltipAction::Editor(action) => state.reason(action),
        _ => None,
    }
}

/// 读取布局完成后的真实尺寸，先尝试按钮下方，再向上翻转并在F12根内收。
pub(super) fn tooltip_position(anchor: Rect, size: Vec2, viewport: Vec2) -> Vec2 {
    let margin = ((viewport - size) * 0.5).clamp(Vec2::ZERO, Vec2::splat(6.0));
    let maximum = (viewport - size - margin).max(margin);
    let below = anchor.max.y + 6.0;
    let top = if below + size.y <= viewport.y - margin.y {
        below
    } else {
        anchor.min.y - size.y - 6.0
    };
    Vec2::new(anchor.min.x, top).clamp(margin, maximum)
}
