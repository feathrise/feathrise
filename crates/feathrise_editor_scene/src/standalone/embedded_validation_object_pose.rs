//! 有限自有夹具只驱动原场景行、原移动模式和原手柄输入；绝不写临时姿态或作者记录。
use super::super::probe as author_probe;
use crate::{authoring::EditorAuthoringSession, components::SceneEditorViewport,
    resources::{SceneEditorState,SceneEditorViewportState,SceneEditorNumericScrubMouseGuard},
    types::EditorView, ui::{gltf_browser::EditorGltfBrowserState,terrain_objects::{self,TerrainObjectsEditorState}},
    viewport::{modes::{ViewportMode,ViewportModeState},placement_move::MoveCamera}};
use bevy::{prelude::*,ui::RelativeCursorPosition};
use feathrise_scene_content::{TerrainId,TerrainObjectTransform};
use feathrise_scene::TerrainObjectTransformPreview;
use serde_json::{Value,json};
use crate::ui::sidebars::{EditorNavigatorTab,EditorSidebarTab,SceneEditorSidebarState,SceneEditorSidebarTabButton};

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum Phase { Select,Row,Hover,Hold,Cancel,Escape,Release,Quiet,Clearing,Restoring,Done }
struct SavedView { center:Vec3,extent:f32,yaw:f32,pitch:f32,view:EditorView,orthographic:bool,selection:Option<String>,navigator:EditorNavigatorTab }
#[derive(Resource,Default)]
struct State { saved:Option<SavedView>,phase:Option<Phase>,pulse:Option<Entity>,origin:Option<TerrainObjectTransform>,
    cursor:Option<Vec2>,delta:f32,error:Option<String>,hover_only:bool,hover_target:u8 }

pub(super) fn install(app:&mut App){
    app.init_resource::<State>().add_systems(Update,inject.after(feathrise_editor_ui::EditorUiSystemSet::NumericInput).before(crate::SceneEditorSystemSet::Input));
}

pub fn start(world:&mut World)->Result<(),String>{
    author_probe(world)?;
    if world.resource::<State>().saved.is_some(){return Err("地形整体姿态验收已启动".into())}
    if world.resource::<ButtonInput<MouseButton>>().any_pressed([MouseButton::Left,MouseButton::Right,MouseButton::Middle]) {
        return Err("地形整体姿态验收需要安静的鼠标边沿".into());
    }
    let origin=world.resource::<EditorAuthoringSession>().draft.as_ref()
        .and_then(|draft|draft.manifest.terrains.as_ref()).filter(|values|values.len()<=8)
        .and_then(|values|values.iter().find(|value|value.id.as_str()=="starter-ground"))
        .map(|value|value.transform).ok_or("自有夹具缺少starter-ground")?;
    let viewport=world.resource::<SceneEditorViewportState>();
    let editor=world.resource::<SceneEditorState>();
    let saved=SavedView {center:viewport.center,extent:viewport.extent,yaw:viewport.yaw,pitch:viewport.pitch,
        view:editor.view,orthographic:editor.orthographic,
        selection:world.resource::<EditorGltfBrowserState>().selected_placement_id.clone(),
        navigator:world.resource::<SceneEditorSidebarState>().navigator};
    crate::viewport::modes::validation_press_select(world)?;
    navigator(world,EditorNavigatorTab::SceneTree)?;
    {let mut pointer=world.resource_mut::<super::Pointer>();pointer.enabled=false;pointer.down=false;}
    {let mut editor=world.resource_mut::<SceneEditorState>();editor.view=EditorView::Perspective;editor.orthographic=false;}
    {let mut viewport=world.resource_mut::<SceneEditorViewportState>();viewport.center=Vec3::from_array(origin.translation);
        viewport.extent=16.;viewport.yaw=0.55;viewport.pitch=0.65;}
    *world.resource_mut::<State>()=State {saved:Some(saved),phase:Some(Phase::Select),origin:Some(origin),..default()};
    Ok(())
}

pub fn move_pointer(world:&mut World,second:bool)->Result<(),String>{
    author_probe(world)?;
    if world.resource::<State>().phase!=Some(Phase::Hold) || !world.resource::<TerrainObjectsEditorState>().transforming {
        return Err("原地形手柄尚未取得本笔鼠标".into());
    }
    world.resource_mut::<State>().delta=if second {2.} else {1.};
    Ok(())
}

/// 只走原场景选择及指针命中，不制造拖动状态；供有限悬停截图链复用同一相机与归还流程。
pub fn start_hover(world:&mut World)->Result<(),String>{
    start(world)?;
    world.resource_mut::<State>().hover_only=true;
    Ok(())
}

pub fn hover_target(world:&mut World,target:u8)->Result<(),String>{
    author_probe(world)?;
    let mut state=world.resource_mut::<State>();
    if !state.hover_only || target>2 {return Err("地形悬停验收目标无效".into())}
    state.hover_target=target;
    Ok(())
}

pub fn end_hover(world:&mut World)->Result<(),String>{
    author_probe(world)?;
    let mut state=world.resource_mut::<State>();
    if !state.hover_only || state.phase!=Some(Phase::Hover) {return Err("地形悬停尚未完成原选择链".into())}
    state.phase=Some(Phase::Quiet);
    Ok(())
}
pub fn cancel(world:&mut World)->Result<(),String>{
    author_probe(world)?;
    if world.resource::<State>().phase!=Some(Phase::Hold){return Err("地形原手柄尚未按住".into())}
    world.resource_mut::<State>().phase=Some(Phase::Cancel);Ok(())
}

fn pointer(world:&mut World,state:&mut State)->Result<(),String>{
    let mut nodes=world.query_filtered::<&ComputedNode,With<SceneEditorViewport>>();
    let node=nodes.single(world).map_err(|_|"原中央视口尚未布局")?;
    let camera=MoveCamera::new(world.resource::<SceneEditorState>(),world.resource::<SceneEditorViewportState>(),node.inverse_scale_factor())
        .ok_or("原地形手柄相机不可用")?;
    let origin=Vec3::from_array(state.origin.unwrap().translation);
    let length=camera.handle_length(origin).ok_or("原X手柄在相机后方")?;
    let cursor=if state.hover_only {
        match state.hover_target {
            0=>Vec2::new(-0.38,0.32), // 原点之外的安静区域，必须由原pick确认没有命中。
            1=>camera.project(origin+Vec3::X*(length*0.65)).ok_or("原X手柄投影无效")?,
            _=>{
                let fraction=(feathrise_author_tools::AUTHOR_PLANE_INNER_FRACTION
                    +feathrise_author_tools::AUTHOR_PLANE_OUTER_FRACTION)*0.5;
                camera.project(origin+Vec3::new(length*fraction,0.,length*fraction)).ok_or("原XZ手柄投影无效")?
            }
        }
    } else {camera.project(origin+Vec3::X*(length*0.65+state.delta)).ok_or("原X手柄投影无效")?};
    if cursor.abs().max_element()>=0.48{return Err("地形手柄验收落点超出中央视口".into())}
    state.cursor=Some(cursor);
    for mut cursor_state in world.query_filtered::<&mut RelativeCursorPosition,With<SceneEditorViewport>>().iter_mut(world){
        cursor_state.cursor_over=true;cursor_state.normalized=Some(cursor);
    }
    Ok(())
}

fn inject(world:&mut World){
    let mut state=world.remove_resource::<State>().unwrap();
    let result=(||->Result<(),String>{
        match state.phase {
            Some(Phase::Select)=>{
                if world.resource::<ViewportModeState>().mode!=ViewportMode::Select
                    || !world.resource::<SceneEditorSidebarState>().scene_tree_visible(){return Ok(())}
                state.pulse=Some(terrain_objects::validation_select_row(world,&TerrainId::parse("starter-ground")?)?);
                state.phase=Some(Phase::Row);
            }
            Some(Phase::Row)=>{
                if !world.resource::<TerrainObjectsEditorState>().selected.as_ref().is_some_and(|id|id.as_str()=="starter-ground"){return Ok(())}
                if let Some(entity)=state.pulse.take(){super::super::release_pulse(world,entity);}
                world.resource_scope(|world,mut editor:Mut<SceneEditorState>|{
                    crate::viewport::placement_move::set_placement_mode(&mut editor,Some(&mut world.resource_mut::<ViewportModeState>()),ViewportMode::Move);
                });
                pointer(world,&mut state)?;
                state.phase=Some(Phase::Hover);
            }
            Some(Phase::Hover)=>{
                pointer(world,&mut state)?;
                if !state.hover_only {
                    world.resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
                    state.phase=Some(Phase::Hold);
                }
            }
            Some(Phase::Hold)=>{pointer(world,&mut state)?;}
            Some(Phase::Cancel)=>{
                pointer(world,&mut state)?;
                world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
                state.phase=Some(Phase::Escape);
            }
            Some(Phase::Escape)=>{
                world.resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::Escape);
                world.resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
                state.phase=Some(Phase::Release);
            }
            Some(Phase::Release)=>{state.phase=Some(Phase::Quiet);}
            _=>{}
        }
        Ok(())
    })();
    if let Err(error)=result{
        state.error=Some(error);state.phase=Some(Phase::Quiet);
        world.resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        world.resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::Escape);
    }
    world.insert_resource(state);
}

pub fn probe(world:&World)->Result<Value,String>{
    let author=author_probe(world)?;
    let state=world.resource::<State>();
    if let Some(error)=&state.error{return Err(error.clone())}
    let session=world.resource::<EditorAuthoringSession>();
    let draft=session.draft.as_ref().unwrap();
    let id=TerrainId::parse("starter-ground")?;
    let pose=world.resource::<TerrainObjectTransformPreview>().for_object(&draft.scene_id,&id);
    let author_pose=draft.manifest.terrains.as_ref().and_then(|values|values.iter().find(|value|value.id==id)).map(|value|value.transform);
    let transforming=world.resource::<TerrainObjectsEditorState>().transforming;
    let down=world.resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left);
    let idle=state.phase==Some(Phase::Quiet)&&!down&&!transforming
        &&!world.resource::<SceneEditorNumericScrubMouseGuard>().blocked();
    let expected=state.origin.map(|mut origin|{if matches!(state.phase,Some(Phase::Hold)){origin.translation[0]+=state.delta;}origin.translation});
    Ok(json!({"phase":format!("{:?}",state.phase),"ready_to_move":state.phase==Some(Phase::Hold)&&transforming&&down,
        "hover_ready":state.hover_only&&state.phase==Some(Phase::Hover),"transforming":transforming,
        "canvas":{"active":draft.terrain_documents.active.as_ref().map(|active|active.definition.id.as_str()),
            "loading":draft.terrain_documents.loading},
        "canvas":{"active":draft.terrain_documents.active.as_ref().map(|active|active.definition.id.as_str()),
            "loading":draft.terrain_documents.loading},
        "idle":idle,"active":pose.is_some()&&transforming,"mouse_down":down,"author":author,
        "author_pose":author_pose,"preview_pose":pose,"expected_translation":expected,"cursor":state.cursor.map(|value|value.to_array())}))
}

pub fn restore(world:&mut World)->Result<bool,String>{
    author_probe(world)?;
    let phase=world.resource::<State>().phase;
    if phase==Some(Phase::Done){return Ok(true)}
    if phase==Some(Phase::Restoring){
        if world.resource::<ViewportModeState>().mode!=ViewportMode::Select{return Ok(false)}
        world.resource_mut::<State>().phase=Some(Phase::Done);return Ok(true)
    }
    if phase!=Some(Phase::Clearing){
        if probe(world)?["idle"]!=true{return Ok(false)}
        crate::viewport::modes::validation_press_select(world)?;
        world.resource_mut::<State>().phase=Some(Phase::Clearing);
        return Ok(false)
    }
    if world.resource::<ViewportModeState>().mode!=ViewportMode::Select{return Ok(false)}
    if world.resource::<TerrainObjectsEditorState>().selected.is_some(){
        // 走原空命中出口清除整对象选择；仅改浏览器选择会留下地形高亮，污染后续线框取证。
        let mut candidates=world.resource_mut::<crate::viewport::content_selection::LayerCandidateState>();
        candidates.clear_for_scene_tree();candidates.additive=false;candidates.clear_objects=true;
        return Ok(false)
    }
    let saved=world.resource_mut::<State>().saved.take().ok_or("地形姿态验收缺少原编辑视角")?;
    if let Some(selection)=saved.selection {super::select_numeric_placement(world,&selection)?;}
    else {world.resource_mut::<EditorGltfBrowserState>().clear_viewport_selection();crate::viewport::modes::validation_press_select(world)?;}
    {let mut editor=world.resource_mut::<SceneEditorState>();editor.view=saved.view;editor.orthographic=saved.orthographic;}
    {let mut viewport=world.resource_mut::<SceneEditorViewportState>();viewport.center=saved.center;viewport.extent=saved.extent;viewport.yaw=saved.yaw;viewport.pitch=saved.pitch;}
    navigator(world,saved.navigator)?;
    world.resource_mut::<State>().phase=Some(Phase::Restoring);Ok(false)
}

fn navigator(world:&mut World,tab:EditorNavigatorTab)->Result<(),String>{
    let entity=world.query::<(Entity,&SceneEditorSidebarTabButton)>().iter(world)
        .find_map(|(entity,value)|(value.0==EditorSidebarTab::Navigator(tab)).then_some(entity))
        .ok_or("原场景导航页签尚未建立")?;
    world.entity_mut(entity).insert(feathrise_editor_ui::EditorButtonClick::triggered());Ok(())
}
