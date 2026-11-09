//! 固定资产预览UI的增量表现同步。

use feathrise_editor_ui::UiInteraction;
use bevy::image::TRANSPARENT_IMAGE_HANDLE; // 非图片阶段立即释放旧UI强句柄。
use bevy::prelude::*; // Node、Text、ImageNode与资源查询。
use crate::ui::theme as colors; // 缩放按钮保持现有F12铜边悬停反馈。

use super::resources::AssetPreviewNavigation; // 倍率回显独立于资产异步加载文案。
use super::compatible_texture::{CompatiblePreviewTexture, sample_compatible};
use super::types::*; // 预览内容和UI专用marker。

/// 只解释原加载状态机，不以已有句柄、状态文字或开始时间猜测完成。
pub(super) fn phase_label(state: &AssetPreviewState, restoring: bool) -> &'static str {
    if restoring
        && !matches!(
            state.content,
            AssetPreviewContent::Image {
                phase: AssetPreviewPhase::Failed,
                ..
            } | AssetPreviewContent::Model {
                phase: AssetPreviewPhase::Failed,
                ..
            } | AssetPreviewContent::Audio {
                phase: AssetPreviewPhase::Failed,
            }
        )
    {
        return "恢复观察中"; // 即使调用方保留恢复票据，失败也必须优先于恢复说明。
    }
    match &state.content {
        AssetPreviewContent::Image { phase, .. } | AssetPreviewContent::Model { phase, .. }
        | AssetPreviewContent::Audio { phase } => {
            match phase {
                AssetPreviewPhase::Loading => "加载中",
                AssetPreviewPhase::Scanning => "准备完整范围",
                AssetPreviewPhase::RevealPending => "等待完整发布",
                AssetPreviewPhase::Ready => "已就绪",
                AssetPreviewPhase::Failed => "加载失败",
            }
        }
        AssetPreviewContent::Document | AssetPreviewContent::Data
        | AssetPreviewContent::Unsupported | AssetPreviewContent::None => "",
    }
}

/// 只在预览状态变化后原位切换图片、模型画布和说明文字。
pub(super) fn sync_asset_preview_presentation(
    state: Res<AssetPreviewState>,
    editor: Option<Res<crate::resources::MapEditorState>>, // 右侧属性不能借用中央固定文件遗留的预览尺寸。
    source_images: Option<Res<Assets<Image>>>, // 只读取所选图片的已采用描述，属性不从状态文案猜测。
    previews: Option<Res<Assets<CompatiblePreviewTexture>>>, // 描述与图像来自同一个后台完整发布，避免新元数据配旧画面。
    server: Option<Res<AssetServer>>,
    documents: Option<Res<super::DocumentState>>,
    media: Option<Res<super::AssetPreviewWorkspace>>,
    mut nodes: Query<
        (
            &mut Node,
            Option<&AssetPreviewPanel>,
            Option<&AssetPreviewImage>,
            Option<&AssetPreviewModelViewport>,
            Option<&AssetPreviewPlaceholder>,
            Option<&AssetPreviewCanvas>,
            Option<&AssetPreviewMetadataText>,
        ),
        Or<(
            With<AssetPreviewPanel>,
            With<AssetPreviewImage>,
            With<AssetPreviewModelViewport>,
            With<AssetPreviewPlaceholder>,
            With<AssetPreviewCanvas>,
            With<AssetPreviewMetadataText>,
        )>,
    >,
    mut texts: Query<
        (
            &mut Text,
            Option<&AssetPreviewTitleText>,
            Option<&AssetPreviewStatusText>,
            Option<&AssetPreviewPlaceholder>,
            Option<&AssetPreviewPhaseText>,
            Option<&AssetPreviewMetadataText>,
            &mut TextColor,
        ),
        Or<(
            With<AssetPreviewTitleText>,
            With<AssetPreviewStatusText>,
            With<AssetPreviewPlaceholder>,
            With<AssetPreviewPhaseText>,
            With<AssetPreviewMetadataText>,
        )>,
    >,
    mut images: Query<&mut ImageNode, With<AssetPreviewImage>>,
    mut model_viewports: Query<&mut Visibility, With<AssetPreviewModelViewport>>,
    mut previous_expanded: Local<Option<(bool, bool, bool, bool, bool)>>,
) {
    let expanded = documents.as_ref().is_some_and(|documents| documents.panel_in_center());
    let central = media.as_ref().is_some_and(|media| media.central_active);
    let document_failed = documents.as_ref().is_some_and(|documents| documents.load_failed());
    let same_selection = central || editor.as_ref().is_none_or(|editor| editor.selected_asset == state.selected_path);
    let failed = matches!(state.content,
        AssetPreviewContent::Image { phase: AssetPreviewPhase::Failed, .. }
        | AssetPreviewContent::Model { phase: AssetPreviewPhase::Failed, .. }
        | AssetPreviewContent::Audio { phase: AssetPreviewPhase::Failed });
    let restoring = central && !failed && media.as_ref().is_some_and(|media| media.restore_view);
    // 失败必须回显原诊断；保留恢复票据，源文件修复后仍能沿原加载链找回观察状态。
    if !state.is_changed() && !source_images.as_ref().is_some_and(|images| images.is_changed())
        && !previews.as_ref().is_some_and(|previews| previews.is_changed())
        && *previous_expanded == Some((expanded, central, restoring, document_failed, same_selection)) {
        return;
    }
    *previous_expanded = Some((expanded, central, restoring, document_failed, same_selection)); // 正文逐字输入不重复刷新图片/模型的固定UI。
    let selected = state.selected_path.is_some();
    let image_handle = match &state.content {
        AssetPreviewContent::Image {
            phase: AssetPreviewPhase::Ready,
            ..
        } => state.display_image.as_ref().filter(|handle| source_images.as_ref()
            .and_then(|images| images.get(*handle)).is_some_and(sample_compatible)).cloned(),
        _ => None,
    };
    let model_ready = !restoring && matches!(
        &state.content,
        AssetPreviewContent::Model {
            phase: AssetPreviewPhase::Ready,
            ..
        }
    );
    let model_selected = matches!(&state.content, AssetPreviewContent::Model { .. });
    let document = matches!(state.content, AssetPreviewContent::Document);
    let audio = matches!(state.content, AssetPreviewContent::Audio { .. });
    let metadata = match &state.content {
        AssetPreviewContent::Image { handle, .. } if same_selection => {
            let adopted = state.texture_preview.as_ref()
                .filter(|handle| server.as_ref().is_none_or(|server| matches!(server.load_state(handle.id()), bevy::asset::LoadState::Loaded)))
                .and_then(|handle| previews.as_ref().and_then(|previews| previews.get(handle)))
                .map(|preview| preview.description.clone());
            adopted.or_else(|| state.texture_preview.is_none().then(|| source_images.as_ref().and_then(|images| images.get(handle))
                .map(|image| format!("二维纹理 · {} × {} px · {:?}", image.texture_descriptor.size.width,
                    image.texture_descriptor.size.height, image.texture_descriptor.format))).flatten())
        },
        _ => None,
    };
    let has_canvas = matches!(state.content, AssetPreviewContent::Image { .. } | AssetPreviewContent::Model { .. }
        | AssetPreviewContent::Document | AssetPreviewContent::Audio { .. }) && (!failed || audio);
    let compact = !has_canvas || audio || document_failed;

    for (mut node, panel, image, model, placeholder, canvas, metadata_text) in &mut nodes {
        if canvas.is_some() {
            let display = if has_canvas { Display::Flex } else { Display::None };
            if node.display != display { node.display = display; }
            // 文档多两行工具栏并需要可编辑正文高度；图片/模型沿用原画布预算。
            let minimum = px(if central {
                0.0 // 中央画布只占正文剩余高度，缩小窗口不能被侧栏最小高度撑开。
            } else if audio || !has_canvas {
                0.0 // 音频由真实控件决定高度；失败或不可预览文件不留下空白画布。
            } else if document_failed {
                96.0 // 读取失败仍保留重新载入等原操作，只压缩无正文的空白。
            } else if document && expanded {
                100.0 // 正文已在中央，侧栏仅保留打开所选文档的入口。
            } else if document {
                360.0
            } else {
                170.0
            });
            if node.min_height != minimum { node.min_height = minimum; }
            let grow = if compact && !central { 0.0 } else { 1.0 };
            if node.flex_grow != grow { node.flex_grow = grow; } // 侧栏音频只占控件实际高度，中央保持完整宿主。
            continue;
        }
        let display = if panel.is_some() {
            if selected {
                Display::Flex
            } else {
                Display::None
            }
        } else if image.is_some() {
            if image_handle.is_some() {
                Display::Flex
            } else {
                Display::None
            }
        } else if model.is_some() {
            if model_selected {
                Display::Flex
            } else {
                Display::None
            }
        } else if placeholder.is_some() {
            Display::None // 加载、失败与恢复诊断进入公共底栏，画布不再承载重复状态卡。
        } else if metadata_text.is_some() {
            if metadata.is_some() { Display::Flex } else { Display::None }
        } else {
            continue;
        };
        if panel.is_some() {
            let grow = if compact && !central { 0.0 } else { 1.0 };
            if node.flex_grow != grow { node.flex_grow = grow; }
            let minimum = px(if central || compact { 0.0 } else { 220.0 });
            if node.min_height != minimum { node.min_height = minimum; }
            let shrink = if central { 1.0 } else { 0.0 };
            if node.flex_shrink != shrink { node.flex_shrink = shrink; }
            let basis = if central { px(0) } else { Val::Auto };
            if node.flex_basis != basis { node.flex_basis = basis; }
        }
        if node.display != display {
            node.display = display;
        }
    }
    if let Ok(mut visibility) = model_viewports.single_mut() {
        let wanted = if model_ready {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted; // 加载期先取得真实画布尺寸，Ready后才显示离屏结果。
        }
    }
    if let Ok(mut image) = images.single_mut() {
        let wanted = image_handle.unwrap_or_else(|| TRANSPARENT_IMAGE_HANDLE.clone());
        if image.image != wanted {
            image.image = wanted; // 隐藏时替换透明句柄，旧大图可以按AssetServer生命周期释放。
        }
    }
    let file_name = state
        .selected_path
        .as_deref()
        .map(|path| path.rsplit('/').next().unwrap_or(path));
    let title = match state.external_title.as_deref() {
        Some(title) => file_name.map(|name| format!("{title} · {name}")).unwrap_or_else(|| title.to_owned()),
        None if !central => "资源预览".to_owned(), // 完整文件名由固定所选资源区负责，中央仍显示自己的文件名。
        None => file_name
            .map(|name| format!("资产预览 · {name}"))
            .unwrap_or_else(|| "资产预览".to_owned()),
    };
    let placeholder = if restoring {
        "正在恢复中央预览的观察状态".to_owned()
    } else {
        String::new() // 同一诊断只由StatusText发送一次；旧占位身份仅保留独立恢复说明。
    };
    for (mut text, title_text, status_text, placeholder_text, phase_text, metadata_text, mut text_color) in
        &mut texts
    {
        let wanted = if title_text.is_some() {
            title.as_str()
        } else if status_text.is_some() {
            state.status.as_str()
        } else if placeholder_text.is_some() {
            placeholder.as_str()
        } else if phase_text.is_some() {
            phase_label(&state, restoring)
        } else if metadata_text.is_some() {
            metadata.as_deref().unwrap_or("")
        } else {
            continue;
        };
        if text.as_str() != wanted {
            **text = wanted.to_owned();
        }
        if phase_text.is_some() {
            text_color.set_if_neq(TextColor(if failed {
                colors::TEXT_ACCENT
            } else {
                colors::TEXT_MUTED
            }));
        }
    }
}

/// 固定导航工具条只在真实内容Ready时出现，倍率和提示不会覆盖资产错误信息。
pub(super) fn sync_asset_preview_controls(
    state: Res<AssetPreviewState>,           // 内容类型与加载资格。
    navigation: Res<AssetPreviewNavigation>, // 当前相对完整取景倍率。
    media: Option<Res<super::AssetPreviewWorkspace>>,
    mut previous_restoring: Local<Option<bool>>,
    mut nodes: Query<(&mut Node, Has<AssetPreviewNavigationHint>), Or<(With<AssetPreviewControls>, With<AssetPreviewNavigationHint>)>>, // 手势文本保留原身份，显示入口改为标题?。
    mut texts: Query<
        (&mut Text, Has<AssetPreviewZoomText>),
        Or<(With<AssetPreviewZoomText>, With<AssetPreviewNavigationHint>)>,
    >, // 提示和倍率共享单个可变Text查询。
    mut buttons: Query<
        (&UiInteraction, &mut BackgroundColor, &mut BorderColor),
        (
            With<AssetPreviewAction>,
            With<AssetPreviewNavigationButton>,
            Changed<UiInteraction>,
        ),
    >, // 稳定按钮无需刷新颜色。
) {
    for (interaction, mut background, mut border) in &mut buttons {
        let color = match interaction {
            UiInteraction::Pressed => colors::SELECT_BG_PRESSED, // 按下采用既有铜金选中色。
            UiInteraction::Hovered => colors::SELECT_BG_HOVER,   // 悬停明确反馈可操作。
            UiInteraction::None => colors::BUTTON_DEV_SMALL_NORMAL, // 离开回到暗墨底。
        }; // 不创建预览专用的新配色。
        background.set_if_neq(BackgroundColor(color)); // 无差异不标记视觉Changed。
        border.set_if_neq(BorderColor::all(if *interaction == UiInteraction::None {
            colors::COPPER_BORDER_WEAK
        } else {
            colors::COPPER_BORDER
        })); // 聚焦时强化已有铜边。
    } // 按钮事件即使处于加载切换期也不会积压。
    let restoring = media.as_ref().is_some_and(|media| media.central_active && media.restore_view);
    if !state.is_changed() && !navigation.is_changed() && *previous_restoring == Some(restoring) {
        return; // 稳定预览不反复构建文字或写布局。
    } // 导航只触发当前固定工具条，不扫描资产目录。
    *previous_restoring = Some(restoring);
    let hint = if restoring { "" } else { match &state.content {
        AssetPreviewContent::Image {
            phase: AssetPreviewPhase::Ready,
            ..
        } => "滚轮缩放 · 左/中/右键拖动平移", // 图片不存在模型轨道旋转。
        AssetPreviewContent::Model {
            phase: AssetPreviewPhase::Ready,
            ..
        } => "滚轮缩放 · 右键旋转 · 中键平移", // 与地图视口手势一致。
        _ => "", // 加载、失败与非可视文件不提供虚假的交互提示。
    } }; // 完整恢复之前不显示会被旧快照覆盖的交互入口。
    let display = if hint.is_empty() {
        Display::None
    } else {
        Display::Flex
    }; // 隐藏工具条不占高度。
    for (mut node, hint_text) in &mut nodes {
        let wanted = if hint_text { Display::None } else { display };
        if node.display != wanted {
            node.display = wanted; // 工具条仍按真实Ready门显示，手势说明不再占正文。
        } // 鼠标持续移动不会改变行高。
    } // 工具条和提示统一显隐。
    let zoom = format!("{:.0}%", navigation.view.zoom * 100.0); // 百分比表示相对适应画布倍率。
    for (mut text, zoom_text) in &mut texts {
        let wanted = if zoom_text { zoom.as_str() } else { hint }; // 用marker区分文字职责。
        if text.as_str() != wanted {
            **text = wanted.to_owned(); // 相同显示倍率不重复分配UI文本。
        } // 错误和尺寸状态由原有表现系统独立维护。
    } // Text可变借用始终在一个查询中完成。
}
