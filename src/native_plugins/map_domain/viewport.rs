//! 通用地图领域的真实输入桥；缺页暂停/断笔、换代取消，绝不把旧悬停坐标补成新落点。
use super::*;
use bevy::ui::RelativeCursorPosition;
use feathrise_editor_map::game_authoring::map_api::{
    components::MapEditorViewport,
    ui::{
        scene_context_menu::{SceneContextMenuState, SceneMenuAction},
        vegetation_preset_editor::VegetationPresetEditorState,
    },
    viewport::{
        modes::ViewportModeState,
        vegetation_ground::{GroundCursorTarget, VegetationGroundCursor},
    },
};

pub(super) struct Gesture {
    identity: MapDomainIdentity,
    target: GroundCursorTarget,
    started: bool,
    last: Option<[f32; 3]>,
    broken: bool,
    shift: bool,
}
fn base(wire: &MapDomainSnapshot, phase: DomainPointerPhase, shift: bool) -> MapDomainViewport {
    MapDomainViewport {
        phase,
        positions: vec![],
        breaks: vec![],
        selected: wire.selected.clone(),
        layer: wire.layer.clone(),
        point: wire.pointer.as_ref().and_then(|p| p.point.clone()),
        selection_ticket: wire.selection_ticket,
        shift,
        terrain_binding: None,
    }
}
fn enqueue(maps: &mut NativeMaps, input: MapDomainViewport) -> bool {
    if input.phase == DomainPointerPhase::Update {
        if let Some(last) = maps.inputs.back_mut().filter(|last| {
            last.phase == DomainPointerPhase::Update
                && last.shift == input.shift
                && last.positions.len() + input.positions.len() <= 4096
        }) {
            let offset = last.positions.len() as u32;
            last.breaks.extend(input.breaks.iter().map(|b| offset + *b));
            last.positions.extend(input.positions);
            return true;
        }
        if maps.inputs.len() >= 6 {
            return false;
        }
    }
    if maps.inputs.len() >= 8 {
        return false;
    }
    maps.inputs.push_back(input);
    true
}
pub(super) fn input(world: &mut World) {
    world.resource_scope(|world, mut maps: Mut<NativeMaps>| {
        let Some(index) = maps.index else {
            return;
        };
        let Some(current) = identity(world, &maps) else {
            return;
        };
        let Some(wire) = world
            .resource::<NativeTools>()
            .tools
            .get(&index)
            .and_then(|t| t.snapshot.map_domain.clone())
        else {
            return;
        };
        let selection = world
            .resource_mut::<MapInteractionState>()
            .selection_request
            .take();
        if let Some(selection) = selection {
            let valid = world
                .resource::<EditorAuthoringSession>()
                .draft
                .as_ref()
                .is_some_and(|draft| selection.identity.current(draft, current.generation));
            if valid {
                let mut input = base(&wire, DomainPointerPhase::Click, false);
                input.selection_ticket = input.selection_ticket.wrapping_add(1);
                match selection.action {
                    MapInteractionSelection::Clear => {
                        input.selected = None;
                        input.layer = None;
                        input.point = None;
                    }
                    MapInteractionSelection::Layer(id) => {
                        input.selected = None;
                        input.layer = Some(id);
                        input.point = None;
                    }
                    MapInteractionSelection::Object(id) => {
                        input.selected = Some(id);
                        input.point = None;
                    }
                }
                enqueue(&mut maps, input);
            }
        }
        let Some(target) = world
            .resource::<MapInteractionState>()
            .cursor_target
            .clone()
            .or_else(|| maps.gesture.as_ref().map(|gesture| gesture.target.clone()))
        else {
            return;
        };
        let keys = world.resource::<ButtonInput<KeyCode>>();
        let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
        let escape = keys.just_pressed(KeyCode::Escape);
        let control = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
        let mouse = world.resource::<ButtonInput<MouseButton>>();
        let pressed = mouse.pressed(MouseButton::Left);
        let begin = mouse.just_pressed(MouseButton::Left);
        let in_view = world
            .query_filtered::<&RelativeCursorPosition, With<MapEditorViewport>>()
            .single(world)
            .is_ok_and(|cursor| cursor.cursor_over());
        let editor = world.resource::<MapEditorState>();
        let active = editor.open
            && !editor.close_requested
            && !editor.new_map_dialog_open
            && !editor.suppress_world_pick_this_frame
            && editor.tool == authoring_tool(&maps)
            && !editor.map_transition_in_progress
            && !editor.save_requested
            && !control
            && !escape
            && in_view
            && !world.resource::<EditorAuthoringSession>().saving
            && !world.resource::<NativeTools>().input_pending
            && world.resource::<VegetationPresetEditorState>().active_tab
                == feathrise_editor_core::PresetWorkspaceTab::Map
            && !world
                .get_resource::<ViewportModeState>()
                .is_some_and(|mode| mode.pointer_over_controls)
            && wire.identity.as_ref() == Some(&current);
        if let Some(gesture) = &mut maps.gesture {
            // 缺页首次按下尚未写入时，可升级到首份真实高度代次；已有样本绝不跨代连接。
            let mut first_identity = gesture.identity.clone();
            first_identity.ground_content = current.ground_content;
            if !gesture.started
                && gesture.identity.ground_content.is_none()
                && first_identity == current
            {
                gesture.identity = current.clone();
                gesture.target = target.clone();
            }
            let valid =
                gesture.identity == current && gesture.target == target && gesture.shift == shift;
            if !valid || escape {
                maps.inputs.clear();
                let started = maps.gesture.take().is_some_and(|g| g.started);
                if started {
                    enqueue(&mut maps, base(&wire, DomainPointerPhase::Cancel, shift));
                }
                return;
            }
            if !pressed || !active {
                let gesture = maps.gesture.take().unwrap();
                if gesture.started {
                    enqueue(
                        &mut maps,
                        base(&wire, DomainPointerPhase::End, gesture.shift),
                    );
                }
                return;
            }
        }
        if !active {
            if let Some(mut ground) = world.get_resource_mut::<VegetationGroundCursor>() {
                ground.cancel_domain_click(maps.port.as_ref().unwrap().domain);
            }
            return;
        }
        let is_clipmap = world
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .is_some_and(|draft| {
                draft.manifest.terrain.kind
                    == feathrise_map_content::MapTerrainKind::SparseClipmap
            });
        let snapshot = world
            .get_resource::<feathrise_scene::DynamicClipmapGroundSurface>()
            .and_then(|surface| surface.snapshot.clone());
        if wire.continuous {
            if begin {
                maps.gesture = Some(Gesture {
                    identity: current.clone(),
                    target: target.clone(),
                    started: false,
                    last: None,
                    broken: false,
                    shift,
                });
            }
            if !pressed || maps.gesture.is_none() {
                return;
            }
            let position = if is_clipmap {
                snapshot
                    .as_ref()
                    .and_then(|snapshot| {
                        world
                            .get_resource::<VegetationGroundCursor>()?
                            .accepted_paint_hit(
                                snapshot,
                                &feathrise_map_content::MapId::parse(&current.map_id).ok()?,
                                &target,
                                current.terrain_revision,
                            )
                    })
                    .filter(|position| {
                        world.resource::<MapEditorState>().brush_cursor_world == Some(*position)
                    })
            } else {
                world
                    .resource::<MapEditorState>()
                    .brush_cursor_world
                    .or(world.resource::<MapEditorState>().cursor_world)
            };
            let Some(position) = position else {
                if !world
                    .get_resource::<VegetationGroundCursor>()
                    .is_some_and(|ground| ground.pending_same_context)
                {
                    maps.gesture.as_mut().unwrap().broken = true;
                }
                return;
            };
            let gesture = maps.gesture.as_mut().unwrap();
            if gesture
                .last
                .is_some_and(|last| Vec3::from_array(last).distance_squared(position) <= 0.05)
            {
                return;
            }
            if let (Some(snapshot), Some(last)) = (snapshot.as_ref(), gesture.last) {
                if !snapshot.segment_is_known(Vec3::from_array(last), position) {
                    gesture.broken = true;
                }
            }
            let first = !gesture.started;
            let gap = gesture.broken && gesture.started;
            if gap && !enqueue(&mut maps, base(&wire, DomainPointerPhase::Gap, shift)) {
                return;
            }
            let mut input = base(
                &wire,
                if first {
                    DomainPointerPhase::Begin
                } else {
                    DomainPointerPhase::Update
                },
                shift,
            );
            input.positions.push(position.to_array());
            if enqueue(&mut maps, input) {
                let gesture = maps.gesture.as_mut().unwrap();
                gesture.started = true;
                gesture.last = Some(position.to_array());
                gesture.broken = false;
            } else {
                maps.gesture.as_mut().unwrap().broken = true;
            }
            return;
        }
        let position = if is_clipmap {
            snapshot
                .as_ref()
                .and_then(|snapshot| {
                    world
                        .get_resource_mut::<VegetationGroundCursor>()?
                        .take_author_click_result(
                            snapshot,
                            &feathrise_map_content::MapId::parse(&current.map_id).ok()?,
                            target.clone(),
                            current.revision,
                        )
                })
                .and_then(|hit| match hit {
                    feathrise_scene::ClipmapGroundHit::Hit(point) => Some(point),
                    _ => None,
                })
        } else {
            world
                .resource_mut::<MapEditorState>()
                .ground_click_world
                .take()
        };
        if let Some(position) = position {
            let mut input = base(&wire, DomainPointerPhase::Click, shift);
            input.positions.push(position.to_array());
            enqueue(&mut maps, input);
        }
    });
}
pub(super) fn scene_menu(world: &mut World) {
    world.resource_scope(|world, mut maps: Mut<NativeMaps>| {
        if maps.port.is_none() || maps.task.is_some() {
            return;
        }
        let request = world.resource::<SceneContextMenuState>().domain_request(
            world.resource::<MapEditorState>(),
            world.resource::<EditorAuthoringSession>(),
        );
        let Some(request) = request else {
            return;
        };
        let action = match request.action {
            SceneMenuAction::Focus => return,
            SceneMenuAction::Rename => DomainSceneAction::Rename,
            SceneMenuAction::Copy => DomainSceneAction::Copy,
            SceneMenuAction::Delete => DomainSceneAction::Delete,
            SceneMenuAction::Visibility => DomainSceneAction::Visibility,
            SceneMenuAction::Lock => DomainSceneAction::Lock,
        };
        maps.scene = Some(MapDomainScene {
            kind: if request.kind == DomainItemKind::Layer {
                DomainSceneKind::Layer
            } else {
                DomainSceneKind::Object
            },
            id: request.id.into(),
            action,
            name: request.name.into(),
        });
    });
}
