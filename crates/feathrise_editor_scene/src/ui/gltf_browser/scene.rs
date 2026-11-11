//! 已摆放GLB与场景显示预览在右栏上半区的静态布局；空容器不伪造基础网格行。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::metrics as ui_metrics;
use bevy::prelude::*; // BSN Scene与UI布局组件。
use bevy::text::FontSourceTemplate; // 静态按钮和状态使用项目中文字体。
use feathrise_scene_content::SceneTerrainKind; // GLB与稀疏Clipmap显示不同的地形作者来源。
use crate::ui::theme as colors; // 与右栏资产浏览器共享水墨铜边配色。

use super::{
    GltfBrowserPageDirection, SceneEditorGltfBrowserList, SceneEditorGltfBrowserPageButton,
    SceneEditorGltfBrowserPagination, SceneEditorGltfBrowserStatusText,
}; // 专用容器和分页组件不进入全局EditorAction。
use crate::ui::scene::FONT_PATH; // 复用主编辑器字体常量。
use crate::ui::terrain_shading::terrain_shading_controls; // 选中基础地形才展开网格着色设置。

#[derive(Component, Clone, Default)] // 场景自身地形单列显示，不伪造可删除的摆放实例。
pub(crate) struct SceneBaseGltfRow;
#[derive(Component, Clone, Default)] // 按钮文字独立于可点击根，避免把Text误当场景行。
pub(crate) struct SceneBaseTerrainLabel;
#[derive(Component, Clone, Default)] // 基础地形被选中时才显示网格属性。
pub(crate) struct SceneBaseTerrainDetails;
#[derive(Component, Clone, Default)] // 分页是实际导航信息，与送到底栏的加载/故障反馈分开保留。
pub(crate) struct GltfBrowserPageLabel;

/// 基础GLB保留真实来源，稀疏Clipmap显示高度块来源而不假装有GLB文件。
fn base_terrain_label(kind: SceneTerrainKind, glb_path: &str, height_path: Option<&str>) -> String {
    if kind == SceneTerrainKind::SparseClipmap {
        format!(
            "原场景基础网格 · Clipmap\n{}",
            height_path.unwrap_or("稀疏高度块")
        ) // 无作者文件时仍可见真实运行时地形。
    } else {
        format!(
            "原场景基础网格 · {}\n{}",
            super::helpers::asset_file_name(glb_path),
            glb_path,
        ) // GLB场景继续显示文件名与来源路径。
    }
}

pub(crate) fn sync_scene_base_gltf_row(
    session: Res<crate::authoring::EditorAuthoringSession>, // 场景清单是基础GLB路径的唯一来源。
    editor: Res<crate::resources::SceneEditorState>,          // 只显示当前已进入场景而非上一张草稿。
    shading: Res<crate::resources::TerrainShadingSession>,  // 场景行高亮与属性展开共用当前选择。
    mut rows: Query<
        (&UiInteraction, &mut Node, &mut BackgroundColor), // 只修改基础地形按钮的布局和底色。
        (With<SceneBaseGltfRow>, Without<SceneBaseTerrainDetails>), // 与下方属性根的可变Node查询静态互斥。
    >,
    mut labels: Query<&mut Text, With<SceneBaseTerrainLabel>>, // 路径文字只在切换场景时更新。
    mut details: Query<
        &mut Node,                                              // 只修改展开属性根的显示状态。
        (With<SceneBaseTerrainDetails>, Without<SceneBaseGltfRow>), // 不与上方地形按钮争用Node。
    >,
) {
    let draft = session
        .draft
        .as_ref()
        .filter(|draft| editor.open && editor.selected_scene_id.as_ref() == Some(&draft.scene_id));
    let available = draft.is_some_and(|draft| {
        draft.manifest.terrains.as_ref().map_or_else(
            || !draft.manifest.terrain.is_empty(), |objects| !objects.is_empty())
    }); // 显式空对象集合绝不回退到旧容器占位；预览展开也不是选择某个网格。
    for (interaction, mut node, mut background) in &mut rows {
        let display = if available {
            Display::Flex
        } else {
            Display::None
        }; // 草稿未加载或已切换场景时不占布局。
        if node.display != display {
            node.display = display;
        } // 稳定帧不重复改布局。
        let color = if shading.preview_expanded && available {
            colors::SELECT_BG // 当前地形被选中时与平滑/平直按钮使用同一高亮色。
        } else {
            match interaction {
                UiInteraction::Pressed => colors::BUTTON_DEV_SMALL_PRESSED,
                UiInteraction::Hovered => colors::BUTTON_DEV_SMALL_HOVERED,
                UiInteraction::None => colors::SLOT_BG,
            }
        };
        if background.0 != color {
            background.0 = color; // 只在交互或选择真实改变时更新UI。
        }
    }
    if let Some(draft) = draft.filter(|_| available) {
        let terrain = &draft.manifest.terrain;
        let label = if draft.manifest.terrains.is_some() {
            "场景显示 · 网格 / 碰撞 / 着色".to_owned() // 多网格场景共用显示入口；真实GLB来源在各网格行及表示参数中显示。
        } else {
            base_terrain_label(terrain.kind, &terrain.glb_path, terrain.height_path.as_deref())
        };
        let label = format!("{} {label}", if shading.preview_expanded { "▼" } else { "▶" });
        for mut text in &mut labels {
            if text.0 != label {
                text.0 = label.clone(); // 固定一个标签，避免稳定帧触发Text重建。
            } // 切换场景或来源变化才更新文字。
        }
    }
    for mut node in &mut details {
        let display = if available && shading.preview_expanded {
            Display::Flex
        } else {
            Display::None
        }; // 无场景、切换场景或未选中时彻底收起属性。
        if node.display != display {
            node.display = display; // 不改变摆放实例属性区或分页。
        }
    }
}

/// 构造一个不进入全局动作枚举的场景树分页按钮。
fn browser_page_button(label: &str, direction: GltfBrowserPageDirection) -> impl Scene {
    bsn! {
        InteractionButton
        SceneEditorGltfBrowserPageButton(direction)
        Node {
            height: px(26), // 分页与作者工具统一命中高度，不改变树行池的20px步长。
            flex_grow: 1.0,
            flex_basis: px(0),
            min_width: px(0),
            flex_shrink: 0.0,
            border: px(1), // 系统按可用、hover和按压状态更新铜边。
            border_radius: BorderRadius::all(px(3)), // 与现有编辑器按钮一致。
            align_items: AlignItems::Center, // 文本垂直居中。
            justify_content: JustifyContent::Center, // 文本水平居中。
            padding: UiRect::horizontal(px(5)), // 防止字体贴边。
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL) // 首帧同步前使用安全常态。
        BorderColor::all(colors::COPPER_BORDER_WEAK) // 首帧同步前使用弱铜边。
        Children [
            Text(label) @feathrise_editor_ui::localization::label_scene(label)
            TextFont {
                font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), // BSN通过AssetServer加载统一中文字体。
                font_size: px(ui_metrics::FONT_CONTROL), // 紧凑分页文字。
            }
            TextColor(colors::TEXT_PRIMARY) // 可用性主要由背景和边框表达。
        ]
    }
}

/// 场景显示入口紧邻网格列表，不再埋在对象管理与子场景列表下方。
pub(crate) fn scene_display_panel() -> impl Scene {
    bsn! { Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column,
        row_gap: px(3), flex_shrink: 0.0 } Children [

                InteractionButton
                SceneBaseGltfRow // 基础地形固定在已摆放实例上方，点击展开设置。
                Node {
                    display: Display::None, // 未加载场景前隐藏。
                    width: percent(100), // 跟随右栏宽度。
                    min_width: px(0),
                    flex_shrink: 0.0, // 长路径换行后不压缩。
                    padding: UiRect::axes(px(3), px(2)), // 显示选项是一行工具入口，不伪装成另一份场景对象。
                    border_radius: BorderRadius::all(px(3)),
                }
                BackgroundColor(colors::SLOT_BG) // 共用场景树暗底。
                BorderColor::all(colors::COPPER_BORDER_WEAK) // 区分基础来源与可操作实例。
                Children [
                    SceneBaseTerrainLabel // 子文字独立同步来源，点击仍由父按钮接收。
                    Text("") // 清单就绪后显示GLB文件或Clipmap高度块。
                    TextFont {
                        font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), // 共用中文字体。
                    font_size: px(ui_metrics::FONT_CONTROL), // 与场景树行保持紧凑密度。
                    }
                    TextColor(colors::TEXT_PRIMARY) // 基础地形作为正式场景内容显示。
                    TextLayout { linebreak: bevy::text::LineBreak::AnyCharacter }
                    Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
                ]
            --
            @terrain_shading_controls() // 基础地形被选中后展开网格设置，不占中央视口。
    ] }
}

/// 构造右栏上半区的已摆放实例与Blender网格浏览器。
pub(super) fn gltf_browser_panel() -> impl Scene {
    bsn! {
        Node {
            width: percent(100), // 与右栏完整宽度一致。
            min_width: px(0),
            flex_shrink: 0.0, // 下方资产列表自行使用剩余弹性空间。
            flex_direction: FlexDirection::Column, // 标题、列表、分页、状态纵向排列。
            row_gap: px(3), // 与主右栏分组密度一致。
            overflow: Overflow::clip(), // 超长路径和极低窗口不越出右栏。
        }
        Children [
            @crate::ui::scene_layers::panel()-- // 独立作者内容与摆放共用同一场景树。
            @crate::ui::scene_objects::panel()--
            @crate::ui::subscenes::panel()-- // 子场景引用属于正式场景树，加载期间也保留可选来源行。

                SceneEditorGltfBrowserList
                Node {
                    display: Display::None, // 零条目时固定八行池完全退出布局。
                    width: percent(100), // 固定行池占满右栏可用宽度。
                    min_width: px(0),
                    height: px(0), // 一项起按当前页真实行数增长，八项后固定。
                    flex_shrink: 0.0, // 动态高度不被状态摘要压缩。
                    flex_direction: FlexDirection::Column, // 行按树顺序纵向排列。
                    row_gap: px(2), // 与现有资产列表一致。
                    overflow: Overflow::clip(), // 固定池之外内容必须通过分页访问。
                }
            --

                SceneEditorGltfBrowserPagination
                Node {
                    display: Display::None, // 只有一页时纯分页条完全收起。
                    width: percent(100), // 分页条横跨浏览器。
                    height: px(26), // 固定按钮高度。
                    min_width: px(0),
                    flex_shrink: 0.0, // 状态文字增长时不压缩按钮。
                    justify_content: JustifyContent::Center, // 两个分页按钮居中。
                    column_gap: px(5), // 保持清晰点击间隔。
                }
                Children [
                    @browser_page_button("上一页", GltfBrowserPageDirection::Previous)--
                    GltfBrowserPageLabel Text("1/1")
                        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                        TextColor(colors::TEXT_MUTED)
                        Node { align_self: AlignSelf::Center } ~{bevy::ui::FocusPolicy::Pass}--
                    @browser_page_button("下一页", GltfBrowserPageDirection::Next)
                ]
            --

                SceneEditorGltfBrowserStatusText
                ~{crate::ui::status_bar::PanelStatusText("场景资产")} feathrise_editor_ui::feedback::GlobalPanelStatus
                Text("场景对象：等待场景草稿\n第 1/1 页") @feathrise_editor_ui::localization::label_scene("场景对象：等待场景草稿\n第 1/1 页")
                TextFont {
                    font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), // 与动态行共享字体资产。
                    font_size: px(ui_metrics::FONT_HINT), // 状态保持可读字号，不靠缩字容纳长路径。
                }
                TextColor(colors::TEXT_HINT) // 摘要弱于可点击树行。
                TextLayout { linebreak: bevy::text::LineBreak::AnyCharacter }
                Node {
                    display: Display::None, // 就绪且零摆放时不保留空分支；真实等待/错误由状态系统显式展开。
                    width: percent(100), // 长加载摘要在右栏内裁切。
                    min_width: px(0),
                    flex_shrink: 0.0, // 不被列表弹性挤掉。
                    overflow: Overflow::clip(), // 选择路径过长也不撑开布局。
                    padding: UiRect::all(px(3)), // 摘要高度只由真实文字与内边距决定。
                }

        ]
    }
}

#[cfg(test)] // 基础地形行不得再把无GLB的Clipmap误判为无场景。
mod tests {
    use super::*; // 直接验证右侧场景行实际采用的来源标签。

    #[test] // 真实App启动时两个可变Node查询必须互斥，不能只做纯标签测试。
    fn base_terrain_row_and_details_sync_without_query_conflict() {
        let mut draft = crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121])); // 使用合法小场景草稿。
        draft.manifest.terrain.kind = SceneTerrainKind::SparseClipmap; // 模拟无GLB的新图。
        draft.manifest.terrain.glb_path.clear(); // Clipmap没有虚构的基础GLB。
        draft.manifest.terrain.height_path =
            Some("scenes/EDGE-TEST/height.sparse_height.json".to_owned()); // 指向作者高度块。
        let scene_id = draft.scene_id.clone(); // 冻结当前正式场景身份。
        let mut app = App::new(); // 不需要窗口或GPU也能检验SystemParam初始化。
        app.init_resource::<crate::authoring::EditorAuthoringSession>()
            .init_resource::<crate::resources::SceneEditorState>()
            .init_resource::<crate::resources::TerrainShadingSession>()
            .add_systems(Update, sync_scene_base_gltf_row); // 执行引发B0001的真实生产系统。
        app.world_mut()
            .resource_mut::<crate::authoring::EditorAuthoringSession>()
            .draft = Some(draft); // 当前地形来源已加载。
        let editor = &mut app
            .world_mut()
            .resource_mut::<crate::resources::SceneEditorState>();
        editor.open = true;
        editor.selected_scene_id = Some(scene_id); // 编辑器正查看同一场景。
        app.world_mut()
            .resource_mut::<crate::resources::TerrainShadingSession>()
            .preview_expanded = true; // 基础地形属性已展开。
        let row = app
            .world_mut()
            .spawn((
                SceneBaseGltfRow,
                UiInteraction::None,
                Node::default(),
                BackgroundColor::default(),
            ))
            .id(); // 按钮根唯一持有Node。
        let label = app
            .world_mut()
            .spawn((SceneBaseTerrainLabel, Text::new("")))
            .id(); // 来源文字独立于按钮根。
        let details = app
            .world_mut()
            .spawn((SceneBaseTerrainDetails, Node::default()))
            .id(); // 属性根持有另一份Node。
        app.update(); // 旧版此处在实际系统初始化时抛出B0001。
        assert_eq!(app.world().get::<Node>(row).unwrap().display, Display::Flex); // 无GLB的Clipmap仍可点击。
        assert_eq!(
            app.world().get::<Node>(details).unwrap().display,
            Display::Flex
        ); // 点击后显示网格设置。
        assert!(
            app.world()
                .get::<Text>(label)
                .unwrap()
                .0
                .contains("Clipmap")
        ); // 标签不伪造GLB。
        {
            let mut session = app.world_mut().resource_mut::<crate::authoring::EditorAuthoringSession>();
            let draft = session.draft.as_mut().unwrap();
            let object = feathrise_scene_content::SceneTerrainObject {clipmap_stitches: Default::default(),
                mesh_lod: None,
                id: feathrise_scene_content::TerrainId::parse("mesh-a").unwrap(),
                display_name: "山体".into(), enabled: true, transform: Default::default(),
                world_min: draft.manifest.world_min, world_max: draft.manifest.world_max,
                terrain: draft.manifest.terrain.clone(), voxel: None, surface_path: None,
                vegetation_path: None, water_path: None, puddles: None,
            };
            draft.manifest.terrain = feathrise_scene_content::SceneTerrainDescriptor::empty();
            draft.manifest.terrains = Some(vec![object].into());
        }
        app.update();
        assert!(app.world().get::<Text>(label).unwrap().0.contains("场景显示 · 网格 / 碰撞 / 着色"));
        assert_eq!(app.world().get::<Node>(details).unwrap().display, Display::Flex);
        app.world_mut().resource_mut::<crate::authoring::EditorAuthoringSession>()
            .draft.as_mut().unwrap().manifest.terrains = Some(Vec::new().into());
        app.update();
        assert_eq!(app.world().get::<Node>(row).unwrap().display, Display::None);
        assert_eq!(app.world().get::<Node>(details).unwrap().display, Display::None);
        // 空容器没有假基础GLB或悬空网格属性，即使展开态仍保存也完全退出布局。
        app.world_mut()
            .resource_mut::<crate::resources::SceneEditorState>()
            .open = false; // 返回玩法时隐藏设置。
        app.update();
        assert_eq!(
            app.world().get::<Node>(details).unwrap().display,
            Display::None
        ); // 关闭后不留悬空属性。
    }

    #[test] // 两种场景路线都能显示真实基础地形，不伪造Clipmap GLB。
    fn base_terrain_row_labels_both_author_routes() {
        let glb = base_terrain_label(
            SceneTerrainKind::Heightfield,
            "scenes/MAP-a/terrain/terrain.glb",
            None,
        ); // 传统GLB作者源。
        assert!(glb.contains("terrain.glb")); // 文件名和来源路径仍可核对。
        let clipmap = base_terrain_label(
            SceneTerrainKind::SparseClipmap,
            "",
            Some("scenes/MAP-b/height.sparse_height.json"),
        ); // 新图不含GLB路径。
        assert!(clipmap.contains("Clipmap")); // 无GLB也必须显示真实旧场景来源。
        assert!(clipmap.contains("height.sparse_height.json")); // 仍指向真正作者源。
        assert!(!clipmap.contains("terrain.glb")); // 不虚构不可编辑的GLB资产。
    }
}
