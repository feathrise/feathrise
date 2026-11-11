//! 地表待补折线、静止曝光与固定队列背压回归；真实输入系统排空后检验配对作者像素和唯一撤销。

use super::*;
use bevy::time::TimeUpdateStrategy;

const SIDE: usize = 401;
const A: Vec3 = Vec3::new(5.0, 0.0, 5.0);
const B: Vec3 = Vec3::new(195.0, 0.0, 5.0);
const C: Vec3 = Vec3::new(195.0, 0.0, 195.0);
const D: Vec3 = Vec3::new(5.0, 0.0, 195.0);

fn dense_app() -> App {
    let mut app = deleted_surface_app();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        20,
    )));
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        draft.manifest.world_min = [0.0; 2];
        draft.manifest.world_max = [200.0; 2];
        draft.surface.as_mut().unwrap().resolution = [SIDE as u32; 2];
        draft.surface_weights = Some(Arc::new(vec![0; SIDE * SIDE * 4]));
        draft.surface_indices = Some(Arc::new(vec![0; SIDE * SIDE * 16]));
        session.undo.clear();
    }
    {
        let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
        editor.brush_cursor_world = Some(A);
        editor.brush_radius = 0.75;
    }
    app
}

fn step_at(app: &mut App, point: Vec3) {
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .brush_cursor_world = Some(point);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
}

fn begin_long_drag(app: &mut App) {
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    step_at(app, A);
    assert!(
        app.world()
            .resource::<SurfaceBrushStroke>()
            .pending_sample
            .is_none()
    );
    step_at(app, B);
    assert!(
        app.world()
            .resource::<SurfaceBrushStroke>()
            .pending_sample
            .is_some()
    );
}

fn layer_weight(app: &App, point: Vec3) -> u8 {
    let draft = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap();
    let texel = (point.z * 2.0).round() as usize * SIDE + (point.x * 2.0).round() as usize;
    let weights = draft.surface_weights.as_ref().unwrap();
    let indices = draft.surface_indices.as_ref().unwrap();
    (0..4)
        .filter_map(|slot| {
            let offset = texel * 16 + slot * 4;
            (u32::from_le_bytes(indices[offset..offset + 4].try_into().unwrap()) == 1)
                .then_some(weights[texel * 4 + slot])
        })
        .sum()
}

fn finish_requested(app: &mut App) -> usize {
    {
        let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
        editor.close_requested = true;
        editor.save_requested = true;
        editor.brush_cursor_world = Some(Vec3::new(100.0, 0.0, 100.0));
    }
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    let mut frames = 0;
    while app.world().resource::<SurfaceBrushStroke>().active {
        let sample = app
            .world()
            .resource::<SurfaceBrushStroke>()
            .pending_sample
            .unwrap();
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        frames += 1;
        let stroke = app.world().resource::<SurfaceBrushStroke>();
        if stroke.active {
            assert!(
                stroke.last_world.unwrap().distance(sample.from) <= 32.0 * 0.75 * 0.35 + 0.001,
                "排空仍每帧只推进32个原密度刷印，不能为关闭/保存增加帧预算"
            );
            assert!(
                app.world()
                    .resource::<EditorAuthoringSession>()
                    .undo
                    .is_empty(),
                "全部待补折线完成前不形成提前历史，也不解除active保存/关闭门"
            );
        }
        assert!(frames < 150, "三条长段必须按有界前缀在有限帧内排空");
    }
    assert!(frames > 1);
    let editor = app.world().resource::<SceneEditorState>();
    assert!(editor.open && editor.save_requested && editor.close_requested);
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().undo.len(),
        1
    );
    frames
}

#[test]
fn surface_queued_polyline_drains_before_save_close_without_chords_and_undoes_once() {
    let mut app = dense_app();
    begin_long_drag(&mut app);
    step_at(&mut app, C);
    step_at(&mut app, D);
    let stroke = app.world().resource::<SurfaceBrushStroke>();
    assert_eq!(stroke.queued_samples.len(), 2);
    assert_eq!(
        (stroke.queued_samples[0].from, stroke.queued_samples[0].to),
        (B, C)
    );
    assert_eq!(
        (stroke.queued_samples[1].from, stroke.queued_samples[1].to),
        (C, D)
    );
    finish_requested(&mut app);
    for coordinate in (12..=388).step_by(2) {
        let meters = coordinate as f32 * 0.5;
        for point in [
            Vec3::new(meters, 0.0, 5.0),
            Vec3::new(195.0, 0.0, meters),
            Vec3::new(meters, 0.0, 195.0),
        ] {
            assert!(
                layer_weight(&app, point) > 0,
                "真实折线不得缺少已受理点 {point:?}"
            );
        }
    }
    assert_eq!(
        layer_weight(&app, Vec3::new(100.0, 0.0, 100.0)),
        0,
        "B→C→D不能压缩为B→D斜弦，保存/关闭后的新位置也不能补写"
    );
    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
    let Some(EditorCommand::SurfaceChanged {
        weight_changes,
        index_changes,
    }) = session.undo.pop()
    else {
        panic!("整条折线必须形成一条配对地表撤销");
    };
    let draft = session.draft.as_mut().unwrap();
    for change in &weight_changes {
        Arc::get_mut(draft.surface_weights.as_mut().unwrap()).unwrap()[change.index] =
            change.before;
    }
    for change in &index_changes {
        Arc::get_mut(draft.surface_indices.as_mut().unwrap()).unwrap()[change.index] =
            change.before;
    }
    assert!(
        draft
            .surface_weights
            .as_ref()
            .unwrap()
            .iter()
            .all(|value| *value == 0)
    );
    assert!(
        draft
            .surface_indices
            .as_ref()
            .unwrap()
            .iter()
            .all(|value| *value == 0)
    );
    for change in weight_changes {
        Arc::get_mut(draft.surface_weights.as_mut().unwrap()).unwrap()[change.index] = change.after;
    }
    for change in index_changes {
        Arc::get_mut(draft.surface_indices.as_mut().unwrap()).unwrap()[change.index] = change.after;
    }
    drop(session);
    assert!(layer_weight(&app, C) > 0, "重做同一条完整命令恢复拐角");
}

#[test]
fn surface_queued_stationary_exposure_only_deepens_the_stationary_corner() {
    let mut control = dense_app();
    let mut held = dense_app();
    for (app, frames) in [(&mut control, 0), (&mut held, 6)] {
        // 空白像素首次绘制会把唯一目标材质规范化为255，不能用来比较曝光深浅。
        // 先铺已存在的背景层0，目标层1仍为零；真实笔刷随后渐进替换背景贡献。
        app.world_mut().resource_mut::<EditorAuthoringSession>()
            .draft.as_mut().unwrap().surface_weights = Some(Arc::new([255, 0, 0, 0].repeat(SIDE * SIDE)));
        begin_long_drag(app);
        step_at(app, C);
        let moving_amount = app.world().resource::<SurfaceBrushStroke>().queued_samples[0].amount;
        for _ in 0..frames {
            step_at(app, C);
        }
        let stroke = app.world().resource::<SurfaceBrushStroke>();
        assert_eq!(
            stroke.queued_samples[0].amount, moving_amount,
            "C处停留不能把静止时间加到尚未执行的B→C移动段"
        );
        if frames > 0 {
            assert_eq!(
                stroke.queued_samples.len(),
                2,
                "同点曝光可合并，不能随停留时长增加队列"
            );
            assert_eq!(
                (stroke.queued_samples[1].from, stroke.queued_samples[1].to),
                (C, C)
            );
            assert!(stroke.queued_samples[1].amount > moving_amount);
        }
        finish_requested(app);
    }
    for z in [20.0, 50.0, 100.0, 150.0, 190.0] {
        let point = Vec3::new(195.0, 0.0, z);
        assert_eq!(
            layer_weight(&held, point),
            layer_weight(&control, point),
            "静止曝光只能作用C点周围，不能回填整条B→C的 {point:?}"
        );
    }
    let control_corner = layer_weight(&control, C);
    let held_corner = layer_weight(&held, C);
    assert!(control_corner > 0 && control_corner < 255,
        "对照拐角必须已有真实贡献且未饱和，才可验证静止曝光：{control_corner}");
    assert!(held_corner > control_corner,
        "合并的C点停留必须真正加深同一像素：held={held_corner}, control={control_corner}");
}

#[test]
fn surface_queued_tail_has_fixed_capacity_and_breaks_connection_after_backpressure() {
    let mut stroke = SurfaceBrushStroke::default();
    let sample = PendingSurfaceBrushSample {
        from: A,
        to: B,
        radius: 0.75,
        erase: false,
        amount: 0.02,
    };
    for index in 0..MAX_QUEUED_SURFACE_SAMPLES {
        let from = Vec3::new(index as f32, 0.0, 0.0);
        assert!(queue_tail(
            &mut stroke,
            PendingSurfaceBrushSample {
                from,
                to: from + Vec3::X,
                ..sample
            }
        ));
    }
    let first = stroke.queued_samples[0];
    let last = *stroke.queued_samples.back().unwrap();
    assert!(!queue_tail(
        &mut stroke,
        PendingSurfaceBrushSample {
            from: last.to,
            to: C,
            ..sample
        }
    ));
    assert_eq!(stroke.queued_samples.len(), MAX_QUEUED_SURFACE_SAMPLES);
    assert_eq!(
        (stroke.queued_samples[0].from, stroke.queued_samples[0].to),
        (first.from, first.to)
    );
    assert_eq!(stroke.queued_samples.back().unwrap().to, last.to);
    assert!(
        stroke.tail_disconnected,
        "满槽后的未受理点不能被后续直线补画"
    );

    // 用真实输入系统恢复满槽后的新采样：即使还有余量，新点也必须独立成点，不能从旧尾端追赶。
    let mut app = dense_app();
    begin_long_drag(&mut app);
    {
        let mut active = app.world_mut().resource_mut::<SurfaceBrushStroke>();
        active.tail_disconnected = stroke.tail_disconnected;
    }
    step_at(&mut app, C);
    let active = app.world().resource::<SurfaceBrushStroke>();
    let resumed = active.queued_samples.back().unwrap();
    assert_eq!((resumed.from, resumed.to), (C, C));
    assert!(!active.tail_disconnected);
    finish_requested(&mut app);
    assert_eq!(
        layer_weight(&app, Vec3::new(195.0, 0.0, 100.0)),
        0,
        "背压期间没有受理的旧尾端到C路径不得复原"
    );
}
