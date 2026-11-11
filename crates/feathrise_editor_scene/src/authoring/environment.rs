//! 场景天空模式与环境 IBL 的作者修改、撤销入口及 F12 离屏相机实时请求同步。

use bevy::prelude::*; // Commands、Query与资源变更检测。
use feathrise_atmosphere::{AuthoredSkyMode, AuthoredSkyboxRequest, WeatherAuthoringPreview}; // 可见模式与 IBL 复用正式加载器，天气预览复用连续输出。
use feathrise_scene_core::view::PrimarySceneView; // 当前动态场景唯一玩法窗口相机。
use feathrise_scene_content::{ActiveDynamicScene, SceneEnvironmentDescriptor, SceneId, SceneSkyMode}; // 清单天空模式、环境格式和玩法场景稳定身份。
use feathrise_scene_core::state::ScenePhase; // 只有统一动态场景场景接受未保存环境覆盖。

use super::resources::{EditorAuthoringSession, EditorCommand}; // 草稿、修订和统一撤销栈。
use crate::components::SceneEditorCamera; // 只把草稿天空安装到独立离屏相机。
use crate::resources::SceneEditorState; // 资产选择、一帧动作和用户可见状态。
use crate::types::EnvironmentEditAction; // 右栏按钮的结构化作者动作。

pub(super) const SKYBOX_MAX_BRIGHTNESS: f32 = 100_000.0; // 可见天空盒与 PBR 环境照明字段统一使用的安全硬上限。

/// 从完整旧值生成离散环境动作的新值；连续亮度、方向和天气比例由公共字段处理。
fn edited_environment(
    current: &SceneEnvironmentDescriptor, // 当前草稿完整环境快照。
    action: EnvironmentEditAction,      // 本帧唯一离散动作。
    selected_asset: Option<&str>,       // 只有“设置天空资源”读取资产浏览器选择。
) -> Result<SceneEnvironmentDescriptor, String> {
    let mut next = current.clone(); // 撤销记录按完整快照工作，修改不能原地留下半值。
    match action {
        EnvironmentEditAction::SetSkyMode(mode) => {
            next.sky_mode = mode; // 下方完整校验会拒绝没有主资源的天空盒模式。
        }
        EnvironmentEditAction::SetSelectedSkybox => {
            let path = selected_asset
                .ok_or_else(|| "请先在右侧资产浏览器选择 KTX2 天空主资源".to_owned())?;
            next.skybox_path = Some(path.to_owned()); // 下方统一验证扩展名与相对路径安全性。
        }
        EnvironmentEditAction::ClearSkybox => {
            next.skybox_path = None; // 清除主 Cube 及两张固定后缀 IBL 的共同身份。
            next.sky_mode = SceneSkyMode::Dynamic; // 清除后立即恢复原生大气与动态云，禁止保存无资源天空盒。
        }
    }
    next.validate()?; // 普通二维纹理、越界路径和非有限参数在写入草稿前拒绝。
    Ok(next)
}

/// 把持久场景枚举显式转换为不依赖 map-content 的氛围运行时枚举。
const fn authored_sky_mode(mode: SceneSkyMode) -> AuthoredSkyMode {
    match mode {
        SceneSkyMode::Dynamic => AuthoredSkyMode::Dynamic, // 只加载 IBL 两件套并显示动态天空。
        SceneSkyMode::Skybox => AuthoredSkyMode::Skybox, // 加载主 Cube 与 IBL 三件套并关闭动态表现。
    }
}

/// 消费右栏环境动作，把动态场景清单修改纳入统一撤销、脏状态和作者修订。
pub(crate) fn commit_environment_change(
    session: &mut EditorAuthoringSession, // 当前场景作者会话。
    editor: &mut SceneEditorState,          // 统一脏状态和可见诊断。
    before: SceneEnvironmentDescriptor,     // 操作开始前完整环境快照。
    after: SceneEnvironmentDescriptor,      // 操作完成后的受检环境快照。
    status: impl Into<String>,            // 用户可见结果说明。
) {
    if before == after {
        editor.status = "环境参数没有变化".to_owned(); // 同值输入不制造伪撤销或换代天空请求。
        return;
    }
    let Some(draft) = session.draft.as_mut() else {
        return; // 草稿换代时迟到操作不得建立环境替身。
    };
    draft.manifest.environment = after.clone(); // 先原子采用完整新环境权威。
    session
        .undo
        .push(EditorCommand::EnvironmentChanged { before, after }); // 一个按钮、精确提交或完整拖动只占一条撤销。
    session.redo.clear(); // 新作者分支使旧重做历史失效。
    session.mark_environment_changed(editor); // 推进一次天空身份并把清单标脏。
    editor.status = status.into(); // 底栏回显调用方的具体字段结果。
}

/// 消费右栏环境动作，把动态场景清单修改纳入统一撤销、脏状态和作者修订。
pub(crate) fn apply_environment_action(
    mut editor: ResMut<SceneEditorState>, // 读取选择、消费动作并发布错误或成功状态。
    mut session: ResMut<EditorAuthoringSession>, // 修改当前草稿环境并登记撤销。
) {
    let Some(action) = editor.environment_action_requested.take() else {
        return; // 稳定帧不访问草稿或分配快照。
    };
    if session.saving {
        editor.status = "场景保存期间不能修改环境配置".to_owned(); // 保存任务冻结的草稿必须保持不可变。
        return;
    }
    let selected_asset = editor.selected_asset.as_deref(); // 在可变借用草稿前冻结只读选择。
    let Some(draft) = session.draft.as_mut() else {
        editor.status = "场景作者数据尚未就绪".to_owned();
        return;
    };
    if !draft.manifest.is_data_driven() {
        editor.status = "当前场景的环境配置由工程管理".to_owned(); // 不越权覆盖HN-01/HN-02既有场景设置。
        return;
    }
    let before = draft.manifest.environment.clone(); // 完整快照用于一次撤销恢复全部环境字段。
    let after = match edited_environment(&before, action, selected_asset) {
        Ok(value) => value,
        Err(error) => {
            editor.status = format!("环境配置修改失败：{error}");
            return;
        }
    };
    let _ = draft; // 结束上方草稿可变借用后统一进入共享完整快照提交入口。
    commit_environment_change(
        &mut session,
        &mut editor,
        before,
        after,
        "场景环境草稿已更新；动态天空与 IBL 保存后由正式运行时热加载采用", // 气候身份只读，环境按钮不再修改气候。
    ); // 离散环境按钮与公共数值字段共享完全相同的撤销和修订语义。
}

/// 把F12内存选择同步为天气核心的纯视觉作者覆盖；稳定帧值不变时不触发资源变更。
pub(crate) fn sync_editor_weather_preview(
    editor: Res<SceneEditorState>, // F12显隐只决定天气表现使用作者相机还是玩法相机。
    mut preview: ResMut<WeatherAuthoringPreview>, // 天气数值继续只来自自然状态与运行时调试覆盖。
) {
    let desired = WeatherAuthoringPreview::new(editor.open); // F12关闭后同帧归还玩法主相机锚点。
    if *preview != desired {
        *preview = desired; // 只有真实UI变化或F12显隐变化才发布新预览修订。
    }
}

/// 判断未保存环境草稿是否正属于当前可见的动态玩法场景。
fn can_preview_environment_in_gameplay(
    draft_scene_id: &SceneId,              // 草稿稳定身份，不能只依赖共享场景枚举。
    runtime_scene: &str,               // 固定场景不得被动态清单作者工具越权覆盖。
    environment_dirty: bool,           // 只有真实未保存修改需要取得临时所有权。
    active_dynamic: &ActiveDynamicScene, // 当前玩法动态场景的真实身份。
    current_scene: ScenePhase,          // Loading或固定场景没有可接管目标。
) -> bool {
    environment_dirty // 干净草稿继续服从正式清单热加载。
        && feathrise_scene_content::SceneRuntimeKind::parse(runtime_scene)
            == Some(feathrise_scene_content::SceneRuntimeKind::DataDriven) // 新旧文件共用公共类型语义。
        && current_scene == ScenePhase::Active // 防止切换场景空窗修改旧玩法相机。
        && active_dynamic.scene_id.as_ref() == Some(draft_scene_id) // 草稿和实景必须是同一个场景。
}

/// 把当前草稿环境和专用修订同步到F12相机，并把未保存修改实时覆盖到匹配玩法相机。
pub(crate) fn sync_editor_skybox_requests(
    mut commands: Commands,                // 只在请求真实改变时插入组件。
    session: Res<EditorAuthoringSession>,  // 草稿环境与环境专用修订是唯一输入。
    active_dynamic: Res<ActiveDynamicScene>, // 防止目录高亮或切换场景空窗误改其他场景。
    current_scene: Res<State<ScenePhase>>,  // 只有当前确实处于动态玩法场景才发布实景覆盖。
    editor_cameras: Query<
        (Entity, Option<&AuthoredSkyboxRequest>), // 比较请求完整值，稳定帧不重复写组件。
        With<SceneEditorCamera>,                    // 编辑器固定一台离屏相机。
    >,
    gameplay_cameras: Query<
        (Entity, Option<&AuthoredSkyboxRequest>), // 复用同一天空异步状态机，不复制Image加载器。
        (With<PrimarySceneView>, Without<SceneEditorCamera>), // 排除离屏相机，只接管唯一玩法窗口视图。
    >,
) {
    let environment = session
        .draft
        .as_ref()
        .map(|draft| draft.manifest.environment.clone())
        .unwrap_or_default(); // 切换场景加载空窗明确恢复相机原清屏色。
    let desired = AuthoredSkyboxRequest::new_with_mode(
        authored_sky_mode(environment.sky_mode), // 场景级二选一模式同步覆盖 F12 与匹配玩法相机。
        environment.skybox_path, // 主路径仍相对 assets 根；动态模式派生IBL，天空盒模式还加载可见Cube。
        environment.skybox_brightness, // 天空盒模式使用该日间基准，动态模式无损保留供切换。
        environment.environment_diffuse_intensity, // 人物与地形天空补光拥有独立日间基准。
        environment.environment_specular_intensity, // 水面与光滑材质反射拥有独立日间基准。
        environment.skybox_yaw_degrees, // 离屏和玩法的可见 Cube 与 IBL 保持同一方向。
        session.environment_revision, // 迟到Image事件不能覆盖新草稿。
    );
    if let Ok((entity, existing)) = editor_cameras.single()
        && existing != Some(&desired)
    {
        commands.entity(entity).insert(desired.clone()); // 离屏相机始终回显当前草稿或明确清屏状态。
    }
    let Some(draft) = session.draft.as_ref() else {
        return; // 没有作者草稿时绝不修改玩法相机。
    };
    if !can_preview_environment_in_gameplay(
        &draft.scene_id,                 // 使用真实SceneId校验当前场景。
        &draft.manifest.runtime_scene, // 固定场景保持既有天空职责。
        draft.environment_dirty,       // 保存前才由草稿持有临时权威。
        &active_dynamic,               // 当前动态玩法SceneId必须完全一致。
        *current_scene.get(),          // Loading与其他场景立即拒绝。
    ) {
        return; // 干净草稿或场景不匹配时由正式运行时继续拥有玩法天空。
    }
    if let Ok((entity, existing)) = gameplay_cameras.single()
        && existing != Some(&desired)
    {
        commands.entity(entity).insert(desired); // 设置、清除、亮度和旋转同帧进入游戏实景异步采用链。
    }
}

#[cfg(test)]
mod tests {
    use super::*; // 验证正式纯作者转换而不启动渲染器。
    use feathrise_scene_content::SceneWeatherClimate; // 验证旧场景气候身份不会被天空按钮改写。

    #[test]
    fn selected_ktx2_produces_a_valid_environment_snapshot() {
        let original = SceneEnvironmentDescriptor::default(); // 新场景从动态天空、无离线资源和中性天空盒亮度开始。
        let selected = edited_environment(
            &original,
            EnvironmentEditAction::SetSelectedSkybox,
            Some("textures/test_skybox.ktx2"),
        )
        .expect("正式立方体KTX2路径应可写入草稿");
        assert_eq!(
            selected.skybox_path.as_deref(),
            Some("textures/test_skybox.ktx2")
        );
        assert_eq!(selected.skybox_brightness, original.skybox_brightness); // 选择资源不暗改连续参数。
        assert_eq!(
            selected.environment_diffuse_intensity,
            original.environment_diffuse_intensity
        ); // 选择资源不暗改独立天空补光强度。
        assert_eq!(
            selected.environment_specular_intensity,
            original.environment_specular_intensity
        ); // 选择资源不暗改独立天空反射强度。
        assert_eq!(selected.skybox_yaw_degrees, original.skybox_yaw_degrees); // 方向只由专业字段修改。
        assert_eq!(selected.sky_mode, SceneSkyMode::Dynamic); // 设置资源只补充 IBL，不暗中改变作者明确选择的可见模式。
        let skybox = edited_environment(
            &selected,
            EnvironmentEditAction::SetSkyMode(SceneSkyMode::Skybox),
            None,
        )
        .expect("已有主资源时应允许切换天空盒模式"); // 模式按钮不依赖当前资产浏览器选择。
        assert_eq!(skybox.sky_mode, SceneSkyMode::Skybox); // 完整环境快照保存互斥模式。
        let cleared = edited_environment(&skybox, EnvironmentEditAction::ClearSkybox, None)
            .expect("清除天空资源必须产生合法动态天空环境"); // 清除动作不可留下无主 Cube 的天空盒模式。
        assert_eq!(cleared.sky_mode, SceneSkyMode::Dynamic); // 动态天空立即恢复。
        assert!(cleared.skybox_path.is_none()); // 主 Cube 与两张 IBL 共同身份一并清除。
    }

    #[test]
    fn skybox_mode_is_rejected_until_a_main_resource_is_set() {
        let result = edited_environment(
            &SceneEnvironmentDescriptor::default(), // 新旧场景默认动态天空且没有离线资源。
            EnvironmentEditAction::SetSkyMode(SceneSkyMode::Skybox), // 作者直接点击天空盒按钮。
            None,                                 // 模式动作不从资产浏览器偷偷补路径。
        );
        assert!(result.is_err()); // UI 必须提示先设置主 KTX2，不能保存必然回退的半套配置。
    }

    #[test]
    fn skybox_action_preserves_existing_weather_identity() {
        // 旧地域字段在读取时已经规范化；天空编辑必须继续保留当前中性天气配置。
        let mut original = SceneEnvironmentDescriptor::default();
        original.weather.climate = SceneWeatherClimate::Uniform;
        original.skybox_path = Some("textures/test_skybox.ktx2".to_owned()); // 模拟需要实际清除的已有天空资源。
        let edited = edited_environment(&original, EnvironmentEditAction::ClearSkybox, None)
            .expect("清除天空资源不应改变合法的天气身份"); // 天空动作继续验证整份环境清单。
        assert!(edited.skybox_path.is_none()); // 确认天空动作确实修改环境而不只是返回同值快照。
        assert_eq!(edited.weather, original.weather); // 场景的既有区域与气候数据完整保留。
    }

    #[test]
    fn regular_texture_cannot_be_registered_as_a_skybox() {
        let result = edited_environment(
            &SceneEnvironmentDescriptor::default(),
            EnvironmentEditAction::SetSelectedSkybox,
            Some("textures/clouds.png"),
        );
        assert!(result.is_err()); // 扩展名正确与Cube内容校验分两级完成。
    }

    #[test]
    fn dirty_environment_only_previews_on_the_matching_dynamic_gameplay_scene() {
        let draft_scene = SceneId::parse("MAP-draft").expect("测试草稿SceneId合法"); // 构造当前作者场景身份。
        let other_scene = SceneId::parse("MAP-other").expect("测试其他SceneId合法"); // 构造切换场景误接管反例。
        let matching_active = ActiveDynamicScene {
            scene_id: Some(draft_scene.clone()), // 当前实景与作者草稿完全一致。
        };
        assert!(can_preview_environment_in_gameplay(
            &draft_scene,                 // 当前草稿身份。
            "data_driven",          // 草稿声明统一动态运行时。
            true,                       // 天空设置尚未保存。
            &matching_active,           // 实景正在显示同一场景。
            ScenePhase::Active, // 当前确实处于动态场景场景。
        )); // 真实未保存修改必须立即进入游戏窗口相机。
        assert!(!can_preview_environment_in_gameplay(
            &draft_scene,                 // 身份仍匹配。
            "data_driven",          // 运行时仍正确。
            false,                      // 保存完成后草稿不再拥有实景覆盖权。
            &matching_active,           // 当前场景不变。
            ScenePhase::Active, // 当前场景不变。
        )); // 干净草稿必须归还正式清单热加载。
        let mismatched_active = ActiveDynamicScene {
            scene_id: Some(other_scene), // 模拟切换场景期间实景已经进入另一个场景。
        };
        assert!(!can_preview_environment_in_gameplay(
            &draft_scene,                 // 仍残留上一张图草稿。
            "data_driven",          // 两者虽然共用同一场景枚举。
            true,                       // 草稿也仍有未保存修改。
            &mismatched_active,         // 真实SceneId不同必须拒绝。
            ScenePhase::Active, // 共享枚举不能作为充分身份。
        )); // 旧草稿不得覆盖新场景玩法天空。
    }
}
