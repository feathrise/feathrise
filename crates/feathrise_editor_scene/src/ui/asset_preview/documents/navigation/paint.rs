//! 行号复用原生可见行；匹配以有界行纹理绘制，全文小场景标记按固定预算推进。

use super::super::{
    components::{DocumentMinimapRail, DocumentSource},
    resources::{DocumentMinimapState, DocumentState},
    types::ViewMode,
};
use super::{components::*, constants::*, index, resources::*, types::*};
use crate::ui::theme as colors;
use bevy::{
    asset::RenderAssetUsages,
    image::ImageSampler,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    text::{EditableText, TextLayoutInfo},
    ui::FocusPolicy,
};

fn image(width: usize, height: usize) -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width: width as u32,
            height: height as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::nearest(); // 单像素行必须独立取样，不能混入邻行标记。
    image
}

/// 二分搜索实际视觉行，避免滚动或查找时从全文首行扫描。
pub(super) fn lower_line(count: usize, mut after: impl FnMut(usize) -> bool) -> usize {
    let (mut start, mut end) = (0, count);
    while start < end {
        let mid = start + (end - start) / 2;
        if after(mid) {
            start = mid + 1;
        } else {
            end = mid;
        }
    }
    start
}

pub(super) fn intersects(
    matches: &[std::ops::Range<usize>],
    range: std::ops::Range<usize>,
) -> bool {
    let first = matches.partition_point(|found| found.end <= range.start);
    matches
        .get(first)
        .is_some_and(|found| found.start < range.end)
}

pub(super) fn fill_row(pixels: &mut [u8], start: f32, end: f32, width: f32) {
    if !start.is_finite() || !end.is_finite() || width <= 0.0 {
        return;
    }
    let start = (start / width * pixels.len() as f32)
        .floor()
        .clamp(0.0, pixels.len() as f32) as usize;
    let end = (end / width * pixels.len() as f32)
        .ceil()
        .clamp(0.0, pixels.len() as f32) as usize;
    if start < end {
        pixels[start..end].fill(1);
    }
}

pub(super) fn sync(
    documents: Res<DocumentState>,
    navigation: Res<DocumentNavigation>,
    minimap: Res<DocumentMinimapState>,
    mut state: ResMut<NavigationPaint>,
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    sources: Query<(
        Entity,
        &DocumentSource,
        &EditableText,
        &ComputedNode,
        &TextLayoutInfo,
        &TextFont,
    )>,
    hosts: Query<
        (Entity, Has<NavigationGutter>),
        Or<(With<NavigationGutter>, With<NavigationMatchLayer>)>,
    >,
    rails: Query<&ComputedNode, With<DocumentMinimapRail>>,
    mut numbers: Query<(&NavigationRow, &mut Node, &mut Text), Without<NavigationHighlight>>,
    mut highlights: Query<
        (&NavigationHighlight, &mut Node, &mut ImageNode),
        Without<NavigationRow>,
    >,
    mut overviews: Query<
        (&mut Node, &mut ImageNode),
        (
            With<NavigationOverview>,
            Without<NavigationRow>,
            Without<NavigationHighlight>,
        ),
    >,
) {
    let current = sources.single().ok().and_then(|source| {
        let doc = documents.current()?;
        let index = navigation
            .index
            .as_ref()
            .filter(|result| index::current(&result.key, doc, &navigation))?;
        (documents.visible
            && doc.mode == ViewMode::Edit
            && source.1.key.as_ref() == Some(&index.key.document)
            && !source.2.is_composing()
            && source.3.content_box().size().min_element() > 0.0)
            .then_some((source, index))
    });
    let Some(((entity, _, native, node, info, font), index)) = current else {
        for (_, mut node, _) in &mut numbers {
            if node.display != Display::None {
                node.display = Display::None;
            }
        }
        for (_, mut node, _) in &mut highlights {
            if node.display != Display::None {
                node.display = Display::None;
            }
        }
        for (mut node, _) in &mut overviews {
            if node.display != Display::None {
                node.display = Display::None;
            }
        }
        return;
    };
    let Some(layout) = native.editor.try_layout() else {
        return;
    };
    let width = node.content_box().size().x;
    let inverse = node.inverse_scale_factor();
    let same = state.key.as_ref().is_some_and(|key| {
        key.source == entity
            && key.index == index.key
            && key.extent == info.size
            && key.width == width
            && key.scale == inverse
            && key.lines == layout.len()
    });
    if !same {
        state.key = Some(PaintKey {
            source: entity,
            index: index.key.clone(),
            extent: info.size,
            width,
            scale: inverse,
            lines: layout.len(),
        });
        for slot in &mut state.rows {
            slot.paint = None;
        }
        state.overview_cursor = 0;
        state.overview_ready = false;
        state.overview_pixels = vec![0; 4 * OVERVIEW_HEIGHT * 4];
    }
    let top = native.viewport.offset.y;
    let bottom = top + node.content_box().size().y;
    let first = lower_line(layout.len(), |i| {
        layout.get(i).unwrap().metrics().block_max_coord <= top
    });
    let end = lower_line(layout.len(), |i| {
        layout.get(i).unwrap().metrics().block_min_coord < bottom
    });
    let desired = end.saturating_sub(first).min(MAX_VISIBLE_ROWS);
    let gutter = hosts
        .iter()
        .find(|(_, gutter)| *gutter)
        .map(|(entity, _)| entity);
    let layer = hosts
        .iter()
        .find(|(_, gutter)| !*gutter)
        .map(|(entity, _)| entity);
    let (Some(gutter), Some(layer)) = (gutter, layer) else {
        return;
    };
    if state
        .rows
        .first()
        .is_some_and(|slot| !numbers.contains(slot.number) || !highlights.contains(slot.highlight))
    {
        state.rows.clear(); // 编辑器UI重建后不复用已经销毁的行槽实体。
    }
    // 可见池按32行/帧增长，最多512行；正文不因标记池未就绪而禁用。
    for _ in 0..desired.saturating_sub(state.rows.len()).min(ROWS_PER_FRAME) {
        let slot = state.rows.len();
        let number = commands
            .spawn((
                NavigationRow(slot),
                Text::new(""),
                TextLayout::no_wrap(),
                TextFont {
                    font: font.font.clone(),
                    font_size: bevy::text::FontSize::Px(12.0),
                    ..default()
                },
                TextColor(colors::TEXT_MUTED),
                FocusPolicy::Pass,
                Node {
                    display: Display::None,
                    position_type: PositionType::Absolute,
                    right: px(4),
                    top: px(0),
                    ..default()
                },
                ChildOf(gutter),
            ))
            .id();
        let highlight = commands
            .spawn((
                NavigationHighlight(slot),
                ImageNode {
                    image_mode: bevy::ui::widget::NodeImageMode::Stretch,
                    color: colors::TEXT_ACCENT.with_alpha(0.22), // 白色遮罩只描述匹配区域，颜色在绘制时随主题变化。
                    ..default()
                },
                FocusPolicy::Pass,
                Node {
                    display: Display::None,
                    position_type: PositionType::Absolute,
                    left: px(0),
                    top: px(0),
                    width: percent(100),
                    height: px(0),
                    ..default()
                },
                ChildOf(layer),
            ))
            .id();
        state.rows.push(RowSlot {
            number,
            highlight,
            paint: None,
        });
    }
    let searching = navigation.mode.searching() && !index.matches.is_empty();
    let color = [255, 255, 255, 255];
    if searching && state.atlas == Handle::default() {
        state.atlas = images.add(image(ROW_WIDTH, MAX_VISIBLE_ROWS));
    }
    let mut budget = CLUSTERS_PER_FRAME;
    let mut completed = 0;
    // 行槽按真实视觉行复用，滚动一行时只补进入视区的新行。
    for line_index in first..end.min(first + MAX_VISIBLE_ROWS) {
        let slot = state
            .rows
            .iter()
            .position(|slot| {
                slot.paint
                    .as_ref()
                    .is_some_and(|row| row.line == line_index)
            })
            .or_else(|| {
                state.rows.iter().position(|slot| {
                    slot.paint
                        .as_ref()
                        .is_none_or(|row| row.line < first || row.line >= end)
                })
            });
        let Some(slot) = slot else {
            break;
        };
        let line = layout.get(line_index).unwrap();
        let metrics = line.metrics();
        if state.rows[slot]
            .paint
            .as_ref()
            .is_none_or(|row| row.line != line_index)
        {
            state.rows[slot].paint = Some(RowPaint {
                line: line_index,
                run: 0,
                cluster: 0,
                x: metrics.offset + metrics.inline_min_coord,
                ready: false,
                pixels: vec![0; ROW_WIDTH],
            });
        }
        let atlas = state.atlas.clone();
        let row = state.rows[slot].paint.as_mut().unwrap();
        if !row.ready && completed < ROWS_PER_FRAME {
            if !intersects(&index.matches, line.text_range()) {
                row.ready = true;
            } else if searching {
                for (run_index, run) in line.runs().enumerate().skip(row.run) {
                    if budget == 0 {
                        break;
                    }
                    if row.run != run_index {
                        row.run = run_index;
                        row.cluster = 0;
                    }
                    while row.cluster < run.len() && budget > 0 {
                        let cluster = run
                            .get(run.visual_to_logical(row.cluster).unwrap())
                            .unwrap();
                        let advance = cluster.advance();
                        if intersects(&index.matches, cluster.text_range()) {
                            fill_row(&mut row.pixels, row.x, row.x + advance, width);
                        }
                        row.x += advance;
                        row.cluster += 1;
                        budget -= 1;
                    }
                    if row.cluster < run.len() {
                        break;
                    }
                    row.run = run_index + 1;
                    row.cluster = 0;
                }
                row.ready = row.run >= line.runs().count();
            }
            if row.ready {
                completed += 1;
                if searching
                    && let Some(mut picture) = images.get_mut(&atlas)
                    && let Some(data) = picture.data.as_mut()
                {
                    for (pixel, on) in data[slot * ROW_WIDTH * 4..(slot + 1) * ROW_WIDTH * 4]
                        .chunks_exact_mut(4)
                        .zip(&row.pixels)
                    {
                        pixel.copy_from_slice(if *on != 0 { &color } else { &[0, 0, 0, 0] });
                    }
                }
            }
        }
    }
    let pad_top = node.content_inset().min_inset.y * inverse;
    for (slot, mut node, mut text) in &mut numbers {
        let row = state.rows.get(slot.0).and_then(|slot| slot.paint.as_ref());
        if let Some(row) = row.filter(|row| row.line >= first && row.line < end) {
            let line = layout.get(row.line).unwrap();
            let byte = line.text_range().start;
            let logical = index
                .lines
                .partition_point(|start| *start <= byte)
                .saturating_sub(1);
            let value = if index.lines[logical] == byte {
                (logical + 1).to_string()
            } else {
                String::new()
            };
            if text.as_str() != value {
                **text = value;
            }
            if node.display != Display::Flex {
                node.display = Display::Flex;
            }
            node.top = px(pad_top + (line.metrics().block_min_coord - top) * inverse);
        } else {
            if node.display != Display::None {
                node.display = Display::None;
            }
        }
    }
    for (slot, mut node, mut picture) in &mut highlights {
        let row = state.rows.get(slot.0).and_then(|slot| slot.paint.as_ref());
        if let Some(row) =
            row.filter(|row| searching && row.ready && row.line >= first && row.line < end)
        {
            let metrics = *layout.get(row.line).unwrap().metrics();
            if node.display != Display::Flex {
                node.display = Display::Flex;
            }
            node.top = px((metrics.block_min_coord - top) * inverse);
            node.height = px((metrics.block_max_coord - metrics.block_min_coord) * inverse);
            if picture.image != state.atlas {
                picture.image = state.atlas.clone();
            }
            let rect = Some(Rect::new(
                0.0,
                slot.0 as f32,
                ROW_WIDTH as f32,
                slot.0 as f32 + 1.0,
            ));
            if picture.rect != rect {
                picture.rect = rect;
            }
        } else {
            if node.display != Display::None {
                node.display = Display::None;
            }
        }
    }
    if searching && minimap.shown && !state.overview_ready {
        let stop = (state.overview_cursor + OVERVIEW_PER_FRAME).min(index.matches.len());
        let color = [255, 255, 255, 255]; // 小场景匹配标记共用白色遮罩与ImageNode强调色。
        for found in &index.matches[state.overview_cursor..stop] {
            let visual = lower_line(layout.len(), |i| {
                layout.get(i).unwrap().text_range().end <= found.start
            })
            .min(layout.len().saturating_sub(1));
            if let Some(line) = layout.get(visual) {
                let y = (line.metrics().block_min_coord / info.size.y * OVERVIEW_HEIGHT as f32)
                    .clamp(0.0, (OVERVIEW_HEIGHT - 1) as f32) as usize;
                for pixel in state.overview_pixels[y * 16..((y + 2).min(OVERVIEW_HEIGHT)) * 16]
                    .chunks_exact_mut(4)
                {
                    pixel.copy_from_slice(&color);
                }
            }
        }
        state.overview_cursor = stop;
        if stop == index.matches.len() {
            let mut picture = image(4, OVERVIEW_HEIGHT);
            picture.data = Some(state.overview_pixels.clone()); // 固定16 KiB，全部完成后才原子发布。
            state.overview = images.add(picture);
            state.overview_ready = true;
        }
    }
    for (mut node, mut picture) in &mut overviews {
        let rail = rails.single().ok();
        if searching
            && minimap.shown
            && state.overview_ready
            && let Some(rail) = rail
        {
            if node.display != Display::Flex {
                node.display = Display::Flex;
            }
            node.height = px((info.size.y * rail.size().x / width)
                .min(rail.size().y)
                .max(1.0)
                * inverse);
            if picture.image != state.overview {
                picture.image = state.overview.clone();
            }
        } else {
            if node.display != Display::None {
                node.display = Display::None;
            }
        }
    }
}
