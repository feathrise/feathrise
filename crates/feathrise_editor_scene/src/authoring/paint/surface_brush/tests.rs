//! 真实地表笔刷与后台轮询的跨帧回归；不创建窗口、渲染世界或磁盘场景。

use super::*; // 子测试直接安装正式输入系统并检查私有笔划来源门。
use crate::authoring::{poll_surface_layer_edit, start_surface_layer_delete}; // 删层、物化和补画走正式single-flight系统。
use feathrise_scene_content::{SCENE_SURFACE_SCHEMA_VERSION, SceneSurfaceAsset, SceneSurfaceLayer}; // 地表夹具沿用正式作者格式。
use std::time::{Duration, Instant}; // 仅测试使用有超时的让出等待，运行时仍只check_ready。

fn update_until_idle(app: &mut App) {
    // 等待真实后台池完成小型输入，并逐帧执行原子轮询和笔刷。
    let deadline = Instant::now() + Duration::from_secs(5); // 测试线程不能在错误生命周期下永久等待。
    loop {
        // 同步等待严格只存在于测试夹具。
        app.update(); // 每轮仍运行完整ECS系统和非阻塞任务轮询。
        if !app.world().resource::<SurfaceLayerEditTask>().is_pending()
            && !app.world().resource::<SurfaceBrushStroke>().active
        {
            // 两个门同时完成才证明笔划已经形成历史。
            return; // 当前事务已经完全收口。
        } // 完成状态检查结束。
        assert!(
            Instant::now() < deadline,
            "小型地表任务应在五秒内完成并收口"
        ); // 超时暴露死循环或被清掉的输入意图。
        std::thread::sleep(Duration::from_millis(1)); // 测试线程让出CPU给真实AsyncComputeTaskPool。
    } // 有界测试等待结束。
} // 后台系统测试等待器结束。

fn deleted_surface_app() -> App {
    // 真正删除一次中间层，让历史持续持有当前像素Arc。
    let mut app = App::new(); // 独立ECS实例避免依赖游戏窗口。
    app.add_plugins(MinimalPlugins); // 安装真实任务池、时间与Update生命周期。
    app.init_resource::<ButtonInput<MouseButton>>(); // 测试手工注入左键按下和松开边沿。
    app.init_resource::<ButtonInput<KeyCode>>(); // 默认没有Shift，测试普通绘制覆盖。
    app.init_resource::<SurfaceBrushStroke>(); // 待补样本和局部历史使用正式资源。
    app.init_resource::<SurfaceStrokeTrail>(); // 正式笔刷同帧发布有界辅助路径。
    app.add_systems(
        Update,
        (poll_surface_layer_edit, apply_surface_brush).chain(),
    ); // 每帧先采用后台完整结果，再补画冻结笔划。
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121])); // 有效地形场景范围为0..10米。
    draft.surface = Some(SceneSurfaceAsset {
        // 三层3×3作者图足以触发真实局部像素绘制。
        schema_version: SCENE_SURFACE_SCHEMA_VERSION, // 使用正式地表版本。
        scene_id: draft.scene_id.clone(),               // 地表和当前草稿保持同一个稳定场景身份。
        weight_source_path: "scenes/EDGE-TEST/surface/weights.rgba8".to_owned(), // 只作为合法作者元数据，不执行IO。
        weight_texture_path: "scenes/EDGE-TEST/surface/weights.ktx2".to_owned(), // 正式权重派生路径保持完整。
        index_source_path: "scenes/EDGE-TEST/surface/indices.rgba32".to_owned(), // 正式索引作者路径保持完整。
        index_texture_path: "scenes/EDGE-TEST/surface/indices.ktx2".to_owned(), // 正式索引派生路径保持完整。
        resolution: [3, 3], // 中心纹素对应世界坐标5,5，半径2只影响该中心。
        layers: (0..3)
            .map(|index| SceneSurfaceLayer {
                // 局部ID和预设ID分别检验作者选择和压缩行为。
                id: format!("layer-{index}"), // 删除中间layer-1后layer-2成为局部索引1。
                label: format!("作者层{index}"), // 中文名称在删除状态和历史中保留。
                preset_id: format!("preset-{index}"), // 既有场景局部引用不需要新建目录项。
            })
            .collect(), // 不把UI槽位数量当作内容容量。
    }); // 完整地表定义结束。
    draft.surface_weights = Some(Arc::new([255, 0, 0, 0].repeat(9))); // 初始所有像素由保底层零完全覆盖。
    draft.surface_indices = Some(Arc::new(vec![0; 9 * 16])); // 四槽均为合法局部零索引。
    let mut editor = SceneEditorState {
        // 模拟已打开的中央地表视口。
        open: true,                                  // 笔刷和后台采用要求F12仍打开。
        selected_scene_id: Some(draft.scene_id.clone()), // UI场景和作者草稿严格匹配。
        tool: EditorTool::Surface,                   // 输入系统允许地表落笔。
        selected_surface_layer: 1,                   // 首次操作删除原中间层。
        brush_cursor_world: Some(Vec3::new(5.0, 0.0, 5.0)), // 中心纹素的实际世界命中。
        brush_radius: 2.0,                           // 只写中心纹素，便于精确验证短按完整强度。
        brush_strength: 0.35, // 快速单击应产生约89权重，不能退化为等待后一帧delta。
        ..Default::default()  // 其余生命周期请求保持未设置。
    }; // 编辑器输入上下文结束。
    let session = EditorAuthoringSession {
        draft: Some(draft),
        ..Default::default()
    }; // 初始历史为空，删除后必须只有一条结构命令。
    let mut task = SurfaceLayerEditTask::default(); // 删除和后续写侧物化共享同一个任务资源。
    start_surface_layer_delete(&mut editor, &session, &mut task, 1, "layer-1")
        .expect("当前中间作者层可以删除"); // 使用正式入口启动真实CPU重映射。
    app.insert_resource(editor); // 安装同一个可见场景选择与状态。
    app.insert_resource(session); // 安装删除开始时冻结的权威草稿。
    app.insert_resource(task); // 保留真实在途任务直到实际完成。
    app.world_mut().spawn((
        // 最小视口标记只供hover门读取，不需要任何真实窗口。
        SceneEditorViewport, // 正式笔刷查询依赖此组件身份。
        RelativeCursorPosition {
            cursor_over: true,
            normalized: Some(Vec2::splat(0.5)),
        }, // 模拟中央视口内的合法左键起点。
    )); // 视口测试实体完成。
    update_until_idle(&mut app); // 正式poll完整采用删除结果并登记历史。
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .surface_selected_preset_id = None; // 此回归只使用已存在场景局部层，独立于全局目录懒加入测试。
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().undo.len(),
        1
    ); // Arc共享必须来自真正的删除历史。
    let weights = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .expect("草稿存在")
        .surface_weights
        .as_ref()
        .expect("权重存在"); // 读取当前权重所有权。
    assert!(Arc::strong_count(weights) > 1); // 下一笔一定经过后台独立写侧门，不能误测只读者已释放的路径。
    app // 后续测试直接注入真实鼠标边沿。
} // 真实删层后笔刷夹具结束。

fn clipmap_surface_app() -> App {
    let mut app = deleted_surface_app();
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .draft
        .as_mut()
        .unwrap()
        .manifest
        .terrain
        .kind = feathrise_scene_content::SceneTerrainKind::SparseClipmap;
    app.init_resource::<VegetationGroundCursor>()
        .init_resource::<feathrise_scene::DynamicClipmapGroundSurface>();
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .brush_cursor_world = Some(Vec3::new(5., 400., 5.));
    app // 故意保留通用旧点，但没有正式已采用曲面。
}

#[test]
fn clipmap_surface_controls_and_observation_views_cannot_arm_a_waiting_press() {
    use crate::viewport::modes::ViewportModeState;
    for (view, controls) in [
        (EditorView::Perspective, true),
        (EditorView::Front, false),
        (EditorView::Side, false),
    ] {
        let mut app = clipmap_surface_app();
        app.init_resource::<ViewportModeState>();
        app.world_mut()
            .resource_mut::<ViewportModeState>()
            .pointer_over_controls = controls;
        app.world_mut().resource_mut::<SceneEditorState>().view = view;
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        assert!(
            app.world()
                .resource::<SurfaceBrushStroke>()
                .pending_ground
                .is_none()
        );
        assert!(!app.world().resource::<SurfaceBrushStroke>().active);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        app.world_mut()
            .resource_mut::<ViewportModeState>()
            .pointer_over_controls = false;
        app.world_mut().resource_mut::<SceneEditorState>().view = EditorView::Top;
        app.update(); // 按住移出控件/切回顶视不能继承一笔；需要新的实际按下。
        assert!(
            app.world()
                .resource::<SurfaceBrushStroke>()
                .pending_ground
                .is_none()
        );
        {
            let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
            mouse.release(MouseButton::Left);
            mouse.clear();
            mouse.press(MouseButton::Left);
        }
        app.update();
        assert!(
            app.world()
                .resource::<SurfaceBrushStroke>()
                .pending_ground
                .is_some()
        );
    }
}

#[test]
fn clipmap_surface_waits_without_writing_fallback_and_release_cancels_start() {
    let mut app = clipmap_surface_app();
    let session = app.world().resource::<EditorAuthoringSession>();
    let revision = session.revision;
    let weights = session
        .draft
        .as_ref()
        .unwrap()
        .surface_weights
        .clone()
        .unwrap();
    let indices = session
        .draft
        .as_ref()
        .unwrap()
        .surface_indices
        .clone()
        .unwrap();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    assert!(
        app.world()
            .resource::<SurfaceBrushStroke>()
            .pending_ground
            .is_some()
    );
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    for _ in 0..3 {
        app.update();
    }
    let stroke = app.world().resource::<SurfaceBrushStroke>();
    assert!(!stroke.active && stroke.pending_sample.is_none());
    assert!(!app.world().resource::<SurfaceLayerEditTask>().is_pending());
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.revision, revision);
    assert_eq!(session.undo.len(), 1); // 只有夹具原先的删层命令，没有隐式新层或空笔。
    assert!(Arc::ptr_eq(
        &weights,
        session
            .draft
            .as_ref()
            .unwrap()
            .surface_weights
            .as_ref()
            .unwrap()
    ));
    assert!(Arc::ptr_eq(
        &indices,
        session
            .draft
            .as_ref()
            .unwrap()
            .surface_indices
            .as_ref()
            .unwrap()
    ));
    assert!(!app.world().resource::<SurfaceStrokeTrail>().has_segments());
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.update();
    assert!(
        app.world()
            .resource::<SurfaceBrushStroke>()
            .pending_ground
            .is_none()
    );
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().undo.len(),
        1
    );
}

#[test]
fn clipmap_surface_waiting_start_cannot_cross_layer_or_preset_selection() {
    let mut app = clipmap_surface_app();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_surface_layer = 0;
    app.update();
    assert!(
        app.world()
            .resource::<SurfaceBrushStroke>()
            .pending_ground
            .is_none()
    );
    {
        let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        mouse.release(MouseButton::Left);
        mouse.clear();
        mouse.press(MouseButton::Left);
    }
    app.update();
    assert!(
        matches!(app.world().resource::<SurfaceBrushStroke>().pending_ground.as_ref(),
        Some((_, GroundCursorTarget::Surface { layer_id, .. }, _, _)) if layer_id == "layer-0")
    );
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .surface_selected_preset_id = Some("other-preset".into());
    app.update();
    assert!(
        app.world()
            .resource::<SurfaceBrushStroke>()
            .pending_ground
            .is_none()
    );
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().undo.len(),
        1
    );
}

#[test]
fn accepted_surface_sample_waits_for_height_identity_then_cancels_on_author_change() {
    let mut app = deleted_surface_app();
    press_and_release_same_frame(&mut app); // 先用生产入口冻结一份真实样本和后台写侧任务。
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .draft
        .as_mut()
        .unwrap()
        .manifest
        .terrain
        .kind = feathrise_scene_content::SceneTerrainKind::SparseClipmap;
    {
        let mut stroke = app.world_mut().resource_mut::<SurfaceBrushStroke>();
        stroke.ground_target = Some(GroundCursorTarget::Surface {
            layer_id: "layer-2".into(),
            preset_id: None,
        });
        stroke.ground_content = Some([1; 7]); // 模拟原来已受理的Clipmap作者高度内容。
    }
    app.init_resource::<VegetationGroundCursor>()
        .init_resource::<feathrise_scene::DynamicClipmapGroundSurface>();
    app.update();
    assert!(app.world().resource::<SurfaceBrushStroke>().active);
    assert!(
        app.world()
            .resource::<SurfaceBrushStroke>()
            .pending_sample
            .is_some()
    ); // 暂无快照只暂停，不能冒充换代。
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.terrain_revision += 1;
        session.revision += 1; // 正式作者高度变更使原样本明确失效。
    }
    update_until_idle(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.undo.len(), 1);
    assert_eq!(
        session
            .draft
            .as_ref()
            .unwrap()
            .surface_weights
            .as_ref()
            .unwrap()
            .as_slice(),
        [255, 0, 0, 0].repeat(9)
    );
    assert!(!app.world().resource::<SurfaceStrokeTrail>().has_segments()); // 取消样本不制造伪笔迹。
}

fn press_and_release_same_frame(app: &mut App) {
    // Bevy允许极短点击在同一Update只留下just_pressed和just_released。
    {
        // 限制可变输入借用范围，随后才执行系统。
        let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>(); // 操作正式输入资源。
        mouse.press(MouseButton::Left); // 当前帧生成一次按下边沿。
        mouse.release(MouseButton::Left); // pressed已经为false，但单击意图仍必须保留。
    } // 鼠标输入借用结束。
    app.update(); // 真实输入系统冻结完整0.35强度样本并开始后台复制。
    assert!(app.world().resource::<SurfaceBrushStroke>().active); // 当前笔已经受理，不能在共享等待时形成空笔。
    assert!(
        app.world()
            .resource::<SurfaceBrushStroke>()
            .pending_sample
            .is_some()
    ); // 原落点在松键后仍然被保存。
    assert!(app.world().resource::<SurfaceLayerEditTask>().is_pending()); // 整图写侧由真实异步任务承担。
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear(); // 后续帧没有重复输入边沿，也没有持续按住。
} // 同帧快速点击注入结束。

fn assert_completed_click(app: &App) {
    // 确认最终画面输入和唯一笔划历史一致。
    let session = app.world().resource::<EditorAuthoringSession>(); // 读取完成后的权威草稿和统一历史。
    assert_eq!(session.undo.len(), 2); // 先前删除一条，加上本次完整单击一条，不允许拆分物化历史。
    assert!(
        matches!(session.undo.last(), Some(EditorCommand::SurfaceChanged { weight_changes, index_changes }) if !weight_changes.is_empty() && !index_changes.is_empty())
    ); // 索引和权重作为一笔同时改变。
    let draft = session.draft.as_ref().expect("草稿存在"); // 所有查询都使用同一最终作者版本。
    let weights = draft.surface_weights.as_ref().expect("权重存在"); // 中心像素必须得到按下时完整强度。
    let indices = draft.surface_indices.as_ref().expect("索引存在"); // 新目标层局部索引随权重配对写入。
    let selected_weight = (0..4)
        .find_map(|slot| {
            // 四槽固定数量，只查中心纹素。
            let offset = 4 * 16 + slot * 4; // 中心纹素下标4，每槽小端u32。
            let index =
                u32::from_le_bytes(indices[offset..offset + 4].try_into().expect("完整u32索引")); // 使用真实文件协议解码身份。
            (index == 1).then_some(weights[4 * 4 + slot]) // 删除后的选中layer-2位于局部索引1。
        })
        .expect("完成笔划后中心纹素必须含有选中作者层"); // 不能只推进状态而漏掉实际绘制。
    assert!(
        selected_weight >= 80,
        "等待后必须应用原0.35强度单击，实际为{selected_weight}"
    ); // 验证不会退化成物化完成帧delta。
    assert!(!app.world().resource::<SurfaceBrushStroke>().active); // 原笔最后形成历史并归还保存关闭门。
    assert!(app.world().resource::<SurfaceStrokeTrail>().has_segments()); // 同帧收口不会吃掉即时反馈，轨迹等待真实PBR采用。
} // 完整短按采用验证结束。

#[test] // 历史长期持有Arc时，单击松键仍必须在后台准备后完整补画。
fn surface_shared_history_quick_click_releases_then_finishes_in_real_systems() {
    // 直接覆盖apply_surface_brush与poll_surface_layer_edit串联路径。
    let mut app = deleted_surface_app(); // 当前图像由真实删除历史共享。
    press_and_release_same_frame(&mut app); // 第一帧留下pending和release_requested。
    update_until_idle(&mut app); // 之后没有新点击，仍应自动补完。
    assert_completed_click(&app); // 精确检查像素强度、身份和一条联合历史。
} // 真实跨帧快速单击回归结束。

#[test] // 等待物化时请求保存和关闭，已受理单击应先完成再交还上层门。
fn surface_pending_click_completes_before_waiting_close_and_save_continue() {
    // 保存/关闭意图不应被当作取消有效输入。
    let mut app = deleted_surface_app(); // 创建真实历史共享状态。
    press_and_release_same_frame(&mut app); // 先受理正在后台等待的完整样本。
    {
        // 同时登记用户要求的后续生命周期动作。
        let mut editor = app.world_mut().resource_mut::<SceneEditorState>(); // 共享原F12作者上下文。
        editor.close_requested = true; // 上层关闭门需要等待active归零。
        editor.save_requested = true; // 上层保存门需要等待最后一笔形成完整历史。
    } // 请求登记结束。
    update_until_idle(&mut app); // 正式poll采用独立写侧，正式输入系统补画并收口。
    assert_completed_click(&app); // 保存关闭意图不能导致漏画或空历史。
    let editor = app.world().resource::<SceneEditorState>(); // 当前请求应该保留供真实上层系统继续。
    assert!(editor.open && editor.close_requested && editor.save_requested); // 补画成功不取消或消费用户的后续请求。
} // 真实保存关闭衔接回归结束。

#[test] // 同图重新加载的地表不能接收旧笔划，旧before字节也不能混入新历史。
fn surface_pending_click_rejects_same_scene_new_surface_without_mixed_undo() {
    // 冻结完整样本后注入同图作者换代。
    let mut app = deleted_surface_app(); // 删除历史为写侧共享提供真实原因。
    press_and_release_same_frame(&mut app); // 当前旧段正在后台准备，尚未修改中心像素。
    {
        // 模拟同图作者源被完整新版本替换。
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>(); // 重载拥有同一个草稿资源。
        session.revision += 1; // 完整作者换代。
        session.surface_revision += 1; // 地表本身也换代，禁止收口旧before差分。
        let draft = session.draft.as_mut().expect("草稿存在"); // 保持SceneId和层ID不变以覆盖同图危险边界。
        draft.surface_weights = Some(Arc::new([255, 0, 0, 0].repeat(9))); // 新权威像素与旧快照有不同Arc身份。
        draft.surface_indices = Some(Arc::new(vec![0; 9 * 16])); // 索引同次换成完整新源。
    } // 同图重载模拟结束。
    app.world_mut()
        .resource_mut::<SurfaceBrushStroke>()
        .before_weights
        .insert(0, 99); // 明确存在旧before记录，错误收口将制造污染命令。
    update_until_idle(&mut app); // 旧复制结果拒收，输入系统丢弃旧笔状态。
    let session = app.world().resource::<EditorAuthoringSession>(); // 新权威草稿和原历史必须保留。
    assert_eq!(session.undo.len(), 1); // 没有使用新像素构造旧before混合撤销。
    assert_eq!(
        session
            .draft
            .as_ref()
            .expect("草稿存在")
            .surface_weights
            .as_ref()
            .expect("权重存在")
            .as_slice(),
        [255, 0, 0, 0].repeat(9)
    ); // 新地表字节完全没有接受旧笔。
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .status
            .contains("停止旧段")
    ); // 作者可见同图换代已取消待补输入。
} // 同图新地表拒收回归结束。

#[test]
fn surface_long_drag_advances_a_bounded_prefix_without_skipping_the_middle_and_undoes_once() {
    let mut app = deleted_surface_app();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        draft.manifest.world_min = [0.0, 0.0];
        draft.manifest.world_max = [200.0, 10.0];
        draft.surface.as_mut().unwrap().resolution = [401, 21];
        draft.surface_weights = Some(Arc::new(vec![0; 401 * 21 * 4]));
        draft.surface_indices = Some(Arc::new(vec![0; 401 * 21 * 16]));
        session.undo.clear();
    }
    let mut stroke = SurfaceBrushStroke { active: true, layer_index: 1, ..Default::default() };
    let mut task = SurfaceLayerEditTask::default();
    let mut trail = SurfaceStrokeTrail::default();
    let mut pending = Some(PendingSurfaceBrushSample {
        from: Vec3::new(1.0, 0.0, 5.0), to: Vec3::new(199.0, 0.0, 5.0),
        radius: 0.75, amount: 0.4, erase: false,
    });
    let mut frames = 0;
    app.world_mut().resource_scope(|world, mut editor: Mut<SceneEditorState>| {
        let mut session = world.resource_mut::<EditorAuthoringSession>();
        while let Some(sample) = pending.take() {
            frames += 1;
            let SurfaceSampleProgress::Applied(remaining) = apply_sample(&mut editor, &mut session,
                &mut stroke, &mut task, sample, &mut trail).unwrap() else { panic!("独占画布不应触发后台复制"); };
            let endpoint = stroke.last_world.unwrap();
            assert!(endpoint.distance(sample.from) <= 32.0 * 0.75 * 0.35 + 0.001,
                "每帧仍最多32个原密度刷印，不能放大局部输入预算");
            if frames == 1 {
                assert!(remaining.is_some());
                let pixels = session.draft.as_ref().unwrap().surface_weights.as_ref().unwrap();
                assert_eq!(&pixels[(10 * 401 + 200) * 4..(10 * 401 + 201) * 4], &[0; 4],
                    "尚未推进到的中段不能跳跃到远端圆印");
            }
            pending = remaining;
            assert!(frames < 100, "测试路径必须在有限预算轮次内完成");
        }
        assert!(frames > 1);
        let pixels = session.draft.as_ref().unwrap().surface_weights.as_ref().unwrap();
        for x in 2..=398 {
            assert!(pixels[(10 * 401 + x) * 4..(10 * 401 + x + 1) * 4].iter().any(|value| *value > 0),
                "200米长段的第{x}列不得因为32印预算留下断口");
        }
        finish_surface_stroke(&mut editor, &mut session, &mut stroke);
        assert_eq!(session.undo.len(), 1);
        let Some(EditorCommand::SurfaceChanged { weight_changes, index_changes }) = session.undo.pop() else { panic!("应产生唯一地表命令"); };
        let draft = session.draft.as_mut().unwrap();
        for change in weight_changes { Arc::get_mut(draft.surface_weights.as_mut().unwrap()).unwrap()[change.index] = change.before; }
        for change in index_changes { Arc::get_mut(draft.surface_indices.as_mut().unwrap()).unwrap()[change.index] = change.before; }
        assert!(draft.surface_weights.as_ref().unwrap().iter().all(|value| *value == 0), "一次撤销恢复整段原空白画布");
        assert!(draft.surface_indices.as_ref().unwrap().iter().all(|value| *value == 0));
    });
}

#[path = "tail_tests.rs"]
mod tail_tests;
