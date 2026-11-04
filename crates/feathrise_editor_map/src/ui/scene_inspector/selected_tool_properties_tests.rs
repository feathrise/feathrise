//! 真实三套表单往返属性与工具宿主，保证唯一数字根、焦点和作者身份都不被重建。

use super::*;
use feathrise_editor_ui::{ActiveNumericInput, NumericInputEditState, NumericInputValue};
use feathrise_map_content::{MapBoundariesAsset, MapPortalRecord, MapPortalTarget, MapSurfaceAsset, MapSurfaceLayer};

/// CPU迁移与GPU工作台共同采用一套真实协议样例，不替换网格清单、地图身份、作者修订或序列号。
fn install_sample_objects(draft: &mut crate::authoring::EditorMapDraft) -> [LayerTarget; 3] {
    draft.surface = Some(MapSurfaceAsset { schema_version: 1, map_id: draft.map_id.clone(),
        resolution: [1, 1], weight_source_path: "weights.rgba8".into(), weight_texture_path: "weights.ktx2".into(),
        index_source_path: "indices.rgba32".into(), index_texture_path: "indices.ktx2".into(),
        layers: vec![MapSurfaceLayer { id: "soil".into(), label: "草地过渡地表".into(), preset_id: "soil".into() }] });
    draft.portals.portals = vec![MapPortalRecord { terrain_binding: None, arrival_terrain_binding: None,
        id: "portal".into(), label: "山谷入口".into(), label_customized: true,
        position: [1.0, 0.0, 1.0], arrival: [2.0, 0.0, 2.0],
        target: MapPortalTarget::default_spawn(draft.map_id.clone()), color_srgba: [1.0; 4], arrow_degrees: 0.0 }];
    draft.boundaries = Some(MapBoundariesAsset::rectangular_default(&draft.map_id, [0.0; 2], [10.0; 2], 0.0, 20.0).unwrap());
    [LayerTarget::Surface("soil".into()), LayerTarget::Portal("portal".into()),
        LayerTarget::Boundary(draft.boundaries.as_ref().unwrap().paths[0].id.clone())]
}

/// 仅调用一次：完整复用原三类表单的固定控件与真实数值回显，不安装业务输入、文档IO或任务轮询。
pub(crate) fn install_selected_tool_properties_snapshot_presentation(app: &mut App) {
    use crate::ui::portal_list;
    app.init_resource::<crate::resources::MapBrowserState>()
        .init_resource::<portal_list::PortalRowEntityRevision>()
        .init_resource::<crate::authoring::BoundaryThicknessInputState>()
        .init_resource::<feathrise_editor_core::EditorSettings>()
        .init_resource::<ActiveNumericInput>()
        .init_resource::<feathrise_editor_ui::ActiveNumericInputScrub>();
    if !app.world().contains_resource::<Assets<feathrise_map_content::MapManifestAsset>>() {
        app.init_asset::<feathrise_map_content::MapManifestAsset>();
    }
    if !app.world().contains_resource::<Assets<feathrise_map_content::MapPortalsAsset>>() {
        app.init_asset::<feathrise_map_content::MapPortalsAsset>();
    }
    if !app.world().contains_resource::<Messages<feathrise_editor_ui::NumericInputSetValue>>() {
        app.add_message::<feathrise_editor_ui::NumericInputSetValue>();
    }
    app.add_systems(Update, (
        crate::authoring::setup_boundary_thickness_input,
        crate::authoring::sync_boundary_thickness_input,
        portal_list::spawn_portal_list_rows,
        portal_list::sync_portal_selection_panel,
        portal_list::sync_current_portal_rows,
        portal_list::sync_target_portal_rows,
        portal_list::sync_portal_name_editor,
        crate::authoring::sync_boundary_text,
        crate::ui::layer_status::sync_editor_layer_status,
    ).chain().after(sync_selected_tool_properties).in_set(crate::MapEditorSystemSet::Presentation));
    crate::authoring::boundary::objects::install_snapshot_presentation(app);
    crate::ui::surface_layers::install_snapshot_presentation(app);
}

/// 仅供内存离屏夹具切换所选表单；不安装运行时作者任务，也不触发地图或文档文件IO。
pub(crate) fn set_selected_tool_properties_fixture(app: &mut App, kind: Option<SelectedToolPropertiesKind>) {
    use crate::ui::{sidebars::EditorLeftSidebarTab, terrain_objects::TerrainObjectsEditorState};
    app.init_resource::<LayerCandidateState>().init_resource::<MapEditorSidebarState>()
        .init_resource::<crate::resources::MapBrowserState>();
    let target = if let Some(kind) = kind {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let [surface, portal, boundary] = install_sample_objects(session.draft.as_mut().expect("工作台夹具必须已有内存草稿"));
        session.portal_revision = session.portal_revision.wrapping_add(1); // 回显采用同一份新样例图，不复用上一场景的列表签名。
        let target = match kind { SelectedToolPropertiesKind::Surface => surface,
            SelectedToolPropertiesKind::Portal => portal, SelectedToolPropertiesKind::Boundary => boundary };
        session.selected_boundary_path = match &target {
            LayerTarget::Boundary(id) => Some((session.draft.as_ref().unwrap().map_id.clone(), id.clone())), _ => None,
        };
        Some(target)
    } else {
        app.world_mut().resource_mut::<EditorAuthoringSession>().selected_boundary_path = None;
        None
    };
    {
        let mut editor = app.world_mut().resource_mut::<MapEditorState>();
        editor.tool = crate::types::EditorTool::Inspect;
        editor.selected_current_portal_id = matches!(kind, Some(SelectedToolPropertiesKind::Portal)).then(|| "portal".into());
        if kind == Some(SelectedToolPropertiesKind::Surface) {
            editor.selected_surface_layer = 0; editor.surface_selected_preset_id = Some("soil".into());
        }
    }
    if let Some(modes) = app.world_mut().get_resource_mut::<crate::viewport::modes::ViewportModeState>().as_mut() {
        modes.mode = crate::viewport::modes::ViewportMode::Select;
    }
    if kind == Some(SelectedToolPropertiesKind::Portal) {
        let map = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().map_id.clone();
        app.world_mut().resource_mut::<crate::resources::MapBrowserState>()
            .select_only(Some(crate::types::MapBrowserItem::Map(map)));
    } // 同图目标直接复用内存清单和端点，让目标列表也显示正式值，不发起其他地图IO。
    if let Some(target) = target {
        app.world_mut().resource_scope(|world, mut selection: Mut<LayerCandidateState>| {
            let generation = world.get_resource::<crate::game_authoring::map_interaction::MapInteractionState>().map_or(0, |state| state.observed_generation);
            let filter = world.get_resource::<crate::viewport::modes::ViewportModeState>()
                .map_or_else(Default::default, |state| state.selection_filter(world.resource::<MapEditorState>()));
            selection.confirm_tree_author(target, Some(0), world.resource::<EditorAuthoringSession>(), generation, filter);
        });
    } else {
        // 恢复同一Clipmap草稿已有的网格选择；保留网格对象、LOD配置与作者序列。
        let terrain = {
            let world = app.world(); let draft = world.resource::<EditorAuthoringSession>().draft.as_ref().unwrap();
            let state = world.get_resource::<TerrainObjectsEditorState>();
            let objects = draft.manifest.terrains.as_deref().unwrap_or(&[]);
            let index = state.and_then(|state| state.selected_index)
                .filter(|index| objects.get(*index).is_some_and(|object| state.and_then(|state| state.selected.as_ref()) == Some(&object.id))).unwrap_or(0);
            objects.get(index).map(|object| (draft.map_id.clone(), object.id.clone(), index))
        };
        if let Some((map, id, index)) = terrain {
            if let Some(mut state) = app.world_mut().get_resource_mut::<TerrainObjectsEditorState>() {
                state.map = Some(map); state.selected = Some(id.clone()); state.selected_index = Some(index);
            }
            app.world_mut().resource_mut::<LayerCandidateState>().confirm_terrain(&id, index);
        } else { app.world_mut().resource_mut::<LayerCandidateState>().clear_for_scene_tree(); }
    }
    let mut sidebar = app.world_mut().resource_mut::<MapEditorSidebarState>();
    sidebar.left = if kind.is_some() { EditorLeftSidebarTab::Environment } else { EditorLeftSidebarTab::Meshes };
    sidebar.right = EditorRightSidebarTab::Scene;
}

fn select(app: &mut App, target: LayerTarget) {
    let kind = match &target { LayerTarget::Surface(_) => SelectedToolPropertiesKind::Surface,
        LayerTarget::Portal(_) => SelectedToolPropertiesKind::Portal,
        LayerTarget::Boundary(_) => SelectedToolPropertiesKind::Boundary, _ => unreachable!() };
    set_selected_tool_properties_fixture(app, Some(kind));
    assert_eq!(app.world().resource::<LayerCandidateState>().selected.as_ref(), Some(&target));
    // 正式选择同时发布领域目标；等权威NumericSetValue经过公共控件后，再模拟新的精确输入。
    // 不能只写候选ID而漏掉selected_boundary_path，否则墙体字段会被正确禁用并回退到默认值。
    app.update();
    app.update();
}

#[test]
fn selected_forms_reparent_real_controls_and_return_to_their_original_homes() {
    let (mut app, _) = crate::ui::scene_inspector::tests::page(0);
    app.world_mut().spawn_scene(crate::ui::terrain_sidebar::terrain_tools_panel()).unwrap();
    app.world_mut().spawn_scene(crate::ui::gameplay_sidebar::gameplay_tools_panel()).unwrap();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        install_sample_objects(session.draft.as_mut().unwrap());
    }
    app.add_systems(Update, sync_selected_tool_properties.before(crate::ui::scene_inspector::sync_scene_inspector));
    install_selected_tool_properties_snapshot_presentation(&mut app);
    app.update();
    let world = app.world_mut();
    let homes = world.query::<(Entity, &SelectedToolProperties, &ChildOf)>().iter(world)
        .map(|(entity, form, parent)| (entity, form.kind, parent.parent())).collect::<Vec<_>>();
    assert_eq!(homes.len(), 3);
    let names = world.query_filtered::<Entity, (With<bevy::text::EditableText>, With<crate::ui::text_input::EditorTextInput>)>()
        .iter(world).filter_map(|entity| {
            let mut ancestor = entity;
            while let Some(parent) = world.get::<ChildOf>(ancestor) {
                ancestor = parent.parent();
                if let Some((_, kind, _)) = homes.iter().find(|(root, _, _)| *root == ancestor) {
                    return Some((*kind, entity));
                }
            }
            None
        }).collect::<Vec<_>>();
    assert_eq!(names.len(), 2);
    for (_, entity) in &names {
        world.get_mut::<bevy::text::EditableText>(*entity).unwrap().editor.set_scale(1.75);
    } // 模拟正式UI已同步的字体缩放；回显重建编辑器会把它丢回1.0，导致文字不可见。
    let mut numeric = world.query_filtered::<Entity, With<NumericInputValue>>().iter(world).collect::<Vec<_>>();
    numeric.sort_unstable_by_key(|entity| entity.to_bits());
    let slot = world.query_filtered::<Entity, With<crate::authoring::BoundaryThicknessInputSlot>>().single(world).unwrap();
    let roots = world.get::<Children>(slot).unwrap().iter().collect::<Vec<_>>();
    assert_eq!(roots.len(), 3, "墙体仍只有唯一的三项数值根");
    world.resource_mut::<MapEditorSidebarState>().left = crate::ui::sidebars::EditorLeftSidebarTab::Environment;
    let boundary = world.resource::<EditorAuthoringSession>().draft.as_ref().unwrap().boundaries.as_ref().unwrap().paths[0].id.clone();
    let revision = world.resource::<EditorAuthoringSession>().revision;
    for (kind, target) in [(SelectedToolPropertiesKind::Surface, LayerTarget::Surface("soil".into())),
        (SelectedToolPropertiesKind::Portal, LayerTarget::Portal("portal".into())),
        (SelectedToolPropertiesKind::Boundary, LayerTarget::Boundary(boundary))] {
        select(&mut app, target);
        if let Some((_, entity)) = names.iter().find(|(name_kind, _)| *name_kind == kind) {
            let text = app.world().get::<bevy::text::EditableText>(*entity).unwrap();
            assert!(!text.value().to_string().is_empty());
            assert_eq!(text.editor.get_scale(), 1.75, "名称回显不能丢弃同一输入编辑器的字体与缩放样式");
        }
        if kind == SelectedToolPropertiesKind::Boundary {
            let world = app.world_mut();
            assert!(world.resource::<EditorAuthoringSession>().selected_boundary_path.is_some());
            assert!(!world.entity(roots[0]).contains::<feathrise_editor_ui::NumericInputDisabled>());
            assert_eq!(world.get::<NumericInputValue>(roots[0]).unwrap().0.as_f32(), Some(2.0));
            world.resource_mut::<ActiveNumericInput>().entity = Some(roots[0]);
            let mut edit = world.get_mut::<NumericInputEditState>(roots[0]).unwrap();
            edit.editing = true; edit.buffer = "2.75".into();
            app.update();
            assert_eq!(app.world().resource::<ActiveNumericInput>().entity, Some(roots[0]));
            assert_eq!(app.world().get::<NumericInputEditState>(roots[0]).unwrap().buffer, "2.75");
            // 模拟随后点击其他页签时的公共失焦；隐藏字段不应继续占用键盘。
            app.world_mut().resource_mut::<ActiveNumericInput>().entity = None;
            app.world_mut().get_mut::<NumericInputEditState>(roots[0]).unwrap().editing = false;
        }
        let world = app.world_mut();
        let host = world.query_filtered::<Entity, With<SelectedToolPropertiesHost>>().single(world).unwrap();
        for (entity, form_kind, home) in &homes {
            assert_eq!(world.get::<ChildOf>(*entity).unwrap().parent(), if *form_kind == kind { host } else { *home });
        }
        assert_eq!(world.resource::<MapEditorSidebarState>().left, crate::ui::sidebars::EditorLeftSidebarTab::Environment);
        world.resource_mut::<MapEditorSidebarState>().right = EditorRightSidebarTab::Tools;
        app.update();
        for (entity, _, home) in &homes {
            assert_eq!(app.world().get::<ChildOf>(*entity).unwrap().parent(), *home);
        }
    }
    let world = app.world_mut();
    let mut after = world.query_filtered::<Entity, With<NumericInputValue>>().iter(world).collect::<Vec<_>>();
    after.sort_unstable_by_key(|entity| entity.to_bits());
    assert_eq!(numeric, after, "页签往返不能复制或重建输入实体");
    assert_eq!(world.resource::<ActiveNumericInput>().entity, None);
    assert_eq!(world.resource::<EditorAuthoringSession>().revision, revision);
    assert!(!world.resource::<MapEditorState>().dirty);
    assert_eq!(world.query_filtered::<Entity, With<crate::authoring::boundary::objects::WallPanelSlot>>().iter(world).count(), 1);
}
