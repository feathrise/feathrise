//! 地图与资产列表固定行池及增量文本同步。

use crate::ui::theme as colors;
use bevy::prelude::*; // UI实体生成、Interaction和文本查询。
use feathrise_map_content::{MapAssetKind, MapCatalogAsset}; // 地图进入测试仍使用公共目录，资产行读取公共类型。 // 动态行使用静态BSN相同水墨配色。

use crate::authoring::{EditorAuthoringSession, GlobalEnvironmentAuthoringState}; // 显式进入与最终切图共用同一作者保存判断。
use crate::components::*; // 列表容器、行和子文本marker。
use crate::constants::{
    MAP_BROWSER_ROW_GAP, MAP_BROWSER_ROW_HEIGHT, MAP_BROWSER_ROW_PITCH, MAP_BROWSER_ROW_POOL_COUNT,
}; // 地图树滚动内容和固定虚拟行池共享同一组尺寸常量。
use crate::resources::{
    EditorAssetCatalog, MapBrowserDragState, MapBrowserIndexState, MapBrowserState,
    MapDeletionTask, MapEditorState,
}; // 当前虚拟目录索引、拖拽落点、选择、目录页和状态文本。
use crate::types::{MapBrowserDropTarget, MapBrowserItem, MapSwitchConfirmation, MapSwitchStage}; // 动态行及未保存切图持有稳定身份。
use crate::ui::sidebars::MapEditorAssetPagination; // 纯资产分页条只在目录跨页时参与布局。

mod asset_window;
pub(crate) use asset_window::AssetViewport;

pub(crate) const ASSET_ROW_COUNT: usize = 12; // 最小可见行池；实际容量只随视口高度增长，不限制目录总量。

/// 返回全部可见树行形成的滚动内容高度；末行之后不额外保留行距。
fn map_tree_content_height(row_count: usize) -> f32 {
    if row_count == 0 {
        0.0 // 空树只显示固定根目录行，内部滚动内容没有高度。
    } else {
        row_count as f32 * MAP_BROWSER_ROW_PITCH - MAP_BROWSER_ROW_GAP // 每行包含行高和后续间距，最后减去尾间距。
    }
}

/// 同步地图树滚动高度与摘要，并继续钳制资产页码、列表高度和纯资产分页条。
pub(crate) fn sync_dynamic_list_layouts(
    map_index: Res<MapBrowserIndexState>, // 完整树总量和当前展开投影。
    editor: Res<MapEditorState>,          // 关闭F12时不刷新静态侧栏。
    mut asset_catalog: ResMut<EditorAssetCatalog>, // 目录刷新后资产页码需安全钳制。
    mut pools: ParamSet<(
        Query<&mut Node, With<MapEditorMapList>>, // 地图树完整滚动内容容器。
        Query<(&ComputedNode, &mut ScrollPosition), With<MapEditorMapTreeScroll>>, // 树缩短后钳制旧滚动位置。
        Query<&mut Node, With<MapEditorAssetList>>, // 资产十二行池容器。
        Query<&mut Node, With<MapEditorAssetPagination>>, // 单页时完全收起纯分页条。
    )>,
    mut summaries: Query<(
        &mut Text,
        Option<&MapEditorMapListTitle>,
        Option<&MapEditorAssetListTitle>,
    )>,
    mut previous: Local<Option<(usize, usize, usize, usize, bool)>>, // 树总量、展开量、资产页和整理态稳定时不重写UI。
) {
    if !editor.open {
        return; // 关闭F12时无需刷新静态侧栏布局。
    }
    let map_total = map_index.total_entries; // 总数与当前公开投影来自同一次原子采用。
    let map_visible = map_index.entries.len(); // 当前展开路径真正参与滚动的项目数。
    let map_content_height = map_tree_content_height(map_visible); // 内容高度不依赖固定虚拟行池容量。
    for (computed, mut scroll) in &mut pools.p1() {
        let viewport_height = computed.size().y * computed.inverse_scale_factor(); // ScrollPosition使用逻辑像素。
        let maximum = (map_content_height - viewport_height).max(0.0); // 折叠或热更新缩短树后不能停在空白区。
        let clamped = scroll.y.clamp(0.0, maximum); // 先只读计算，稳定帧不误触Bevy变化检测。
        if clamped != scroll.y {
            scroll.y = clamped; // 只有越界时才写组件，虚拟行池稳定帧不会反复刷新。
        }
    }
    let asset_total = asset_catalog.entries.len();
    if asset_catalog.page != 0 { asset_catalog.page = 0; } // 旧内部字段保留兼容，但不再定义目录窗口。
    let signature = (
        map_total,
        map_visible,
        asset_total,
        0,
        map_index.rebuilding,
    );
    if previous.as_ref() == Some(&signature) {
        return; // 稳定帧不重写固定行池Node或摘要Text，避免无意义UI布局重算。
    }
    *previous = Some(signature); // 首个打开帧和任何目录/页码变化都完整刷新两类列表。
    for mut node in &mut pools.p0() {
        node.height = px(map_content_height); // 完整滚动范围随展开结果一次更新，UI实体数量保持固定。
    }
    for mut node in &mut pools.p3() {
        node.display = Display::None; // 兼容旧夹具marker，正式浏览器不创建翻页控件。
    }
    for (mut text, map_title, asset_title) in &mut summaries {
        if map_title.is_some() {
            **text = format!(
                "地图 · 显示{map_visible}项 / 共{map_total}项{}",
                if map_index.rebuilding {
                    " · 整理中"
                } else {
                    ""
                },
            );
        } else if asset_title.is_some() {
            **text = format!("工程 · {asset_total}项");
        }
    }
}

/// 在BSN容器下创建固定地图虚拟行池与资产分页行池。
pub(crate) fn spawn_dynamic_rows(
    mut commands: Commands,
    map_lists: Query<Entity, Added<MapEditorMapList>>,
    asset_lists: Query<(Entity, Option<&Children>), With<MapEditorAssetList>>,
    viewport: AssetViewport,
    asset_server: Res<AssetServer>,
    icons: Option<Res<super::map_sidebar::MapBrowserTreeIcons>>,
    mut root_icons: Query<(&MapEditorMapRowGlyph, &mut ImageNode), Added<MapEditorMapRowGlyph>>,
    mut tree_glyphs: Query<(&MapEditorTreeGlyph, &mut ImageNode), (Added<MapEditorTreeGlyph>, Without<MapEditorMapRowGlyph>)>,
) {
    for (glyph, mut image) in &mut tree_glyphs {
        image.image = asset_server.load(glyph.0);
        image.color = colors::TEXT_PRIMARY;
    } // 只处理首次挂载的固定行池，不在稳定帧重新加载图标。
    let font = feathrise_editor_core::editor_font(&asset_server, "fonts/open_source/noto_sans/cjk/NotoSansCJKsc-VF.otf");
    let tree_icons = if !map_lists.is_empty() || root_icons.iter().any(|(glyph, _)| matches!(glyph, MapEditorMapRowGlyph::Root)) {
        let handles = icons.as_deref().cloned().unwrap_or_else(|| super::map_sidebar::MapBrowserTreeIcons::load(&asset_server));
        if icons.is_none() { commands.insert_resource(handles.clone()); }
        Some(handles)
    } else { None };
    if let Some(icons) = &tree_icons {
        for (glyph, mut image) in &mut root_icons {
            if matches!(glyph, MapEditorMapRowGlyph::Root) { image.image = icons.root.clone(); }
        }
    }
    for container in &map_lists {
        commands.entity(container).with_children(|parent| {
            for slot in 0..MAP_BROWSER_ROW_POOL_COUNT {
                parent
                    .spawn((
                        Button,
                        MapEditorMapRow { slot, item: None },
                        bevy::ui::RelativeCursorPosition::default(), // 区分真实悬停和移出后仍保留的Pressed，文件夹拖动才能正确识别落点。
                        Node {
                            display: Display::None,                // 首次投影完成前隐藏未绑定槽位。
                            position_type: PositionType::Absolute, // 每个槽按完整树绝对下标定位，不创建上方占位实体。
                            top: px(0),  // 同步系统按滚动窗口绑定后写入真实树位置。
                            left: px(0), // 行从滚动内容左边缘开始。
                            width: percent(100), // 深度只改变内边距，不改变命中宽度。
                            height: px(MAP_BROWSER_ROW_HEIGHT), // 固定紧凑行高支持稳定虚拟换绑。
                            flex_shrink: 0.0, // 绝对定位行不参与父级压缩。
                            align_items: AlignItems::Center, // 目录与地图文字垂直居中。
                            padding: UiRect::horizontal(px(5)), // 同步系统在左侧叠加有限树缩进。
                            column_gap: px(5),
                            border_radius: BorderRadius::all(px(2)), // 树行使用连续背景，去掉每行四周的按钮边框。
                            overflow: Overflow::clip_x(), // 长名称裁切在当前栏宽内。
                            ..default()
                        },
                        BackgroundColor(colors::PANEL_BG),
                        BorderColor::all(colors::COPPER_BORDER_WEAK), // 继续保留原交互样式组件，常态不绘制边框。
                    ))
                    .with_children(|row| {
                        let icons = tree_icons.as_ref().expect("地图行池与图标一起初始化");
                        row.spawn((MapEditorMapRowGlyph::Disclosure, ImageNode { image: icons.expand.clone(), color: colors::TEXT_MUTED, ..default() },
                            Node { width: px(14), height: px(14), flex_shrink: 0.0, ..default() }, Pickable::IGNORE));
                        row.spawn((MapEditorMapRowGlyph::Entry, ImageNode { image: icons.map.clone(), color: colors::TEXT_PRIMARY, ..default() },
                            Node { width: px(18), height: px(18), flex_shrink: 0.0, ..default() }, Pickable::IGNORE));
                        row.spawn((MapEditorMapRowNameCell, Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0), overflow: Overflow::clip_x(), ..default() }, Pickable::IGNORE))
                            .with_child((
                                MapEditorMapRowText, // 文本子实体保持固定，滚动时只原地改内容。
                                Text::new(""),       // 首次绑定前没有可见占位文字。
                                TextFont {
                                    font: font.clone().into(), // 名称和固定根行共用正式中文字体与字号。
                                    font_size: 13.0.into(),
                                    ..default()
                                },
                                TextColor(colors::TEXT_PRIMARY), // 同步系统按选择和类别切换文字色。
                                TextLayout::no_wrap(), Node { min_width: px(0), ..default() }, Pickable::IGNORE,
                            ));
                        row.spawn((MapEditorMapRowCurrent, Text::new(""), TextFont { font: font.clone().into(), font_size: 11.0.into(), ..default() },
                            TextColor(colors::TEXT_MUTED), TextLayout::no_wrap(), Node { flex_shrink: 0.0, ..default() }, Pickable::IGNORE));
                    });
            }
        });
    }
    let capacity = viewport.geometry(usize::MAX).capacity;
    for (container, children) in &asset_lists {
        let existing = children.map_or(0, Children::len);
        if existing >= capacity { continue; }
        commands.entity(container).with_children(|parent| {
            for slot in existing..capacity {
                parent
                    .spawn((
                        Button,
                        MapEditorAssetRow {
                            slot,
                            entry_index: None,
                        },
                        AssetRowBinding::default(),
                        Node {
                            display: Display::None,
                            position_type: PositionType::Absolute, left: px(0), top: px(0),
                            width: percent(100),
                            height: px(feathrise_editor_ui::metrics::ROW_HEIGHT),
                            flex_shrink: 0.0,
                            align_items: AlignItems::Center,
                            padding: UiRect::horizontal(px(5)),
                            border_radius: BorderRadius::all(px(2)),
                            column_gap: px(5),
                            overflow: Overflow::clip_x(),
                            ..default()
                        },
                        BackgroundColor(colors::PANEL_BG),
                        BorderColor::all(colors::COPPER_BORDER_WEAK),
                    ))
                    .with_children(|row| {
                        crate::ui::asset_thumbnails::spawn_thumbnail_nodes(row, slot, &font);
                        row.spawn((MapEditorAssetRowGlyph, ImageNode::default(),
                            Node { width: px(16), height: px(16), flex_shrink: 0.0, ..default() }, Pickable::IGNORE));
                        row.spawn((
                            MapEditorAssetRowText,
                            Text::new(""),
                            TextFont {
                                font: font.clone().into(),
                                font_size: feathrise_editor_ui::metrics::FONT_CONTROL.into(),
                                ..default()
                            },
                            TextColor(colors::TEXT_PRIMARY),
                            TextLayout::no_wrap(), Node { min_width: px(0), flex_grow: 1.0, flex_basis: px(0), ..default() }, Pickable::IGNORE,
                        ));
                    });
            }
        });
    }
}

/// 完整投影、选择、拖放或滚动改变时原地刷新地图树虚拟行池。
pub(crate) fn sync_map_rows(
    index: Res<MapBrowserIndexState>, // 当前展开路径的完整可见树投影。
    browser: Res<MapBrowserState>,    // 根、文件夹或地图稳定高亮目标与展开集合。
    drag: Res<MapBrowserDragState>,   // 有效文件夹落点需要额外边框反馈。
    editor: Res<MapEditorState>,      // 当前已真正进入的地图身份。
    scrolls: Query<(Ref<ScrollPosition>, Option<&ComputedNode>), With<MapEditorMapTreeScroll>>, // 滚动和实际栏宽共同约束固定行池。
    mut previous_width: Local<Option<f32>>,
    mut rows: Query<(
        &mut MapEditorMapRow,     // 固定行池当前绑定的文件夹或MapId。
        &mut Node,                // 目录项不存在时只原地隐藏行。
        &mut BackgroundColor,     // 高亮选择跟随编辑器主题，默认使用参考图的蓝灰色。
        &mut BorderColor,         // 保留原落点样式状态，普通树行不绘制按钮边框。
        &Children,                // 文本子节点保持固定实体。
        Option<Ref<Interaction>>, // 固定行池的悬停边沿也需刷新；兼容原域无按钮夹具。
        Option<&mut feathrise_editor_ui::EditorButtonClick>, // 行池换绑时取消原按钮手势。
    )>,
    mut presented_scope: Local<Option<(u64, Option<(crate::types::MapBrowserIndexSignature, u64)>)>>,
    mut texts: Query<(&mut Text, &mut TextColor), With<MapEditorMapRowText>>,
    name_cells: Query<&Children, With<MapEditorMapRowNameCell>>,
    mut current_labels: Query<&mut Text, (With<MapEditorMapRowCurrent>, Without<MapEditorMapRowText>)>,
    mut glyphs: Query<(&MapEditorMapRowGlyph, &mut ImageNode)>,
    icons: Option<Res<super::map_sidebar::MapBrowserTreeIcons>>,
) {
    let Ok((scroll, computed)) = scrolls.single() else {
        return; // 静态BSN尚未建立唯一树视口时不写任何行。
    };
    let width = computed.filter(|node| node.size().x > 0.0).map_or(260.0, |node| node.size().x * node.inverse_scale_factor());
    let width_changed = previous_width.as_ref() != Some(&width);
    *previous_width = Some(width);
    let scroll_changed = scroll.is_changed(); // 滚动期间只重绑固定二十八槽，不重建实体。
    if !editor.open
        || !(editor.is_changed()
            || index.is_changed()
            || browser.is_changed()
            || drag.is_changed()
            || scroll_changed
            || width_changed
            || rows.iter().any(|(_, _, _, _, _, interaction, _)| {
                interaction.is_some_and(|value| value.is_changed())
            }))
    {
        return; // 稳定帧不重写任何虚拟行实体。
    }
    let first_visible = (scroll.y.max(0.0) / MAP_BROWSER_ROW_PITCH).floor() as usize; // 逻辑像素偏移映射到完整树下标。
    let start = first_visible.saturating_sub(1); // 额外保留一行上方缓冲，避免亚像素滚动露白。
    let scope = (browser.document_revision, index.applied_projection.clone());
    let scope_changed = presented_scope.as_ref() != Some(&scope);
    *presented_scope = Some(scope);
    for (mut row, mut node, mut background, mut border, children, interaction, click) in &mut rows {
        let absolute_index = start.saturating_add(row.slot); // 固定槽位映射当前滚动窗口的绝对树行。
        if (scope_changed || row.item.as_ref() != index.entries.get(absolute_index).map(|entry| &entry.item))
            && let Some(mut click) = click
        {
            click.invalidate(); // 滚动、目录换代或隐藏槽不能把旧按下重定向给新目录项。
        }
        let Some(entry) = index.entries.get(absolute_index) else {
            if row.item.is_some() {
                row.item = None;
            } // 隐藏槽不能保留上一个滚动窗口的稳定身份。
            if node.display != Display::None {
                node.display = Display::None;
            }
            continue;
        };
        if row.item.as_ref() != Some(&entry.item) {
            row.item = Some(entry.item.clone());
        } // 行池原地绑定完整业务对象。
        if node.display != Display::Flex {
            node.display = Display::Flex;
        }
        let top = px(absolute_index as f32 * MAP_BROWSER_ROW_PITCH);
        if node.top != top {
            node.top = top;
        } // 行保持在完整滚动内容的真实位置。
        // 根目录单独占一行；首层目录从5px起，每级增加18px，图标列严格对齐。
        let indentation = (entry.depth.saturating_sub(1).min(8) as f32 * 18.0).min((width - 104.0).max(0.0));
        let left = px(5.0 + indentation); // 深层窄栏也为类型图标、短名称和右侧“当前”保留位置。
        if node.padding.left != left {
            node.padding.left = left;
        } // 深层缩进封顶，名称槽裁切且为独立当前标记保留宽度。
        if node.padding.right != px(5) {
            node.padding.right = px(5);
        }
        let selected = browser.item_selected(&entry.item); // 整组选中地图和文件夹共用主题高亮。
        let active = matches!(
            &entry.item,
            MapBrowserItem::Map(map_id) if editor.selected_map_id.as_ref() == Some(map_id)
        ); // 当前地图标记独立于目录高亮。
        let drop_target = matches!(
            (&entry.item, drag.drop_target.as_ref()),
            (MapBrowserItem::Folder(folder_id), Some(MapBrowserDropTarget::Folder(target_id)))
                if folder_id == target_id
        ); // 只有文件夹行能成为拖拽落点。
        let (background_color, border_color) = super::map_sidebar::tree_row_colors(
            selected,
            drop_target,
            interaction.map_or(Interaction::None, |value| *value),
        );
        background.set_if_neq(BackgroundColor(background_color));
        border.set_if_neq(border_color); // 与根行共用落点、选中和悬停优先级，只写差异。
        for child in children.iter() {
            if let Ok(mut text) = current_labels.get_mut(child) {
                let value = if active { "当前" } else { "" };
                if text.as_str() != value { **text = value.to_owned(); }
            }
            if let Some(icons) = icons.as_ref()
                && let Ok((role, mut image)) = glyphs.get_mut(child)
            {
                let folder = matches!(entry.item, MapBrowserItem::Folder(_));
                let expanded = matches!(&entry.item, MapBrowserItem::Folder(id) if browser.expanded_folder_ids.contains(id));
                let handle = match role {
                    MapEditorMapRowGlyph::Entry if folder => &icons.folder,
                    MapEditorMapRowGlyph::Entry => &icons.map,
                    MapEditorMapRowGlyph::Disclosure if expanded => &icons.collapse,
                    MapEditorMapRowGlyph::Disclosure => &icons.expand,
                    MapEditorMapRowGlyph::Root => &icons.root,
                };
                if image.image != *handle { image.image = handle.clone(); }
                // 地图保留箭头列的宽度但不画箭头，名称和同级文件夹保持统一基线。
                let color = if matches!(role, MapEditorMapRowGlyph::Disclosure) && !folder { Color::NONE } else { colors::TEXT_PRIMARY };
                if image.color != color { image.color = color; }
            }
        }
        // 名称只有一份；兼容原最小夹具的直接Text子节点，正式页面经固定裁切单元访问。
        let labels = children.iter().chain(children.iter().filter_map(|child| name_cells.get(child).ok()).flat_map(|cell| cell.iter()));
        for child in labels {
            if let Ok((mut text, mut color)) = texts.get_mut(child) {
                let label = &entry.display_name; // 稳定ID仍留在业务绑定与所选详情，常态树行只显示作者名称。
                if text.as_str() != label {
                    **text = label.clone();
                }
                let text_color = if selected || drop_target {
                    colors::TEXT_ACCENT
                } else if matches!(entry.item, MapBrowserItem::Folder(_) | MapBrowserItem::Root) {
                    colors::TEXT_VALUE // 文件夹名称使用次强值色与地图区分。
                } else if active {
                    colors::TEXT_VALUE // 当前图但非操作目标时使用次强值色。
                } else {
                    colors::TEXT_PRIMARY
                };
                color.set_if_neq(TextColor(text_color));
            }
        }
    }
}

/// 验证目录选择并登记显式进入请求；本函数不执行加载或文件IO。
pub(crate) fn request_selected_map_entry(
    editor: &mut MapEditorState,       // 读取目录选择并登记待切图MapId。
    deletion: &MapDeletionTask,        // 地图回收的任一阶段都与切图互斥。
    catalog: Option<&MapCatalogAsset>, // 按钮帧重新验证选中项仍存在。
    session: &EditorAuthoringSession,
    global: &GlobalEnvironmentAuthoringState,
    preserve_document: bool, // 正式多文档入口保留源草稿；独立旧入口仍支持原切图确认。
) {
    if deletion.busy() {
        editor.status = "地图回收事务完成前不能进入其他地图".to_owned(); // 不让场景加载目录正在移动。
        return;
    }
    if editor.pending_map_transition.is_some()
        || editor.map_transition_in_progress
        || editor.map_switch_confirmation.is_some()
    {
        editor.status = "已有地图进入请求正在等待资源".to_owned(); // 稳定目标不被后一次点击覆盖。
        return;
    }
    let Some(map_id) = editor.map_list_selection_id.clone() else {
        editor.status = "请先在地图目录中选中一张地图".to_owned(); // 空选择不猜测任意地图。
        return;
    };
    let Some(entry) =
        catalog.and_then(|catalog| catalog.maps.iter().find(|entry| entry.id == map_id))
    else {
        editor.status = format!("地图{map_id}已不在当前可见树中"); // 热更删除后的旧选择不能无限等待。
        return;
    };
    if editor.selected_map_id.as_ref() == Some(&map_id) {
        editor.status = format!("当前已在地图“{}” [{}]", entry.display_name, entry.id); // 同图进入不重走LoadingScene。
        return;
    }
    if session.saving || editor.save_requested {
        editor.status = "当前地图正在保存，请完成后再进入其他地图".to_owned();
        return; // 仅补登记的保存也必须等待文件事务收口。
    }
    if !preserve_document && session.needs_save_before_map_switch(editor, global) {
        editor.map_switch_confirmation = Some(MapSwitchConfirmation {
            source_map_id: editor.selected_map_id.clone(),
            source_revision: session.revision,
            global_revision: global.revision,
            target_map_id: map_id,
            target_display_name: entry.display_name.clone(),
            diagnostic: None,
            stage: MapSwitchStage::Choosing,
        }); // 只冻结身份；实际保存或放弃须由对话框的明确选择启动。
        editor.status = "当前地图有未保存修改；请选择保存并切换、放弃修改并切换或取消".to_owned();
        return;
    }
    let display_name = entry.display_name.clone(); // 状态文字不借用后续可热更目录。
    editor.queue_map_transition(
        map_id,                                      // 显式进入按钮和地图双击把目录选择升级为切图目标。
        format!("正在准备进入地图“{display_name}”"), // 驱动系统继续异步等待清单。
    );
}

/// 资产目录或翻页改变时原地刷新十二个资产行。
pub(crate) fn sync_asset_rows(
    mut catalog: ResMut<EditorAssetCatalog>,
    editor: Res<MapEditorState>,
    settings: Option<Res<feathrise_editor_core::EditorSettings>>,
    asset_server: Option<Res<AssetServer>>,
    viewport: AssetViewport,
    mut pools: Query<&mut Node, (With<MapEditorAssetList>, Without<MapEditorAssetRow>, Without<MapEditorAssetRowGlyph>)>,
    mut rows: Query<(&mut MapEditorAssetRow, &mut Node, &Children, Option<&mut feathrise_editor_ui::EditorButtonClick>, Option<&mut AssetRowBinding>), (Without<MapEditorAssetRowGlyph>, Without<MapEditorAssetList>)>,
    mut binding: Local<Option<(u64, usize)>>,
    mut previous_geometry: Local<Option<(bool, usize)>>,
    mut texts: Query<(&mut Text, &mut TextColor), With<MapEditorAssetRowText>>,
    mut glyphs: Query<(&mut ImageNode, &mut Node), (With<MapEditorAssetRowGlyph>, Without<MapEditorAssetRow>, Without<MapEditorAssetList>)>,
) {
    if !editor.open { return; }
    let thumbnails = settings.as_ref().is_some_and(|settings| {
        settings.asset_browser_view_mode == feathrise_editor_core::AssetBrowserViewMode::Thumbnails
    });
    let geometry = viewport.geometry(catalog.entries.len());
    let previous = *previous_geometry;
    if previous.is_some() && previous != Some((thumbnails, geometry.columns))
        && binding.as_ref().is_some_and(|(revision, _)| *revision == catalog.applied_revision)
        && !catalog.entries.is_empty() {
        let anchor = catalog.visible_start;
        catalog.request_scroll_anchor(anchor); // 新网格按原可见文件重新定位，不用旧像素偏移跳到另一文件。
    }
    *previous_geometry = Some((thumbnails, geometry.columns));
    let start = geometry.start;
    let count = geometry.capacity.min(rows.iter().count()).min(catalog.entries.len().saturating_sub(start));
    if catalog.visible_start != start || catalog.visible_count != count {
        let moved = catalog.visible_start != start;
        catalog.visible_start = start; catalog.visible_count = count;
        if moved { catalog.viewport_revision = catalog.viewport_revision.wrapping_add(1); }
        // 池容量随布局补齐不代表用户改变导航，不能取消正在采用的合法资源定位。
    }
    for mut pool in &mut pools {
        let height = crate::ui::continuous_list::content_height(catalog.entries.len(), geometry.columns, geometry.row_height, geometry.gap);
        let display = if catalog.entries.is_empty() { Display::None } else { Display::Flex };
        if pool.height != px(height) { pool.height = px(height); }
        if pool.display != display { pool.display = display; }
    }
    let identity = (catalog.applied_revision, start);
    let directory_changed = *binding != Some(identity);
    *binding = Some(identity);
    for (mut row, mut node, children, click, stamp) in &mut rows {
        let entry_index = start + row.slot;
        let wanted = (row.slot < count).then(|| catalog.entries.get(entry_index).map(|_| entry_index)).flatten();
        // 固定行换目录、换页或换绑时，原按钮按住期间的松开不能改派给新文件。
        if (directory_changed || row.entry_index != wanted)
            && let Some(mut click) = click
        {
            click.invalidate();
        }
        if let Some(mut stamp) = stamp {
            let current = AssetRowBinding { revision: catalog.applied_revision, viewport_revision: catalog.viewport_revision, index: wanted };
            if stamp.revision != current.revision || stamp.viewport_revision != current.viewport_revision || stamp.index != current.index { *stamp = current; }
        }
        let Some(entry) = wanted.and_then(|index| catalog.entries.get(index)) else {
            if row.entry_index.is_some() {
                row.entry_index = None;
            }
            if node.display != Display::None {
                node.display = Display::None;
            }
            continue;
        };
        if row.entry_index != Some(entry_index) {
            row.entry_index = Some(entry_index);
        }
        if node.display != Display::Flex {
            node.display = Display::Flex;
        }
        let top = (entry_index / geometry.columns) as f32 * (geometry.row_height + geometry.gap);
        let left = if thumbnails { px((entry_index % geometry.columns) as f32 * (geometry.width + geometry.gap)) } else { px(0) };
        if node.position_type != PositionType::Absolute { node.position_type = PositionType::Absolute; }
        if node.top != px(top) { node.top = px(top); }
        if node.left != left { node.left = left; }
        let (icon, color) = if entry.is_directory {
            ("tree_folder", colors::TEXT_PRIMARY)
        } else {
            match entry.kind {
                MapAssetKind::Scene => ("tree_mesh", colors::TEXT_PRIMARY),
                MapAssetKind::Texture => ("tree_image", colors::TEXT_PRIMARY),
                MapAssetKind::Data => ("tree_file", colors::TEXT_PRIMARY),
                MapAssetKind::Unsupported => ("tree_file", colors::TEXT_MUTED),
            }
        };
        let selected = editor.selected_asset.as_deref() == Some(entry.asset_path.as_str());
        for child in children.iter() {
            if let Ok((mut image, mut icon_node)) = glyphs.get_mut(child) {
                let display = if thumbnails { Display::None } else { Display::Flex };
                if icon_node.display != display { icon_node.display = display; }
                if let Some(server) = asset_server.as_ref() {
                    let handle = server.load(format!("icons/editor/{icon}.png"));
                    if image.image != handle { image.image = handle; }
                    if image.color != color { image.color = color; }
                }
            }
            if let Ok((mut text, mut text_color)) = texts.get_mut(child) {
                let label = entry.display_name.clone(); // 类别由独立图标表达，列表和缩略图都只显示真实文件名。
                if text.as_str() != label {
                    **text = label;
                }
                text_color.set_if_neq(TextColor(if selected {
                    colors::TEXT_ACCENT
                } else {
                    color
                }));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use feathrise_map_content::{MAP_SCHEMA_VERSION, MapCatalogEntry, MapId}; // 使用正式目录数据结构锁定按钮语义。

    use super::*; // 直接测试无IO的显式进入保护门。

    fn catalog_with(current: &MapId, target: &MapId) -> MapCatalogAsset {
        let entry = |id: &MapId, name: &str| MapCatalogEntry {
            id: id.clone(),                // 稳定身份与按钮选择完全一致。
            display_name: name.to_owned(), // 状态栏显示作者可读名称。
            folder: format!("maps/{id}"),  // 模拟编辑器私有地图目录。
            published: true,               // 两张图都可进入正式玩法场景。
            surface_path: None,            // 本测试不加载内容资产。
            vegetation_path: None,         // 本测试不加载内容资产。
            water_path: None,              // 本测试不加载内容资产。
            characters_path: None,         // 旧目录保持角色来源缺席。
            boundary_path: None,           // 本测试不加载内容资产。
        };
        MapCatalogAsset {
            schema_version: MAP_SCHEMA_VERSION, // 使用当前公共格式版本。
            maps: vec![entry(current, "当前地图"), entry(target, "目标地图")],
        }
    }

    #[test]
    fn explicit_enter_queues_the_highlighted_map_without_changing_the_current_identity() {
        let current = MapId::parse("HN-01").expect("测试当前MapId合法");
        let target = MapId::parse("MAP-target").expect("测试目标MapId合法");
        let catalog = catalog_with(&current, &target);
        let mut editor = MapEditorState {
            selected_map_id: Some(current.clone()), // 当前世界和作者草稿仍属于HN-01。
            map_list_selection_id: Some(target.clone()), // 单击目录行只产生这个高亮选择。
            ..default()
        };

        request_selected_map_entry(
            &mut editor,
            &MapDeletionTask::default(),
            Some(&catalog),
            &EditorAuthoringSession::default(),
            &GlobalEnvironmentAuthoringState::default(),
            false,
        );

        assert_eq!(editor.selected_map_id.as_ref(), Some(&current)); // 按钮帧仍不能提前冒充已经进入目标图。
        assert_eq!(editor.map_list_selection_id.as_ref(), Some(&target)); // 高亮目标保持可见。
        assert_eq!(editor.pending_map_transition.as_ref(), Some(&target)); // 只有显式按钮产生正式进入请求。
    }

    #[test]
    fn dirty_current_map_opens_confirmation_and_keeps_the_directory_selection() {
        let current = MapId::parse("HN-01").expect("测试当前MapId合法");
        let target = MapId::parse("MAP-target").expect("测试目标MapId合法");
        let catalog = catalog_with(&current, &target);
        let mut editor = MapEditorState {
            selected_map_id: Some(current),              // 当前作者草稿身份稳定。
            map_list_selection_id: Some(target.clone()), // 作者已经选中另一张目录项。
            dirty: true,                                 // 模拟尚未保存的当前地图修改。
            ..default()
        };

        request_selected_map_entry(
            &mut editor,
            &MapDeletionTask::default(),
            Some(&catalog),
            &EditorAuthoringSession::default(),
            &GlobalEnvironmentAuthoringState::default(),
            false,
        );

        assert!(editor.pending_map_transition.is_none()); // 未保存内容不能被切图丢弃。
        assert_eq!(editor.map_list_selection_id.as_ref(), Some(&target)); // 作者可保存后直接再次点击进入，不必重选。
        assert!(editor.status.contains("未保存")); // 状态明确说明阻塞原因。
        let confirmation = editor
            .map_switch_confirmation
            .as_ref()
            .expect("真实编辑需要明确处理修改");
        assert_eq!(confirmation.target_map_id, target);
        assert_eq!(confirmation.stage, MapSwitchStage::Choosing);
    }
}
