//! F12显式“进入场景”与新建完成共用的正式场景切换事务。
//!
//! 请求只保存稳定SceneId；目录热更新和清单异步加载完成后，本模块才写入动态场景身份、
//! 出生点与共享加载事务；F12外壳保持可见，使用编辑器自己的场景加载提示。

use bevy::ecs::system::SystemParam; // 两个作者资源共用切换场景保存判断，并保持浏览器系统参数数量受控。
use bevy::prelude::*; // ECS资源、状态与主窗口查询。
use bevy::window::{PrimaryWindow, Window}; // 切换场景释放旧弹框的中文输入法。
use feathrise_scene_content::{ActiveDynamicScene, SceneContentRegistry, SceneId, SceneManifestAsset}; // 目录强句柄、清单和动态场景稳定身份。
use feathrise_portal::PortalDestination; // 正式场景加载沿用统一出生点资源。
use feathrise_scene_core::state::{ScenePhase, LoadingProgress, PendingSceneTransition}; // 复用就绪门，展示方式由编辑器请求单独指定。

use super::actions::set_ime_enabled; // 切换场景时释放旧新建场景弹框的输入法上下文。
use super::vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState}; // 切换场景返回永久场景页，保留全局预设的已开标签和草稿。
use crate::authoring::{EditorAuthoringSession, GlobalEnvironmentAuthoringState, WaterBrushStroke}; // 切换场景保护区分真实编辑与可重建加载补齐。
use crate::resources::SceneEditorState; // 唯一选中目标和等待切换场景请求。

/// 类型兼容由公共场景格式解释，本地只负责进入正式加载生命周期。
fn manifest_scene(value: &str) -> Option<ScenePhase> {
    feathrise_scene_content::SceneRuntimeKind::parse(value).map(|kind| match kind {
        feathrise_scene_content::SceneRuntimeKind::DataDriven => ScenePhase::Active,
    })
}

/// 同为DynamicSceneRoot时仍必须比较SceneId，防止把旧动态场景误判为已经到达。
fn already_on_target(
    current_scene: ScenePhase,
    target_scene: ScenePhase,
    active_dynamic_scene: Option<&SceneId>,
    requested_scene: &SceneId,
) -> bool {
    current_scene == target_scene
        && (target_scene != ScenePhase::Active || active_dynamic_scene == Some(requested_scene))
}

#[derive(SystemParam)]
pub(crate) struct SceneTransitionAuthoring<'w> {
    pub(crate) session: ResMut<'w, EditorAuthoringSession>,
    pub(crate) global: ResMut<'w, GlobalEnvironmentAuthoringState>,
}

/// 放弃选择不是通用脏位豁免；目标、源场景和两份作者修订必须仍与明确授权完全相同。
fn discard_is_approved(
    editor: &SceneEditorState,
    session: &EditorAuthoringSession,
    global: &GlobalEnvironmentAuthoringState,
) -> bool {
    editor
        .scene_switch_confirmation
        .as_ref()
        .is_some_and(|request| {
            request.stage == crate::types::SceneSwitchStage::DiscardApproved
                && editor.pending_scene_transition.as_ref() == Some(&request.target_scene_id)
                && super::scene_switch_dialog::source_matches(request, editor, session, global)
        })
}

fn transition_became_unsafe(
    editor: &SceneEditorState,
    session: &EditorAuthoringSession,
    global: &GlobalEnvironmentAuthoringState,
) -> bool {
    editor.close_requested
        || (!discard_is_approved(editor, session, global)
            && session.needs_save_before_scene_switch(editor, global)) // 等待期间的新编辑仍作废请求；仅精确匹配的一次放弃授权可通过。
}

/// 等待目录和清单完整可用，再一次性切换场景；工作台持续显示并等待完整就绪。
#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_pending_scene_transition(
    mut commands: Commands,
    registry: Option<Res<SceneContentRegistry>>,
    manifests: Res<Assets<SceneManifestAsset>>,
    current_scene: Res<State<ScenePhase>>,
    mut editor: ResMut<SceneEditorState>,
    mut workspace: ResMut<VegetationPresetEditorState>,
    mut active_dynamic: ResMut<ActiveDynamicScene>,
    mut portal_destination: ResMut<PortalDestination>,
    mut next_scene: ResMut<NextState<ScenePhase>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    water_stroke: Res<WaterBrushStroke>,
    mut authoring: SceneTransitionAuthoring,
    puddles: Option<Res<crate::authoring::PuddleAuthoringState>>, // 切换场景等待同场景整笔后台完成。
    mut documents: super::scene_documents::SceneDocumentTransition,
) {
    if authoring.session.water_preset_delete_pending { return; } // 保留切换场景请求；全局预设文件事务不能跨作者来源提交。
    let preparing = documents
        .documents
        .as_ref()
        .is_some_and(|docs| docs.preparing_target.is_some());
    if !editor.open || (editor.scene_transition_in_progress && !preparing) {
        return; // 关闭编辑器或已有采用中的请求时，迟到清单不得开始另一轮切换场景。
    }
    let Some(scene_id) = editor.pending_scene_transition.clone() else {
        return; // 稳定帧没有请求时不读取目录或遍历资产。
    };
    let target = if let Some(target) = documents
        .documents
        .as_ref()
        .and_then(|docs| docs.preparing_target.as_ref())
    {
        target.clone() // 已验证事务冻结完整小型目标；目录热更不能在后台交接期间偷换目标。
    } else {
        let Some(registry) = registry.as_deref() else {
            return; // 新目录尚未首次加载，保留请求等待资产事件驱动注册表。
        };
        let Some(handles) = registry.handles(&scene_id) else {
            return; // 新目录热更新尚未把私有句柄原子发布到注册表。
        };
        let Some(manifest) = manifests.get(&handles.manifest) else {
            return; // 清单仍在AssetServer异步加载，下一帧只做常数次句柄查询。
        };
        if manifest.id != scene_id {
            editor.pending_scene_transition = None; // 身份错误不能持续重试并污染其他场景。
            editor.scene_switch_confirmation = None; // 目标验证失败不消费放弃，也不留下旧授权。
            editor.status = format!("场景{scene_id}清单身份不一致，已取消自动进入"); // 明确反馈损坏项。
            return;
        }
        let Some(target_scene) = manifest_scene(&manifest.runtime_scene) else {
            editor.pending_scene_transition = None; // 未发布场景需要作者修正文件后重新选择。
            editor.scene_switch_confirmation = None;
            editor.status = format!("场景{scene_id}尚未发布有效运行时场景"); // 禁止无声停留在旧场景。
            return;
        };
        super::scene_documents::SceneDocumentSceneTarget {
            scene_id: scene_id.clone(),
            scene: target_scene,
            name: manifest.display_name.clone(),
            spawn: manifest.default_spawn_vec3(),
        }
    };
    let target_scene = target.scene;
    debug_assert_eq!(target.scene_id, scene_id);
    if authoring.session.vegetation_cross_busy {
        editor.status="等待跨网格植被笔划和完整预览发布后再切换场景".into();return;
    }
    if puddles.as_ref().is_some_and(|state| state.busy()) {
        // 第一笔尚未dirty也不可跨场景。
        editor.status = "等待当前积水笔划完成后再切换场景".into(); // 保留请求，由后续dirty门提示保存。
        return; // 不使后台结果误写目标场景。
    } // 结束积水跨场景保护。
    if water_stroke.active {
        editor.status = "当前水笔刷段正在等待作者源并收口；完成后再继续进入目标场景".to_owned();
        return; // 保留pending_scene_transition，下一帧先由dirty门决定保存，不允许跨场景补画。
    }
    if authoring.session.saving || editor.save_requested {
        editor.status = "等待当前场景保存完成后再切换场景".to_owned();
        return; // 即使只有补登记责任，也不能在文件事务尚未收口时更换当前草稿。
    }
    if documents.documents.is_some() && documents.busy() {
        editor.status = "等待当前场景作者任务与完整笔划收口；未保存修改会保留在原标签".into();
        return; // 不丢弃持有唯一可写源的稀疏任务，也不无限增开后台文档任务。
    }
    if (editor.close_requested && !preparing)
        || (documents.documents.is_none()
            && transition_became_unsafe(&editor, &authoring.session, &authoring.global))
    {
        editor.pending_scene_transition = None;
        editor.scene_switch_confirmation = None;
        editor.status = if editor.dirty {
            "等待进入场景期间当前草稿又产生未保存修改；已取消切换场景，请先保存后重新进入".to_owned()
        } else {
            "编辑器正在关闭；已取消尚未采用的场景切换".to_owned()
        };
        return; // 二次门必须位于任何目标身份、出生点或场景状态写入之前。
    }
    if discard_is_approved(&editor, &authoring.session, &authoring.global) {
        if authoring.global.dirty {
            authoring.global.draft = authoring.global.saved.clone(); // 全局仅含固定六组设置；大场景草稿及历史仍由既有加载采用/后台回收处理。
            authoring.global.dirty = false;
            authoring.global.revision = authoring.global.revision.wrapping_add(1);
            authoring.global.initialized = false; // 同帧同步系统重新采用最新正式基线，包括作者编辑期间的外部热更新。
        }
        editor.dirty = false; // 所有目标验证和忙碌门已通过，从正式切换场景采用点起放弃旧图作者责任。
    }
    editor.scene_switch_confirmation = None; // 授权只能消费一次；下一个场景沿用正常未保存保护。
    let runtime_already_here = already_on_target(
        *current_scene.get(),           // 比较当前正式玩法场景。
        target_scene,                   // 比较清单声明的目标场景。
        active_dynamic.scene_id.as_ref(), // 动态场景还要验证当前实际SceneId。
        &scene_id,                        // 新建或点击请求的稳定SceneId。
    );
    let was_already_here = runtime_already_here
        && (documents.documents.is_none()
            || authoring
                .session
                .draft
                .as_ref()
                .is_none_or(|draft| draft.scene_id == scene_id));
    if !was_already_here && !preparing && documents.documents.is_some() {
        super::scene_documents::park_document(
            &mut documents,
            &mut editor,
            &mut authoring.session,
            &scene_id,
            &mut commands,
        );
        if !runtime_already_here && documents.residency.is_some() {
            let target_scene_id = scene_id.clone();
            commands.queue(move |world: &mut World| {
                feathrise_scene::prepare_resident_scene(world, target_scene_id, target_scene)
            });
        }
        documents.documents.as_mut().unwrap().preparing_target = Some(target);
        editor.scene_transition_in_progress = true;
        editor.status = "正在准备完整场景文档；原标签的修改已暂停保留".into();
        return; // 临时预览与历史全部就绪后才进入LoadingScene，不让磁盘版本提前显示为草稿。
    }
    let warm = documents.residency.as_ref().is_some_and(|scenes| {
        scenes.preparation(&scene_id) == feathrise_scene::ResidentScenePreparation::WarmReady
    });
    if preparing
        && !runtime_already_here
        && let Some(scenes) = documents.residency.as_ref()
    {
        if matches!(
            scenes.preparation(&scene_id),
            feathrise_scene::ResidentScenePreparation::Parking
                | feathrise_scene::ResidentScenePreparation::Restoring
        ) {
            editor.status = "正在恢复完整三维场景、碰撞及渲染资源".into();
            return;
        }
    }
    if let Some(documents) = documents.documents.as_mut() {
        documents.preparing_target = None;
    }
    editor.selected_scene_id = Some(scene_id.clone()); // 目录与清单完整验证后才把目标发布为当前作者场景。
    editor.scene_list_selection_id = Some(scene_id.clone()); // 切换场景期间目录高亮与冻结目标保持一致。
    if target_scene == ScenePhase::Active {
        active_dynamic.select(scene_id.clone()); // LoadingScene进入前写入动态场景唯一身份。
        portal_destination.dynamic_scene_id = Some(scene_id.as_str().to_owned()); // 玩家生成读取同一SceneId。
    } else {
        portal_destination.dynamic_scene_id = None; // 静态专用场景不得继承上一张动态场景身份。
    }
    portal_destination.spawn_pos = (!warm && !runtime_already_here).then_some(target.spawn); // 驻留返回保留原玩家位置，冷图采用清单出生点。
    editor.pending_scene_transition = None; // 完整输入已经采用，本请求只能消费一次。
    workspace.active_tab = PresetWorkspaceTab::Scene;
    workspace.touch_ui(); // 重复打开同SceneId与返回干净标签都更新活动页和底线。
    if runtime_already_here {
        editor.status = format!("已进入场景{}", target.name); // 同图选择无需经过加载页。
        return;
    }
    // 【关闭设计】文档已暂停或场景已交接时，必须完整采用目标再执行关闭；保留交接期间的关闭意图。
    editor.new_scene_dialog_open = false; // 切换场景不得留下覆盖到目标场景的旧弹框。
    editor.scene_transition_in_progress = true; // 保持F12外壳与玩法输入暂停，只锁定尚未就绪的场景操作。
    editor.status = format!("正在载入场景{}", target.name); // 加载页前保留明确目标。
    set_ime_enabled(&mut windows, false); // 新建名称输入法不能泄漏到加载页或玩家控制。
    if documents.residency.is_some() && !runtime_already_here {
        commands.queue(feathrise_scene::arm_resident_scene_transition);
    }
    if warm {
        next_scene.set(target_scene); // 原实体和完整资源已经准备好，跳过重复生成并保留原就绪证据。
        return;
    }
    let transition = PendingSceneTransition::editor_scene(
        target_scene, // 正式加载系统最终进入清单场景。
        target.name,  // 编辑器进度显示作者设置的场景名。
    );
    commands.insert_resource(LoadingProgress::for_transition(&transition)); // 同帧撤下旧场景视口，不把旧世界误当目标已就绪。
    commands.insert_resource(transition);
    next_scene.set(ScenePhase::Loading); // 复用碰撞、资源与动态实体启动门。
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap; // 构造只含刚创建场景的最小目录注册表。

    #[test]
    fn close_during_resident_parking_keeps_the_preparation_request() {
        let (mut app, map) = ready_transition_app();
        app.init_resource::<feathrise_scene::ResidentScenes>()
            .insert_resource(feathrise_scene_core::residency::SceneResidencyActivity {
                enabled: true,
                ..default()
            })
            .insert_resource(crate::ui::scene_documents::SceneDocuments::default());
        feathrise_scene::prepare_resident_scene(app.world_mut(), map.clone(), ScenePhase::Active);
        app.world_mut()
            .resource_mut::<crate::ui::scene_documents::SceneDocuments>()
            .preparing_target = Some(crate::ui::scene_documents::SceneDocumentSceneTarget {
                scene_id: map.clone(),
                scene: ScenePhase::Active,
                name: "准备中的场景".into(),
                spawn: Vec3::ZERO,
            });
        {
            let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
            editor.open = true;
            editor.scene_transition_in_progress = true;
            editor.pending_scene_transition = Some(map.clone());
            editor.request_close(); // 顶栏在交接期间仍允许关闭，不能遗留没有请求的准备事务。
        }
        for _ in 0..4 {
            app.update();
        }
        let editor = app.world().resource::<SceneEditorState>();
        assert!(editor.open && editor.close_requested && editor.scene_transition_in_progress);
        assert_eq!(editor.pending_scene_transition.as_ref(), Some(&map));
        assert!(
            app.world()
                .resource::<crate::ui::scene_documents::SceneDocuments>()
                .preparing_target
                .is_some()
        );
        assert!(app.world().resource::<ActiveDynamicScene>().scene_id.is_none());
        assert!(!app.world().contains_resource::<PendingSceneTransition>());
        assert_eq!(
            app.world()
                .resource::<feathrise_scene::ResidentScenes>()
                .preparation(&map),
            feathrise_scene::ResidentScenePreparation::Parking
        ); // 此夹具尚未推进实体暂停；关闭不能丢弃继续准备所需的唯一请求。
    }

    #[test]
    fn close_during_document_preparation_completes_adoption_and_keeps_close_intent() {
        let (mut app, map) = ready_transition_app();
        app.init_resource::<crate::ui::scene_documents::SceneDocuments>();
        app.world_mut()
            .resource_mut::<crate::ui::scene_documents::SceneDocuments>()
            .preparing_target = Some(crate::ui::scene_documents::SceneDocumentSceneTarget {
                scene_id: map.clone(),
                scene: ScenePhase::Active,
                name: "已准备的场景".into(),
                spawn: Vec3::ZERO,
            });
        {
            let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
            editor.open = true;
            editor.scene_transition_in_progress = true;
            editor.pending_scene_transition = Some(map.clone());
            editor.request_close();
        }
        app.update();
        let editor = app.world().resource::<SceneEditorState>();
        assert!(editor.open && editor.close_requested && editor.scene_transition_in_progress);
        assert!(editor.pending_scene_transition.is_none());
        assert!(
            app.world()
                .resource::<crate::ui::scene_documents::SceneDocuments>()
                .preparing_target
                .is_none()
        );
        assert_eq!(
            app.world().resource::<ActiveDynamicScene>().scene_id.as_ref(),
            Some(&map)
        );
        assert!(app.world().contains_resource::<PendingSceneTransition>());
        app.update();
        assert_eq!(
            *app.world().resource::<State<ScenePhase>>().get(),
            ScenePhase::Loading
        );
        assert!(app.world().resource::<SceneEditorState>().close_requested);
    }

    #[test]
    fn close_before_document_preparation_cancels_only_the_unstarted_request() {
        let (mut app, map) = ready_transition_app();
        app.init_resource::<crate::ui::scene_documents::SceneDocuments>();
        {
            let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
            editor.open = true;
            editor.pending_scene_transition = Some(map);
            editor.request_close();
        }
        app.update();
        let editor = app.world().resource::<SceneEditorState>();
        assert!(editor.close_requested && !editor.scene_transition_in_progress);
        assert!(editor.pending_scene_transition.is_none());
        assert!(
            app.world()
                .resource::<crate::ui::scene_documents::SceneDocuments>()
                .preparing_target
                .is_none()
        );
        assert!(app.world().resource::<ActiveDynamicScene>().scene_id.is_none());
        assert!(!app.world().contains_resource::<PendingSceneTransition>());
    }

    use bevy::asset::AssetPlugin; // 提供真实Assets存储与AssetServer基础资源。
    use bevy::state::app::StatesPlugin; // 最小测试应用显式安装真实状态迁移调度。
    use feathrise_scene_content::{
        SCENE_SCHEMA_VERSION, SceneCatalogAsset, SceneContentHandles, ScenePlacementsAsset,
        ScenePortalsAsset, SceneSurfaceAsset, SceneTerrainDescriptor, SceneTerrainKind, SceneVegetationAsset,
        SceneWaterAsset,
    }; // 集成测试使用真实公共资产类型而不是伪造旁路状态。

    use super::*; // 纯函数测试锁定共享动态场景的SceneId语义。

    #[test]
    fn scene_transition_uses_public_type_parser_for_both_file_generations() {
        for value in ["data_driven", "data_driven"] {
            assert_eq!(manifest_scene(value), Some(ScenePhase::Active));
        }
        for value in ["KaifengScene", "FutureRuntime", ""] {
            assert_eq!(manifest_scene(value), None);
        }
    }

    #[test]
    fn two_dynamic_scenes_are_not_treated_as_the_same_target() {
        let current = SceneId::parse("MAP-current").unwrap(); // 模拟当前已经运行的动态场景。
        let created = SceneId::parse("MAP-created").unwrap(); // 模拟刚创建并请求自动进入的场景。
        assert!(!already_on_target(
            ScenePhase::Active,
            ScenePhase::Active,
            Some(&current),
            &created,
        )); // 虽然枚举相同，但SceneId不同必须重新走LoadingScene。
        assert!(already_on_target(
            ScenePhase::Active,
            ScenePhase::Active,
            Some(&created),
            &created,
        )); // 只有场景和动态SceneId同时一致才允许跳过加载。
    }

    #[test]
    fn delayed_transition_is_blocked_by_new_dirty_or_close_state() {
        let mut editor = SceneEditorState::default();
        let session = EditorAuthoringSession::default();
        let global = GlobalEnvironmentAuthoringState::default();
        assert!(!transition_became_unsafe(&editor, &session, &global));
        editor.dirty = true; // 模拟请求登记后、目标清单就绪前继续编辑旧图。
        assert!(transition_became_unsafe(&editor, &session, &global));
        editor.dirty = false;
        editor.close_requested = true; // F12关闭与目标资产同帧就绪也不能继续切换场景。
        assert!(transition_became_unsafe(&editor, &session, &global));
    }

    #[test]
    fn ready_created_scene_request_enters_loading_scene_once() {
        ready_scene_request_enters_loading_scene(false);
    }

    #[test]
    fn untouched_migrated_scene_can_transition_to_a_second_dynamic_scene() {
        ready_scene_request_enters_loading_scene(true);
    }

    fn ready_transition_app() -> (App, SceneId) {
        let scene_id = SceneId::parse("MAP-created").unwrap(); // 模拟创建事务返回的稳定身份。
        let mut app = App::new(); // 用最小ECS应用验证真实资源写侧。
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), StatesPlugin)) // 提供资产、Update与状态调度。
            .init_state::<ScenePhase>() // 提供当前场景与NextState正式资源。
            .init_resource::<SceneEditorState>() // 提供F12选中和等待请求。
            .init_resource::<EditorAuthoringSession>() // 最终进入门必须读取实际作者修订。
            .init_resource::<GlobalEnvironmentAuthoringState>() // 全局未保存修改继续受保护。
            .init_resource::<VegetationPresetEditorState>() // 正式切换场景同时选中永久场景工作区。
            .init_resource::<WaterBrushStroke>() // 切换场景事务需要确认没有待补水笔刷段。
            .init_resource::<ActiveDynamicScene>() // 提供动态场景唯一身份。
            .init_resource::<PortalDestination>() // 提供统一玩家出生点。
            .init_asset::<SceneManifestAsset>() // 提供清单句柄到资产的正式查询路径。
            .add_systems(Update, drive_pending_scene_transition); // 运行与产品完全相同的切换场景驱动。
        let manifest = app
            .world_mut()
            .resource_mut::<Assets<SceneManifestAsset>>()
            .add(SceneManifestAsset {
                vegetation_paint_groups: Default::default(),
                terrains: None,
                subscenes: Default::default(),
                schema_version: SCENE_SCHEMA_VERSION,  // 使用当前公共数据版本。
                id: scene_id.clone(),                  // 清单身份必须与待进入请求一致。
                display_name: "新建场景".to_owned(), // 状态栏使用作者显示名称。
                notes: String::new(),                // 进入场景测试不依赖可选备注。
                runtime_scene: feathrise_scene_content::SceneRuntimeKind::DataDriven.as_str().to_owned(), // 新建场景使用公共名称。
                world_min: [-512.0, -512.0],         // 提供合法一公里场景范围。
                world_max: [512.0, 512.0],           // 提供合法一公里场景范围。
                default_spawn: [0.0, 2.0, 0.0],      // 玩家在白模地形上方安全出生。
                environment: default(),              // 缺省环境不增加天空盒加载前置条件。
                puddles: None, // 新图及旧图夹具默认没有积水，首次有效落笔才创建。
                terrain: SceneTerrainDescriptor {
                    resolved_stitches:Default::default(),
                    collision_lods: Vec::new(),         // GLB测试不消费稀疏碰撞配置。
                    display_lods: None,                 // GLB测试不消费SparseClipmap显示LOD。
                    kind: SceneTerrainKind::Heightfield,  // 新建场景使用可编辑高度场。
                    collision_mode: Default::default(), // 旧GLB路线不读取Clipmap碰撞选择。
                    player_collision_preload_radius_meters:
                        feathrise_scene_content::DEFAULT_PLAYER_COLLISION_PRELOAD_RADIUS_METERS, // GLB占位值不参与物理。
                    role_collision_preload_radius_meters: None, // GLB不登记稀疏粗级碰撞参数。
                    player_collision_chunk_cells: None,
                    role_collision_chunk_cells: None,
                    player_collision_span_meters: None, // GLB过渡不读取联动设置。
                    role_collision_span_meters: None,   // GLB过渡不读取联动设置。
                    glb_path: "scenes/MAP-created/terrain/terrain.glb".to_owned(), // 保持私有白模路径。
                    height_path: None,         // 动态场景CPU高度由GLB派生。
                    height_texture_path: None, // GPU高度Image同样只存在于内存。
                    resolution: [129, 129],    // 默认一公里场景的受检高度点数量。
                    base_height: -80.0,        // 与创建器默认编码范围一致。
                    height_scale: 160.0,       // 与创建器默认编码范围一致。
                },
                asset_roots: Vec::new(), // 切换场景事务不依赖资产浏览根。
            });
        let mut scenes = BTreeMap::new(); // 注册表按稳定SceneId保存正式私有句柄。
        scenes.insert(
            scene_id.clone(), // 刚创建场景成为唯一可进入条目。
            SceneContentHandles {
                manifest,                                                        // 清单已经完成热加载。
                portals: Handle::<ScenePortalsAsset>::default(), // 本测试不读取传送图内容。
                placements: Handle::<ScenePlacementsAsset>::default(), // 本测试不读取摆放内容。
                surface: None::<Handle<SceneSurfaceAsset>>,      // 切换场景前不要求地表资产完成。
                vegetation: None::<Handle<SceneVegetationAsset>>, // 切换场景前不要求植被资产完成。
                water: None::<Handle<SceneWaterAsset>>,          // 切换场景前不要求水域资产完成。
                characters_path: None,                              // 切换场景测试不要求新角色来源资产。
                boundaries: None::<Handle<feathrise_scene_content::SceneBoundariesAsset>>, // 切换场景测试不要求区域边界资产完成。
            },
        );
        app.insert_resource(SceneContentRegistry {
            catalog: Handle::<SceneCatalogAsset>::default(), // 注册表测试不需要实际目录资产值。
            revision: 1,                                   // 表示创建后的目录已经原子采用一次。
            scenes,                                          // 发布包含新场景的完整句柄表。
            last_error: None,                              // 当前目录没有校验错误。
        });
        (app, scene_id)
    }

    fn ready_scene_request_enters_loading_scene(migrated_current_scene: bool) {
        let (mut app, scene_id) = ready_transition_app();
        if migrated_current_scene {
            let mut draft =
                crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121]));
            draft.water_catalog_registration_required = true;
            let current_scene = draft.scene_id.clone();
            app.insert_resource(EditorAuthoringSession {
                draft: Some(draft),
                loaded_migration_revision: Some(0),
                ..default()
            })
            .insert_resource(State::new(ScenePhase::Active))
            .insert_resource(ActiveDynamicScene {
                scene_id: Some(current_scene.clone()),
            });
            let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
            editor.selected_scene_id = Some(current_scene);
            editor.dirty = true; // 完成第一个场景的自动补齐后，第二次正式进入仍应通过最后一道门。
        }
        {
            let mut editor = app.world_mut().resource_mut::<SceneEditorState>(); // 模拟F12仍处于打开状态。
            editor.open = true; // 等待中的自动进入只在当前作者会话有效时执行。
            editor.queue_scene_transition(scene_id.clone(), "等待自动进入"); // 模拟创建完成结果登记请求。
        }
        app.update(); // 目录和清单齐备时必须在这一帧采用完整过渡。
        let editor = app.world().resource::<SceneEditorState>(); // 读取采用后的编辑器状态。
        assert!(editor.open); // 加载期间工作台必须持续显示。
        assert!(editor.scene_transition_in_progress); // 完整就绪前显示编辑器进度并锁定场景操作。
        assert!(editor.pending_scene_transition.is_none()); // 请求只能消费一次。
        assert_eq!(editor.selected_scene_id.as_ref(), Some(&scene_id)); // 清单验证完成后才正式发布当前作者场景身份。
        assert_eq!(editor.scene_list_selection_id.as_ref(), Some(&scene_id)); // 目录高亮与已经采用的目标一起收口。
        assert_eq!(
            app.world().resource::<ActiveDynamicScene>().scene_id.as_ref(),
            Some(&scene_id)
        ); // LoadingScene开始前必须已写入新场景身份。
        let pending = app.world().resource::<PendingSceneTransition>(); // 检查正式加载事务而非仅UI选中态。
        assert_eq!(pending.target, ScenePhase::Active); // 新场景最终目标必须是统一动态场景。
        assert!(!pending.fresh_world); // 作者切换场景保持驻留重用，不请求重建运行会话。
        assert_eq!(
            pending.presentation(),
            feathrise_scene_core::state::SceneTransitionPresentation::Authoring,
        ); // 编辑器不得走游戏加载展示。
        assert_eq!(
            app.world().resource::<LoadingProgress>().presentation(),
            feathrise_scene_core::state::SceneTransitionPresentation::Authoring,
        ); // 同帧进度使旧场景视口立即失效，而不是等到下一次状态切换。
        assert_eq!(pending.display_name(), "新建场景"); // 加载页必须获得场景清单中的实际名称。
    }

    fn authorize_dirty_transition(app: &mut App, target: SceneId) -> SceneId {
        let mut draft = crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121]));
        draft.environment_dirty = true;
        let source = draft.scene_id.clone();
        app.insert_resource(EditorAuthoringSession {
            draft: Some(draft),
            revision: 4,
            ..default()
        });
        {
            let mut global = app
                .world_mut()
                .resource_mut::<GlobalEnvironmentAuthoringState>();
            global.revision = 6;
            global.dirty = true;
        }
        {
            let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
            editor.open = true;
            editor.dirty = true;
            editor.selected_scene_id = Some(source.clone());
            editor.queue_scene_transition(target.clone(), "等待采用放弃选择");
            editor.scene_switch_confirmation = Some(crate::types::SceneSwitchConfirmation {
                source_scene_id: Some(source.clone()),
                source_revision: 4,
                global_revision: 6,
                target_scene_id: target,
                target_display_name: "新建场景".to_owned(),
                diagnostic: None,
                stage: crate::types::SceneSwitchStage::DiscardApproved,
            });
        }
        source
    }

    #[test]
    fn discard_is_consumed_only_at_valid_target_adoption() {
        let (mut app, target) = ready_transition_app();
        let source = authorize_dirty_transition(&mut app, target.clone());
        app.update();
        let editor = app.world().resource::<SceneEditorState>();
        assert_eq!(editor.selected_scene_id.as_ref(), Some(&target));
        assert!(
            !editor.dirty
                && editor.scene_transition_in_progress
                && editor.scene_switch_confirmation.is_none()
        );
        let global = app.world().resource::<GlobalEnvironmentAuthoringState>();
        assert!(!global.dirty && !global.initialized);
        assert_eq!(global.revision, 7);
        assert_eq!(global.draft, global.saved);
        // 驱动只切换权威身份；整图草稿和历史在后续正常加载采用时后台回收。
        assert_eq!(
            app.world()
                .resource::<EditorAuthoringSession>()
                .draft
                .as_ref()
                .unwrap()
                .scene_id,
            source
        );
    }

    #[test]
    fn invalid_target_preserves_scene_and_global_drafts() {
        let (mut app, target) = ready_transition_app();
        let source = authorize_dirty_transition(&mut app, target.clone());
        let manifest = app
            .world()
            .resource::<SceneContentRegistry>()
            .handles(&target)
            .unwrap()
            .manifest
            .clone();
        app.world_mut()
            .resource_mut::<Assets<SceneManifestAsset>>()
            .get_mut(&manifest)
            .unwrap()
            .runtime_scene = "未发布场景".to_owned();
        app.update();
        let editor = app.world().resource::<SceneEditorState>();
        assert_eq!(editor.selected_scene_id.as_ref(), Some(&source));
        assert!(
            editor.dirty
                && editor.pending_scene_transition.is_none()
                && editor.scene_switch_confirmation.is_none()
        );
        assert!(
            app.world()
                .resource::<GlobalEnvironmentAuthoringState>()
                .dirty
        );
        assert_eq!(
            app.world()
                .resource::<GlobalEnvironmentAuthoringState>()
                .revision,
            6
        );
        assert!(
            app.world()
                .get_resource::<PendingSceneTransition>()
                .is_none()
        );
    }

    #[test]
    fn discard_cannot_drop_edits_that_arrived_after_authorization() {
        let (mut app, target) = ready_transition_app();
        let source = authorize_dirty_transition(&mut app, target);
        app.world_mut()
            .resource_mut::<EditorAuthoringSession>()
            .revision += 1;
        app.update();
        let editor = app.world().resource::<SceneEditorState>();
        assert_eq!(editor.selected_scene_id.as_ref(), Some(&source));
        assert!(editor.dirty && editor.pending_scene_transition.is_none());
        assert!(
            app.world()
                .resource::<GlobalEnvironmentAuthoringState>()
                .dirty
        );
    }

    #[test]
    fn discard_waits_for_active_brush_before_consuming_any_modification() {
        let (mut app, target) = ready_transition_app();
        let source = authorize_dirty_transition(&mut app, target.clone());
        app.world_mut().resource_mut::<WaterBrushStroke>().active = true;
        app.update();
        assert_eq!(
            app.world()
                .resource::<SceneEditorState>()
                .selected_scene_id
                .as_ref(),
            Some(&source)
        );
        assert!(app.world().resource::<SceneEditorState>().dirty);
        assert!(
            app.world()
                .resource::<GlobalEnvironmentAuthoringState>()
                .dirty
        );
        app.world_mut().resource_mut::<WaterBrushStroke>().active = false;
        app.update();
        assert_eq!(
            app.world()
                .resource::<SceneEditorState>()
                .selected_scene_id
                .as_ref(),
            Some(&target)
        );
    }
}
