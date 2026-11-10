//! 当前传送点中央视口徽标的固定节点创建与增量文案同步。

use bevy::picking::Pickable; // 徽标必须完整穿透指针给下方ViewportNode。
use bevy::prelude::*; // UI节点、文本、颜色、字体和ECS查询。
use bevy::ui::FocusPolicy; // 徽标不参与现有Interaction焦点命中。
use crate::ui::theme as colors; // 复用暗墨底、铜金边和低饱和值色。

use super::SelectedPortalViewportState; // 文案缓存与点击状态收口在同一选择资源。
use crate::components::{
    SceneEditorSelectedPortalBadge, SceneEditorSelectedPortalBadgeText, SceneEditorViewport,
}; // 固定UI结构通过marker同步，不依赖中文Name查询。

const SELECTION_FONT_PATH: &str = "fonts/open_source/noto_sans/cjk/NotoSansCJKsc-VF.otf"; // 与传送Billboard和F12正文共用完整中文字体。

/// 为中央ViewportNode创建紧凑传送点身份徽标。
pub(crate) fn setup_selected_portal_badge(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    viewports: Query<Entity, Added<SceneEditorViewport>>,
) {
    let Ok(viewport_entity) = viewports.single() else {
        return; // 中央视口尚未建立时不生成游离覆盖节点。
    };
    let font: Handle<Font> = feathrise_editor_core::editor_font(&asset_server, SELECTION_FONT_PATH);
    commands.entity(viewport_entity).with_children(|viewport| {
        viewport
            .spawn((
                Name::new("场景编辑器当前传送点徽标"),
                SceneEditorSelectedPortalBadge,
                Node {
                    position_type: PositionType::Absolute,
                    left: px(8), // 传送工具与静水工具互斥，复用中央画面第一信息位。
                    top: px(88), // 位于统一模式与筛选条下方。
                    max_width: px(680),
                    border: UiRect::all(px(1)),
                    padding: UiRect {
                        left: px(8),
                        right: px(8),
                        top: px(4),
                        bottom: px(4),
                    },
                    border_radius: BorderRadius::all(px(5)),
                    ..default()
                },
                BackgroundColor(colors::OVERLAY_BG),
                BorderColor::all(colors::COPPER_BORDER),
                Visibility::Hidden,
                ZIndex(20),
                FocusPolicy::Pass,
                Pickable::IGNORE,
            ))
            .with_child((
                Name::new("场景编辑器当前传送点徽标文字"),
                SceneEditorSelectedPortalBadgeText,
                Text::new(""),
                TextFont {
                    font: font.into(),
                    font_size: 11.0.into(),
                    ..default()
                },
                TextColor(colors::TEXT_VALUE),
                Pickable::IGNORE,
            ));
    });
}

/// 原位显隐中央徽标，并只在真实文案变化时触发Text布局。
pub(super) fn sync_badge(
    visible: bool,
    desired_text: Option<String>,
    state: &mut SelectedPortalViewportState,
    badges: &mut Query<&mut Visibility, With<SceneEditorSelectedPortalBadge>>,
    texts: &mut Query<&mut Text, With<SceneEditorSelectedPortalBadgeText>>,
) {
    if let Ok(mut visibility) = badges.single_mut() {
        let desired_visibility = if visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != desired_visibility {
            *visibility = desired_visibility;
        }
    }
    let Some(desired_text) = desired_text else {
        return; // 隐藏时保留上次字符串，下一有效选择再比较。
    };
    if state.badge_text.as_deref() == Some(desired_text.as_str()) {
        return; // 稳定帧不反复触发文字布局。
    }
    let Ok(mut text) = texts.single_mut() else {
        return; // setup命令尚未可见时下一帧自然重试。
    };
    text.0 = desired_text.clone();
    state.badge_text = Some(desired_text);
}
