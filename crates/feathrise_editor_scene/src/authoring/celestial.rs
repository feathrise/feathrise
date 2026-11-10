//! 全局天体配置作者基线同步与动态场景未保存实时预览发布。

use bevy::prelude::*; // Res、ResMut 与资源变更检测组成轻量同步系统。
use feathrise_scene_content::{
    DynamicCelestialLightingPreview, WORLD_ENVIRONMENT_SCHEMA_VERSION, WorldEnvironmentAsset,
    WorldEnvironmentRegistry, resolve,
}; // 编辑器不复制全局默认、场景继承或运行时预览协议。

use super::resources::{EditorAuthoringSession, GlobalEnvironmentAuthoringState}; // 两份作者草稿共同生成最终动态场景预览。
use crate::resources::SceneEditorState; // F12 显隐与两份草稿脏位共同决定临时预览权威。

/// 首次加载或全局资产热更后，把安全有效配置同步为 F12 作者基线。
pub(crate) fn sync_global_environment_authoring(
    registry: Option<Res<WorldEnvironmentRegistry>>, // 文件缺失时注册表仍提供编译期默认值与错误。
    mut authoring: ResMut<GlobalEnvironmentAuthoringState>, // 已脏草稿不能被迟到 AssetServer 事件覆盖。
) {
    let Some(registry) = registry.as_deref() else {
        if !authoring.initialized {
            authoring.initialized = true; // 独立编辑器测试没有场景内容插件时继续使用自身安全默认值。
            authoring.load_error =
                Some("全局世界环境注册表尚未安装；当前使用编译期默认值".to_owned()); // UI 明确说明配置来源。
        }
        return;
    };
    if authoring.initialized && authoring.source_revision == registry.revision {
        return; // 稳定帧不克隆配置或触发 UI 变更检测。
    }
    authoring.source_revision = registry.revision; // 无论是否脏都记录已观察的正式资产修订。
    authoring.load_error.clone_from(&registry.last_error); // 文件错误持续可见，但不阻塞编辑与保存修复。
    authoring.initialized = true; // 首次注册表快照已经完成采用。
    if authoring.dirty {
        return; // 外部热更不能覆盖作者尚未保存的全局草稿。
    }
    let asset = WorldEnvironmentAsset {
        schema_version: WORLD_ENVIRONMENT_SCHEMA_VERSION, // 注册表只暴露已经通过当前版本校验的有效配置。
        celestial_lighting: registry.celestial_lighting.clone(), // 采用完整六组安全快照。
    };
    authoring.saved = asset.clone(); // 后续撤销可判断是否回到磁盘基线。
    authoring.draft = asset; // F12 数值控件和实时预览读取同一完整值。
}

/// F12 打开或天体草稿仍未保存时，发布“全局草稿 → 场景覆盖”的完整实时预览。
pub(crate) fn sync_dynamic_celestial_preview(
    editor: Res<SceneEditorState>, // 关闭只隐藏作者界面，不能自动丢弃仍脏的实景预览。
    session: Res<EditorAuthoringSession>, // 当前场景覆盖来自未保存清单草稿。
    global: Res<GlobalEnvironmentAuthoringState>, // 全局未保存修改也必须立即进入有效值。
    mut preview: ResMut<DynamicCelestialLightingPreview>, // 场景运行时只读取匹配场景的完整快照。
) {
    let Some(draft) = session
        .draft
        .as_ref()
        .filter(|draft| draft.manifest.is_data_driven())
    else {
        preview.clear(); // 无草稿或固定 HN 场景时下一光照帧恢复各自正式旧配置。
        return;
    };
    if !editor.open && !global.dirty && !draft.environment_dirty {
        preview.clear(); // 已关闭且两份作者配置都干净时，正式全局注册表与场景清单重新接管。
        return;
    }
    let resolved = resolve(
        &global.draft.celestial_lighting, // 未保存全局草稿优先于 AssetServer 正式值。
        &draft.manifest.environment.celestial_overrides, // 六组属性逐项选择继承或场景覆盖。
    );
    if resolved.validate().is_err() {
        preview.clear(); // UI 正常不会产生非法值，防御外部草稿损坏时保持正式安全配置。
        return;
    }
    preview.publish(draft.scene_id.clone(), resolved); // 运行时仅在当前 ActiveDynamicScene 身份匹配时采用。
}

#[cfg(test)]
mod tests {
    use std::sync::Arc; // 最小动态场景草稿共享测试高度场，不复制大型作者缓冲。

    use super::*; // 验证脏草稿不会被资产热更或 F12 显隐覆盖。
    use feathrise_scene_content::{CelestialLightingProfile, SceneCelestialLightingOverrides}; // 两类作者入口都必须覆盖完整六组天体参数。

    /// 建立只运行天体预览同步的最小 ECS 应用，并返回测试动态场景身份。
    fn preview_app() -> (App, feathrise_scene_content::SceneId) {
        let draft =
            crate::authoring::terrain::tests::test_heightfield_draft(Arc::new(vec![0; 121])); // 复用完整合法动态场景草稿形状。
        let scene_id = draft.scene_id.clone(); // 断言必须通过公开场景身份读取预览。
        let mut session = EditorAuthoringSession::default(); // 只安装当前场景草稿，不启动作者加载器。
        session.draft = Some(draft); // 预览系统必须拥有动态场景上下文。
        let mut app = App::new(); // 无窗口和渲染器即可验证权威生命周期。
        app.insert_resource(SceneEditorState {
            open: true, // 第一帧模拟作者正在 F12 内调参。
            ..default()
        });
        app.insert_resource(session); // 安装场景作者草稿。
        app.init_resource::<GlobalEnvironmentAuthoringState>(); // 从安全全局默认值开始。
        app.init_resource::<DynamicCelestialLightingPreview>(); // 运行时共享的完整预览资源。
        app.add_systems(Update, sync_dynamic_celestial_preview); // 只验证被测同步系统。
        (app, scene_id)
    }

    /// 返回六组字段都不同于编译期默认值的完整作者配置。
    fn distinct_profile() -> CelestialLightingProfile {
        CelestialLightingProfile {
            day_base_illuminance_lux: 12_345.0,          // 日间照度。
            day_base_color_srgb: [0.81, 0.72, 0.63],     // 日间颜色。
            night_peak_illuminance_lux: 456.0,           // 夜间照度。
            night_base_color_srgb: [0.21, 0.32, 0.43],   // 夜间颜色。
            azimuth_offset_degrees: -30.0,               // 发布时应规范为三百三十度。
            light_source_angular_diameter_degrees: 1.25, // 光栅与 Solari 共用角直径。
        }
    }

    #[test]
    fn dirty_global_draft_survives_a_new_registry_revision() {
        let mut state = GlobalEnvironmentAuthoringState::default(); // 初始使用编译期默认值。
        state.initialized = true;
        state.dirty = true;
        state.draft.celestial_lighting.day_base_illuminance_lux = 12_345.0; // 模拟未保存作者输入。
        let authored = state.draft.celestial_lighting.clone();
        let registry = WorldEnvironmentRegistry {
            handle: Handle::default(), // 纯同步测试不读取真实资产。
            celestial_lighting: CelestialLightingProfile::default(),
            revision: 9,
            last_error: None,
        };
        let mut app = App::new();
        app.insert_resource(state);
        app.insert_resource(registry);
        app.add_systems(Update, sync_global_environment_authoring);
        app.update();
        assert_eq!(
            app.world()
                .resource::<GlobalEnvironmentAuthoringState>()
                .draft
                .celestial_lighting,
            authored
        ); // 正式热更不能覆盖尚未保存的用户草稿。
    }

    #[test]
    fn closing_f12_keeps_all_dirty_global_celestial_parameters_in_gameplay() {
        let (mut app, scene_id) = preview_app(); // 建立当前动态场景作者会话。
        let authored = distinct_profile(); // 六组属性共同组成不可拆分的全局草稿快照。
        {
            let mut global = app
                .world_mut()
                .resource_mut::<GlobalEnvironmentAuthoringState>(); // 模拟 F12 全局区已经产生真实修改。
            global.draft.celestial_lighting = authored.clone(); // UI 当前显示并预览这份完整值。
            global.dirty = true; // 未保存全局草稿在关闭后仍拥有临时权威。
        }
        app.update(); // F12 打开时先发布实时预览。
        app.world_mut().resource_mut::<SceneEditorState>().open = false; // 模拟真正隐藏 F12。
        app.update(); // 关闭边沿不得清空脏全局值。

        assert_eq!(
            app.world()
                .resource::<DynamicCelestialLightingPreview>()
                .profile_for(&scene_id),
            Some(&authored.normalized()),
        ); // 日夜照度、两组颜色、方位角和角直径全部继续作用于玩法方向光。
    }

    #[test]
    fn closing_f12_keeps_all_dirty_scene_celestial_overrides_in_gameplay() {
        let (mut app, scene_id) = preview_app(); // 场景覆盖同样通过完整预览资源进入运行时。
        let overrides = SceneCelestialLightingOverrides {
            day_base_illuminance_lux: Some(7_654.0), // 场景日间照度覆盖。
            day_base_color_srgb: Some([0.77, 0.66, 0.55]), // 场景日间颜色覆盖。
            night_peak_illuminance_lux: Some(234.0), // 场景夜间照度覆盖。
            night_base_color_srgb: Some([0.11, 0.22, 0.33]), // 场景夜间颜色覆盖。
            azimuth_offset_degrees: Some(-45.0),     // 场景方位角覆盖应规范为三百一十五度。
            light_source_angular_diameter_degrees: Some(2.0), // 场景软阴影角范围覆盖。
        };
        let expected = resolve(
            &CelestialLightingProfile::default(), // 未覆盖的值理论上继续继承全局默认。
            &overrides,                           // 本测试实际覆盖全部六组字段。
        );
        {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>(); // 模拟 F12 当前场景覆盖区完成一次编辑。
            let draft = session.draft.as_mut().expect("测试动态场景草稿存在");
            draft.manifest.environment.celestial_overrides = overrides; // 完整场景覆盖进入清单草稿。
            draft.environment_dirty = true; // 未保存场景环境在关闭后仍拥有临时权威。
        }
        app.update(); // F12 打开时发布场景覆盖结果。
        app.world_mut().resource_mut::<SceneEditorState>().open = false; // 模拟真正隐藏 F12。
        app.update(); // 关闭边沿不得清空脏场景值。

        assert_eq!(
            app.world()
                .resource::<DynamicCelestialLightingPreview>()
                .profile_for(&scene_id),
            Some(&expected),
        ); // 六组场景覆盖共同保留，不允许只修日间照度。
    }

    #[test]
    fn closing_f12_clears_a_clean_celestial_preview() {
        let (mut app, scene_id) = preview_app(); // 干净草稿打开时仍为 F12 离屏画面提供同值预览。
        app.update(); // 首帧发布当前有效配置。
        assert!(
            app.world()
                .resource::<DynamicCelestialLightingPreview>()
                .profile_for(&scene_id)
                .is_some()
        ); // 证明后续确实执行了清除而非从未发布。
        app.world_mut().resource_mut::<SceneEditorState>().open = false; // 没有任何未保存作者值。
        app.update(); // 正式配置应重新取得唯一权威。

        assert!(
            app.world()
                .resource::<DynamicCelestialLightingPreview>()
                .profile_for(&scene_id)
                .is_none()
        ); // 干净关闭不会让过期预览长期遮蔽正式热更新。
    }
}
