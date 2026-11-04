//! 水体预设独立样品的无窗口回归测试。

use bevy::camera::visibility::RenderLayers; // 验证当前世界方向光只额外取得样品专层。
use bevy::mesh::VertexAttributeValues; // 核对样品真实米制范围与正式水Shader顶点色。
use bevy::prelude::*; // App、Node、Vec2与Update调度。
use feathrise_editor_core::constants::render_layers::MAP_EDITOR_WATER_PRESET_PREVIEW_LAYER; // 固定专层不得污染原层。
use feathrise_map_content::{WaterMacroWave, WaterMicroNormalLayer}; // 构造合法完整水体描述。

use super::environment::preview_light_layers; // 测试启停只增删样品层。
use super::navigation::{WaterPresetLivePreviewNavigation, test_apply_navigation}; // 测试轨道、平移和缩放数学。
use super::scene::build_preview_water_mesh; // 测试正式创建入口而非复制夹具。
use super::*; // 访问父模块草稿投影、资源和显隐系统。
use crate::ui::water_preset_editor::types::{WaterDraft, WaterPresetField}; // 构造编辑标签与合法数值输入。

/// 构造不依赖外部纹理的合法静水预设。
fn descriptor() -> WaterPresetDescriptor {
    WaterPresetDescriptor {
        id: "calm_lake".to_owned(),                 // 稳定预设身份。
        aliases: Vec::new(),                        // 测试不需要历史别名。
        label: "平静湖面".to_owned(),               // 显示名应被视觉比较去除。
        base_color_srgba: [0.05, 0.18, 0.24, 0.72], // 合法半透明水色。
        perceptual_roughness: 0.12,                 // 合法PBR粗糙度。
        reflectance: 0.5,                           // 合法反射率。
        trough_darkening: 0.08,                     // 合法波谷压暗。
        shore_wave_attenuation: 1.0,                // 样品无岸带时保持完整波幅。
        macro_waves: [WaterMacroWave {
            direction: [1.0, 0.0],        // 沿X方向的测试波。
            amplitude_meters: 0.1,        // 草稿上一完整合法振幅。
            wavelength_meters: 8.0,       // 五十米样品可观察多个周期。
            speed_meters_per_second: 0.6, // 合法温和传播速度。
            steepness: 0.1,               // 合法低陡峭度。
        }; 4],
        micro_normals: std::array::from_fn(|_| WaterMicroNormalLayer {
            texture_path: None,                     // 无窗口测试不触发资产加载。
            tile_meters: 1.0,                       // 合法米制平铺。
            velocity_meters_per_second: [0.0, 0.0], // 静止微法线便于固定断言。
            strength: 0.0,                          // 测试不依赖纹理细节。
        }),
    }
}

#[test]
fn hidden_inspector_stops_water_camera_without_discarding_the_draft_or_active_tab() {
    let mut app = App::new();
    app.insert_resource(MapEditorState {
        open: true,
        ..default()
    })
    .init_resource::<MapEditorLayoutState>()
    .init_resource::<MapEditorSidebarState>()
    .init_resource::<WaterPresetLivePreviewRuntime>()
    .init_resource::<VegetationPresetEditorState>()
    .add_systems(Update, sync_live_preview_visibility);
    let (state, tab_id) = editing_state();
    let original = state.tabs[0].draft.descriptor.clone();
    app.insert_resource(state);
    app.world_mut()
        .resource_mut::<VegetationPresetEditorState>()
        .active_tab = PresetWorkspaceTab::Water(tab_id);
    let camera = app
        .world_mut()
        .spawn((Camera::default(), WaterPresetLivePreviewCamera))
        .id();
    let sample = app
        .world_mut()
        .spawn((Visibility::Inherited, WaterPresetLivePreviewRenderable))
        .id();
    app.update();
    assert!(app.world().get::<Camera>(camera).unwrap().is_active);
    app.world_mut()
        .resource_mut::<MapEditorLayoutState>()
        .toggle(crate::types::EditorLayoutAction::Focus);
    app.update();
    assert!(!app.world().get::<Camera>(camera).unwrap().is_active);
    assert_eq!(
        app.world().get::<Visibility>(sample),
        Some(&Visibility::Hidden)
    );
    app.world_mut()
        .resource_mut::<MapEditorLayoutState>()
        .toggle(crate::types::EditorLayoutAction::Focus);
    for (visible, fits) in [(false, true), (true, false), (true, true)] {
        {
            let mut layout = app.world_mut().resource_mut::<MapEditorLayoutState>();
            layout.inspector_visible = visible;
            layout.inspector_fits = fits;
        }
        app.update();
        assert_eq!(
            app.world().get::<Camera>(camera).unwrap().is_active,
            visible && fits
        );
        assert_eq!(
            app.world().get::<Visibility>(sample),
            Some(&if visible && fits {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            })
        );
        assert_eq!(
            app.world()
                .resource::<WaterPresetLivePreviewRuntime>()
                .active_tab,
            Some(tab_id)
        );
    }
    assert_eq!(
        app.world().resource::<WaterPresetEditorState>().tabs[0]
            .draft
            .descriptor,
        original
    );
}

/// 建立一个真实“编辑所选”标签并返回其稳定ID。
fn editing_state() -> (WaterPresetEditorState, u64) {
    let original = descriptor(); // 完整旧值用于后续不提交断言。
    let mut state = WaterPresetEditorState::default(); // 空编辑器会话。
    let tab_id = state.open_draft(WaterDraft::new(
        WaterDraftMode::Edit {
            original_id: original.id.clone(), // 编辑模式保留正式目标身份。
        },
        original, // 标签内部持有完整草稿副本。
    ));
    (state, tab_id) // 返回会话和中央工作区标签身份。
}

#[test]
fn valid_numeric_buffer_previews_without_committing_the_draft() {
    let (mut state, tab_id) = editing_state(); // 建立合法编辑标签。
    let original = state.tabs[0].draft.descriptor.clone(); // 冻结真实草稿旧值。
    state.active_field = Some(WaterPresetField::WaveAmplitude(0)); // 模拟第一组波浪数值框。
    state.input_buffer = "0.75".to_owned(); // 合法但尚未提交的字符缓冲。

    let (_, preview) = active_edit_candidate(&state, PresetWorkspaceTab::Water(tab_id))
        .expect("编辑标签应产生样品候选");

    assert_eq!(preview.macro_waves[0].amplitude_meters, 0.75); // 样品立即显示合法字符值。
    assert_eq!(state.tabs[0].draft.descriptor, original); // 草稿和撤销链保持未提交状态。
}

#[test]
fn invalid_numeric_buffer_keeps_the_last_complete_draft_value() {
    let (mut state, tab_id) = editing_state(); // 建立合法编辑标签。
    let original = state.tabs[0].draft.descriptor.clone(); // 冻结真实草稿旧值。
    state.active_field = Some(WaterPresetField::WaveAmplitude(0)); // 模拟第一组波浪数值框。
    state.input_buffer = "99".to_owned(); // 超出正式范围的中间字符。

    let (_, preview) = active_edit_candidate(&state, PresetWorkspaceTab::Water(tab_id))
        .expect("无效中间态应保留上一完整样品候选");

    assert_eq!(preview.macro_waves[0].amplitude_meters, 0.1); // 样品不采用半套非法值。
    assert_eq!(state.tabs[0].draft.descriptor, original); // 草稿仍完全不变。
}

#[test]
fn preview_revision_changes_only_on_real_snapshot_edges() {
    let snapshot = WaterPresetLivePreviewSnapshot {
        target_preset_id: "calm_lake".to_owned(), // 固定正式目标。
        descriptor: visual_descriptor(descriptor()), // 只保留GPU视觉参数。
        image_handles: [None, None],              // 无纹理仍是完整合法快照。
    };
    let mut preview = WaterPresetLivePreview::default(); // 初始没有样品候选。

    preview.replace(Some(snapshot.clone())); // 首次采用推进修订。
    let applied_revision = preview.revision; // 冻结已采用身份。
    preview.replace(Some(snapshot)); // 相同完整值不得制造GPU更新。
    assert_eq!(preview.revision, applied_revision); // 稳定帧修订不变。
    preview.replace(None); // 离开编辑标签形成真实边沿。
    assert!(preview.revision > applied_revision); // 清除也必须作废旧采用身份。
    assert!(preview.snapshot.is_none()); // 不保留隐藏草稿覆盖。
}

#[test]
fn workbench_section_switch_keeps_sample_identity_and_manual_sidebar_choice() {
    use crate::ui::water_preset_editor::types::{WaterPresetAction, WaterSection};
    use crate::ui::water_preset_editor::workbench_tests::{interaction_app, press};

    let (state, tab_id) = editing_state();
    let author = state.tabs[0].draft.descriptor.clone();
    let mut app = interaction_app(state);
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_resource::<WaterPresetLivePreview>()
        .init_resource::<WaterPresetLivePreviewRuntime>()
        .init_resource::<MapEditorSidebarState>()
        .init_resource::<Assets<DynamicWaterMaterial>>()
        .add_systems(
            Update,
            (sync_live_preview_draft, sync_live_preview_visibility)
                .chain()
                .after(crate::ui::water_preset_editor::interaction::handle_buttons),
        );
    let camera = app
        .world_mut()
        .spawn((Camera::default(), WaterPresetLivePreviewCamera))
        .id();
    let material = app
        .world_mut()
        .resource_mut::<Assets<DynamicWaterMaterial>>()
        .add(DynamicWaterMaterial::default());
    app.world_mut()
        .resource_mut::<WaterPresetLivePreviewRuntime>()
        .material = Some(material.clone());
    app.add_systems(
        Update,
        sync_live_preview_material.after(sync_live_preview_draft),
    );
    app.update();
    let revision = app.world().resource::<WaterPresetLivePreview>().revision;
    let snapshot = app
        .world()
        .resource::<WaterPresetLivePreview>()
        .snapshot
        .clone();
    assert_eq!(
        app.world()
            .resource::<WaterPresetLivePreviewRuntime>()
            .active_tab,
        Some(tab_id)
    );
    app.world_mut()
        .resource_mut::<MapEditorSidebarState>()
        .right = EditorRightSidebarTab::Assets;
    for section in [
        WaterSection::Waves,
        WaterSection::Normals,
        WaterSection::Material,
        WaterSection::Appearance,
    ] {
        press(&mut app, WaterPresetAction::Section(section));
        let preview = app.world().resource::<WaterPresetLivePreview>();
        assert_eq!(preview.revision, revision);
        assert_eq!(preview.snapshot, snapshot);
        assert_eq!(
            app.world().resource::<MapEditorSidebarState>().right,
            EditorRightSidebarTab::Assets
        );
        assert!(!app.world().get::<Camera>(camera).unwrap().is_active);
        assert_eq!(
            app.world()
                .resource::<WaterPresetLivePreviewRuntime>()
                .material
                .as_ref(),
            Some(&material)
        );
        assert_eq!(
            app.world().resource::<Assets<DynamicWaterMaterial>>().len(),
            1
        );
    }
    assert_eq!(
        app.world().resource::<WaterPresetEditorState>().tabs[0]
            .draft
            .descriptor,
        author
    );
    {
        let mut state = app.world_mut().resource_mut::<WaterPresetEditorState>();
        state.active_field = Some(WaterPresetField::WaveAmplitude(0));
        state.input_buffer = "0.75".into();
    }
    app.update();
    let numeric_revision = app.world().resource::<WaterPresetLivePreview>().revision;
    assert!(numeric_revision > revision);
    press(&mut app, WaterPresetAction::Section(WaterSection::Normals));
    assert_eq!(
        app.world().resource::<WaterPresetLivePreview>().revision,
        numeric_revision
    );
    assert_eq!(
        app.world().resource::<WaterPresetEditorState>().tabs[0]
            .draft
            .descriptor
            .macro_waves[0]
            .amplitude_meters,
        0.75
    );
    assert_eq!(
        app.world()
            .resource::<crate::authoring::EditorAuthoringSession>()
            .water_revision,
        0
    );
}

#[test]
fn sample_mesh_is_exactly_ten_by_fifty_meters_and_has_vertex_colors() {
    let mesh = build_preview_water_mesh(); // 使用运行时同一网格入口。
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        panic!("样品水面必须使用Float32x3位置"); // 正式水Shader依赖三维位置。
    };
    let min_x = positions
        .iter()
        .map(|position| position[0])
        .fold(f32::INFINITY, f32::min); // 求真实X下界。
    let max_x = positions
        .iter()
        .map(|position| position[0])
        .fold(f32::NEG_INFINITY, f32::max); // 求真实X上界。
    let min_z = positions
        .iter()
        .map(|position| position[2])
        .fold(f32::INFINITY, f32::min); // 求真实Z下界。
    let max_z = positions
        .iter()
        .map(|position| position[2])
        .fold(f32::NEG_INFINITY, f32::max); // 求真实Z上界。
    assert_eq!((min_x, max_x), (-5.0, 5.0)); // X轴严格覆盖十米真实河宽。
    assert_eq!((min_z, max_z), (-25.0, 25.0)); // Z轴严格覆盖五十米真实河长。

    let Some(VertexAttributeValues::Float32x4(colors)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR)
    else {
        panic!("样品水面必须提供Float32x4顶点色"); // 水Shader读取岸带与波谷权重。
    };
    assert_eq!(colors.len(), positions.len()); // 每个顶点都有完整颜色输入。
    assert!(colors.iter().all(|color| *color == [1.0; 4])); // 固定样品不伪造岸带衰减。
}

#[test]
fn preview_camera_supports_orbit_pan_and_zoom_without_touching_the_map() {
    let mut navigation = WaterPresetLivePreviewNavigation::default(); // 初始完整取景。
    let (initial_center, initial_distance, initial_yaw, initial_pitch) = navigation.test_values(); // 冻结四个观察自由度。

    test_apply_navigation(
        &mut navigation,       // 只修改样品会话状态。
        Vec2::new(18.0, -9.0), // 模拟右键水平及向上拖动。
        Vec2::new(12.0, 7.0),  // 模拟中键屏幕平移。
        2.0,                   // 正滚轮放大。
        190.0,                 // 与右栏默认画布高度一致。
    );
    let (center, distance, yaw, pitch) = navigation.test_values(); // 读取最终纯相机状态。

    assert!(distance < initial_distance); // 正滚轮必须拉近相机。
    assert_ne!(yaw, initial_yaw); // 水平右拖必须改变方位。
    assert!(pitch < initial_pitch); // 向上右拖必须降低俯仰并允许看向底侧。
    assert_ne!(center, initial_center); // 中键拖动必须移动观察中心。
}

#[test]
fn current_celestial_lights_gain_only_the_preview_layer_while_visible() {
    let original = RenderLayers::from_layers(&[0, 3, 11]); // 模拟当前场景已有地图与角色光照层。
    let active = preview_light_layers(&original, true); // 打开右侧样品场景。

    assert!(active.intersects(&RenderLayers::layer(0))); // 默认地图层保持不变。
    assert!(active.intersects(&RenderLayers::layer(3))); // 既有角色层保持不变。
    assert!(active.intersects(&RenderLayers::layer(11))); // 其他既有层保持不变。
    assert!(active.intersects(&RenderLayers::layer(MAP_EDITOR_WATER_PRESET_PREVIEW_LAYER))); // 当前太阳/月亮新增唯一样品层。

    let inactive = preview_light_layers(&active, false); // 切页或关闭预览。
    assert_eq!(inactive, original); // 精确移除样品层且不破坏当前场景原光照层。
}

#[test]
fn entering_edit_tab_reveals_scene_once_without_stealing_manual_sidebar_choice() {
    let mut app = App::new(); // 最小ECS应用不启动渲染器。
    app.init_resource::<MapEditorState>() // 提供F12显隐状态。
        .init_resource::<MapEditorSidebarState>() // 提供右栏页签状态。
        .init_resource::<WaterPresetLivePreviewRuntime>() // 记录首次自动切页边沿。
        .init_resource::<VegetationPresetEditorState>() // 共享中央工作区活动标签。
        .add_systems(Update, sync_live_preview_visibility); // 运行真实显隐系统。
    app.world_mut().resource_mut::<MapEditorState>().open = true; // 模拟F12已经打开。
    let (state, tab_id) = editing_state(); // 建立“编辑所选”水体标签。
    app.insert_resource(state); // 安装水体编辑状态。
    app.world_mut()
        .resource_mut::<VegetationPresetEditorState>()
        .active_tab = PresetWorkspaceTab::Water(tab_id); // 选中水体标签。
    let panel = app
        .world_mut()
        .spawn((
            WaterPresetLivePreviewPanel, // 真实父根marker。
            Node {
                display: Display::None, // 首帧尚未显示。
                ..default()             // 其他布局字段不参与回归。
            },
        ))
        .id();
    app.world_mut()
        .resource_mut::<MapEditorSidebarState>()
        .right = EditorRightSidebarTab::Assets; // 进入标签前作者停留在资产页。

    app.update(); // 新标签边沿应揭示一次场景页。
    assert_eq!(
        app.world().resource::<MapEditorSidebarState>().right,
        EditorRightSidebarTab::Scene
    ); // 首次进入自动展示样品。
    assert_eq!(
        app.world().get::<Node>(panel).unwrap().display,
        Display::Flex
    ); // 样品宿主参与右栏布局。

    app.world_mut()
        .resource_mut::<MapEditorSidebarState>()
        .right = EditorRightSidebarTab::Environment; // 作者随后主动检查环境页。
    app.update(); // 稳定标签不得抢回场景页。
    assert_eq!(
        app.world().resource::<MapEditorSidebarState>().right,
        EditorRightSidebarTab::Environment
    ); // 保留用户手动页签选择。
}
