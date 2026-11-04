//! 新建模入口走正式输入、幽灵预览与单次历史；测试不伪造内部movement或直接调用拓扑算法。

use super::*;
use bevy::{ecs::system::RunSystemOnce, ui::RelativeCursorPosition};

fn ortho_app() -> (App, Entity, Entity) {
    let (mut app,root,node) = app(); app.world_mut().resource_mut::<MapEditorState>().orthographic = true;
    (app,root,node)
}

fn mouse(app: &mut App, down: bool) {
    let mut input = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
    if down { input.press(MouseButton::Left); } else { input.release(MouseButton::Left); }
}

fn selection(app: &mut App, node: Entity, kind: MeshElementKind, selected: u128) {
    request(app,MeshEditAction::Kind(kind)); fixture::step(app,node,Vec2::ZERO);
    app.world_mut().resource_mut::<MeshEditState>().selected = selected;
}

fn preview(app: &App) -> MapPlacementRecord {
    let movement = app.world().resource::<MeshEditState>().movement.as_ref().expect("正式动作应进入预览");
    assert!(movement.valid,"{}",app.world().resource::<MapEditorState>().status);
    movement.preview.clone()
}

fn assert_one_commit(app: &App, before: &MapPlacementRecord, after: &MapPlacementRecord) {
    assert_eq!(current(app),after); let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.undo.len(),1);
    assert!(matches!(&session.undo[0],EditorCommand::PlacementChanged { before:old,after:new,.. } if old==before && new==after));
    let mut restored = after.clone(); restored.asset_target = before.asset_target;
    assert_eq!(&restored,before,"元素操作只改本地几何，实例姿态及其他作者字段保持");
    assert!(!app.world().resource::<MeshEditState>().moving());
}

#[test]
fn modal_s_scales_only_selected_shared_points_and_cancel_then_commit_has_one_history() {
    let (mut app,_,node) = ortho_app(); enter(&mut app,node);
    app.world_mut().resource_mut::<MeshEditState>().selected = 0b11001100;
    let before = current(&app).clone(); let old = world_vertices(&before).unwrap();
    let pivot = selection_center(&old,0b11001100).unwrap(); let camera = fixture::camera(&app);
    let start = camera.project(pivot+Vec3::X*0.7).unwrap(); let end = camera.project(pivot+Vec3::X*0.85).unwrap();
    press(&mut app,KeyCode::KeyS); fixture::step(&mut app,node,start); fixture::step(&mut app,node,end);
    let candidate = preview(&app); assert_ne!(candidate.asset_target,before.asset_target);
    let scaled = world_vertices(&candidate).unwrap();
    for index in [0,1,4,5] { assert_eq!(scaled[index],old[index]); }
    for index in [2,3,6,7] { assert!(scaled[index].distance(old[index])>0.01); }
    assert_eq!(current(&app),&before); assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    request(&mut app,MeshEditAction::Cancel); fixture::step(&mut app,node,end);
    assert_eq!(current(&app),&before); assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    request(&mut app,MeshEditAction::Scale); fixture::step(&mut app,node,start); fixture::step(&mut app,node,end);
    let candidate = preview(&app); request(&mut app,MeshEditAction::Confirm); fixture::step(&mut app,node,end);
    assert_one_commit(&app,&before,&candidate); fixture::step(&mut app,node,end);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(),1);
}

#[test]
fn scaling_one_shared_point_is_noop_and_keeps_recipe_revision_and_redo() {
    let (mut app,_,node) = ortho_app(); enter(&mut app,node);
    app.world_mut().resource_mut::<MeshEditState>().selected = 1<<7;
    let before = current(&app).clone(); let revision = app.world().resource::<EditorAuthoringSession>().revision;
    app.world_mut().resource_mut::<EditorAuthoringSession>().redo.push(EditorCommand::PlacementChanged {record_index:0,before:before.clone(),after:before.clone()});
    let origin = world_vertices(&before).unwrap()[7]; let camera = fixture::camera(&app);
    let start = camera.project(origin+Vec3::X*0.5).unwrap(); let end = camera.project(origin+Vec3::X*0.8).unwrap();
    request(&mut app,MeshEditAction::Scale); fixture::step(&mut app,node,start); fixture::step(&mut app,node,end);
    assert_eq!(preview(&app),before); request(&mut app,MeshEditAction::Confirm); fixture::step(&mut app,node,end);
    assert_eq!(current(&app),&before); let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.revision,revision); assert!(session.undo.is_empty()); assert_eq!(session.redo.len(),1);
    assert!(matches!(current(&app).asset_target,MapPlacementAssetTarget::Cuboid{..}));
}

#[test]
fn b_box_selects_visible_elements_without_history_and_escape_or_outside_release_restores_selection() {
    for ending in 0..3 {
        let (mut app,_,node) = ortho_app(); enter(&mut app,node);
        let before = current(&app).clone(); app.world_mut().resource_mut::<MeshEditState>().selected = 1<<7;
        let camera = fixture::camera(&app); let vertices = world_vertices(&before).unwrap();
        let projected = vertices.iter().map(|point| camera.project(*point).unwrap()).collect::<Vec<_>>();
        let start = projected.iter().copied().fold(Vec2::splat(f32::INFINITY),Vec2::min)-Vec2::splat(0.01);
        let end = projected.iter().copied().fold(Vec2::splat(f32::NEG_INFINITY),Vec2::max)+Vec2::splat(0.01);
        press(&mut app,KeyCode::KeyB); fixture::step(&mut app,node,start);
        assert!(app.world().resource::<MeshEditState>().boxing()); mouse(&mut app,true); fixture::step(&mut app,node,start);
        fixture::step(&mut app,node,end); assert_eq!(current(&app),&before);
        if ending == 1 { request(&mut app,MeshEditAction::Cancel); }
        if ending == 2 { app.world_mut().entity_mut(node).get_mut::<RelativeCursorPosition>().unwrap().cursor_over = false; }
        mouse(&mut app,false); fixture::step(&mut app,node,end);
        let state = app.world().resource::<MeshEditState>(); assert!(!state.boxing());
        if ending == 0 {
            let expected = vertices.iter().enumerate().filter(|(_,point)| visible_point(camera,**point,&vertices)).fold(0u128,|mask,(index,_)|mask|(1<<index));
            assert_eq!(state.selected,expected); assert_eq!(state.selected.count_ones(),4);
        } else { assert_eq!(state.selected,1<<7); }
        assert_eq!(current(&app),&before); assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
        assert!(!app.world().resource::<MapEditorState>().dirty);
    }
}

fn install_history(app: &mut App) {
    use crate::authoring::*;
    app.insert_resource(EditorPreviewAssets {
        portal_base_mesh:default(),portal_arrow_head_mesh:default(),portal_arrow_shaft_mesh:default(),
        portal_edge_beam_mesh:default(),portal_edge_ring_mesh:default(),portal_base_material:default(),
        portal_arrow_material:default(),portal_edge_beam_material:default(),portal_edge_ring_material:default(),portal_label_font:default(),
    }).init_resource::<courtyards::CourtyardAuthoringState>()
        .init_resource::<GlobalEnvironmentAuthoringState>().init_resource::<WaterMaskMaterializeTask>()
        .init_resource::<crate::authoring::surface_layer_edit::SurfaceLayerEditTask>()
        .init_resource::<SurfaceBrushStroke>().init_resource::<WaterBrushStroke>()
        .init_resource::<crate::ui::placement_water::PlacementInstanceIndex>().init_resource::<WaterFileHandoff>();
}

#[test]
fn e_extrusion_preview_isolated_then_commits_serializes_and_real_undo_restores_original() {
    let (mut app,_,node) = ortho_app(); enter(&mut app,node); selection(&mut app,node,MeshElementKind::Face,1<<1);
    let before = current(&app).clone(); let vertices = world_vertices(&before).unwrap();
    let origin = selection_center(&vertices,element_vertices(MeshElementKind::Face,1)).unwrap(); let camera = fixture::camera(&app);
    let start = camera.project(origin).unwrap(); let end = camera.project(origin+Vec3::X*0.25).unwrap();
    press(&mut app,KeyCode::KeyE); fixture::step(&mut app,node,start); fixture::step(&mut app,node,end);
    let candidate = preview(&app); let geometry = candidate.asset_target.editable_geometry().unwrap();
    assert_eq!((geometry.vertex_count,geometry.face_count),(12,10)); assert_eq!(current(&app),&before);
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    request(&mut app,MeshEditAction::Confirm); fixture::step(&mut app,node,end); assert_one_commit(&app,&before,&candidate);
    let decoded: MapPlacementRecord = serde_json::from_str(&serde_json::to_string(current(&app)).unwrap()).unwrap(); assert_eq!(decoded,candidate);
    request(&mut app,MeshEditAction::Toggle); fixture::step(&mut app,node,end); install_history(&mut app);
    app.world_mut().resource_mut::<MapEditorState>().undo_requested = true;
    app.world_mut().run_system_once(crate::authoring::apply_undo_redo).unwrap();
    assert_eq!(current(&app),&before); assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    assert_eq!(app.world().resource::<EditorAuthoringSession>().redo.len(),1);
}

#[test]
fn each_modeling_action_uses_real_preview_confirm_and_one_history() {
    for (action,kind,mask) in [
        (MeshEditAction::Inset,MeshElementKind::Face,1<<3),
        (MeshEditAction::Bevel,MeshElementKind::Edge,1),
        (MeshEditAction::LoopCut,MeshElementKind::Edge,1),
        (MeshEditAction::Knife,MeshElementKind::Face,1<<4),
        (MeshEditAction::Subdivide,MeshElementKind::Face,(1<<6)-1),
    ] {
        let (mut app,_,node) = ortho_app(); enter(&mut app,node); selection(&mut app,node,kind,mask);
        let before = current(&app).clone(); let center = Vec3::from_array(before.translation); let camera = fixture::camera(&app);
        let start = camera.project(center-Vec3::Y*1.5).unwrap();
        let end = if action == MeshEditAction::Knife { camera.project(center+Vec3::Y*1.5).unwrap() } else {start};
        request(&mut app,action); fixture::step(&mut app,node,start); fixture::step(&mut app,node,end);
        let candidate = preview(&app); assert_ne!(candidate,before,"{action:?}必须生成实际拓扑");
        assert_eq!(current(&app),&before); assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
        candidate.asset_target.validate().unwrap(); request(&mut app,MeshEditAction::Confirm); fixture::step(&mut app,node,end);
        assert_one_commit(&app,&before,&candidate);
    }
}
