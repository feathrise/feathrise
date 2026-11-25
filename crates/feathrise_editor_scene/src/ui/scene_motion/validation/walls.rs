//! 在独占夹具中通过建筑页、真实鼠标射线和工具栏验证 L 形围墙的完整作者流程。
use super::*;
use crate::ui::building_sidebar::BuildingSidebarPage;

#[derive(Clone, Copy, Debug, Default)]
pub(super) enum Check {
    #[default]
    Building,
    Wall,
    Start,
    Point(u8),
    PointAccepted(u8),
    PickGate,
    GatePoint,
    GateAccepted,
    TryConflict,
    Rejected,
    RemoveEnd,
    SafeEnd,
    Preview,
    PreviewImage,
    Confirm,
    Wait,
    Undo,
    Empty,
    Redo,
    Restored,
    SavedImage,
    Image,
}
pub(super) fn enabled() -> bool {
    std::env::var("FEATHRISE_WALL_LINE_VALIDATION").as_deref() == Ok("1")
}

fn proof(world: &World) -> Result<Value, String> {
    let session = world.resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().ok_or("缺少围墙验收草稿")?;
    let records: Vec<_> = draft
        .placements
        .placements
        .iter()
        .filter(|r| r.id.starts_with("wall-line-"))
        .collect();
    if records.len() != 10
        || records
            .iter()
            .filter(|r| r.building_preset_id.as_deref() == Some("fixture-gate"))
            .count()
            != 1
        || records.iter().filter(|r| r.yaw_degrees == 0.0).count() != 5
        || records.iter().filter(|r| r.yaw_degrees == 90.0).count() != 5
    {
        return Err(format!(
            "L 形围墙的墙段、门楼或转角错误：{} 个模块，{:?}",
            records.len(),
            records
                .iter()
                .map(|r| (&r.building_preset_id, r.yaw_degrees))
                .collect::<Vec<_>>()
        ));
    }
    serde_json::to_value(records).map_err(|e| e.to_string())
}
pub(super) fn report(world: &World, run: &Run) -> Result<Value, String> {
    let session = world.resource::<EditorAuthoringSession>();
    Ok(
        json!({"passed":true,"reopened":run.reopen,"placements":proof(world)?,"save_sequence":session.successful_save_sequence}),
    )
}

fn author_state(world: &World) -> Value {
    let session = world.resource::<EditorAuthoringSession>();
    json!({"revision":session.revision, "undo":session.undo.len(), "redo":session.redo.len(),
        "placements":session.draft.as_ref().map(|d| &d.placements.placements)})
}

fn models_visible(world: &mut World) -> bool {
    use feathrise_scene::ScenePlacementInstance;
    use std::collections::BTreeSet;
    let mut roots = world.query::<(Entity, &ScenePlacementInstance)>();
    let mut children = world.query::<&Children>();
    let mut walls = BTreeSet::new();
    let mut gates = BTreeSet::new();
    for (root, instance) in roots
        .iter(world)
        .filter(|(_, i)| i.is_active() && i.id().starts_with("wall-line-"))
    {
        let meshes = children
            .query(world)
            .iter_descendants(root)
            .filter(|e| {
                world.get::<Mesh3d>(*e).is_some()
                    && world
                        .get::<InheritedVisibility>(*e)
                        .is_some_and(|v| v.get())
            })
            .count();
        match instance.asset_path() {
            "scene/fixture-wall.glb" if meshes >= 1 => {
                walls.insert(instance.id());
            }
            "scene/fixture-gate.glb" if meshes >= 3 => {
                gates.insert(instance.id());
            }
            _ => {}
        }
    }
    walls.len() == 9 && gates.len() == 1
}
pub(super) fn advance(world: &mut World, run: &mut Run) -> Result<bool, String> {
    use Check::*;
    let get_count = |world: &World| {
        world
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .map_or(0, |d| {
                d.placements
                    .placements
                    .iter()
                    .filter(|r| r.id.starts_with("wall-line-"))
                    .count()
            })
    };
    match run.walls {
        Building | Wall | Start | PickGate | TryConflict | RemoveEnd | Confirm | Undo | Redo => {
            let (entity, next) = match run.walls {
                Building => (
                    sidebar(
                        world,
                        EditorSidebarTab::Left(crate::ui::sidebars::EditorLeftSidebarTab::Building),
                    ),
                    Wall,
                ),
                Wall => (
                    triggers::named_button(world, "沿线围墙"),
                    if run.reopen { Restored } else { Start },
                ),
                Start => (triggers::named_button(world, "开始绘制"), Point(0)),
                PickGate => (triggers::named_button(world, "指定门楼位置"), GatePoint),
                TryConflict => (triggers::named_button(world, "确认铺设"), Rejected),
                RemoveEnd => (triggers::named_button(world, "撤回上一点"), SafeEnd),
                Confirm => (triggers::named_button(world, "确认铺设"), Wait),
                Undo => (action_button(world, EditorAction::Undo), Empty),
                _ => (action_button(world, EditorAction::Redo), Restored),
            };
            if click(world, run, entity)? {
                run.walls = next;
            }
        }
        Point(_) | GatePoint | SafeEnd => {
            let (point, next) = match run.walls {
                Point(0) => (Vec3::new(-12., 0., -9.), PointAccepted(0)),
                Point(1) => (Vec3::new(6., 0., -9.), PointAccepted(1)),
                Point(_) => (Vec3::new(6., 0., 6.), PointAccepted(2)),
                SafeEnd => (Vec3::new(6., 0., -24.), Preview),
                _ => (Vec3::new(-3., 0., -9.), GateAccepted),
            };
            let pixel = triggers::projected(world, point)?;
            let window = world
                .query_filtered::<Entity, With<PrimaryWindow>>()
                .single(world)
                .map_err(|e| e.to_string())?;
            if pointer_click(world, &mut run.click, window, pixel, MouseButton::Left) {
                run.walls = next;
            }
        }
        PointAccepted(i) => {
            let (points, _, _) = world
                .resource::<crate::authoring::courtyards::wall::WallTool>()
                .observed();
            if points == usize::from(i) + 1 {
                run.walls = if i == 2 { PickGate } else { Point(i + 1) };
            }
        }
        GateAccepted => {
            if world
                .resource::<crate::authoring::courtyards::wall::WallTool>()
                .observed()
                .2
            {
                run.wall_baseline = Some(author_state(world));
                run.walls = TryConflict;
            }
        }
        Rejected => {
            let status = &world.resource::<SceneEditorState>().status;
            if status.contains("sample-model") && status.contains("冲突") {
                let after = author_state(world);
                if run.wall_baseline.as_ref() != Some(&after) {
                    return Err(format!(
                        "拒绝后的摆放、修订或历史发生变化：{:?} → {after}",
                        run.wall_baseline
                    ));
                }
                run.walls = RemoveEnd;
            }
        }
        Preview => {
            let (points, count, gate) = world
                .resource::<crate::authoring::courtyards::wall::WallTool>()
                .observed();
            if !gate || points != 3 {
                return Ok(false);
            }
            if count != 10 {
                return Err(format!("预览的 L 形墙预期十个模块，实际 {count}"));
            }
            if get_count(world) != 0
                || *world.resource::<BuildingSidebarPage>() != BuildingSidebarPage::Wall
            {
                return Err("确认前已写入摆放或建筑工具页失效".into());
            }
            request_screenshot(world, run.directory.join("wall-route-preview.png"));
            run.walls = PreviewImage;
        }
        PreviewImage => {
            if let Some(done) = screenshot_result(world) {
                done?;
                run.walls = Confirm;
            }
        }
        Wait => {
            if get_count(world) > 0 {
                proof(world)?;
                run.walls = Undo;
            }
        }
        Empty => {
            if get_count(world) == 0 {
                run.walls = Redo;
            }
        }
        Restored => {
            if get_count(world) > 0 {
                proof(world)?;
                run.walls = SavedImage;
            }
        }
        SavedImage => {
            if world
                .resource::<crate::authoring::courtyards::CourtyardAuthoringState>()
                .visual_sync_pending()
                || !models_visible(world)
            {
                return Ok(false);
            }
            request_screenshot(world, run.directory.join("wall-line.png"));
            run.walls = Image;
        }
        Image => {
            if let Some(done) = screenshot_result(world) {
                done?;
                return Ok(true);
            }
        }
    }
    Ok(false)
}
