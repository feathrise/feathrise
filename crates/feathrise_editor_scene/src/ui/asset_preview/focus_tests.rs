//! 应用扩展预览期间的资产浏览：同路径换来源、唯一画布、真实裁剪与关闭恢复。

use feathrise_editor_ui::UiInteraction;
use super::*; // 预览来源合同。
use crate::ui::asset_preview::{
    loading::sync_asset_preview_selection, mounting::mount_asset_preview_panel,
    resources::AssetPreviewNavigation, types::*,
}; // 执行正式系统，不复制路径选择逻辑。
use crate::ui::{
    sidebars::{EditorRightSidebarTab, SceneEditorSidebarState},
    vegetation_preset_editor::VegetationPresetEditorState,
}; // 真实共享宿主与作者会话。
use crate::{
    asset_catalog::handle_asset_row_clicks,
    components::SceneEditorAssetRow,
    resources::{EditorAssetCatalog, SceneEditorNumericScrubMouseGuard, SceneEditorState},
    types::EditorAssetEntry,
}; // 真正文件点击入口。
use bevy::{
    app::TaskPoolPlugin,
    asset::{AssetApp, AssetPlugin},
    gltf::Gltf,
    prelude::*,
    world_serialization::WorldAsset,
}; // 仅资产服务与ECS，不创建GPU或原生窗口。
use feathrise_scene_content::SceneAssetKind; // 文件夹与模型沿用实际协议。

/// 测试只提供中性来源/宿主；模型生命周期、点击和裁剪仍运行正式系统。
#[derive(Resource)]
struct FixturePreview {
    path: Option<String>,
    host: Entity,
}
fn publish_fixture_preview(
    editor: Res<SceneEditorState>,
    workspace: Res<VegetationPresetEditorState>,
    focus: Res<AssetPreviewFocus>,
    fixture: Res<FixturePreview>,
    mut request: ResMut<crate::game_authoring::ExternalAssetPreviewRequest>,
    mut sidebar: ResMut<SceneEditorSidebarState>,
) {
    let active = editor.open
        && workspace.active_tab == PresetWorkspaceTab::Domain("test.author", 1)
        && !focus.browsing(workspace.active_tab);
    if active && !request.active {
        sidebar.reveal_scene_properties();
    }
    *request = crate::game_authoring::ExternalAssetPreviewRequest {
        active,
        identity: active.then_some(crate::game_authoring::ExternalPreviewIdentity {
            domain: "test.author",
            tab: 1,
            revision: 1,
            ticket: 0,
        }),
        model_path: fixture.path.clone(),
        host: Some(fixture.host),
        root: None,
        constructed: false,
        title: Some("扩展预览".into()),
    };
}

fn fixture(model: Option<&str>) -> (App, Entity, Entity, Entity, Entity, PresetWorkspaceTab) {
    let mut app = App::new(); // 单帧选择与挂载可以无窗口验证。
    app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default())) // 后台加载请求使用正式AssetServer。
        .init_asset::<Gltf>()
        .init_asset::<WorldAsset>()
        .init_asset::<Image>() // 只注册预览会请求的资产类型。
        .init_resource::<AssetPreviewState>()
        .init_resource::<AssetPreviewNavigation>()
        .init_resource::<AssetPreviewFocus>() // 唯一预览与意图。
        .init_resource::<SceneEditorState>()
        .init_resource::<SceneEditorSidebarState>()
        .init_resource::<SceneEditorNumericScrubMouseGuard>() // 真正输入门。
        .init_resource::<EditorAssetCatalog>()
        .init_resource::<VegetationPresetEditorState>()
        .init_resource::<crate::game_authoring::ExternalAssetPreviewRequest>()
        .add_systems(
            Update,
            (
                handle_asset_row_clicks,
                publish_fixture_preview,
                sync_asset_preview_selection,
                mount_asset_preview_panel,
            )
                .chain(),
        ); // 与正式阶段相同的点击→加载→挂载顺序。
    let tab = PresetWorkspaceTab::Domain("test.author", 1);
    app.world_mut().resource_mut::<SceneEditorState>().open = true; // F12已经打开。
    app.world_mut()
        .resource_mut::<SceneEditorSidebarState>()
        .right = EditorRightSidebarTab::Assets; // 右侧浏览器可见。
    let original = app.world_mut().spawn(Node::default()).id(); // 原资产页父节点。
    let equipment_host = app.world_mut().spawn(Node::default()).id();
    app.insert_resource(FixturePreview {
        path: model.map(str::to_owned),
        host: equipment_host,
    });
    let panel = app
        .world_mut()
        .spawn((AssetPreviewPanel, Node::default(), ChildOf(original)))
        .id(); // 全程复用此面板。
    let camera = app
        .world_mut()
        .spawn((AssetPreviewCamera, Camera::default()))
        .id(); // 相机显隐可观测，但不安装渲染器。
    app.update(); // 先记录资产面板原父级。
    app.world_mut()
        .resource_mut::<VegetationPresetEditorState>()
        .active_tab = tab; // 随后进入独立装备草稿。
    app.update(); // 完整装备预览会将面板借到场景栏宿主。
    (app, original, equipment_host, panel, camera, tab) // 保留轻量可观测身份。
}

fn click(app: &mut App, path: &str) {
    // 文件行位于左侧资产页；点击后由正式系统切换右侧属性来源。
    app.world_mut()
        .resource_mut::<SceneEditorSidebarState>()
        .navigator = crate::ui::sidebars::EditorNavigatorTab::Assets;
    app.world_mut()
        .resource_mut::<EditorAssetCatalog>()
        .entries
        .push(EditorAssetEntry {
            display_name: "候选模型.glb".into(), // 实际资产行显示名。
            asset_path: path.into(),             // 可以与装备模型路径完全相同。
            is_directory: false,                 // 点击文件才请求预览。
            kind: SceneAssetKind::Scene,           // 实际GLB模型用途。
        });
    let index = app.world().resource::<EditorAssetCatalog>().entries.len() - 1; // 明确当前点击的目录项。
    app.world_mut().spawn((
        feathrise_editor_ui::EditorButtonClick::triggered(),
        UiInteraction::Pressed,
        SceneEditorAssetRow {
            slot: 0,
            entry_index: Some(index),
        },
    )); // 重复同路径也走已确认点击的正式动作入口。
    app.update(); // 一次点击必须同步切换来源和宿主。
}

#[test] // 同路径不应被字符串相等短路，装备预览票据必须退出。
fn external_browser_click_restores_sidebar_even_for_the_same_glb() {
    let path = "fixture-equipment.glb"; // 不读取真实大型美术资源。
    let (mut app, original, equipment_host, panel, camera, tab) = fixture(Some(path));
    assert_eq!(
        app.world().get::<ChildOf>(panel).unwrap().parent(),
        equipment_host
    ); // 确认重现问题所需的中央占用前置条件。
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_asset = Some(path.into()); // 文件此前已经选中过。
    click(&mut app, path); // 重选同一个GLB也应取回浏览权。
    let state = app.world().resource::<AssetPreviewState>(); // 检查加载实际消费的来源，而非只检查Focus布尔值。
    assert_eq!(state.selected_path.as_deref(), Some(path)); // 真正预览右侧所选文件。
    assert!(state.external_preview.is_none()); // 恢复primitive、引用与普通导航入口。
    assert_eq!(
        app.world().get::<ChildOf>(panel).unwrap().parent(),
        original
    ); // 画布确实归还右侧。
    assert_eq!(
        app.world()
            .resource::<VegetationPresetEditorState>()
            .active_tab,
        tab
    ); // 不通过关闭草稿来实现浏览。
    let root = state.scene_root().unwrap(); // 保持这次正式加载创建的唯一模型根。
    app.world_mut().resource_mut::<AssetPreviewState>().content = AssetPreviewContent::Model {
        handle: Handle::default(),
        root,
        phase: AssetPreviewPhase::Ready,
    }; // 模拟异步模型已完成，检查可见门无需GPU。
    app.update(); // 场景中故意没有中央视口，右栏不应被中央裁剪条件停用。
    assert!(app.world().get::<Camera>(camera).unwrap().is_active); // 浏览GLB可以渲染。
    app.world_mut()
        .resource_mut::<SceneEditorSidebarState>()
        .right = EditorRightSidebarTab::Scene; // 用户切走右栏。
    app.update();
    assert!(!app.world().get::<Camera>(camera).unwrap().is_active); // 隐藏右栏仍停止额外视图。
    app.world_mut()
        .resource_mut::<AssetPreviewFocus>()
        .show_workspace(); // 显式重新预览交回装备；后台结果本身不做此操作。
    app.update();
    assert_eq!(
        app.world().get::<ChildOf>(panel).unwrap().parent(),
        equipment_host
    ); // 同一个面板回装备场景栏。
    assert!(
        app.world()
            .resource::<AssetPreviewState>()
            .external_preview
            .is_some()
    ); // 同路径也恢复正确预设票据。
    assert_eq!(
        app.world_mut()
            .query_filtered::<Entity, With<AssetPreviewPanel>>()
            .iter(app.world())
            .count(),
        1
    ); // 没有第二面板。
    assert_eq!(
        app.world_mut()
            .query_filtered::<Entity, With<AssetPreviewCamera>>()
            .iter(app.world())
            .count(),
        1
    ); // 没有第二相机。
}

#[test]
fn external_scene_preview_uses_its_own_clip_and_stops_when_sidebar_is_hidden() {
    let (mut app, _, host, panel, camera, _) = fixture(Some("fixture.glb"));
    use crate::ui::sidebars::{EditorSidebarTab, SceneEditorSidebarPage};
    app.world_mut().spawn((
        AssetPreviewCanvas,
        ComputedNode {
            size: Vec2::splat(200.0),
            ..default()
        },
        UiGlobalTransform::default(),
    ));
    let page = app
        .world_mut()
        .spawn((
            SceneEditorSidebarPage(EditorSidebarTab::Right(EditorRightSidebarTab::Scene)),
            ComputedNode {
                size: Vec2::new(340.0, 700.0),
                ..default()
            },
            UiGlobalTransform::default(),
        ))
        .id();
    let root = app
        .world()
        .resource::<AssetPreviewState>()
        .scene_root()
        .unwrap();
    app.world_mut().resource_mut::<AssetPreviewState>().content = AssetPreviewContent::Model {
        handle: Handle::default(),
        root,
        phase: AssetPreviewPhase::Ready,
    };
    app.init_resource::<crate::resources::SceneEditorLayoutState>();
    app.update();
    assert_eq!(app.world().get::<ChildOf>(panel).unwrap().parent(), host);
    assert_eq!(
        app.world().resource::<SceneEditorSidebarState>().right,
        EditorRightSidebarTab::Scene
    );
    assert!(app.world().get::<Camera>(camera).unwrap().is_active); // 没有中央视口也能按右栏裁剪区显示。
    app.world_mut()
        .resource_mut::<SceneEditorSidebarState>()
        .right = EditorRightSidebarTab::Assets;
    app.update();
    assert!(!app.world().get::<Camera>(camera).unwrap().is_active);
    assert_eq!(
        app.world().resource::<SceneEditorSidebarState>().right,
        EditorRightSidebarTab::Assets
    ); // 稳定帧不得抢页签。
    app.world_mut()
        .resource_mut::<SceneEditorSidebarState>()
        .right = EditorRightSidebarTab::Scene;
    app.world_mut()
        .resource_mut::<crate::resources::SceneEditorLayoutState>()
        .inspector_visible = false;
    app.update();
    assert!(!app.world().get::<Camera>(camera).unwrap().is_active); // 装备不再绕过收起右栏的门。
    app.world_mut()
        .resource_mut::<crate::resources::SceneEditorLayoutState>()
        .inspector_visible = true;
    app.world_mut().get_mut::<ComputedNode>(page).unwrap().size = Vec2::ZERO;
    app.update();
    assert!(!app.world().get::<Camera>(camera).unwrap().is_active); // 不可见布局不能保留额外渲染。
}

#[test] // 防具或校验未完成时，独立文件预览同样必须工作。
fn external_without_model_can_browse_and_close_clears_override() {
    let (mut app, original, _, panel, _, tab) = fixture(None); // 无装备模型仍保持真实活动草稿。
    click(&mut app, "candidate.glb");
    assert_eq!(
        app.world()
            .resource::<AssetPreviewState>()
            .selected_path
            .as_deref(),
        Some("candidate.glb")
    ); // 不再被装备None路径覆盖。
    assert_eq!(
        app.world().get::<ChildOf>(panel).unwrap().parent(),
        original
    ); // 右栏正常显示候选文件。
    app.update(); // 后台/稳定帧不得重新占用中央。
    assert!(app.world().resource::<AssetPreviewFocus>().browsing(tab));
    app.world_mut().resource_mut::<SceneEditorState>().open = false; // F12关闭仍退出所有预览。
    app.update();
    assert!(!app.world().resource::<AssetPreviewFocus>().browsing(tab)); // 关闭后的旧浏览意图不复活。
    assert!(
        app.world()
            .resource::<AssetPreviewState>()
            .selected_path
            .is_none()
    ); // 保持原释放路径。
}
