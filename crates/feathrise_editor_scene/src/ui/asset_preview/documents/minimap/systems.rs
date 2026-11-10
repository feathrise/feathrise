//! 原生字形按固定预算取样，后台生成固定纹理，完整身份匹配后才采用。

use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    tasks::{AsyncComputeTaskPool, futures::check_ready},
    text::{EditableText, TextLayoutInfo},
};

use super::super::{components::*, constants::*, resources::*, types::*};
use super::{
    math::{self, OverviewMetrics},
    raster,
};

pub(super) fn sync_visibility(
    documents: Res<DocumentState>,
    mut state: ResMut<DocumentMinimapState>,
    areas: Query<&ComputedNode, With<DocumentEditArea>>,
    mut roots: Query<&mut Node, With<DocumentMinimapRoot>>,
) {
    let shown = documents.visible
        && documents.in_center()
        && documents
            .current()
            .is_some_and(|doc| doc.mode == ViewMode::Edit)
        && areas.single().is_ok_and(|node| {
            node.size().x * node.inverse_scale_factor() >= MINIMAP_MIN_EDITOR_WIDTH
        });
    if state.shown != shown {
        state.shown = shown;
    }
    for mut node in &mut roots {
        let display = if shown { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
}

/// 借用当前身份比较，稳定帧只读长度与数值，不拷贝路径或扫描字形。
fn matches(
    key: &MinimapKey,
    source: Entity,
    doc: &Document,
    layout: &TextLayoutInfo,
    node: &ComputedNode,
) -> bool {
    key.source == source
        && key.document.path == doc.path
        && key.document.generation == doc.generation
        && key.revision == doc.revision
        && key.extent == layout.size
        && key.view_width == node.content_box().size().x
        && key.glyphs == layout.glyphs.len()
}

pub(super) fn update_outline(
    documents: Res<DocumentState>,
    mut state: ResMut<DocumentMinimapState>,
    sources: Query<(Entity, &DocumentSource, &ComputedNode, &TextLayoutInfo)>,
    mut images: ResMut<Assets<Image>>,
) {
    let current = sources
        .single()
        .ok()
        .and_then(|(entity, binding, node, layout)| {
            let doc = documents.current()?;
            (state.shown
                && documents.visible
                && documents.in_center()
                && doc.mode == ViewMode::Edit
                && binding
                    .key
                    .as_ref()
                    .is_some_and(|key| key.path == doc.path && key.generation == doc.generation)
                && layout.size.is_finite()
                && layout.size.x >= 0.0 && layout.size.y > 0.0 // 空文档仍有一行高度，轮廓应为空而不是永久等待。
                && node.content_box().size().x > 0.0)
                .then_some((entity, doc, node, layout))
        });
    if let Some(result) = state.task.as_mut().and_then(check_ready) {
        state.task = None;
        if current.is_some_and(|(entity, doc, node, layout)| {
            matches(&result.key, entity, doc, layout, node)
        }) {
            let image = Image::new(
                Extent3d {
                    width: MINIMAP_WIDTH,
                    height: MINIMAP_HEIGHT,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                result.pixels,
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::default(),
            );
            state.picture = Some(MinimapPicture {
                key: result.key,
                image: images.add(image),
            });
        }
    }
    let Some((entity, doc, node, layout)) = current else {
        state.collecting = None;
        state.picture = None;
        state.rejected = None;
        return; // 在途任务仍占唯一槽，关闭或快速切文件不会堆积并发任务。
    };
    if state
        .picture
        .as_ref()
        .is_some_and(|picture| !matches(&picture.key, entity, doc, layout, node))
    {
        state.picture = None;
    }
    if state.task.is_some()
        || state
            .picture
            .as_ref()
            .is_some_and(|picture| matches(&picture.key, entity, doc, layout, node))
        || state
            .rejected
            .as_ref()
            .is_some_and(|key| matches(key, entity, doc, layout, node))
    {
        return;
    }
    if state
        .collecting
        .as_ref()
        .is_none_or(|collection| !matches(&collection.key, entity, doc, layout, node))
    {
        let key = MinimapKey {
            source: entity,
            document: doc.source_key(),
            revision: doc.revision,
            extent: layout.size,
            view_width: node.content_box().size().x,
            glyphs: layout.glyphs.len(),
        };
        state.rejected = None;
        if layout.glyphs.len() > MINIMAP_MAX_GLYPHS {
            state.collecting = None;
            state.rejected = Some(key);
            return;
        }
        state.collecting = Some(MinimapCollection {
            key,
            cursor: 0,
            marks: Vec::with_capacity(layout.glyphs.len()),
        });
    }
    let collection = state.collecting.as_mut().unwrap();
    let end = (collection.cursor + MINIMAP_GLYPHS_PER_FRAME).min(layout.glyphs.len());
    for glyph in &layout.glyphs[collection.cursor..end] {
        // 横向保留原生视区比例，短行不能被放大到整条侧轨；纵向按全文高度归一化。
        let extent = Vec2::new(
            layout.size.x.max(node.content_box().size().x),
            layout.size.y,
        );
        if let Some(mark) = math::mark(glyph.position, glyph.atlas_info.rect.size(), extent) {
            collection.marks.push(mark);
        }
    }
    collection.cursor = end;
    if end == layout.glyphs.len() {
        let collection = state.collecting.take().unwrap();
        // 只烘焙白色轮廓遮罩；ImageNode负责语义着色，切换主题不重新分词或上传纹理。
        let color = [255, 255, 255, 255];
        state.task = Some(
            AsyncComputeTaskPool::get()
                .spawn(async move { raster::rasterize(collection.key, collection.marks, color) }),
        );
    }
}

pub(super) fn sync_viewport(
    documents: Res<DocumentState>,
    state: Res<DocumentMinimapState>,
    sources: Query<(Entity, &ComputedNode, &TextLayoutInfo, &EditableText), With<DocumentSource>>,
    rails: Query<&ComputedNode, With<DocumentMinimapRail>>,
    mut pictures: Query<
        (&mut ImageNode, &mut Node),
        (With<DocumentMinimapImage>, Without<DocumentMinimapThumb>),
    >,
    mut thumbs: Query<&mut Node, (With<DocumentMinimapThumb>, Without<DocumentMinimapImage>)>,
    mut labels: Query<&mut Text, With<DocumentMinimapLabel>>,
) {
    let current = sources.single().ok().zip(rails.single().ok()).and_then(
        |((entity, node, layout, scroll), rail)| {
            let metrics = OverviewMetrics::new(
                layout.size,
                node.content_box().size(),
                rail.size(),
                scroll.viewport.offset.y,
                node.inverse_scale_factor().recip(),
            )?;
            Some((entity, node, layout, metrics))
        },
    );
    let picture = current.and_then(|(entity, node, layout, _)| {
        let doc = documents.current()?;
        state
            .picture
            .as_ref()
            .filter(|picture| state.shown && matches(&picture.key, entity, doc, layout, node))
    });
    for (mut image, mut node) in &mut pictures {
        let handle = picture
            .map(|picture| picture.image.clone())
            .unwrap_or_default();
        if image.image != handle {
            image.image = handle;
        }
        let display = if picture.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        if let Some((_, source, _, metrics)) = current {
            let height = px(metrics.height * source.inverse_scale_factor());
            if node.height != height {
                node.height = height;
            }
        }
    }
    for mut node in &mut thumbs {
        let display = if state.shown && current.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        if let Some((_, source, _, metrics)) = current {
            let top = px(metrics.thumb_top * source.inverse_scale_factor());
            let height = px(metrics.thumb_height * source.inverse_scale_factor());
            if node.top != top {
                node.top = top;
            }
            if node.height != height {
                node.height = height;
            }
        }
    }
    let label = if !state.shown {
        ""
    } else if state.rejected.is_some() {
        "轮廓不可用"
    } else if picture.is_none() && state.shown {
        "轮廓更新中…"
    } else {
        "" // 手势说明由标题旁?展示，底栏只收真实轮廓状态。
    };
    for mut text in &mut labels {
        if text.as_str() != label {
            **text = label.into();
        }
    }
}
