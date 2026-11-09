//! 原生三键反馈回归：使用原生命中位置修正旧客户区光标，按住、拖出和恢复均有准确绘制态。

use feathrise_editor_ui::UiInteraction;
use super::*;

#[test]
fn native_captions_keep_hover_press_and_drag_out_feedback_in_original_three_states() {
    let rect = PixelRect {
        left: 100,
        top: 0,
        right: 166,
        bottom: 54,
    };
    for action in [
        WindowAction::Minimize,
        WindowAction::Maximize,
        WindowAction::Close,
    ] {
        let colors = if action == WindowAction::Close {
            ButtonColors::new(
                Color::NONE,
                Color::srgb_u8(196, 43, 54),
                Color::srgb_u8(160, 28, 38),
            )
        } else {
            ButtonColors::new(
                Color::NONE,
                feathrise_editor_ui::colors::BUTTON_DEV_SMALL_HOVERED,
                feathrise_editor_ui::colors::BUTTON_DEV_SMALL_PRESSED,
            )
        };
        let (hover, cursor, background) =
            caption_feedback(rect, true, Some((133, 27)), false, false, &colors);
        assert_eq!(hover, UiInteraction::Hovered);
        assert!(cursor.cursor_over());
        assert_eq!(cursor.normalized, Some(Vec2::ZERO));
        assert_eq!(background, colors.hovered);

        // NC按压没有Winit CursorMoved，旧RelativeCursorPosition仍可为false；生产结果须取代它。
        let mut stale_cursor = RelativeCursorPosition::default();
        assert!(!stale_cursor.cursor_over());
        let (pressed, cursor, background) =
            caption_feedback(rect, true, Some((133, 27)), true, true, &colors);
        stale_cursor = cursor;
        assert_eq!(pressed, UiInteraction::Pressed);
        assert!(stale_cursor.cursor_over());
        assert_eq!(background, colors.pressed);
        assert_ne!(background, colors.hovered);
        assert!(!background.is_fully_transparent());

        let (outside, cursor, background) =
            caption_feedback(rect, true, Some((168, 27)), true, true, &colors);
        assert_eq!(outside, UiInteraction::None);
        assert!(!cursor.cursor_over());
        assert!(
            background.is_fully_transparent(),
            "捕获保留，但移出后不能绘制假按压"
        );
        let (restored, _, background) =
            caption_feedback(rect, true, Some((133, 27)), true, true, &colors);
        assert_eq!(restored, UiInteraction::Pressed);
        assert_eq!(background, colors.pressed);
        let (released, _, background) =
            caption_feedback(rect, true, Some((133, 27)), false, false, &colors);
        assert_eq!(released, UiInteraction::Hovered);
        assert_eq!(background, colors.hovered);
        let (lost, cursor, background) =
            caption_feedback(rect, false, Some((133, 27)), true, true, &colors);
        assert_eq!(lost, UiInteraction::None);
        assert!(!cursor.cursor_over());
        assert!(cursor.normalized.is_none());
        assert!(background.is_fully_transparent());
    }
}

#[test]
fn native_caption_feedback_does_not_press_without_capture_or_an_available_rect() {
    let colors = ButtonColors::new(
        Color::NONE,
        Color::srgb(0.3, 0.3, 0.3),
        Color::srgb(0.4, 0.4, 0.4),
    );
    let rect = PixelRect {
        left: 100,
        top: 0,
        right: 166,
        bottom: 54,
    };
    assert_eq!(
        caption_feedback(rect, true, Some((133, 27)), true, false, &colors).0,
        UiInteraction::Hovered
    );
    for (rect, cursor) in [(rect, None), (PixelRect::default(), Some((0, 0)))] {
        let (interaction, position, background) =
            caption_feedback(rect, true, cursor, true, true, &colors);
        assert_eq!(interaction, UiInteraction::None);
        assert!(!position.cursor_over());
        assert!(position.normalized.is_none());
        assert!(background.is_fully_transparent());
    }
}
