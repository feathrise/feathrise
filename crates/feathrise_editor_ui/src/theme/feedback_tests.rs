//! 按钮最终反馈回归：业务覆盖、旧三态归一、语义底色以及同实体其他绘制层隔离。

use super::{
    button_feedback::{ButtonFeedback, interaction_for_cursor},
    extract,
    resources::*,
    tests::*,
};
use crate::colors as c;
use bevy::{
    prelude::*,
    ui_render::{ExtractedUiItem, NodeType},
};
use feathrise_ui_widgets::{ButtonColors, lighten_color};

#[test]
fn feedback_cancels_captured_press_after_cursor_leaves_the_button() {
    let cursor = bevy::ui::RelativeCursorPosition {
        cursor_over: false,
        normalized: Some(Vec2::splat(1.5)),
    };
    assert_eq!(
        interaction_for_cursor(Interaction::Pressed, Some(&cursor)),
        Interaction::None
    );
    assert_eq!(
        interaction_for_cursor(Interaction::Pressed, None),
        Interaction::Pressed
    );
}

fn feedback(background: Color, interaction: Interaction) -> ButtonFeedback {
    ButtonFeedback {
        background: background.to_linear(),
        interaction,
        disabled: false,
        colors: None,
    }
}

#[test]
fn feedback_survives_business_background_rewrites_in_every_theme() {
    for palette in [
        EditorPalette::neutral_gray(),
        EditorPalette::graphite_blue(),
        EditorPalette::warm_copper(),
    ] {
        for base in [
            c::BUTTON_DEV_SMALL_NORMAL,
            c::PANEL_BG,
            c::SELECT_BG,
            c::DANGER_BG,
        ] {
            let render = |interaction| {
                let target = entity(2);
                let mut nodes = extracted([(target, node(background(base)))]);
                extract::apply(
                    &mut nodes,
                    &palette,
                    &mut ThemeExtraction::default(),
                    |_| None,
                    |_| true,
                    |_| false,
                    |_| false,
                    |_| Some(feedback(base, interaction)),
                );
                color(draw(&nodes, target, 0))
            };
            let normal = render(Interaction::None);
            let hovered = render(Interaction::Hovered);
            let pressed = render(Interaction::Pressed);
            assert_eq!(normal, palette.resolve(base).to_linear());
            assert!(hovered.red > normal.red && pressed.red > hovered.red);
            assert_eq!(normal.alpha, hovered.alpha);
            assert_eq!(hovered.alpha, pressed.alpha);
            if base == c::DANGER_BG {
                assert!(pressed.red > pressed.green, "危险按钮必须保留红色语义");
            }
            if base == c::SELECT_BG && palette.name() != "WarmCopper" {
                assert!(pressed.blue > pressed.red, "选中项必须保留蓝灰底色");
            }
        }
    }
}

#[test]
fn feedback_normalizes_generic_and_semantic_state_colors_once() {
    let palette = EditorPalette::neutral_gray();
    let normal = c::BUTTON_DEV_SMALL_NORMAL;
    let colors = ButtonColors::new(
        normal,
        lighten_color(normal, 0.12),
        lighten_color(normal, 0.25),
    );
    let expected = feedback(normal, Interaction::Pressed).resolve(&palette);
    for current in [normal, colors.hovered, colors.pressed] {
        let mut state = feedback(current, Interaction::Pressed);
        state.colors = Some(colors);
        assert_eq!(
            state.resolve(&palette),
            expected,
            "公共颜色系统和业务覆写的最终效果必须一致"
        );
    }
    for current in [c::SELECT_BG, c::SELECT_BG_HOVER, c::SELECT_BG_PRESSED] {
        assert_eq!(
            feedback(current, Interaction::Pressed).resolve(&palette),
            feedback(c::SELECT_BG, Interaction::Pressed).resolve(&palette),
        );
    }
}

#[test]
fn feedback_disabled_buttons_do_not_retain_hover_or_press_brightness() {
    let palette = EditorPalette::graphite_blue();
    let base = c::SLOT_BG_DISABLED;
    let colors = ButtonColors::new(base, lighten_color(base, 0.12), lighten_color(base, 0.25));
    for current in [base, colors.hovered, colors.pressed] {
        let mut state = feedback(current, Interaction::Pressed);
        state.disabled = true;
        state.colors = Some(colors);
        assert_eq!(state.resolve(&palette), palette.resolve(base).to_linear());
    }
}

#[test]
fn feedback_only_touches_editor_button_background_draws() {
    let editor = entity(1);
    let game = entity(2);
    let base = c::BUTTON_DEV_SMALL_NORMAL;
    let mut border = node(background(base));
    if let ExtractedUiItem::Node { node_type, .. } = &mut border.item {
        *node_type = NodeType::Border(0);
    }
    let mut image = node(background(base));
    image.z_order = 0.04;
    let mut outer = node(background(base));
    if let ExtractedUiItem::Node { node_type, .. } = &mut outer.item {
        *node_type = NodeType::Inverted;
    }
    let mut nodes = extracted([
        (editor, node(background(base))),
        (editor, border),
        (editor, image),
        (editor, outer),
        (game, node(background(base))),
        (editor, node(ExtractedUiItem::Glyphs {
            glyphs: vec![glyph(c::TEXT_PRIMARY.to_linear(), Vec2::ZERO)],
        })),
    ]);
    let palette = EditorPalette::neutral_gray();
    extract::apply(
        &mut nodes,
        &palette,
        &mut ThemeExtraction::default(),
        |_| None,
        |e| e == editor,
        |_| false,
        |_| false,
        |_| Some(feedback(base, Interaction::Pressed)),
    );
    assert_eq!(
        color(draw(&nodes, editor, 0)),
        feedback(base, Interaction::Pressed).resolve(&palette)
    );
    for index in 1..4 {
        assert_eq!(color(draw(&nodes, editor, index)), palette.resolve(base).to_linear());
    }
    assert_eq!(color(draw(&nodes, game, 0)), base.to_linear());
    assert_eq!(
        glyphs(draw(&nodes, editor, 4))[0].color,
        palette.resolve(c::TEXT_PRIMARY).to_linear()
    );
}

#[test]
fn transparent_normal_preserves_explicit_caption_state_colors_and_restores_transparency() {
    for palette in [
        EditorPalette::neutral_gray(),
        EditorPalette::graphite_blue(),
        EditorPalette::warm_copper(),
    ] {
        for colors in [
            ButtonColors::new(
                Color::NONE,
                Color::srgb_u8(196, 43, 54),
                Color::srgb_u8(160, 28, 38),
            ),
            ButtonColors::new(
                Color::NONE,
                c::BUTTON_DEV_SMALL_HOVERED,
                c::BUTTON_DEV_SMALL_PRESSED,
            ),
        ] {
            for (interaction, expected) in [
                (Interaction::None, colors.normal),
                (Interaction::Hovered, colors.hovered),
                (Interaction::Pressed, colors.pressed),
            ] {
                for current in [colors.normal, colors.hovered, colors.pressed] {
                    let mut state = feedback(current, interaction);
                    state.colors = Some(colors);
                    assert_eq!(
                        state.resolve(&palette),
                        palette.resolve(expected).to_linear()
                    );
                }
            }
            let mut disabled = feedback(colors.pressed, Interaction::Pressed);
            disabled.colors = Some(colors);
            disabled.disabled = true;
            assert_eq!(disabled.resolve(&palette).alpha, 0.0);

            // 透明按钮上的业务选中底色不属于三态色，仍沿用原语义映射和统一增亮。
            let mut selected = feedback(c::SELECT_BG, Interaction::Pressed);
            selected.colors = Some(colors);
            assert_eq!(
                selected.resolve(&palette),
                feedback(c::SELECT_BG, Interaction::Pressed).resolve(&palette)
            );
        }
    }
}
