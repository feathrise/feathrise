//! 异步失败与失效回归；只运行生产轮询系统，无渲染窗口。

use super::super::loading::{invalidate_changed_assets, poll_reference, validate_image}; // 真实完成采用与类型检查。
use super::interaction::app; // 共享最小Assets/AssetServer资源。
use super::*; // 正式草稿及字段身份夹具。
use crate::resources::SceneEditorState; // 关闭F12的入口。
use bevy::render::render_resource::{
    TextureDimension, TextureViewDescriptor, TextureViewDimension,
}; // 图片的实际采样维度。

#[test] // 超时、换分区和关闭时，请求必须结束并保留全部参数。
fn loading_cancels_without_writing_after_navigation_close_or_timeout() {
    for reason in 0..3 {
        let mut app = app(false); // 一个Blade草稿和实际Assets集合。
        app.add_systems(Update, poll_reference); // 生产轮询系统会真实初始化全部Query/Res。
        let original = app
            .world()
            .resource::<VegetationPresetEditorState>()
            .active_draft()
            .unwrap()
            .descriptor
            .clone(); // 完整原值。
        {
            let mut state = app
                .world_mut()
                .resource_mut::<VegetationPresetEditorState>(); // 放入一个尚未完成的来源请求。
            let mut pending = request(&state, mesh_reference_field(false)); // 原分区身份被冻结。
            pending.phase = ReferencePhase::Loading; // 正在等待非阻塞根加载。
            if reason == 0 {
                pending.started -= std::time::Duration::from_secs(60);
            } // 测试超时，不真的等待一分钟。
            state.asset_reference = Some(pending); // 使用同一请求槽。
            if reason == 1 {
                state.active_draft_mut().unwrap().section = PresetEditorSection::BladeLod;
            } // 已离开原字段分区。
        }
        if reason == 2 {
            app.world_mut().resource_mut::<SceneEditorState>().open = false;
        } // 模拟关闭F12。
        app.update(); // 三种路径都应在任何落值前短路。
        let state = app.world().resource::<VegetationPresetEditorState>(); // 检查生产采用边界。
        assert!(state.asset_reference.is_none()); // 没有悬挂busy状态。
        assert_eq!(state.active_draft().unwrap().descriptor, original); // 原引用及其他全部参数保留。
    }
}

#[test] // 标准二维纹理可用，Cube与多层纹理必须在渲染管线报错前拒绝。
fn image_reference_rejects_cube_array_and_three_dimensional_textures() {
    let mut image = Image::default(); // Bevy默认一像素二维纹理。
    assert!(validate_image(&image).is_ok()); // 合法二维图片。
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    }); // 显式立方视图。
    assert!(validate_image(&image).is_err()); // 同为Image不意味着采样类型相同。
    image.texture_view_descriptor = None; // 测试数组本身。
    image.texture_descriptor.size.depth_or_array_layers = 2; // 多层数组。
    assert!(validate_image(&image).is_err()); // 不自动取第一层。
    image.texture_descriptor.size.depth_or_array_layers = 1; // 恢复层数再改变真实维度。
    image.texture_descriptor.dimension = TextureDimension::D3; // 三维体纹理。
    assert!(validate_image(&image).is_err()); // 不能进入叶片基础色字段。
}

#[test] // 枚举完成后根GLTF热更，已显示的编号候选立即失效。
fn model_hot_reload_invalidates_the_visible_picker_without_writing() {
    let mut app = app(true); // 当前模型预设。
    app.add_systems(Update, invalidate_changed_assets); // 真实AssetEvent消息读取。
    let root = app.world_mut().resource_mut::<Assets<Gltf>>().add(gltf()); // 根句柄属于实际Assets集合。
    let original = app
        .world()
        .resource::<VegetationPresetEditorState>()
        .active_draft()
        .unwrap()
        .descriptor
        .clone(); // 所有路径原值。
    {
        let mut state = app
            .world_mut()
            .resource_mut::<VegetationPresetEditorState>(); // 建立正在选择的请求。
        let mut pending = request(&state, mesh_reference_field(true)); // 只测事件失效，目标为真实Mesh网格。
        pending.gltf = Some(root.clone()); // 明确目标根身份。
        pending.phase = ReferencePhase::Choosing; // 旧列表已经对作者可见。
        state.asset_reference = Some(pending); // 同一个生产请求槽。
    }
    app.world_mut()
        .write_message(AssetEvent::<Gltf>::Modified { id: root.id() }); // 热更新修改真实根。
    app.update(); // 消息系统应取消旧编号选择。
    let state = app.world().resource::<VegetationPresetEditorState>(); // 原字段不能被热更顺带覆盖。
    assert!(state.asset_reference.is_none()); // 旧按钮票据失去请求。
    assert_eq!(state.active_draft().unwrap().descriptor, original); // 无草稿副作用。
    assert!(state.status.contains("热更新")); // 给出可以重新选择的具体诊断。
}
