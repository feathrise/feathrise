//! 色板、所有权、重挂载与长文档可见绘制预算回归；不写设置或重建业务UI。
use super::{constants::COLORS, extract, resources::*, *};
use crate::colors as c;
use bevy::{
    prelude::*,
    ui_render::{ExtractedGlyph, ExtractedUiItem, ExtractedUiNode, ExtractedUiNodes, NodeType},
};

pub(super) fn entity(index: u32) -> Entity {
    Entity::from_raw_u32(index).unwrap()
}
pub(super) fn node(main: Entity, item: ExtractedUiItem) -> ExtractedUiNode {
    ExtractedUiNode {
        z_order: 0.0,
        image: default(),
        clip: None,
        extracted_camera_entity: entity(99),
        item,
        main_entity: main.into(),
        render_entity: entity(99),
        transform: default(),
    }
}
pub(super) fn background(color: Color) -> ExtractedUiItem {
    ExtractedUiItem::Node {
        color: color.to_linear(),
        rect: default(),
        atlas_scaling: None,
        flip_x: false,
        flip_y: false,
        border_radius: default(),
        border: default(),
        node_type: NodeType::Rect,
    }
}
pub(super) fn color(node: &ExtractedUiNode) -> LinearRgba {
    let ExtractedUiItem::Node { color, .. } = node.item else {
        panic!("预期背景节点")
    };
    color
}

#[test]
fn theme_palette_has_distinct_semantic_keys_and_expected_backgrounds() {
    let keys: std::collections::HashSet<_> =
        COLORS.iter().map(|row| key(row.0.to_linear())).collect();
    assert_eq!(keys.len(), COLORS.len(), "RGB身份不能发生别名覆盖");
    assert_eq!(
        EditorPalette::default()
            .resolve(c::OVERLAY_BG)
            .to_srgba()
            .to_u8_array(),
        [30, 30, 30, 255]
    );
    assert_eq!(
        EditorPalette::graphite_blue()
            .resolve(c::COPPER_BORDER)
            .to_srgba()
            .to_u8_array(),
        [134, 183, 232, 255]
    );
    assert_ne!(
        EditorPalette::default().syntax_colors()[0],
        EditorPalette::default().resolve(c::TEXT_ACCENT)
    );
}

#[test]
fn theme_preserves_alpha_variants_custom_colors_and_warm_original() {
    let palette = EditorPalette::neutral_gray();
    assert!(
        (palette
            .resolve(c::TEXT_ACCENT.with_alpha(0.16))
            .to_linear()
            .alpha
            - 0.16)
            .abs()
            < 0.0001
    );
    let custom = Color::srgba(0.12, 0.68, 0.91, 0.3);
    assert_eq!(palette.resolve(custom).to_linear(), custom.to_linear());
    assert_eq!(palette.resolve(Color::WHITE).to_linear(), LinearRgba::WHITE);
    for &(source, _, _) in COLORS {
        assert_eq!(
            EditorPalette::warm_copper().resolve(source).to_linear(),
            source.to_linear()
        );
    }
}

#[test]
fn literal_data_colors_keep_original_gold_and_button_feedback_inside_editor_theme() {
    let literal = entity(71);
    let ordinary = entity(72);
    let source = c::TEXT_ACCENT;
    let mut nodes = ExtractedUiNodes {
        uinodes: vec![
            node(literal, background(source)),
            node(literal, ExtractedUiItem::Glyphs { range: 0..1 }),
            node(ordinary, background(source)),
            node(ordinary, ExtractedUiItem::Glyphs { range: 1..2 }),
        ],
        glyphs: (0..2).map(|_| ExtractedGlyph {
            color: source.to_linear(), translation: Vec2::ZERO, rect: default(),
        }).collect(),
    };
    let palette = EditorPalette::graphite_blue();
    extract::apply(&mut nodes, &palette, &mut ThemeExtraction::default(),
        |_| None, |_| true, |_| false, |e| e == literal, |e| {
            (e == literal).then_some(super::button_feedback::ButtonFeedback {
                interaction: Interaction::Hovered, disabled: false,
                background: source.to_linear(), colors: None,
            })
        });
    assert_eq!(nodes.glyphs[0].color, source.to_linear(), "时间轴游标/ANSI字形不能被换成主题色");
    assert!(color(&nodes.uinodes[0]).red > source.to_linear().red,
        "原色按钮仍需有实际悬停反馈");
    assert_eq!(color(&nodes.uinodes[2]), palette.resolve(source).to_linear());
    assert_eq!(nodes.glyphs[1].color, palette.resolve(source).to_linear());
}

#[test]
fn theme_only_changes_editor_descendants_and_code_surface_layer() {
    let root = entity(1);
    let input = entity(2);
    let code = entity(3);
    let hud = entity(4);
    let mut nodes = ExtractedUiNodes {
        uinodes: vec![
            node(input, background(c::INPUT_BG)),
            node(code, background(c::INPUT_BG)),
            node(hud, background(c::INPUT_BG)),
            node(code, ExtractedUiItem::Glyphs { range: 0..2 }),
            node(hud, ExtractedUiItem::Glyphs { range: 2..3 }),
        ],
        glyphs: (0..3)
            .map(|i| ExtractedGlyph {
                color: if i == 1 {
                    LinearRgba::WHITE
                } else {
                    c::TEXT_ACCENT.to_linear()
                },
                translation: Vec2::ZERO,
                rect: default(),
            })
            .collect(),
    };
    let palette = EditorPalette::graphite_blue();
    extract::apply(
        &mut nodes,
        &palette,
        &mut ThemeExtraction::default(),
        |e| (e == input || e == code).then_some(root),
        |e| e == root,
        |e| e == code,
        |_| false,
        |_| None,
    );
    assert_eq!(
        color(&nodes.uinodes[0]),
        palette.resolve(c::INPUT_BG).to_linear()
    );
    assert_eq!(
        color(&nodes.uinodes[1]),
        palette.resolve(c::OVERLAY_BG).to_linear()
    );
    assert_eq!(color(&nodes.uinodes[2]), c::INPUT_BG.to_linear());
    assert_eq!(
        nodes.glyphs[0].color,
        palette.resolve(c::TEXT_ACCENT).to_linear()
    );
    assert_eq!(
        nodes.glyphs[1].color,
        LinearRgba::WHITE,
        "原生选区和Emoji白色不被改写"
    );
    assert_eq!(
        nodes.glyphs[2].color,
        c::TEXT_ACCENT.to_linear(),
        "同色游戏字形必须保留"
    );
}

#[test]
fn theme_topology_observes_non_node_reparent_and_root_lifecycle() {
    let mut app = App::new();
    app.add_plugins(EditorThemePlugin);
    let root = app.world_mut().spawn(EditorThemeRoot).id();
    let other = app.world_mut().spawn_empty().id();
    let wrapper = app.world_mut().spawn(ChildOf(root)).id();
    let text = app
        .world_mut()
        .spawn((Node::default(), ChildOf(wrapper)))
        .id();
    let belongs = |world: &World| {
        let mut state = ThemeExtraction::default();
        extract::owned(
            text,
            &mut state,
            |e| world.get::<ChildOf>(e).map(ChildOf::parent),
            |e| world.get::<EditorThemeRoot>(e).is_some(),
        )
    };
    assert!(belongs(app.world()));
    let revision = app.world().resource::<ThemeTopology>().0;
    app.world_mut().entity_mut(wrapper).insert(ChildOf(other));
    assert!(app.world().resource::<ThemeTopology>().0 > revision);
    assert!(!belongs(app.world()));
    app.world_mut().entity_mut(other).insert(EditorThemeRoot);
    assert!(belongs(app.world()));
    app.world_mut()
        .entity_mut(other)
        .remove::<EditorThemeRoot>();
    assert!(!belongs(app.world()));
}

#[test]
fn theme_long_clipped_document_only_maps_visible_glyph_rows() {
    let mut nodes = ExtractedUiNodes {
        uinodes: vec![node(
            entity(1),
            ExtractedUiItem::Glyphs { range: 0..100_000 },
        )],
        glyphs: (0..100_000)
            .map(|i| ExtractedGlyph {
                color: c::TEXT_PRIMARY.to_linear(),
                translation: Vec2::new(0.0, i as f32 * 20.0),
                rect: Rect::new(0.0, 0.0, 10.0, 20.0),
            })
            .collect(),
    };
    nodes.uinodes[0].clip = Some(Rect::new(0.0, 400_000.0, 500.0, 400_600.0));
    let range = extract::visible_glyphs(&nodes, 0, 0..100_000);
    assert!(range.contains(&20_000) && range.contains(&20_030));
    assert!(
        range.len() < 100,
        "长文档不能逐帧映射全部隐藏字形: {}",
        range.len()
    );
    extract::apply(
        &mut nodes,
        &EditorPalette::neutral_gray(),
        &mut ThemeExtraction::default(),
        |_| None,
        |_| true,
        |_| false,
        |_| false,
        |_| None,
    );
    assert_eq!(nodes.glyphs[0].color, c::TEXT_PRIMARY.to_linear());
    assert_eq!(
        nodes.glyphs[20_001].color,
        EditorPalette::neutral_gray()
            .resolve(c::TEXT_PRIMARY)
            .to_linear()
    );
    nodes.uinodes[0].clip = Some(Rect::new(0.0, 3_000_000.0, 500.0, 3_001_000.0));
    assert!(extract::visible_glyphs(&nodes, 0, 0..100_000).is_empty());
}

#[test]
fn theme_cache_is_bounded_and_cycles_fail_closed() {
    let mut cache = ThemeExtraction::default();
    for i in 0..20_000 {
        assert!(extract::owned(entity(i), &mut cache, |_| None, |_| true));
    }
    assert!(cache.scopes.len() <= 16_384);
    assert!(!extract::owned(
        entity(30_000),
        &mut cache,
        |e| Some(e),
        |_| false
    ));
}

#[test]
fn theme_single_axis_clips_preserve_title_and_input_glyph_mapping() {
    let mut nodes = ExtractedUiNodes {
        uinodes: vec![node(entity(1), ExtractedUiItem::Glyphs { range: 0..2 })],
        glyphs: (0..2)
            .map(|i| ExtractedGlyph {
                color: c::TEXT_ACCENT.to_linear(),
                translation: Vec2::new(i as f32 * 10.0, 6.0),
                rect: default(),
            })
            .collect(),
    };
    for clip in [
        Rect::new(f32::NEG_INFINITY, 70.0, f32::INFINITY, 1074.0),
        Rect::new(100.0, f32::NEG_INFINITY, 300.0, f32::INFINITY),
    ] {
        nodes.uinodes[0].clip = Some(clip);
        nodes.uinodes[0].transform = bevy::math::Affine2::from_translation(Vec2::new(100.0, 199.0));
        assert_eq!(extract::visible_glyphs(&nodes, 0, 0..2), 0..2);
    }
    let palette = EditorPalette::graphite_blue();
    extract::apply(
        &mut nodes,
        &palette,
        &mut ThemeExtraction::default(),
        |_| None,
        |_| true,
        |_| false,
        |_| false,
        |_| None,
    );
    assert!(
        nodes
            .glyphs
            .iter()
            .all(|glyph| glyph.color == palette.resolve(c::TEXT_ACCENT).to_linear())
    );
}
