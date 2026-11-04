//! 连续正文的固定预算构造与原子展示；旧正文按叶子到根退役，不在一帧遍历完整树。

use super::{
    components::{DocumentPreviewBody, DocumentPreviewScroll},
    constants::*,
    resources::{DocumentState, PreviewRenderState},
    types::*,
};
use crate::asset_catalog::DocumentFormat;
use crate::ui::{scene::FONT_PATH, theme as colors};
use bevy::{
    prelude::*,
    text::{FontStyle, FontWeight},
};

pub(super) fn sync(
    mut commands: Commands,
    state: Res<DocumentState>,
    server: Res<AssetServer>,
    bodies: Query<Entity, With<DocumentPreviewBody>>,
    children: Query<&Children>,
    visibility: Query<&Visibility>,
    mut scrolls: Query<&mut ScrollPosition, With<DocumentPreviewScroll>>,
    mut rendering: ResMut<PreviewRenderState>,
) {
    // 宿主UI重建后不能继续缓存已删除的实体；业务文档仍在，会按原预算恢复正文。
    if rendering
        .active
        .as_ref()
        .is_some_and(|active| !visibility.contains(active.root))
    {
        rendering.active = None;
    }
    if rendering
        .building
        .as_ref()
        .is_some_and(|build| !visibility.contains(build.root))
    {
        rendering.building = None;
    }
    if rendering
        .retiring
        .is_some_and(|root| !visibility.contains(root))
    {
        rendering.retiring = None;
    }
    // 正文树只有宿主、Text与Span三层。每帧只读取固定数量的首部节点，延迟删除落地后再读。
    if let Some(root) = rendering.retiring {
        if let Some(blocks) = children.get(root).ok().filter(|blocks| !blocks.is_empty()) {
            let mut budget = PREVIEW_RETIRE_NODES;
            for block in blocks.iter().take(PREVIEW_BUILD_BLOCKS) {
                if budget == 0 {
                    break;
                }
                if let Some(spans) = children.get(block).ok().filter(|spans| !spans.is_empty()) {
                    for span in spans.iter().take(budget) {
                        commands.entity(span).despawn();
                        budget -= 1;
                    }
                } else {
                    commands.entity(block).despawn();
                    budget -= 1;
                }
            }
        } else {
            commands.entity(root).despawn();
            rendering.retiring = None;
        }
    }
    let doc = state.current();
    let wanted = doc.map(|doc| PreviewIdentity {
        path: doc.path.clone(),
        generation: doc.generation,
        revision: doc.revision,
    });
    if let Some(active) = &rendering.active {
        if Some(&active.identity) != wanted.as_ref()
            && visibility
                .get(active.root)
                .is_ok_and(|value| *value != Visibility::Hidden)
        {
            commands.entity(active.root).insert(Visibility::Hidden);
        }
    }
    if rendering
        .building
        .as_ref()
        .is_some_and(|build| Some(&build.identity) != wanted.as_ref())
    {
        if rendering.retiring.is_none() {
            rendering.retiring = rendering.building.take().map(|build| build.root);
        }
        return; // 旧构造树真实退役前继续占槽，只保留当前资源中的最新目标。
    }
    if !state.visible || doc.is_none_or(|doc| doc.mode != ViewMode::Preview) {
        return;
    }
    let Some(doc) = doc.filter(|doc| doc.preview_revision == Some(doc.revision)) else {
        return;
    };
    let identity = wanted.unwrap();
    if rendering
        .active
        .as_ref()
        .is_some_and(|active| active.identity == identity)
    {
        let root = rendering.active.as_ref().unwrap().root;
        if visibility
            .get(root)
            .is_ok_and(|value| *value != Visibility::Inherited)
        {
            commands.entity(root).insert(Visibility::Inherited);
        }
        return;
    }
    let Ok(body) = bodies.single() else {
        return;
    };
    let font = feathrise_editor_core::editor_font(&server, FONT_PATH);
    if rendering.building.is_none() {
        if rendering.retiring.is_some() {
            return;
        }
        let root = commands
            .spawn((
                Node {
                    display: Display::None,
                    width: percent(100),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(8),
                    flex_shrink: 0.0,
                    ..default()
                },
                Visibility::Hidden,
            ))
            .id();
        commands.entity(body).add_child(root);
        let literal = (doc.format != DocumentFormat::Markdown).then(|| {
            let text = commands
                .spawn((
                    Text::new(""),
                    super::syntax::SyntaxPreview::new(doc),
                    TextFont::from_font_size(12.0).with_font(font.clone()),
                    TextColor(colors::TEXT_PRIMARY),
                    TextLayout::linebreak(bevy::text::LineBreak::WordOrCharacter),
                    Node {
                        width: percent(100),
                        flex_shrink: 0.0,
                        ..default()
                    },
                ))
                .id();
            commands.entity(root).add_child(text);
            text
        });
        rendering.building = Some(PreviewBuild {
            identity,
            root,
            literal,
            pages: doc.preview.clone(),
            page: 0,
            block: 0,
        });
    }
    let mut build = rendering.building.take().unwrap();
    for _ in 0..PREVIEW_BUILD_BLOCKS {
        while build.page < build.pages.len() && build.block == build.pages[build.page].blocks.len()
        {
            build.page += 1;
            build.block = 0;
        }
        let Some(block) = build
            .pages
            .get(build.page)
            .and_then(|page| page.blocks.get(build.block))
        else {
            break;
        };
        if let Some(text) = build.literal {
            // 普通文本始终是同一Text布局；后台块边界不能增加换行或段距。
            commands.entity(text).with_children(|text| {
                for span in &block.spans {
                    text.spawn((
                        TextSpan::new(&span.text),
                        TextFont::from_font_size(12.0).with_font(font.clone()),
                        TextColor(colors::TEXT_PRIMARY),
                    ));
                }
            });
        } else {
            spawn_block(&mut commands, build.root, block, font.clone());
        }
        build.block += 1;
    }
    let complete = build.page >= build.pages.len()
        || build.page + 1 == build.pages.len()
            && build.block == build.pages[build.page].blocks.len();
    if complete {
        if build.pages.len() <= 1
            && build
                .pages
                .first()
                .is_none_or(|page| page.blocks.is_empty())
        {
            commands.entity(build.root).with_children(|root| {
                root.spawn((
                    Text::new("空文档，点击“编辑”开始编写"),
                    TextFont::from_font_size(12.0).with_font(font),
                    TextColor(colors::TEXT_HINT),
                ));
            });
        }
        commands.entity(build.root).insert((
            Node {
                width: percent(100),
                flex_direction: FlexDirection::Column,
                row_gap: px(8),
                flex_shrink: 0.0,
                ..default()
            },
            Visibility::Inherited,
        ));
        if let Some(old) = rendering.active.replace(RenderedPreview {
            identity: build.identity,
            root: build.root,
        }) {
            commands.entity(old.root).insert((
                Visibility::Hidden,
                Node {
                    display: Display::None,
                    ..default()
                },
            ));
            rendering.retiring = Some(old.root);
        }
        for mut scroll in &mut scrolls {
            scroll.x = doc.preview_scroll.x;
            scroll.y = doc.preview_scroll.y;
        }
    } else {
        rendering.building = Some(build);
    }
}

fn spawn_block(commands: &mut Commands, root: Entity, block: &Block, font: Handle<Font>) {
    let size = match block.kind {
        BlockKind::Heading(1) => 21.0,
        BlockKind::Heading(2) => 18.0,
        BlockKind::Heading(_) => 15.0,
        BlockKind::Code => 11.0,
        _ => 12.0,
    };
    let mut text = commands.spawn((
        Text::new(""),
        TextFont::from_font_size(size).with_font(font.clone()),
        TextColor(colors::TEXT_PRIMARY),
        TextLayout::linebreak(bevy::text::LineBreak::WordOrCharacter),
        Node {
            width: percent(100),
            flex_shrink: 0.0,
            ..default()
        },
    ));
    let entity = text.id();
    if matches!(block.kind, BlockKind::Code | BlockKind::Quote) {
        text.insert((
            BackgroundColor(colors::INPUT_BG),
            BorderColor::all(colors::COPPER_BORDER_WEAK),
            Node {
                width: percent(100),
                flex_shrink: 0.0,
                padding: UiRect::all(px(5)),
                border: UiRect::left(px(2)),
                ..default()
            },
        ));
    }
    text.with_children(|text| {
        for span in &block.spans {
            text.spawn((
                TextSpan::new(&span.text),
                TextFont {
                    font: bevy::text::FontSource::Handle(font.clone()),
                    font_size: bevy::text::FontSize::Px(size),
                    weight: if span.style.bold || matches!(block.kind, BlockKind::Heading(_)) {
                        FontWeight::BOLD
                    } else {
                        FontWeight::NORMAL
                    },
                    style: if span.style.italic {
                        FontStyle::Italic
                    } else {
                        FontStyle::Normal
                    },
                    ..default()
                },
                TextColor(if span.style.struck {
                    colors::TEXT_MUTED
                } else if span.style.link || span.style.code {
                    colors::TEXT_VALUE
                } else if matches!(block.kind, BlockKind::Heading(_)) {
                    colors::TEXT_ACCENT
                } else {
                    colors::TEXT_PRIMARY
                }),
            ));
        }
    });
    commands.entity(root).add_child(entity);
}
