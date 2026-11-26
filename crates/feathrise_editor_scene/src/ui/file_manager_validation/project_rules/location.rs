//! 将两个已知水平坐标投影到实际视口并点击，回读表单与独立进程重开的地点范围。
use super::*;
use crate::{
    components::SceneEditorViewport, resources::SceneEditorViewportState, types::EditorTool,
};
use bevy::ui::RelativeCursorPosition;

const FIRST: Vec3 = Vec3::new(-3., 0.5, -2.);
const SECOND: Vec3 = Vec3::new(5., 0.5, 4.);

pub(super) fn point(
    world: &mut World,
    run: &mut Run,
    window: Entity,
    second: bool,
) -> Result<bool, String> {
    if run.keys == 1 {
        super::super::mouse(world, window, ButtonState::Released);
        run.keys = 2;
        return Ok(false);
    }
    if run.keys == 2 {
        return Ok(if second {
            world.resource::<SceneEditorState>().tool != EditorTool::LocationRegion
                && layout::caption(world, "RulesPageTitle").as_deref() == Some("地点区域")
        } else {
            world
                .resource::<SceneEditorState>()
                .status
                .contains("另一对角")
        });
    }
    let position = if second { SECOND } else { FIRST };
    if world.resource::<SceneEditorState>().tool != EditorTool::LocationRegion
        || layout::caption(world, "RulesPageTitle").as_deref() != Some("地点区域")
    {
        return Ok(false);
    }
    let Some(canvas) = visible(world, |_: &SceneEditorViewport| true) else {
        return Ok(false);
    };
    let rect = layout::bounds(world, canvas).ok_or("场景视口缺少布局")?;
    let view = world.resource::<SceneEditorViewportState>();
    let camera = world.get::<Camera>(view.camera).ok_or("场景相机丢失")?;
    let pose = GlobalTransform::from(
        *world
            .get::<Transform>(view.camera)
            .ok_or("场景相机没有姿态")?,
    );
    let size = view.render_size.as_vec2();
    let pixel = camera
        .world_to_viewport(&pose, position)
        .map_err(|e| e.to_string())?;
    let physical = rect.min + (pixel / size) * rect.size();
    if !rect.contains(physical) {
        return Err(format!("夹具取点 {position:?} 超出视口 {rect:?}"));
    }
    // 误差取目标硬件上一个窗口物理像素所覆盖的地面距离，允许输入与渲染的像素取整。
    let footprint = size / rect.size();
    for offset in [
        footprint,
        -footprint,
        Vec2::new(footprint.x, -footprint.y),
        Vec2::new(-footprint.x, footprint.y),
    ] {
        let ray = camera
            .viewport_to_world(&pose, pixel + offset)
            .map_err(|e| e.to_string())?;
        let distance = ray
            .intersect_plane(position, InfinitePlane3d::new(Vec3::Y))
            .ok_or("取点落在相机水平线上")?;
        run.pixel_tolerance = run
            .pixel_tolerance
            .max(ray.get_point(distance).distance(position));
    }
    run.wheel_point = Some(physical);
    move_cursor(world, window, physical.as_dvec2());
    let ready = world
        .get::<RelativeCursorPosition>(canvas)
        .filter(|p| p.cursor_over())
        .and_then(|p| p.normalized)
        .is_some_and(|p| {
            ((p + Vec2::splat(0.5) - pixel / size) * rect.size())
                .abs()
                .max_element()
                <= 1.
        });
    if !ready {
        return Ok(false);
    }
    if second && !run.layout.contains_key("location-drawing") {
        let path = run.work.join("rules-location-drawing.png");
        let Some(shot) = evidence::capture(world, &mut run.capture, &path)? else {
            return Ok(false);
        };
        run.layout.insert(
            "location-drawing".into(),
            json!({"screenshot":shot,"click":physical.to_array(),"center":run.center}),
        );
    }
    super::super::mouse(world, window, ButtonState::Pressed);
    run.keys = 1;
    Ok(false)
}

pub(super) fn restored(world: &mut World, run: &mut Run) -> Result<bool, String> {
    if form::field_text(world, "RulesFieldScene").as_deref() != Some("M1-SAMPLE") {
        return Ok(false);
    }
    let xyz = |world: &mut World, field| -> Option<[serde_json::Number; 3]> {
        form::field_text(world, field)?
            .split(',')
            .map(|v| v.trim().parse().ok())
            .collect::<Option<Vec<_>>>()?
            .try_into()
            .ok()
    };
    let Some((min, max)) = xyz(world, "RulesFieldMin").zip(xyz(world, "RulesFieldMax")) else {
        return Ok(false);
    };
    let actual = json!({"scene":"M1-SAMPLE","min":min,"max":max});
    let min = min.each_ref().map(|value| value.as_f64().unwrap());
    let max = max.each_ref().map(|value| value.as_f64().unwrap());
    if run.phase == "draw-reopen" {
        let prior: Value = serde_json::from_slice(
            &fs::read(run.work.join("rules-empty.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        if actual != prior["layout"]["location-fields"]["range"] {
            run.layout_wait = format!("等待重开显示绘制过的两角，当前 {actual}");
            return Ok(false);
        }
    } else {
        let tolerance = f64::from(run.pixel_tolerance);
        if min[1] != 0.5
            || max[1] != 3.5
            || tolerance <= 0.
            || [
                (min[0], FIRST.x),
                (min[2], FIRST.z),
                (max[0], SECOND.x),
                (max[2], SECOND.z),
            ]
            .into_iter()
            .any(|(got, wanted)| (got - f64::from(wanted)).abs() > tolerance)
        {
            run.layout_wait =
                format!("等待视口两角回填，当前 {actual}，一像素地面距离 {tolerance}");
            return Ok(false);
        }
    }
    run.layout.insert(
        "location-fields".into(),
        json!({"range":actual,"one_pixel_world":run.pixel_tolerance}),
    );
    Ok(true)
}
