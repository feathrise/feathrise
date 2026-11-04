//! 用原生UI线段绘制小图标，保持DPI缩放清晰，颜色继承编辑器三套主题。
use super::{
    components::{Icon, IconStroke},
    types::Glyph,
};
use bevy::prelude::*;
use feathrise_editor_ui::colors as c;

fn line(world: &mut World, parent: Entity, x: f32, y: f32, width: f32, height: f32, angle: f32) {
    let entity = world
        .spawn((
            IconStroke,
            Pickable::IGNORE,
            Node {
                position_type: PositionType::Absolute,
                left: px(x),
                top: px(y),
                width: px(width),
                height: px(height),
                ..default()
            },
            UiTransform::from_rotation(Rot2::radians(angle)),
            BackgroundColor(c::TEXT_PRIMARY),
        ))
        .id();
    world.entity_mut(parent).add_child(entity);
}
fn outline(world: &mut World, parent: Entity, x: f32, y: f32, width: f32, height: f32) {
    for (x, y, w, h) in [
        (x, y, width, 1.),
        (x, y + height - 1., width, 1.),
        (x, y, 1., height),
        (x + width - 1., y, 1., height),
    ] {
        line(world, parent, x, y, w, h, 0.);
    }
}
pub(super) fn spawn(world: &mut World, parent: Entity, glyph: Glyph) -> Entity {
    let icon = world
        .spawn((
            Icon(glyph),
            Pickable::IGNORE,
            Node {
                width: px(16),
                height: px(16),
                flex_shrink: 0.,
                ..default()
            },
        ))
        .id();
    world.entity_mut(parent).add_child(icon);
    draw(world, icon, glyph);
    icon
}
pub(super) fn draw(world: &mut World, icon: Entity, glyph: Glyph) {
    if let Some(children) = world.get::<Children>(icon) {
        let children: Vec<_> = children.iter().collect();
        for child in children {
            world.despawn(child);
        }
    }
    world.entity_mut(icon).insert(Icon(glyph));
    match glyph {
        Glyph::Minimize => line(world, icon, 3., 8., 10., 1., 0.),
        Glyph::Maximize => outline(world, icon, 3., 3., 10., 10.),
        Glyph::Restore => {
            outline(world, icon, 5., 2., 9., 9.);
            outline(world, icon, 2., 5., 9., 9.);
        }
        Glyph::Close => {
            line(world, icon, 2., 7.5, 12., 1., std::f32::consts::FRAC_PI_4);
            line(world, icon, 2., 7.5, 12., 1., -std::f32::consts::FRAC_PI_4);
        }
        Glyph::Save => {
            outline(world, icon, 2., 2., 12., 12.);
            outline(world, icon, 5., 2., 6., 4.);
            outline(world, icon, 5., 9., 6., 5.);
        }
        Glyph::Undo | Glyph::Redo => {
            let flip = glyph == Glyph::Redo;
            let map = |x: f32, w: f32| if flip { 16. - x - w } else { x };
            for (x, y, w, h, a) in [
                (4., 4., 7., 1., 0.),
                (10., 4., 3., 1., 0.7),
                (12., 6., 1., 5., 0.),
                (8., 11., 5., 1., 0.),
                (2., 3., 5., 1., -0.8),
                (2., 5., 5., 1., 0.8),
            ] {
                line(world, icon, map(x, w), y, w, h, if flip { -a } else { a });
            }
        }
        // 左右栏用相同窗口轮廓及对应分栏区分，图标只负责呈现，点击仍归原布局按钮。
        Glyph::Navigation | Glyph::Inspector => {
            outline(world, icon, 2., 3., 12., 10.);
            let divider = if glyph == Glyph::Navigation { 6. } else { 9. };
            line(world, icon, divider, 3., 1., 10., 0.);
        }
        Glyph::Focus => {
            // 四角表示扩大中央工作区，与导航／工作区的侧栏符号保持可辨认差异。
            for (x, y, width, height) in [
                (2., 2., 5., 1.), (2., 2., 1., 5.),
                (9., 2., 5., 1.), (13., 2., 1., 5.),
                (2., 13., 5., 1.), (2., 9., 1., 5.),
                (9., 13., 5., 1.), (13., 9., 1., 5.),
            ] {
                line(world, icon, x, y, width, height, 0.);
            }
        }
        Glyph::Settings | Glyph::Filter => {
            for (y, x) in [(4., 6.), (8., 10.), (12., 4.)] {
                line(world, icon, 2., y, 12., 1., 0.);
                line(world, icon, x, y - 2., 2., 5., 0.);
            }
        }
        Glyph::More => {
            for x in [3., 7., 11.] {
                line(world, icon, x, 7., 2., 2., 0.);
            }
        }
    }
}
