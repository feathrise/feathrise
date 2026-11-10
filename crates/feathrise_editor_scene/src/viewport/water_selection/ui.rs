//! 当前静水、河流或瀑布中央视口徽标的固定节点创建与增量文案同步。

use bevy::picking::Pickable; // 徽标必须完整穿透指针给下方ViewportNode。
use bevy::prelude::*; // UI节点、文本、颜色、字体和ECS查询。
use bevy::ui::FocusPolicy; // 徽标不参与现有Interaction焦点命中。
use feathrise_scene::PaintedWaterSurface; // 徽标查询排除世界水面以满足Visibility互斥访问。
use crate::ui::theme as colors; // 复用水墨底色、铜金边和数值文字色。

use super::SelectedWaterViewportState; // 文案签名缓存归属于统一选择资源。
use crate::components::{
    SceneEditorSelectedWaterBadge, SceneEditorSelectedWaterBadgeText, SceneEditorSelectedWaterHighlight,
    SceneEditorViewport,
}; // 固定结构使用marker而不依赖中文Name查询。

const SELECTION_FONT_PATH: &str = "fonts/open_source/noto_sans/cjk/NotoSansCJKsc-VF.otf"; // 与F12其余中文作者文字共享字体。

/// 为中央ViewportNode创建紧凑身份徽标；世界高亮材质在首次真实选择时惰性创建。
pub(crate) fn setup_selected_water_badge(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    viewports: Query<Entity, Added<SceneEditorViewport>>,
) {
    let Ok(viewport_entity) = viewports.single() else {
        return; // BSN中央节点尚未出现或结构异常时不生成游离覆盖节点。
    };
    let font: Handle<Font> = feathrise_editor_core::editor_font(&asset_server, SELECTION_FONT_PATH); // 唯一徽标文字持有中文字体句柄。
    commands.entity(viewport_entity).with_children(|viewport| {
        viewport
            .spawn((
                Name::new("场景编辑器当前水域对象徽标"), // 调试层级中明确三类水域对象共用该节点。
                SceneEditorSelectedWaterBadge,             // 增量显隐查询使用稳定marker。
                Node {
                    position_type: PositionType::Absolute, // 固定覆盖中央画面，不参与Flex布局。
                    left: px(8),                           // 与视口铜边保持紧凑内距。
                    top: px(88),        // 位于统一模式与筛选条下方，避免信息重叠。
                    max_width: px(520), // 长作者名仍限制在中央画面内。
                    border: UiRect::all(px(1)), // 一像素铜金边延续F12视觉语言。
                    padding: UiRect {
                        left: px(8),   // 文本左右保留清晰呼吸空间。
                        right: px(8),  // 文本左右保持对称。
                        top: px(4),    // 徽标高度保持紧凑。
                        bottom: px(4), // 徽标高度保持紧凑。
                    },
                    border_radius: BorderRadius::all(px(5)), // 小圆角避免像调试器裸标签。
                    ..default()
                },
                BackgroundColor(colors::OVERLAY_BG), // 暗墨半透明底保证山体和水面上都可读。
                BorderColor::all(colors::COPPER_BORDER), // 铜金细边明确这是作者选择信息。
                Visibility::Hidden,                  // 只有有效水域对象选择时显示。
                ZIndex(20),                          // 与方向轴同层且二者位于相反角落。
                FocusPolicy::Pass,                   // 不参与UI焦点与Interaction命中。
                Pickable::IGNORE,                    // Picking后端同样完整穿透。
            ))
            .with_child((
                Name::new("场景编辑器当前水域对象徽标文字"), // 调试层级可直接识别动态文案节点。
                SceneEditorSelectedWaterBadgeText,             // 只在签名变化时改写Text。
                Text::new(""),                               // 隐藏首帧不显示占位内容。
                TextFont {
                    font: font.into(),      // 使用统一中文字体。
                    font_size: 11.0.into(), // 比底栏略醒目但不遮挡画面。
                    ..default()
                },
                TextColor(colors::TEXT_VALUE), // 淡青绿值色呼应静水高亮且保持武侠UI低饱和。
                Pickable::IGNORE,              // 子文字也不接管指针。
            ));
    });
}

/// 原位显隐中央徽标并仅在文案真实变化时触发Text布局。
pub(super) fn sync_badge(
    visible: bool,
    desired_text: Option<String>,
    state: &mut SelectedWaterViewportState,
    badges: &mut Query<
        &mut Visibility,
        (
            With<SceneEditorSelectedWaterBadge>,
            Without<PaintedWaterSurface>,
            Without<SceneEditorSelectedWaterHighlight>,
        ),
    >, // 与生产系统参数保持同一互斥过滤契约，避免辅助函数弱化B0001修复。
    texts: &mut Query<&mut Text, With<SceneEditorSelectedWaterBadgeText>>,
) {
    if let Ok(mut visibility) = badges.single_mut() {
        let desired_visibility = if visible {
            Visibility::Inherited // 有效水域对象选择显示在中央画面左上角。
        } else {
            Visibility::Hidden // 其他工具或关闭编辑器时完整隐藏。
        };
        if *visibility != desired_visibility {
            *visibility = desired_visibility; // 稳定帧不反复标记Visibility变化。
        }
    }
    let Some(desired_text) = desired_text else {
        return; // 隐藏时保留上次字符串，下一次显示再按签名比较。
    };
    if state.badge_text.as_deref() == Some(desired_text.as_str()) {
        return; // 选择、修订和就绪状态都没变化时不触发文本重排。
    }
    let Ok(mut text) = texts.single_mut() else {
        return; // setup命令尚未可见时留待下一帧重试。
    };
    text.0 = desired_text.clone(); // 原位改写唯一Text组件，不重建UI实体。
    state.badge_text = Some(desired_text); // 缓存最终文案供稳定帧短路。
}
