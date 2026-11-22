//! 场景树以固定行池解析拖拽，外层归属送原摆放管理，内部节点送 GLB 作者事务。

use super::{
    EditorGltfBrowserState, SceneEditorGltfBrowserRow, SceneMeshObjectsHeading,
    systems::GltfRowCaption,
};
use crate::{
    SceneEditorSystemSet,
    authoring::EditorAuthoringSession,
    resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState},
    ui::{
        numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard,
        scene_context_menu::{
            MenuContext, SceneContextMenuState, SceneMenuAction, SceneMenuDispatch, SceneObjectKey,
            SceneObjectRow, SceneObjectTarget,
        },
        scene_objects::{SceneObjectState, create_folder, queue_group_drop},
        scene_source_glb::SourceGlbRequest,
        theme as colors,
    },
};
use bevy::{
    camera::NormalizedRenderTarget,
    picking::pointer::{PointerAction, PointerButton, PointerId, PointerInput},
    prelude::*,
    ui::{CalculatedClip, ComputedUiTargetCamera, RelativeCursorPosition},
    window::PrimaryWindow,
};
use feathrise_editor_ui::UiInteraction;

struct Press {
    source: SceneObjectTarget,
    entries_revision: u64,
    origin: Vec2,
}

#[derive(Resource, Default)]
pub(crate) struct PlacementTreeDrag {
    press: Option<Press>,
    active: bool,
    hovered: Option<Entity>,
    dropped: Option<(SceneObjectTarget, SceneObjectTarget)>,
}

impl PlacementTreeDrag {
    pub(crate) fn owns_mouse(&self) -> bool {
        self.active
    }
}

pub(crate) fn install(app: &mut App) {
    app.init_resource::<PlacementTreeDrag>()
        .add_systems(
            Update,
            capture
                .after(crate::scene_browser::update_scene_browser_drag)
                .before(update_scene_editor_numeric_scrub_mouse_guard)
                .in_set(SceneEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            apply
                .after(SceneMenuDispatch)
                .in_set(SceneEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            (bind_root_menu, highlight_drop)
                .after(super::sync_gltf_browser_rows)
                .in_set(SceneEditorSystemSet::Presentation),
        );
}

fn bind_root_menu(
    mut commands: Commands,
    editor: Res<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    rows: Query<(Entity, Option<&SceneObjectRow>), With<SceneMeshObjectsHeading>>,
) {
    let target = session
        .draft
        .as_ref()
        .filter(|draft| editor.selected_scene_id.as_ref() == Some(&draft.scene_id))
        .map(|draft| SceneObjectTarget {
            scene_id: draft.scene_id.clone(),
            revision: session.revision,
            key: SceneObjectKey::PlacementTreeRoot,
            name: "网格对象".into(),
            hidden: false,
            locked: false,
            copyable: false,
        });
    for (entity, previous) in &rows {
        if previous.is_none_or(|previous| previous.0 != target) {
            commands
                .entity(entity)
                .insert(SceneObjectRow(target.clone()));
        }
    }
}

type Rows<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static SceneObjectRow,
        &'static UiInteraction,
        &'static RelativeCursorPosition,
        &'static Node,
        Option<&'static SceneEditorGltfBrowserRow>,
    ),
    Or<(
        With<SceneEditorGltfBrowserRow>,
        With<SceneMeshObjectsHeading>,
    )>,
>;

type Captions<'w, 's> = Query<
    'w,
    's,
    (
        &'static ChildOf,
        &'static ComputedNode,
        &'static UiGlobalTransform,
        Option<&'static CalculatedClip>,
        &'static InheritedVisibility,
        &'static ComputedUiTargetCamera,
    ),
    With<GltfRowCaption>,
>;

fn current_row_target<'a>(
    row: &'a SceneObjectRow,
    node: &Node,
    glyph: Option<&SceneEditorGltfBrowserRow>,
    browser: &EditorGltfBrowserState,
    editor: &SceneEditorState,
    session: &EditorAuthoringSession,
) -> Option<&'a SceneObjectTarget> {
    if node.display == Display::None
        || glyph.is_some_and(|glyph| {
            glyph.entries_revision != browser.published_entries_revision && glyph.main_glb.is_none()
        })
    {
        return None;
    }
    row.0
        .as_ref()
        .filter(|target| target.current(editor, session))
}

fn caption_at(origin: Vec2, captions: &Captions, cameras: &Query<&Camera>) -> Option<Entity> {
    captions
        .iter()
        .find_map(|(parent, node, transform, clip, visible, camera)| {
            if !visible.get() || node.size().min_element() <= 0.0 {
                return None;
            }
            let viewport = cameras.get(camera.get()?).ok()?.physical_viewport_rect()?;
            let point = origin - viewport.min.as_vec2();
            (node.contains_point(*transform, point)
                && clip.is_none_or(|clip| clip.contains_point(point)))
            .then(|| parent.parent())
        })
}

fn editable_source(target: &SceneObjectTarget, session: &EditorAuthoringSession) -> bool {
    !target.locked
        && target.copyable
        && target.glb_path(session).is_some()
        && matches!(&target.key, SceneObjectKey::Placement { mesh, .. } if
            mesh.as_ref().is_none_or(|mesh| !target.hidden && mesh.node_index.is_some()))
}

fn drop_allowed(
    source: &SceneObjectTarget,
    target: &SceneObjectTarget,
    browser: &EditorGltfBrowserState,
    session: &EditorAuthoringSession,
) -> bool {
    if source.key == target.key {
        return false;
    }
    match (&source.key, &target.key) {
        (
            SceneObjectKey::Placement { mesh: None, .. },
            SceneObjectKey::PlacementTreeRoot
            | SceneObjectKey::PlacementFolder
            | SceneObjectKey::Group { .. },
        ) => true,
        (
            SceneObjectKey::Placement { mesh: Some(_), .. },
            SceneObjectKey::Placement {
                id, index, mesh, ..
            },
        ) => {
            !target.locked
                && !target.hidden
                && target.glb_path(session).is_some()
                && if let Some(mesh) = mesh {
                    mesh.is_source_folder()
                } else {
                    browser.expanded_placements.contains(id)
                        && session.draft.as_ref().is_some_and(|draft| {
                            draft
                                .placements
                                .placements
                                .get(*index)
                                .is_some_and(|record| record.asset_target.is_scene0())
                        })
                }
        }
        _ => false,
    }
}

fn capture(
    context: MenuContext,
    editor: Res<SceneEditorState>,
    browser: Res<EditorGltfBrowserState>,
    guard: Res<SceneEditorNumericScrubMouseGuard>,
    menu: Res<SceneContextMenuState>,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    windows: Query<(Entity, &Window), With<PrimaryWindow>>,
    rows: Rows,
    captions: Captions,
    cameras: Query<&Camera>,
    mut pointer: MessageReader<PointerInput>,
    mut drag: ResMut<PlacementTreeDrag>,
) {
    drag.hovered = None;
    if drag.press.is_none() {
        drag.active = false;
    }
    let window = windows.single().ok();
    let position = window.and_then(|(_, window)| window.physical_cursor_position());
    let pressed_at = pointer.read().filter_map(|input| {
        let (entity, window) = window?;
        (input.pointer_id == PointerId::Mouse && matches!(input.action, PointerAction::Press(PointerButton::Primary))
            && matches!(&input.location.target, NormalizedRenderTarget::Window(target) if target.entity() == entity))
            .then(|| input.location.position * window.scale_factor())
    }).last();
    let ready = context.enabled(&editor)
        && !context.session.source_glb_busy
        && !menu.owns_mouse()
        && !mouse.any_pressed([MouseButton::Right, MouseButton::Middle])
        && !keyboard.any_just_pressed([KeyCode::F12, KeyCode::Escape])
        && browser.signature_build.is_none()
        && browser.entries_build.is_none();
    let target_for = |(_, row, _, _, node, glyph)| {
        current_row_target(row, node, glyph, &browser, &editor, &context.session)
    };
    let hovered = || {
        rows.iter()
            .filter_map(|row @ (entity, _, hit, cursor, _, _)| {
                if !cursor.cursor_over()
                    || !matches!(hit, UiInteraction::Pressed | UiInteraction::Hovered)
                {
                    return None;
                }
                target_for(row).map(|target| (entity, target))
            })
    };
    if drag.press.is_none()
        && ready
        && !guard.blocked()
        && mouse.just_pressed(MouseButton::Left)
        && let Some(position) = position
    {
        let moved = pressed_at
            .filter(|origin| crate::scene_browser::drag_threshold_reached(*origin, position));
        let source = if let Some(origin) = moved {
            // 同一帧可能已经移动甚至松开。拾取三态只反映终点，按下坐标仍保留在有序鼠标消息中。
            // 仅命中源行图标与名称区域，避开同一行的展开箭头和三个快捷按钮。
            caption_at(origin, &captions, &cameras)
                .and_then(|entity| rows.get(entity).ok())
                .and_then(target_for)
                .map(|target| (origin, target))
        } else {
            hovered()
                .find(|(_, target)| editable_source(target, &context.session))
                .map(|(_, target)| (position, target))
        };
        if let Some((origin, source)) =
            source.filter(|(_, target)| editable_source(target, &context.session))
        {
            // 冻结作者身份，后续滚动行池可以换绑，但不能换掉拖动的对象。
            drag.press = Some(Press {
                source: source.clone(),
                origin,
                entries_revision: browser.published_entries_revision,
            });
        }
    }
    if let Some(press) = drag.press.take() {
        if !ready
            || press.entries_revision != browser.published_entries_revision
            || !press.source.current(&editor, &context.session)
        {
            drag.active = true; // 取消也要阻断原按键松开穿透到树选择和场景。
            return;
        }
        if !drag.active
            && position.is_some_and(|position| {
                crate::scene_browser::drag_threshold_reached(press.origin, position)
            })
        {
            drag.active = true;
        }
        let destination = drag
            .active
            .then(|| {
                hovered().find(|(_, target)| {
                    drop_allowed(&press.source, target, &browser, &context.session)
                })
            })
            .flatten();
        if mouse.just_released(MouseButton::Left) {
            drag.dropped = destination.map(|(_, target)| (press.source, target.clone()));
            return; // 保留 active 到本帧结束，随后公共鼠标门等待静默。
        }
        if mouse.pressed(MouseButton::Left) {
            drag.hovered = destination.map(|(entity, _)| entity);
            drag.press = Some(press);
        }
    }
}

fn apply(
    mut drag: ResMut<PlacementTreeDrag>,
    menu: Res<SceneContextMenuState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut editor: ResMut<SceneEditorState>,
    mut objects: ResMut<SceneObjectState>,
    mut browser: ResMut<EditorGltfBrowserState>,
    mut source_requests: MessageWriter<SourceGlbRequest>,
) {
    if let Some((source, target)) = drag.dropped.take() {
        if !source.current(&editor, &session) || !target.current(&editor, &session) {
            return;
        }
        if matches!(&source.key, SceneObjectKey::Placement { mesh: Some(_), .. }) {
            source_requests.write(SourceGlbRequest::Move {
                source,
                destination: target,
            });
        } else {
            let name =
                (!matches!(target.key, SceneObjectKey::PlacementTreeRoot)).then_some(target.name);
            if queue_group_drop(&mut objects, &session, &source, name.clone())
                && let Some(name) = name
            {
                browser.expanded_groups.insert(name); // 展开目标以直接看到放入的摆放。
            }
        }
        return;
    }
    let Some(request) = menu.ready.as_ref().filter(|request| {
        request.action == SceneMenuAction::NewPlacementFolder
            && request.target.current(&editor, &session)
    }) else {
        return;
    };
    let name = request.name.trim();
    match create_folder(&mut objects, &mut session, &mut editor, name) {
        Ok(()) => editor.status = format!("已新建文件夹「{name}」"),
        Err(error) => editor.status = error,
    }
}

fn highlight_drop(
    drag: Res<PlacementTreeDrag>,
    mut rows: Query<
        &mut BackgroundColor,
        (
            With<SceneEditorGltfBrowserRow>,
            Without<SceneMeshObjectsHeading>,
        ),
    >,
    mut headings: Query<(Entity, &mut BackgroundColor), With<SceneMeshObjectsHeading>>,
) {
    if let Some(target) = drag.hovered
        && let Ok(mut fill) = rows.get_mut(target)
    {
        fill.0 = colors::SELECT_BG_HOVER;
    }
    for (entity, mut fill) in &mut headings {
        let next = if drag.hovered == Some(entity) {
            colors::SELECT_BG_HOVER
        } else {
            colors::PANEL_BG
        };
        if fill.0 != next {
            fill.0 = next;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fast_drag_uses_the_original_row_even_when_pressed_and_released_in_one_frame() {
        use bevy::{
            input::{ButtonState, mouse::MouseButtonInput},
            window::{CursorMoved, WindowEvent},
        };
        use feathrise_editor_ui::InteractionButton;

        let (mut app, window) = crate::ui::terrain_objects::page_tests::page_without_tree_input();
        app.init_resource::<PlacementTreeDrag>()
            .add_systems(Update, capture.in_set(SceneEditorSystemSet::Input));
        let mut record = crate::viewport::placement_move::interaction_tests::record();
        record.asset_path = "models/source.glb".into();
        let source_id = record.id.clone();
        let world = app.world_mut();
        let (scene_id, revision) = {
            let mut session = world.resource_mut::<EditorAuthoringSession>();
            let revision = session.revision;
            let draft = session.draft.as_mut().unwrap();
            draft.placements.placements = vec![record];
            (draft.scene_id.clone(), revision)
        };
        let target = |key| SceneObjectTarget {
            scene_id: scene_id.clone(),
            revision,
            ..object(key)
        };
        let source = target(SceneObjectKey::Placement {
            index: 0,
            id: source_id,
            mesh: None,
        });
        let destination = target(SceneObjectKey::PlacementTreeRoot);
        let row_node = |top| Node {
            position_type: PositionType::Absolute,
            left: px(720.0),
            top: px(top),
            width: px(170.0),
            height: px(23.0),
            ..default()
        };
        let source_row = world
            .spawn((
                InteractionButton,
                row_node(62.0),
                SceneObjectRow(Some(source.clone())),
                SceneEditorGltfBrowserRow {
                    slot: 0,
                    entry_index: Some(0),
                    entries_revision: 0,
                    main_glb: None,
                },
            ))
            .id();
        world.spawn((
            ChildOf(source_row),
            GltfRowCaption,
            Pickable::IGNORE,
            Node {
                width: percent(100),
                height: percent(100),
                ..default()
            },
        ));
        let destination_row = world
            .spawn((
                InteractionButton,
                row_node(146.0),
                SceneMeshObjectsHeading,
                SceneObjectRow(Some(destination.clone())),
            ))
            .id();
        app.update();
        app.update();
        let world = app.world_mut();
        let edge = |world: &mut World, entity, state| {
            let position = world.get::<UiGlobalTransform>(entity).unwrap().translation;
            world
                .get_mut::<Window>(window)
                .unwrap()
                .set_physical_cursor_position(Some(position.as_dvec2()));
            let cursor = CursorMoved {
                window,
                position,
                delta: None,
            };
            world.write_message(WindowEvent::CursorMoved(cursor));
            let mouse = MouseButtonInput {
                window,
                button: MouseButton::Left,
                state,
            };
            world.write_message(WindowEvent::MouseButtonInput(mouse));
            world.write_message(mouse);
        };
        edge(world, source_row, ButtonState::Pressed);
        edge(world, destination_row, ButtonState::Released);
        app.update();
        assert_eq!(
            app.world().resource::<PlacementTreeDrag>().dropped.as_ref(),
            Some(&(source, destination)),
        );
    }

    fn object(key: SceneObjectKey) -> SceneObjectTarget {
        SceneObjectTarget {
            scene_id: feathrise_scene_content::SceneId("场景树拖放".into()),
            revision: 1,
            key,
            name: String::new(),
            hidden: false,
            locked: false,
            copyable: true,
        }
    }

    #[test]
    fn group_and_root_accept_an_instance_but_never_an_internal_node() {
        let source = |mesh| {
            object(SceneObjectKey::Placement {
                index: 1,
                id: "摆放".into(),
                mesh,
            })
        };
        let mesh = super::super::EditorGltfMeshSelection {
            placement_id: "摆放".into(),
            node_path: vec!["网格".into()],
            mesh_name: "网格".into(),
            mesh_index: Some(0),
            node_index: Some(1),
            valid: true,
            orphaned_water_binding: false,
        };
        for key in [
            SceneObjectKey::PlacementTreeRoot,
            SceneObjectKey::PlacementFolder,
            SceneObjectKey::Group {
                first_member: 0,
                id: "首项".into(),
            },
        ] {
            let target = object(key);
            let session = EditorAuthoringSession::default();
            let browser = EditorGltfBrowserState::default();
            assert!(drop_allowed(&source(None), &target, &browser, &session));
            assert!(!drop_allowed(
                &source(Some(mesh.clone())),
                &target,
                &browser,
                &session
            ));
        }
    }
}
