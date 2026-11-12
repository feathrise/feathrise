//! 正式工作台“工程”单目录的实际GPU回读；原目录按钮、资产定位和文档IO都走生产入口。

use feathrise_editor_ui::UiInteraction;
use super::{components::*, types::*, ProjectBrowserState};
use crate::{
    asset_catalog::{EditorAssetCatalog, project_files::EditorProjectFileCatalog},
    components::{SceneEditorAssetRow, SceneEditorActionButton},
    resources::SceneEditorState,
    types::EditorAction,
    ui::asset_context_menu::{AssetContextMenuState, AssetMenuFavoriteButton},
    ui::sidebars::{EditorNavigatorTab, EditorSidebarTab, SceneEditorSidebarTabButton, SceneEditorSidebarPage},
    workbench::{WorkbenchModel, WorkbenchUiPlugin},
};
use bevy::{
    prelude::*,
    render::{gpu_readback::{Readback, ReadbackComplete},
        render_resource::{TextureFormat, TextureUsages}},
    text::EditableText,
};
use feathrise_editor_documents_model::components::{DocumentAction, DocumentCenterHost, DocumentPanel, DocumentSource, DocumentStatus};
use std::{fs::OpenOptions, path::Path, time::{Duration, Instant}};

#[derive(Resource, Default)]
struct NextPointer(Option<(Entity, bool)>);

#[derive(Resource, Default)]
struct NextRightPointer(Option<(Entity, bool)>);

#[derive(Resource, Default)]
struct MenuInputProbe(bool);

fn probe_menu_input(menu: Res<AssetContextMenuState>, mut probe: ResMut<MenuInputProbe>) {
    probe.0 = menu.owns_mouse();
}

#[derive(Resource)]
struct ChineseFont(Handle<Font>);

// 显式测试在官方焦点投影之后提供按下/释放，原EditorButtonInput仍负责捕获、可见性和合法释放。
fn pointer(
    mut pending: ResMut<NextPointer>,
    mut right: ResMut<NextRightPointer>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut buttons: Query<(&mut UiInteraction, &mut bevy::ui::RelativeCursorPosition)>,
) {
    let (entity, down, button) = if let Some((entity, down)) = pending.0.take() {
        (entity, down, MouseButton::Left)
    } else if let Some((entity, down)) = right.0.take() {
        (entity, down, MouseButton::Right)
    } else { return; };
    let (mut interaction, mut cursor) = buttons.get_mut(entity).unwrap();
    cursor.cursor_over = true;
    *interaction = if down && button == MouseButton::Left { UiInteraction::Pressed } else { UiInteraction::Hovered };
    if down { mouse.press(button); } else { mouse.release(button); }
}

fn frame(app: &mut App) {
    app.update();
    let font = app.world().resource::<ChineseFont>().0.clone();
    let world = app.world_mut();
    for mut text in world.query::<&mut TextFont>().iter_mut(world) {
        if text.font != font.clone().into() { text.font = font.clone().into(); }
    }
    std::thread::sleep(Duration::from_millis(8)); // 只在测试驱动等待真实后台与GPU，不安装生产等待系统。
}

fn wait(app: &mut App, ready: impl Fn(&mut World) -> bool) {
    let start = Instant::now();
    loop {
        frame(app);
        if ready(app.world_mut()) { return; }
        if start.elapsed() >= Duration::from_secs(20) {
            let world = app.world_mut();
            let statuses = world.query_filtered::<&Text, With<DocumentStatus>>().iter(world)
                .map(|text| text.0.clone()).collect::<Vec<_>>();
            panic!("工程浏览GPU状态未在期限内完成：{}；文档：{statuses:?}", world.resource::<SceneEditorState>().status);
        }
    }
}

fn click(app: &mut App, entity: Entity) {
    assert!(!app.world().get::<ComputedNode>(entity).unwrap().is_empty(), "只点击真实布局中的控件");
    app.world_mut().resource_mut::<NextPointer>().0 = Some((entity, true));
    frame(app);
    assert!(!app.world().get::<feathrise_editor_ui::EditorButtonClick>(entity).unwrap().is_clicked(),
        "按下不能冒充普通按钮动作");
    app.world_mut().resource_mut::<NextPointer>().0 = Some((entity, false));
    frame(app);
    frame(app);
}

fn right_click(app: &mut App, entity: Entity) -> Option<Entity> {
    assert!(!app.world().get::<ComputedNode>(entity).unwrap().is_empty(), "只右击真实布局中的工程行");
    let primary = {
        let world = app.world_mut();
        world.query_filtered::<Entity, With<bevy::window::PrimaryWindow>>().iter(world).next()
    };
    let created = primary.is_none().then(|| app.world_mut().spawn((bevy::window::PrimaryWindow,
        Window { focused: true, ..default() })).id()); // 离屏工作台原本无Winit窗口；菜单仍需原生坐标来源。
    for _ in 0..4 { frame(app); } // 新窗口进入焦点和布局系统后，再读取真实工程行中心。
    let center = app.world().get::<UiGlobalTransform>(entity).unwrap().translation;
    {
        let world = app.world_mut();
        let mut window = world.get_mut::<Window>(primary.or(created).unwrap()).unwrap();
        window.focused = true;
        window.set_physical_cursor_position(Some(bevy::math::DVec2::new(center.x as f64, center.y as f64)));
    }
    app.world_mut().resource_mut::<NextRightPointer>().0 = Some((entity, true));
    frame(app);
    assert!(app.world().resource::<AssetContextMenuState>().owns_mouse(), "工程行右键必须由正式路径菜单取得鼠标；input={}，view={:?}，open={}，active={}，loading={}，viewport={}，scroll={:?}，row={:?}，rowrev={:?}，interaction={:?}，guard={}，cursor={:?}",
        app.world().resource::<MenuInputProbe>().0,
        app.world().resource::<ProjectBrowserState>().view, app.world().resource::<SceneEditorState>().open,
        app.world().resource::<EditorProjectFileCatalog>().active, app.world().resource::<EditorProjectFileCatalog>().is_loading(),
        app.world().resource::<super::ProjectFileViewport>().revision,
        app.world().resource::<super::ProjectFileViewport>().scroll,
        app.world().get::<ProjectFileRow>(entity).unwrap().entry_index,
        app.world().get::<ProjectFileRow>(entity).unwrap().viewport_revision,
        app.world().get::<UiInteraction>(entity).unwrap(),
        app.world().resource::<crate::resources::SceneEditorNumericScrubMouseGuard>().blocked(),
        app.world().get::<Window>(primary.or(created).unwrap()).unwrap().physical_cursor_position());
    app.world_mut().resource_mut::<NextRightPointer>().0 = Some((entity, false));
    frame(app);
    frame(app);
    created
}

fn project_action(app: &mut App, wanted: ProjectFileAction) -> Entity {
    let world = app.world_mut();
    world.query::<(Entity, &ProjectFileActionButton)>().iter(world)
        .find_map(|(entity, button)| (button.0 == wanted).then_some(entity)).unwrap()
}

fn asset_action(app: &mut App, wanted: EditorAction) -> Entity {
    let world = app.world_mut();
    world.query::<(Entity, &SceneEditorActionButton, &ComputedNode)>().iter(world)
        .find_map(|(entity, button, node)| (button.0 == wanted && !node.is_empty()).then_some(entity)).unwrap()
}

fn no_scope_switches(world: &mut World) {
    let page = world.query::<(Entity, &SceneEditorSidebarPage)>().iter(world)
        .find_map(|(entity, page)| (page.0 == EditorSidebarTab::Navigator(EditorNavigatorTab::Assets)).then_some(entity)).unwrap();
    for (entity, text) in world.query::<(Entity, &Text)>().iter(world) {
        if !matches!(text.0.as_str(), "资产" | "工程文件" | "上一页" | "下一页" | "打开文本" | "外部IDE") { continue; }
        let mut ancestor = entity;
        for _ in 0..32 {
            assert_ne!(ancestor, page, "工程目录不应保留分类或分页按钮：{}", text.0);
            let Some(parent) = world.get::<ChildOf>(ancestor) else { break; };
            ancestor = parent.parent();
        }
    }
}

/// 只改变正式唯一滚动宿主的位置；不写目录、行绑定或选择来冒充已浏览深处条目。
fn scroll_content(app: &mut App, view: ProjectBrowserView, fraction: f32) {
    let world = app.world_mut();
    let (entity, maximum) = world.query::<(Entity, &ProjectBrowserContent, &ComputedNode)>().iter(world)
        .find_map(|(entity, content, node)| (content.0 == view).then_some((entity,
            (node.content_size().y - node.size().y).max(0.) * node.inverse_scale_factor()))).unwrap();
    world.get_mut::<ScrollPosition>(entity).unwrap().y = maximum * fraction;
    for _ in 0..8 { frame(app); }
}

fn assert_row_visible(world: &World, entity: Entity, view: ProjectBrowserView) {
    let mut parent = entity;
    let viewport = loop {
        if world.get::<ProjectBrowserContent>(parent).is_some_and(|content| content.0 == view) { break parent; }
        parent = world.get::<ChildOf>(parent).expect("目录行必须属于原浏览正文").parent();
    };
    let bounds = |entity| {
        let node = world.get::<ComputedNode>(entity).unwrap();
        Rect::from_center_size(world.get::<UiGlobalTransform>(entity).unwrap().translation, node.size())
    };
    let item = bounds(entity);
    let clip = bounds(viewport);
    assert!(item.min.y >= clip.min.y - 1. && item.max.y <= clip.max.y + 1.,
        "深处目标必须实际位于可见区，不能点击屏幕外的槽：{item:?}/{clip:?}");
}

/// 中段没有目录表单遮挡；实际可见行必须覆盖整段视口，不能只留下旧十二槽的短列表。
fn assert_window_covered(world: &mut World, view: ProjectBrowserView) {
    let viewport = world.query::<(&ProjectBrowserContent, &ComputedNode, &UiGlobalTransform)>().iter(world)
        .find_map(|(content, node, transform)| (content.0 == view)
            .then_some(Rect::from_center_size(transform.translation, node.size()))).unwrap();
    let rows = world.query::<(Option<&ProjectFileRow>, Option<&SceneEditorAssetRow>, &Node, &ComputedNode, &UiGlobalTransform)>().iter(world)
        .filter(|(file, asset, node, ..)| node.display != Display::None && match view {
            ProjectBrowserView::ProjectFiles => file.is_some(),
            ProjectBrowserView::Assets => asset.is_some(),
        })
        .map(|(_, _, _, node, transform)| Rect::from_center_size(transform.translation, node.size()))
        .filter(|item| item.max.y > viewport.min.y && item.min.y < viewport.max.y).collect::<Vec<_>>();
    assert!(!rows.is_empty(), "目录中段应有真实行，不能只剩完整高度占位");
    let first = rows.iter().map(|row| row.min.y).min_by(f32::total_cmp).unwrap();
    let last = rows.iter().map(|row| row.max.y).max_by(f32::total_cmp).unwrap();
    assert!(first <= viewport.min.y + 6. && last >= viewport.max.y - 6., // 原缩略图行距5px，另留1px字形/布局取整。
        "可见池必须覆盖中段整个视口：{first}..{last} / {viewport:?}");
}

fn file_row(app: &mut App, path: &str) -> Entity {
    let world = app.world_mut();
    let index = world.resource::<EditorProjectFileCatalog>().entries.iter()
        .position(|entry| entry.asset_path == path).unwrap();
    world.query::<(Entity, &ProjectFileRow)>().iter(world)
        .find_map(|(entity, row)| (row.entry_index == Some(index)).then_some(entity)).unwrap()
}

fn asset_row(app: &mut App, path: &str) -> Entity {
    let world = app.world_mut();
    let (index, catalog_state) = {
        let catalog = world.resource::<EditorAssetCatalog>();
        let index = catalog.entries.iter().position(|entry| entry.asset_path == path)
            .unwrap_or_else(|| panic!("目标资产不在已采用目录：path={path}，folder={}，revision={}/{}，entries={}",
                catalog.current_folder, catalog.applied_revision, catalog.requested_revision, catalog.entries.len()));
        (index, format!("folder={}，revision={}/{}，window={}+{}，anchor={:?}", catalog.current_folder,
            catalog.applied_revision, catalog.requested_revision, catalog.visible_start, catalog.visible_count, catalog.scroll_anchor))
    };
    let found = world.query::<(Entity, &SceneEditorAssetRow)>().iter(world)
        .find_map(|(entity, row)| (row.entry_index == Some(index)).then_some(entity));
    found.unwrap_or_else(|| {
        // 失败时保留真实目录、行池和宿主布局，区分定位未采用与已采用却未滚入正文。
        let bound = world.query::<&SceneEditorAssetRow>().iter(world)
            .map(|row| (row.slot, row.entry_index)).collect::<Vec<_>>();
        let scrolls = world.query::<(&ProjectBrowserContent, &ScrollPosition, &ComputedNode)>().iter(world)
            .map(|(content, scroll, node)| (content.0, scroll.0, node.size(), node.content_size())).collect::<Vec<_>>();
        panic!("目标资产未进入可见行池：path={path}，index={index}，{catalog_state}；bound={bound:?}；scrolls={scrolls:?}");
    })
}

fn assets_ready(world: &mut World) -> bool {
    let catalog = world.resource::<EditorAssetCatalog>();
    catalog.requested_revision == catalog.applied_revision
        && catalog.feedback.as_ref().is_some_and(|feedback| feedback.result.is_ok())
}

fn files_ready(world: &mut World) -> bool {
    let catalog = world.resource::<EditorProjectFileCatalog>();
    !catalog.is_loading() && catalog.feedback.as_ref().is_some_and(|feedback| feedback.result.is_ok())
}

fn snapshot(app: &mut App, target: &Handle<Image>, path: &Path) {
    for _ in 0..8 { frame(app); }
    let (send, receive) = std::sync::mpsc::channel();
    app.world_mut().spawn(Readback::texture(target.clone())).observe(
        move |event: On<ReadbackComplete>, mut commands: Commands| {
            let _ = send.send(event.data.clone());
            commands.entity(event.entity).try_despawn();
        });
    let start = Instant::now();
    let bytes = loop {
        frame(app);
        if let Ok(bytes) = receive.try_recv() { break bytes; }
        assert!(start.elapsed() < Duration::from_secs(15), "工程浏览截图回读超时");
    };
    let size = app.world().resource::<Assets<Image>>().get(target).unwrap().size();
    let stride = size.x as usize * 4;
    let padded = stride.div_ceil(256) * 256;
    let pixels = bytes.chunks_exact(padded).take(size.y as usize)
        .flat_map(|row| row[..stride].iter().copied()).collect::<Vec<_>>();
    let image = Image::new(bevy::render::render_resource::Extent3d {
        width: size.x, height: size.y, depth_or_array_layers: 1,
    }, bevy::render::render_resource::TextureDimension::D2, pixels,
        TextureFormat::Rgba8UnormSrgb, default());
    let mut file = OpenOptions::new().write(true).create_new(true).open(path).unwrap();
    image.try_into_dynamic().unwrap().write_to(&mut file,
        bevy::image::ImageFormat::Png.as_image_crate_format().unwrap()).unwrap();
}

fn glyphs(world: &mut World, label: &str) {
    let visible = world.query::<(&Text, &bevy::text::TextLayoutInfo, &ComputedNode)>().iter(world)
        .any(|(text, layout, node)| text.0 == label && !layout.glyphs.is_empty() && !node.is_empty());
    assert!(visible, "正式中文标签必须完成实际字形布局：{label}");
}

#[test]
#[ignore = "显式GPU验收；FEATHRISE_PROJECT_BROWSER_GPU_REPORT必须为新的独占绝对目录"]
fn production_project_browser_gpu_navigates_project_assets_and_opens_source() {
    let output = std::path::PathBuf::from(std::env::var_os("FEATHRISE_PROJECT_BROWSER_GPU_REPORT")
        .expect("必须指定独占FEATHRISE_PROJECT_BROWSER_GPU_REPORT目录"));
    assert!(output.is_absolute());
    std::fs::create_dir_all(&output).unwrap();
    OpenOptions::new().write(true).create_new(true).open(output.join("owner")).unwrap();
    let root = output.join("工程夹具");
    std::fs::create_dir(&root).unwrap();
    for path in ["assets", "assets/models", "src", "crates", "target", ".trash"] {
        std::fs::create_dir(root.join(path)).unwrap();
    }
    std::fs::write(root.join(".git"), "gitdir: deliberately-hidden-fixture").unwrap();
    std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"browser_fixture\"\nversion = \"0.1.0\"\n").unwrap();
    std::fs::write(root.join("assets/models/保留选择.bin"), [1, 2, 3, 4]).unwrap();
    let source = "// 工程浏览验证\nfn main() { println!(\"代码预览\"); }\n";
    std::fs::write(root.join("src/main.rs"), source).unwrap();
    // 两个目录都超过固定可见行池容量；末项只能靠真实滚动换绑进入可见区。
    for index in 0..192 {
        std::fs::write(root.join(format!("src/unit_{index:03}.rs")), "// 连续目录夹具\n").unwrap();
        std::fs::write(root.join(format!("assets/models/asset_{index:03}.bin")), [index as u8]).unwrap();
    }
    std::fs::write(root.join("src/zzz-last.rs"), source).unwrap();

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.build()
        .set(WindowPlugin { primary_window: None, exit_condition: bevy::window::ExitCondition::DontExit, ..default() })
        .set(AssetPlugin { file_path: feathrise_editor_core::editor_asset_root().to_string_lossy().into_owned(),
            watch_for_changes_override: Some(false), ..default() })
        .disable::<bevy::winit::WinitPlugin>()
        .disable::<bevy::audio::AudioPlugin>()
        .disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>()
        .disable::<bevy::log::LogPlugin>())
        .insert_resource(feathrise_editor_animation::scenes())
        .add_plugins((feathrise_editor_ui::EditorUiPlugin,
            feathrise_editor_documents::DocumentsEditorPlugin,
            feathrise_editor_weights::WeightPanelPlugin, WorkbenchUiPlugin))
        .init_resource::<feathrise_editor_core::EditorSettings>()
        .init_resource::<crate::resources::EditorTextInputGuard>()
        .init_resource::<crate::resources::SceneBrowserDragState>()
        .init_resource::<crate::authoring::TerrainExternalChangeState>()
        .init_resource::<feathrise_editor_ui::ActiveNumericInputScrub>()
        .init_resource::<crate::ui::asset_preview::types::AssetPreviewState>()
        .init_resource::<NextPointer>()
        .init_resource::<NextRightPointer>()
        .init_resource::<MenuInputProbe>()
        .insert_resource(WorkbenchModel { context: 71, asset_root: Some(root.join("assets")),
            project_root: Some(root.clone()), ..default() })
        .add_systems(PreUpdate, pointer.after(bevy::ui::UiSystems::Focus)
            .before(feathrise_editor_ui::update_editor_button_clicks));
    crate::ui::asset_context_menu::install(&mut app); // 夹具显式安装生产右键系统；普通工作台插件只装基础浏览器。
    app.add_systems(Update, probe_menu_input.after(crate::ui::asset_context_menu::AssetMenuInput)
        .before(crate::SceneEditorSystemSet::Tasks));
    crate::ui::asset_preview::install_project_browser_test_documents(&mut app);
    crate::ui::asset_thumbnails::install_presentation_test(&mut app); // 核对原缩略图布局；不声称执行bin资产的GPU烘焙。
    let started = Instant::now();
    while app.plugins_state() != bevy::app::PluginsState::Ready {
        bevy::tasks::tick_global_task_pools_on_main_thread();
        assert!(started.elapsed() < Duration::from_secs(30));
        std::thread::sleep(Duration::from_millis(10));
    }
    app.finish(); app.cleanup();
    let font = app.world_mut().resource_mut::<Assets<Font>>().add(Font::from_bytes(feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES.to_vec()));
    app.insert_resource(ChineseFont(font));
    let target = app.world_mut().resource_mut::<Assets<Image>>().add({
        let mut image = Image::new_target_texture(1440, 900, TextureFormat::Rgba8UnormSrgb, None);
        image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        image
    });
    app.world_mut().spawn((Camera2d, bevy::ui::IsDefaultUiCamera, Camera::default(),
        bevy::camera::RenderTarget::from(target.clone())));
    wait(&mut app, |world| world.query::<(&SceneEditorSidebarTabButton, &ComputedNode)>().iter(world)
        .any(|(button, node)| button.0 == EditorSidebarTab::Navigator(EditorNavigatorTab::Assets) && !node.is_empty()));
    let navigator = {
        let world = app.world_mut();
        world.query::<(Entity, &SceneEditorSidebarTabButton)>().iter(world)
            .find_map(|(entity, button)| (button.0 == EditorSidebarTab::Navigator(EditorNavigatorTab::Assets)).then_some(entity)).unwrap()
    };
    click(&mut app, navigator);
    wait(&mut app, files_ready);
    assert_eq!(app.world().resource::<ProjectBrowserState>().view, ProjectBrowserView::ProjectFiles);
    assert!(app.world().resource::<EditorProjectFileCatalog>().current_folder.is_empty());
    glyphs(app.world_mut(), "工程");
    no_scope_switches(app.world_mut());
    let paths = app.world().resource::<EditorProjectFileCatalog>().entries.iter()
        .map(|entry| entry.asset_path.as_str()).collect::<Vec<_>>();
    assert!(paths.contains(&"src") && paths.contains(&"Cargo.toml") && paths.contains(&"assets"));
    assert!(!paths.contains(&"target") && !paths.contains(&".git") && !paths.contains(&".trash"));
    snapshot(&mut app, &target, &output.join("01-project-root.png"));
    let cargo = file_row(&mut app, "Cargo.toml");
    assert_row_visible(app.world(), cargo, ProjectBrowserView::ProjectFiles);
    let mock_window = right_click(&mut app, cargo);
    let favorite_display = {
        let world = app.world_mut();
        world.query_filtered::<&Node, With<AssetMenuFavoriteButton>>().iter(world)
            .map(|node| node.display).collect::<Vec<_>>()
    };
    assert_eq!(favorite_display, vec![Display::None], "工程文件菜单不应显示资产专属收藏入口");
    snapshot(&mut app, &target, &output.join("01-project-root-menu.png"));
    app.world_mut().resource_mut::<NextRightPointer>().0 = Some((cargo, true));
    frame(&mut app);
    app.world_mut().resource_mut::<NextRightPointer>().0 = Some((cargo, false));
    frame(&mut app);
    frame(&mut app);
    assert!(!app.world().resource::<AssetContextMenuState>().owns_mouse());
    if let Some(window) = mock_window { app.world_mut().despawn(window); }
    let src = file_row(&mut app, "src");
    click(&mut app, src);
    wait(&mut app, |world| files_ready(world) && world.resource::<EditorProjectFileCatalog>().current_folder == "src");
    let file_pool = {
        let world = app.world_mut();
        world.query_filtered::<Entity, With<ProjectFileRow>>().iter(world).count()
    };
    assert!(file_pool < 194, "工程行池不能按完整文件数量实例化");
    let first_file = file_row(&mut app, "src/main.rs");
    assert_row_visible(app.world(), first_file, ProjectBrowserView::ProjectFiles);
    app.world_mut().resource_mut::<NextPointer>().0 = Some((first_file, true));
    frame(&mut app);
    scroll_content(&mut app, ProjectBrowserView::ProjectFiles, 0.5);
    app.world_mut().resource_mut::<NextPointer>().0 = Some((first_file, false));
    frame(&mut app); frame(&mut app);
    assert!(app.world().resource::<EditorProjectFileCatalog>().selected_file.is_none(),
        "真实按下后滚动换绑，同一槽释放不得转为另一个文件的点击");
    assert_window_covered(app.world_mut(), ProjectBrowserView::ProjectFiles);
    scroll_content(&mut app, ProjectBrowserView::ProjectFiles, 1.);
    let main = file_row(&mut app, "src/zzz-last.rs");
    assert_row_visible(app.world(), main, ProjectBrowserView::ProjectFiles);
    click(&mut app, main);
    assert_eq!(app.world().resource::<EditorProjectFileCatalog>().selected_file.as_ref().unwrap().relative_path, "src/zzz-last.rs");
    no_scope_switches(app.world_mut());
    snapshot(&mut app, &target, &output.join("02-project-last-file.png"));
    click(&mut app, main);
    click(&mut app, main); // 移除重复动作按钮后，仍经原行双击进入唯一中央文档会话。
    // 原打开行为先显示只读代码预览；必须通过真实“编辑”按钮，才能核对唯一原生输入。
    wait(&mut app, |world| world.query_filtered::<&Text, With<DocumentStatus>>().iter(world)
        .any(|text| text.contains("UTF-8")));
    let edit = {
        let world = app.world_mut();
        world.query::<(Entity, &DocumentAction)>().iter(world)
            .find_map(|(entity, action)| (*action == DocumentAction::Edit).then_some(entity)).unwrap()
    };
    click(&mut app, edit);
    wait(&mut app, |world| world.query_filtered::<&EditableText, With<DocumentSource>>().iter(world)
        .any(|text| text.value().to_string() == source));
    let central = {
        let world = app.world_mut();
        world.query_filtered::<Entity, With<DocumentCenterHost>>().single(world).unwrap()
    };
    let panel = {
        let world = app.world_mut();
        world.query_filtered::<Entity, With<DocumentPanel>>().single(world).unwrap()
    };
    assert_eq!(app.world().get::<ChildOf>(panel).unwrap().parent(), central,
        "源码必须沿真实消息、IO和原文档工厂进入中央，不能只改变按钮状态");
    assert_ne!(app.world().get::<Node>(central).unwrap().display, Display::None);
    assert!(app.world().get::<ComputedNode>(central).unwrap().size().y > 600.,
        "中央代码文档应占用完整工作区，不能与空场景提示各分半屏");
    snapshot(&mut app, &target, &output.join("02-project-source.png"));

    // 单一目录路径：源码上级回工程根，再真实进入已登记资产目录，不能点击旧视图或写scope。
    scroll_content(&mut app, ProjectBrowserView::ProjectFiles, 0.);
    let up = project_action(&mut app, ProjectFileAction::Up);
    click(&mut app, up);
    wait(&mut app, |world| files_ready(world) && world.resource::<EditorProjectFileCatalog>().current_folder.is_empty());
    let assets = file_row(&mut app, "assets");
    click(&mut app, assets);
    wait(&mut app, |world| assets_ready(world)
        && world.resource::<ProjectBrowserState>().assets_visible()
        && world.resource::<EditorAssetCatalog>().current_folder.is_empty());
    let _ = asset_action(&mut app, EditorAction::RefreshAssets);
    let models = asset_row(&mut app, "models");
    click(&mut app, models);
    wait(&mut app, |world| assets_ready(world) && world.resource::<EditorAssetCatalog>().current_folder == "models");
    let asset_pool = {
        let world = app.world_mut();
        world.query_filtered::<Entity, With<SceneEditorAssetRow>>().iter(world).count()
    };
    assert!(asset_pool < 193, "资产行池不能按完整文件数量实例化");
    scroll_content(&mut app, ProjectBrowserView::Assets, 0.5);
    assert_window_covered(app.world_mut(), ProjectBrowserView::Assets);
    scroll_content(&mut app, ProjectBrowserView::Assets, 1.);
    let data = asset_row(&mut app, "models/保留选择.bin");
    assert_row_visible(app.world(), data, ProjectBrowserView::Assets);
    click(&mut app, data);
    let selected = Some("models/保留选择.bin".to_owned());
    assert_eq!(app.world().resource::<SceneEditorState>().selected_asset, selected);
    no_scope_switches(app.world_mut());
    wait(&mut app, |world| world.query_filtered::<&Text, With<crate::components::SceneEditorAssetFolderText>>().iter(world)
        .any(|text| text.as_str() == "工程/assets/models/"));
    snapshot(&mut app, &target, &output.join("03-assets.png"));

    app.world_mut().resource_mut::<feathrise_editor_core::EditorSettings>().asset_browser_view_mode =
        feathrise_editor_core::AssetBrowserViewMode::Thumbnails;
    for _ in 0..8 { frame(&mut app); }
    scroll_content(&mut app, ProjectBrowserView::Assets, 0.5);
    assert_window_covered(app.world_mut(), ProjectBrowserView::Assets);
    scroll_content(&mut app, ProjectBrowserView::Assets, 1.);
    let data = asset_row(&mut app, "models/保留选择.bin");
    assert_row_visible(app.world(), data, ProjectBrowserView::Assets);
    click(&mut app, data);
    assert_eq!(app.world().resource::<SceneEditorState>().selected_asset, selected);
    no_scope_switches(app.world_mut());
    snapshot(&mut app, &target, &output.join("05-asset-thumbnails-last.png"));
    app.world_mut().resource_mut::<feathrise_editor_core::EditorSettings>().asset_browser_view_mode =
        feathrise_editor_core::AssetBrowserViewMode::List;
    for _ in 0..8 { frame(&mut app); }
    scroll_content(&mut app, ProjectBrowserView::Assets, 0.);

    let up = asset_action(&mut app, EditorAction::AssetFolderUp);
    click(&mut app, up);
    wait(&mut app, |world| assets_ready(world) && world.resource::<EditorAssetCatalog>().current_folder.is_empty());
    let up = asset_action(&mut app, EditorAction::AssetFolderUp);
    click(&mut app, up);
    wait(&mut app, |world| files_ready(world)
        && !world.resource::<ProjectBrowserState>().assets_visible()
        && world.resource::<EditorProjectFileCatalog>().current_folder.is_empty());
    assert_eq!(app.world().resource::<SceneEditorState>().selected_asset, selected,
        "离开资产目录不能清除原选择或武装工程源码摆放");

    // 外部资源字段与收藏使用的正式定位入口产生真实请求，再由原目录扫描和单目录桥完成采用。
    app.world_mut().resource_scope(|world, mut catalog: Mut<EditorAssetCatalog>| {
        world.resource_scope(|world, mut editor: Mut<SceneEditorState>| {
            crate::asset_catalog::request_asset_reveal(&mut catalog, &mut editor,
                &mut world.resource_mut::<crate::ui::sidebars::SceneEditorSidebarState>(), "models/保留选择.bin").unwrap();
        });
    });
    wait(&mut app, |world| assets_ready(world) && world.resource::<ProjectBrowserState>().assets_visible()
        && world.resource::<EditorAssetCatalog>().current_folder == "models"
        && world.resource::<SceneEditorState>().selected_asset.as_deref() == Some("models/保留选择.bin"));
    for _ in 0..8 { frame(&mut app); }
    let revealed = asset_row(&mut app, "models/保留选择.bin");
    assert_row_visible(app.world(), revealed, ProjectBrowserView::Assets);
    no_scope_switches(app.world_mut());
    assert_eq!(std::fs::read_to_string(root.join("src/main.rs")).unwrap(), source);
    assert_eq!(std::fs::read_to_string(root.join("src/zzz-last.rs")).unwrap(), source);
    snapshot(&mut app, &target, &output.join("04-asset-revealed.png"));
    let mut report = OpenOptions::new().write(true).create_new(true).open(output.join("report.json")).unwrap();
    serde_json::to_writer_pretty(&mut report, &serde_json::json!({
        "passed": true, "source": "production-project-directory-buttons-asset-reveal-document-io-and-ReadbackComplete",
        "os_input_verified": false, "asset_folder": "models", "asset_selection": selected,
        "project_source": "src/zzz-last.rs", "source_unchanged": true,
        "chinese_labels": ["工程"], "scope_switch_buttons": false, "asset_root_up_returns_project": true,
        "reference_reveal_uses_original_scan": true, "source_center_full_height": true,
        "hidden_entries": ["target", ".git", ".trash"],
        "pagination_controls": false, "project_directory_entries": 194, "asset_directory_entries": 193,
        "project_root_context_menu": true, "project_only_favorite_hidden": true, "project_action_buttons": false,
        "project_last_file_opened": true, "asset_last_file_selected": true,
        "thumbnail_last_file_visible": true, "deep_reference_reveal_visible": true,
        "project_row_pool": file_pool, "asset_row_pool": asset_pool,
    })).unwrap();
}
