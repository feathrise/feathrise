//! 建筑页的沿线围墙工具。路线和门楼位置不进入草稿，确认后复用正式建筑地基任务。
mod plan;
mod submission;
pub(super) use submission::WallSubmission;
pub(crate) use ui::panel;
mod ui;
use super::{
    CourtyardAuthoringState,
    state::{OperationIdentity, OperationResult},
};
use crate::{
    authoring::{
        EditorAuthoringSession,
        buildings::{BuildingAuthoringState, courtyard_validator_snapshot},
    },
    components::SceneEditorViewport,
    resources::SceneEditorState,
    types::EditorTool,
    ui::{
        building_sidebar::BuildingSidebarPage,
        sidebars::{EditorLeftSidebarTab, SceneEditorSidebarState},
    },
    viewport::{
        SceneEditorBrushGizmos,
        modes::{ViewportMode, ViewportModeState},
        vegetation_ground::{GroundCursorTarget, VegetationGroundCursor},
    },
};
use bevy::{prelude::*, ui::RelativeCursorPosition};
use feathrise_editor_ui::EditorButtonClick;
use feathrise_scene::DynamicClipmapGroundSurface;
use feathrise_scene_content::{
    BuildingPresetCategory, BuildingPresetRegistry, SceneId, SceneTerrainKind,
};
use plan::{Module, Plan};
use std::sync::Arc;

#[derive(Resource, Default)]
pub(crate) struct WallTool {
    pub(crate) epoch: u64,
    catalog: Option<u64>,
    wall: Option<Module>,
    door: Option<Module>,
    owner: Option<(SceneId, u64)>,
    points: Vec<Vec3>,
    gate: Option<(usize, f32)>,
    picking_gate: bool,
    plan: Arc<Plan>,
}
impl WallTool {
    #[cfg(feature = "validation")]
    pub(crate) fn observed(&self) -> (usize, usize, bool) {
        (self.points.len(), self.plan.count, self.gate.is_some())
    }
    pub(super) fn clear_route(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.owner = None;
        self.points.clear();
        self.gate = None;
        self.picking_gate = false;
        self.plan = default();
    }
    fn switch_module(
        &mut self,
        registry: &BuildingPresetRegistry,
        gate_only: bool,
    ) -> Result<(), String> {
        // 切换预设保留已点击的路线；组合无法铺设时恢复上一组仍可确认的预览。
        let old = (self.wall.clone(), self.door.clone(), self.gate);
        if !gate_only {
            self.wall = select(
                registry,
                BuildingPresetCategory::Wall,
                None,
                self.wall.as_ref(),
            );
        }
        self.door = self.wall.as_ref().and_then(|wall| {
            let current = self.door.as_ref().filter(|gate| gate.kind == wall.kind);
            if !gate_only && current.is_some() {
                return current.cloned();
            }
            select(
                registry,
                BuildingPresetCategory::Gate,
                Some(&wall.kind),
                current,
            )
        });
        if self.door.is_none() {
            self.gate = None;
        }
        if let Err(error) = self.rebuild() {
            (self.wall, self.door, self.gate) = old;
            return Err(error);
        }
        Ok(())
    }
    fn rebuild(&mut self) -> Result<(), String> {
        let wall = self
            .wall
            .as_ref()
            .ok_or("工程需要带同类、反向连接点的围墙建筑预设")?;
        let gate = self
            .gate
            .and_then(|(i, d)| self.door.as_ref().map(|module| (module, i, d)));
        self.plan = Arc::new(Plan::build(&self.points, wall, gate)?);
        self.epoch = self.epoch.wrapping_add(1);
        Ok(())
    }
}

fn select(
    registry: &BuildingPresetRegistry,
    category: BuildingPresetCategory,
    kind: Option<&str>,
    current: Option<&Module>,
) -> Option<Module> {
    let presets = registry.presets();
    let start = current
        .and_then(|m| presets.iter().position(|p| p.id == m.preset.id))
        .map_or(0, |i| i + 1);
    (0..presets.len())
        .map(|i| &presets[(i + start) % presets.len()])
        .filter(|p| p.category == category)
        .find_map(|p| Module::resolve(p, kind))
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<WallTool>().add_systems(
        Update,
        (input, ui::present)
            .chain()
            .run_if(
                |page: Option<Res<BuildingSidebarPage>>, tool: Res<WallTool>| {
                    tool.owner.is_some() || page.is_some_and(|p| *p == BuildingSidebarPage::Wall)
                },
            )
            .run_if(crate::scene_residency::document_active)
            .in_set(crate::SceneEditorSystemSet::Presentation),
    );
}

#[derive(bevy::ecs::system::SystemParam)]
struct Ground<'w, 's> {
    buildings: Option<Res<'w, BuildingPresetRegistry>>,
    index: Res<'w, BuildingAuthoringState>,
    batch: ResMut<'w, CourtyardAuthoringState>,
    clipmap: Option<Res<'w, DynamicClipmapGroundSurface>>,
    cursor: ResMut<'w, VegetationGroundCursor>,
    canvas: Query<'w, 's, &'static RelativeCursorPosition, With<SceneEditorViewport>>,
}

#[allow(clippy::too_many_arguments)]
fn input(
    mut tool: ResMut<WallTool>,
    mut mode: ResMut<ViewportModeState>,
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    page: Res<BuildingSidebarPage>,
    side: Res<SceneEditorSidebarState>,
    mut ground: Ground,
    buttons: Query<(&EditorButtonClick, &ui::Action), Changed<EditorButtonClick>>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut gizmos: Gizmos<SceneEditorBrushGizmos>,
) {
    let visible = editor.open
        && !editor.close_requested
        && !editor.new_scene_dialog_open
        && !editor.scene_transition_in_progress
        && editor.scene_switch_confirmation.is_none()
        && *page == BuildingSidebarPage::Wall
        && side.tool_page_visible(EditorLeftSidebarTab::Building);
    let armed = mode.mode == ViewportMode::Edit && editor.tool == EditorTool::WallLine;
    if tool.owner.as_ref().is_some_and(|(scene, rev)| {
        !visible
            || !armed
            || *rev != session.revision
            || editor.selected_scene_id.as_ref() != Some(scene)
    }) || keys.just_pressed(KeyCode::Escape) && tool.owner.is_some()
    {
        tool.clear_route();
    }
    if !visible || session.saving || editor.save_requested {
        return;
    }
    let Some(registry) = ground.buildings.as_deref() else {
        return;
    };
    if tool.catalog != Some(registry.revision) {
        tool.clear_route();
        tool.catalog = Some(registry.revision);
        tool.wall = select(registry, BuildingPresetCategory::Wall, None, None);
        tool.door = tool
            .wall
            .as_ref()
            .and_then(|w| select(registry, BuildingPresetCategory::Gate, Some(&w.kind), None));
    }
    let Some(draft) = session.draft.as_ref() else {
        return;
    };
    if editor.selected_scene_id.as_ref() != Some(&draft.scene_id) {
        return;
    }
    if let Some(action) = buttons
        .iter()
        .find_map(|(click, action)| click.is_clicked().then_some(*action))
    {
        use ui::Action::*;
        if ground.batch.operation_pending() && !matches!(action, Cancel) {
            return;
        }
        let status = match action {
            Wall | Gate => tool
                .switch_module(registry, matches!(action, Gate))
                .map_or_else(|error| error, |_| "已按所选墙段和门楼更新路线".into()),
            Start => {
                if tool.wall.is_none() {
                    editor.status = "当前工程缺少带同类型反向连接点的围墙预设".into();
                    return;
                }
                tool.clear_route();
                tool.owner = Some((draft.scene_id.clone(), session.revision));
                mode.mode = ViewportMode::Edit;
                mode.remember_tool(EditorTool::WallLine);
                editor.tool = EditorTool::WallLine;
                "请在地面点选起点、转角和终点".into()
            }
            ChooseGate if tool.plan.count > 0 && tool.door.is_some() => {
                tool.picking_gate = true;
                tool.epoch = tool.epoch.wrapping_add(1);
                "请在要放置门楼的墙段上点击".into()
            }
            ChooseGate => "请先绘制一段围墙，并选择连接点类型相同的门楼".into(),
            UndoPoint => {
                tool.points.pop();
                if tool.gate.is_some_and(|(i, _)| i + 1 >= tool.points.len()) {
                    tool.gate = None;
                }
                tool.picking_gate = false;
                tool.rebuild()
                    .err()
                    .unwrap_or_else(|| "已撤回上一点".into())
            }
            RemoveGate => {
                tool.gate = None;
                tool.picking_gate = false;
                tool.rebuild()
                    .err()
                    .unwrap_or_else(|| "已移除门楼预留位置".into())
            }
            Cancel => {
                tool.clear_route();
                "已取消围墙预览".into()
            }
            Confirm if tool.owner.is_some() && tool.plan.count > 0 => {
                match submit(
                    &tool,
                    &mut session,
                    registry,
                    &ground.index,
                    &mut ground.batch,
                    ground.clipmap.as_deref(),
                ) {
                    Ok(()) => "正在校验整条围墙的地基和净空".into(),
                    Err(error) => error,
                }
            }
            Confirm => "请先在地面确定起点和终点".into(),
        };
        editor.status = status;
        return;
    }
    if !armed || tool.owner.is_none() {
        return;
    }
    tool.plan.draw(&mut gizmos);
    if let Some(&first) = tool.points.first() {
        gizmos.cross(tool.plan.end(first), 0.35, Color::srgb(1., 0.8, 0.3));
        if let Some(cursor) = editor.cursor_world
            && !tool.picking_gate
            && let Some((start, end)) = tool
                .wall
                .as_ref()
                .and_then(|wall| tool.plan.guide(first, cursor, wall))
        {
            gizmos.line(start, end, Color::srgb(0.7, 0.9, 0.85));
        }
    }
    if ground.batch.operation_pending()
        || mode.pointer_over_controls
        || !ground.canvas.single().is_ok_and(|c| c.cursor_over())
    {
        return;
    }
    let point = if draft.manifest.terrain.kind == SceneTerrainKind::SparseClipmap {
        ground
            .clipmap
            .as_deref()
            .and_then(|s| s.snapshot.as_ref())
            .and_then(|snapshot| {
                match ground.cursor.take_author_click_result(
                    snapshot,
                    &draft.scene_id,
                    GroundCursorTarget::WallLine(tool.epoch),
                    session.revision,
                ) {
                    Some(feathrise_scene::ClipmapGroundHit::Hit(p)) => Some(p),
                    _ => None,
                }
            })
    } else if draft.manifest.terrains.is_some() {
        // 冻结的真实地形点击可在松键后完成；悬停点没有本次点击的身份。
        editor.ground_click_world.take()
    } else if mouse.just_pressed(MouseButton::Left) {
        editor.cursor_world
    } else {
        None
    };
    let Some(point) = point else { return };
    if tool.picking_gate {
        let old = tool.gate;
        tool.gate = tool.plan.nearest(point);
        if let Err(error) = tool.rebuild() {
            tool.gate = old;
            editor.status = error;
        } else {
            tool.picking_gate = false;
            editor.status = "已预留门楼位置，可以确认铺设或继续添加墙段".into();
        }
    } else {
        tool.points.push(point);
        if let Err(error) = tool.rebuild() {
            tool.points.pop();
            editor.status = error;
        } else if tool.points.len() == 1 {
            editor.status = "已选起点，请点选转角或终点".into();
        } else {
            editor.status = "可继续点选转角、指定门楼位置，或确认铺设".into();
        }
    }
}

fn submit(
    tool: &WallTool,
    session: &mut EditorAuthoringSession,
    buildings: &BuildingPresetRegistry,
    index: &BuildingAuthoringState,
    state: &mut CourtyardAuthoringState,
    clipmap: Option<&DynamicClipmapGroundSurface>,
) -> Result<(), String> {
    if state.save_pending() || state.visual_sync_pending() {
        return Err("上一批建筑仍在处理，请稍后确认".into());
    }
    let validator = courtyard_validator_snapshot(session, buildings, index, clipmap)?;
    let prefix = crate::authoring::next_author_id(session, "wall-line");
    let draft = session.draft.as_ref().ok_or("场景已卸载")?;
    state.ticket = state.ticket.wrapping_add(1).max(1);
    let identity = OperationIdentity {
        scene_id: draft.scene_id.clone(),
        author_revision: session.revision,
        terrain_revision: session.terrain_revision,
        ground_content: validator
            .sampler
            .as_ref()
            .and_then(crate::authoring::PlacementTerrainSampler::clipmap_content_revision),
        structure_revision: session.placement_structure_revision,
        building_revision: buildings.revision,
        courtyard_revision: 0,
        ticket: state.ticket,
    };
    let wall = WallSubmission::new(tool.epoch, session, draft,
        submission::GroundStart { plan: tool.plan.clone(), id_prefix: prefix, validator });
    state.operation_waiting = Some(OperationResult {
        identity,
        source: vec![],
        creates_new: true,
        result: Err(String::new()),
        description: format!("沿线铺设 {} 个建筑模块", tool.plan.count),
        required_mode: None,
        ground_work: None,
        input_target: None,
        wall: Some(Box::new(wall)),
    });
    Ok(())
}
