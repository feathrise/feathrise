//! 场景树以固定行池解析拖拽，外层归属送原摆放管理，内部节点送 GLB 作者事务。

use super::{
    EditorGltfBrowserState, MainSceneGlbRow, SceneEditorGltfBrowserRow, SceneTreeContents, SceneTreeRootRow,
    systems::GltfRowCaption,
};
use crate::{
    SceneEditorSystemSet,
    authoring::EditorAuthoringSession,
    components::SceneEditorGltfBrowserRowGlyph,
    resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState},
    ui::{
        asset_icons,
        numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard,
        scene_context_menu::{
            MenuContext, SceneContextMenuState, SceneMenuAction, SceneMenuDispatch, SceneObjectKey,
            SceneObjectRow, SceneObjectTarget,
        },
        scene_objects::{SceneObjectState, create_folder, move_folder, queue_group_drop},
        scene_source_glb::SourceGlbRequest,
    },
};
use bevy::{
    camera::NormalizedRenderTarget,
    picking::pointer::{PointerAction, PointerButton, PointerId, PointerInput},
    prelude::*,
    ui::{CalculatedClip, ComputedUiTargetCamera, RelativeCursorPosition},
    window::PrimaryWindow,
};
use feathrise_editor_ui::{EditorButtonClick, UiInteraction, localization::EditorLocale};

struct Press {
    source: SceneObjectTarget,
    label: String,
    entries_revision: u64,
    origin: Vec2,
}

#[derive(Resource, Default)]
pub(crate) struct PlacementTreeDrag {
    press: Option<Press>,
    active: bool,
    hovered: Option<Entity>,
    rejected: bool,
    dropped: Option<(SceneObjectTarget, SceneObjectTarget)>,
}

impl PlacementTreeDrag {
    pub(crate) fn owns_mouse(&self) -> bool {
        self.active
    }

    pub(crate) fn highlights(&self, row: Entity) -> bool {
        self.hovered == Some(row)
    }

    pub(crate) fn preview(&self) -> Option<(&str, bool)> {
        let press = self.press.as_ref().filter(|_| self.active)?;
        Some((&press.label, self.rejected))
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
            sync_root_row
                .after(super::sync_gltf_browser_rows)
                .before(super::sync_gltf_row_toggles)
                .in_set(SceneEditorSystemSet::Presentation),
        );
}

pub(super) fn sync_root_row(
    editor: Res<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    browser: Res<EditorGltfBrowserState>,
    drag: Res<PlacementTreeDrag>,
    locale: Option<Res<EditorLocale>>,
    server: Option<Res<AssetServer>>,
    images: Option<Res<Assets<Image>>>,
    mut rows: Query<(
        Entity, &Children, &UiInteraction, &mut EditorButtonClick, &mut SceneEditorGltfBrowserRow,
        &mut SceneObjectRow, &mut Node, &mut BackgroundColor, &mut BorderColor,
    ), (With<SceneTreeRootRow>, Without<SceneTreeContents>)>,
    mut contents: Query<&mut Node, (With<SceneTreeContents>, Without<SceneTreeRootRow>)>,
    captions: Query<&Children, With<GltfRowCaption>>,
    mut labels: Query<&mut Text>,
    mut icons: Query<&mut ImageNode, With<SceneEditorGltfBrowserRowGlyph>>,
) {
    let draft = session.draft.as_ref().filter(|draft| {
        editor.open && !editor.scene_transition_in_progress
            && editor.selected_scene_id.as_ref() == Some(&draft.scene_id)
    });
    let main = draft.filter(|draft| draft.placement_glb_hash.is_some());
    let kind = main.map(|_| MainSceneGlbRow::File);
    let locale = locale.as_deref().copied().unwrap_or_default();
    for (entity, children, interaction, mut click, mut row, mut object, mut node, mut background, mut border) in &mut rows {
        let display = if draft.is_some() { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
        if row.main_glb != kind || row.entries_revision != browser.published_entries_revision
            || object.0.as_ref().map(|target| &target.scene_id) != draft.map(|draft| &draft.scene_id)
        {
            row.main_glb = kind;
            row.entries_revision = browser.published_entries_revision;
            click.invalidate(); // 固定行换场景或文件格式时丢弃上一行的按压。
        }
        if let Some(draft) = draft {
            // 老格式没有可选的主 GLB，沿用场景名称承接根级归类。
            let (key, name, label, icon) = if main.is_some() {
                (SceneObjectKey::MainSceneGlb, "scene.glb", locale.text("label.main-scene-glb"), asset_icons::GLB)
            } else {
                let name = draft.manifest.display_name.as_str();
                (SceneObjectKey::PlacementTreeRoot, name, name, "icons/editor/tree_scene.png")
            };
            if object.0.as_ref().is_none_or(|target| target.scene_id != draft.scene_id
                || target.revision != session.revision || target.key != key || target.name != name)
            {
                object.0 = Some(SceneObjectTarget {
                    scene_id: draft.scene_id.clone(), revision: session.revision,
                    key, name: name.into(), hidden: false, locked: false, copyable: false,
                });
            }
            if let Some(caption) = children.iter().find_map(|child| captions.get(child).ok()) {
                for child in caption.iter() {
                    if let Ok(mut text) = labels.get_mut(child) && text.as_str() != label {
                        label.clone_into(&mut text.0);
                    }
                    if let Some(server) = server.as_ref().filter(|_| images.is_some())
                        && let Ok(mut image) = icons.get_mut(child)
                        && image.image.path().is_none_or(|path| path.path() != std::path::Path::new(icon))
                    {
                        image.image = server.load(icon);
                        image.color = asset_icons::tint(icon, crate::ui::theme::TEXT_ACCENT);
                    }
                }
            }
        } else if object.0.is_some() {
            object.0 = None;
        }
        let highlighted = drag.highlights(entity);
        let selected = main.is_some_and(|draft| browser.is_main_glb_selected(&draft.scene_id));
        let interaction = if drag.owns_mouse() { UiInteraction::None } else { *interaction };
        let (fill, outline) = crate::ui::scene_sidebar::tree_row_colors(selected, highlighted, interaction);
        background.set_if_neq(BackgroundColor(fill));
        border.set_if_neq(outline);
    }
    let collapsed = main.is_some_and(|draft| browser.collapsed_main_glb.as_ref() == Some(&draft.scene_id));
    let display = if collapsed { Display::None } else { Display::Flex };
    for mut node in &mut contents {
        if node.display != display { node.display = display; }
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
        With<SceneTreeRootRow>,
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

fn folder(key: &SceneObjectKey) -> bool {
    matches!(key, SceneObjectKey::PlacementFolder | SceneObjectKey::Group { .. })
}

fn scene_root(key: &SceneObjectKey) -> bool {
    matches!(key, SceneObjectKey::MainSceneGlb | SceneObjectKey::PlacementTreeRoot)
}

fn editable_source(target: &SceneObjectTarget, session: &EditorAuthoringSession) -> bool {
    folder(&target.key) || (!target.locked
        && target.copyable
        && target.glb_path(session).is_some()
        && matches!(&target.key, SceneObjectKey::Placement { mesh, .. } if
            mesh.as_ref().is_none_or(|mesh| !target.hidden && mesh.node_index.is_some())))
}

fn drop_allowed(
    source: &SceneObjectTarget,
    target: &SceneObjectTarget,
    browser: &EditorGltfBrowserState,
    session: &EditorAuthoringSession,
) -> bool {
    if folder(&source.key) {
        let parent = if folder(&target.key) { Some(target.name.as_str()) }
            else if scene_root(&target.key) { None }
            else { return false; };
        // 两个空文件夹共用行类型，实际身份由当前场景的唯一名称区分。
        return session.draft.as_ref().is_some_and(|draft| draft.object_folders.can_move(&source.name, parent));
    }
    if source.key == target.key {
        return false;
    }
    match (&source.key, &target.key) {
        (
            SceneObjectKey::Placement { mesh: None, .. },
            parent,
        ) => scene_root(parent) || folder(parent),
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
    drag.rejected = false;
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
                .and_then(|row| target_for(row).map(|target| (origin, row.0, target)))
        } else {
            hovered()
                .find(|(_, target)| editable_source(target, &context.session))
                .map(|(entity, target)| (position, entity, target))
        };
        if let Some((origin, row, source)) =
            source.filter(|(_, _, target)| editable_source(target, &context.session))
        {
            let label = rows.get(row).ok().and_then(|row| row.5?.entry_index)
                .and_then(|index| browser.entries.get(index))
                .map_or_else(|| source.name.clone(), |entry| entry.display_name.clone());
            // 冻结作者身份，后续滚动行池可以换绑，但不能换掉拖动的对象。
            drag.press = Some(Press {
                source: source.clone(),
                label,
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
        let mut destination = None;
        if drag.active {
            for (entity, target) in hovered() {
                if drop_allowed(&press.source, target, &browser, &context.session) {
                    destination = Some((entity, target));
                    break;
                }
                drag.rejected = true;
            }
            drag.rejected &= destination.is_none();
        }
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
        if folder(&source.key) {
            let parent = folder(&target.key).then_some(target.name.as_str());
            match move_folder(&mut objects, &mut session, &mut editor, &source.name, parent) {
                Ok(true) => {
                    if let Some(parent) = parent { browser.expanded_groups.insert(parent.to_owned()); }
                    editor.status = parent.map_or_else(|| format!("已将文件夹「{}」移回根级", source.name),
                        |parent| format!("已将文件夹「{}」移入「{parent}」", source.name));
                }
                Ok(false) => {}
                Err(error) => editor.status = error,
            }
        } else if matches!(&source.key, SceneObjectKey::Placement { mesh: Some(_), .. }) {
            source_requests.write(SourceGlbRequest::Move {
                source,
                destination: target,
            });
        } else {
            // 整件 GLB 放到主文件根行时只调整场景树归属，保留资产引用。
            let name = folder(&target.key).then_some(target.name);
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
    if !browser.document_selection_ready(&request.target.scene_id)
        || !browser.placement_index_ready(session.placement_structure_revision)
    {
        editor.status = "场景树正在更新，请刷新完成后再新建文件夹".into();
        return;
    }
    if browser.placement_group_names.contains(name) {
        editor.status = "已存在同名文件夹".into();
        return;
    }
    let parent = folder(&request.target.key).then_some(request.target.name.as_str());
    match create_folder(&mut objects, &mut session, &mut editor, name, parent) {
        Ok(()) => {
            if let Some(parent) = parent { browser.expanded_groups.insert(parent.to_owned()); }
            editor.status = format!("已新建文件夹「{name}」");
        }
        Err(error) => editor.status = error,
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
            draft.placement_glb_hash = Some("测试主场景".into());
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
        let destination = target(SceneObjectKey::MainSceneGlb);
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
                SceneTreeRootRow,
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
            SceneObjectKey::MainSceneGlb,
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
