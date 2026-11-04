//! 显式自有夹具的原输入端口。只选择、发按钮/原始指针键盘消息；不改密度、Transform或历史。

use super::{probe, Installed};
use crate::{authoring::{EditorAuthoringSession, VegetationBrushStroke, terrain_documents::{self, TerrainDocumentTask}},
    components::{MapEditorActionButton, MapEditorViewport}, resources::{MapEditorState, MapEditorViewportState},
    types::{EditorAction, EditorTool, EditorView}, ui::{terrain_objects::TerrainObjectsEditorState,
    gltf_browser::EditorGltfBrowserState, placement_transform::{PlacementTransformField, PlacementTransformInputState}},
    viewport::modes::{ViewportMode, ViewportModeState}};
use bevy::{prelude::*, ui::RelativeCursorPosition};
use feathrise_map_content::{TerrainId, MapVegetationKind};
use feathrise_scene::DynamicBladeDraft;
use feathrise_editor_ui::NumericInputDisabled;
use serde_json::{Value, json};
use crate::ui::sidebars::{EditorRightSidebarTab,EditorSidebarTab,MapEditorSidebarState,MapEditorSidebarTabButton};

#[path = "embedded_validation_numeric.rs"]
mod numeric;

#[path = "embedded_validation_object_pose.rs"]
pub mod object_pose;

#[derive(Resource, Default)]
struct Pointer { enabled: bool, down: bool, was_down: bool, tail: bool }

pub(super) fn install(app: &mut App) {
    numeric::install(app);
    object_pose::install(app);
    app.init_resource::<Pointer>().add_systems(Update, inject
        .in_set(crate::MapEditorSystemSet::Cursor).before(crate::viewport::sync_editor_viewport_camera));
}

/// 原光标/相机/地面查询顺序与作者工具验收一致；不伪造brush_cursor_world或terrain命中。
fn inject(world: &mut World) {
    let (enabled, down, was_down, tail) = {
        let mut pointer = world.resource_mut::<Pointer>();
        let state = (pointer.enabled, pointer.down, pointer.was_down, pointer.tail);
        pointer.was_down = pointer.down;
        state
    };
    if was_down && !down { world.resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left); }
    if !enabled { return; }
    if let Some(mut mode) = world.get_resource_mut::<ViewportModeState>() { mode.mode=ViewportMode::Edit; mode.pointer_over_controls=false; }
    {
        let mut editor = world.resource_mut::<MapEditorState>();
        // 原草渲染器在正交视图退回最粗代理；微型夹具改用原透视视图，验证细笔划真实草而非改生产LOD。
        editor.tool=EditorTool::Vegetation; editor.view=EditorView::Perspective; editor.orthographic=false;
        editor.brush_radius=3.; editor.brush_strength=1.;
    }
    {
        let mut view=world.resource_mut::<MapEditorViewportState>();
        view.center=Vec3::new(-8.,0.,8.); view.extent=16.; view.yaw=0.; view.pitch=1.4;
    }
    for mut cursor in world.query_filtered::<&mut RelativeCursorPosition, With<MapEditorViewport>>().iter_mut(world) {
        cursor.cursor_over=true; cursor.normalized=Some(Vec2::new(if tail {0.12} else {-0.12},0.));
    }
    if down { world.resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left); }
}

pub fn focus_grass_terrain(world:&mut World)->Result<(),String> {
    let _=probe(world)?;
    let id=TerrainId::parse("starter-ground")?;
    let accepted=world.resource_scope(|world,session:Mut<EditorAuthoringSession>| {
        world.resource_scope(|world,mut task:Mut<TerrainDocumentTask>| {
            terrain_documents::request_focus(&mut task,&mut world.resource_mut::<TerrainObjectsEditorState>(),session.draft.as_ref().unwrap(),&id)
        })
    });
    if !accepted { return Err("连续预览原地形画布选择未受理".into()); }
    world.resource_mut::<MapEditorState>().terrain_canvas_target=Some(id);
    Ok(())
}

pub fn grass_terrain_ready(world:&World)->bool {
    let session=world.resource::<EditorAuthoringSession>();
    terrain_documents::canvas_writable(session,world.resource::<MapEditorState>())
        && session.draft.as_ref().and_then(|draft|draft.terrain_documents.active.as_ref())
            .is_some_and(|active|active.definition.id.as_str()=="starter-ground")
}

pub fn request_grass_layer(world:&mut World)->Result<Entity,String> {
    let _=probe(world)?;
    if !grass_terrain_ready(world) { return Err("真实植被画布未就绪".into()); }
    let mut editor=world.resource_mut::<MapEditorState>();
    editor.vegetation_selected_preset_id=Some("embedded-live-blade".into()); editor.tool=EditorTool::Vegetation;
    Ok(world.spawn((MapEditorActionButton(EditorAction::VegetationAddLayer),feathrise_editor_ui::EditorButtonClick::triggered())).id())
}

pub fn activate_grass(world:&mut World)->Result<bool,String> {
    let index={
        let session=world.resource::<EditorAuthoringSession>();
        let Some(vegetation)=session.draft.as_ref().and_then(|draft|draft.vegetation.as_ref()) else {return Ok(false)};
        let Some(output)=vegetation.layers.iter().find(|layer|layer.runtime_profile.as_deref()==Some("embedded-live-blade")) else {return Ok(false)};
        if output.kind!=MapVegetationKind::Blade {return Err("验收新层没有使用真实Blade后端".into())}
        let Some(index)=vegetation.authoring.as_ref().and_then(|author|author.layers.iter().position(|layer|layer.output_layer_id==output.id)) else {return Ok(false)};
        index
    };
    world.resource_scope(|world,session:Mut<EditorAuthoringSession>|crate::ui::actions::activate_vegetation_layer(&mut world.resource_mut::<MapEditorState>(),&session,index,None));
    world.resource_mut::<Pointer>().enabled=true;
    Ok(true)
}

pub fn grass_pointer(world:&mut World,down:bool,tail:bool)->Result<(),String> {
    let _=probe(world)?;
    let mut pointer=world.resource_mut::<Pointer>(); pointer.down=down; pointer.tail=tail;
    Ok(())
}

pub fn grass_probe(world:&World)->Result<Value,String> {
    let author=probe(world)?;
    let session=world.resource::<EditorAuthoringSession>();
    let draft=session.draft.as_ref().unwrap();
    let stroke=world.resource::<VegetationBrushStroke>();
    let editor=world.resource::<MapEditorState>();
    let published=world.resource::<DynamicBladeDraft>().frozen_for_map(&draft.map_id);
    Ok(json!({"author":author,"active":stroke.active,"changed_samples":stroke.before.len(),
        "last_world":stroke.last_world.map(|value|value.to_array()),"mouse_down":world.resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left),
        "cursor":editor.brush_cursor_world.map(|value|value.to_array()),"terrain":editor.brush_terrain_id,
        "source_revision":published.as_ref().map(|value|value.source_revision),
        "nonzero":published.as_ref().is_some_and(|value|value.nonzero_layers.contains("embedded-live-blade")),
        "vegetation_revision":session.vegetation_revision,"compiled_revision":session.vegetation_compiled_revision}))
}

pub fn select_numeric_placement(world:&mut World,id:&str)->Result<(),String> {
    let author=probe(world)?;
    if !author.placements.iter().any(|record|record.id==id) {return Err("连续数值验收对象不在当前自有草稿".into())}
    world.resource_mut::<Pointer>().enabled=false;
    world.resource_mut::<Pointer>().down=false;
    // 与真实选择对象前的操作一致：退出草笔刷模式，再编辑原数值字段。
    // 只写Inspect会被原Edit工具记忆恢复，导致F12安全重开Select时徽标区域才首次出现。
    crate::viewport::modes::validation_press_select(world)?;
    if !world.resource_mut::<EditorGltfBrowserState>().select_primitive_placement_from_viewport(id.into()) {return Err("原实例选择拒绝重复身份".into())}
    numeric::select_author(world,id);
    // 退出笔刷不会自动切换右侧工具页；必须像作者一样点击“属性”，不能操作隐藏的数值根。
    press_numeric_properties(world)?;
    // 最终恢复到X=9的对象居中，避免相机与X=8网格/左侧边共面，原网格叠加遮住线框取证。
    let mut view=world.resource_mut::<MapEditorViewportState>(); view.center=Vec3::new(9.,1.,-2.); view.extent=8.;
    Ok(())
}

fn press_numeric_properties(world:&mut World)->Result<Entity,String> {
    let button=world.query::<(Entity,&MapEditorSidebarTabButton)>().iter(world)
        .find_map(|(entity,tab)|(tab.0==EditorSidebarTab::Right(EditorRightSidebarTab::Scene)).then_some(entity))
        .ok_or("原右侧属性页签尚未建立")?;
    if !numeric::field_ready(world,button) {return Err(format!("原属性页签不可见：{}",numeric::field_diagnostic(world,button)))}
    world.entity_mut(button).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    Ok(button)
}

pub fn numeric_field(world:&mut World)->Option<Entity> {
    if !world.contains_resource::<Installed>() {return None}
    let mode=world.resource::<ViewportModeState>().mode;
    let tool=world.resource::<MapEditorState>().tool;
    let sidebar=world.resource::<MapEditorSidebarState>();
    let mut diagnostic=json!({"mode":format!("{mode:?}"),"tool":format!("{tool:?}"),
        "sidebar_right":format!("{:?}",sidebar.right),"inspector":format!("{:?}",sidebar.inspector)});
    let field=world.query::<(Entity,&PlacementTransformField,Has<NumericInputDisabled>)>().iter(world)
        .find_map(|(entity,field,disabled)|(*field==PlacementTransformField::TranslationX).then_some((entity,disabled)));
    let ready=field.and_then(|(entity,disabled)| {
        diagnostic["field"]=numeric::field_diagnostic(world,entity);
        diagnostic["disabled"]=json!(disabled);
        let (synced,sync)=numeric::author_ready(world,entity);
        diagnostic["author_sync"]=sync;
        (mode==ViewportMode::Select&&tool==EditorTool::Inspect&&!disabled&&numeric::field_ready(world,entity)&&synced).then_some(entity)
    });
    if field.is_none() {diagnostic["reason"]=json!("原 TranslationX 字段尚未建立")}
    numeric::record_field_diagnostic(world,diagnostic);
    ready
}

pub fn numeric_change(world:&mut World,entity:Entity,value:f32)->Result<(),String> {
    let _=probe(world)?;
    if world.get::<PlacementTransformField>(entity)!=Some(&PlacementTransformField::TranslationX)
        || world.get::<NumericInputDisabled>(entity).is_some() {return Err("原X数值控件不可编辑".into())}
    numeric::change(world,entity,value)
}

pub fn numeric_finish(world:&mut World,entity:Entity,cancel:bool)->Result<(),String> {
    let _=probe(world)?;
    numeric::finish(world,entity,cancel)
}

pub fn numeric_active(world:&World)->bool {
    world.resource::<PlacementTransformInputState>().embedded_scrub().is_some()
}

pub fn numeric_idle(world:&World)->bool { numeric::idle(world) }

pub fn numeric_input_probe(world:&World)->Result<Value,String> { numeric::probe(world) }

/// 通过原视口选择入口临时移开高亮；线框取证结束须恢复，不能把覆盖框当作物体边线。
pub fn render_selection(world:&World)->Option<String> {
    world.resource::<EditorGltfBrowserState>().selected_placement_id.clone()
}

pub fn set_render_selection(world:&mut World,id:Option<&str>)->Result<(),String> {
    let author=probe(world)?;
    if !numeric_idle(world) {return Err("数值手势尚未释放，不能切换线框取证选择".into())}
    if let Some(id)=id {
        if !author.placements.iter().any(|record|record.id==id) {return Err("线框取证恢复对象不在当前草稿".into())}
        if !world.resource_mut::<EditorGltfBrowserState>().select_primitive_placement_from_viewport(id.into()) {
            return Err("原视口拒绝恢复取证前的对象选择".into())
        }
    } else {
        world.resource_mut::<EditorGltfBrowserState>().clear_viewport_selection();
    }
    Ok(())
}

pub fn render_overlay_empty(world:&World)->bool {
    super::tool_previews::empty(world)
}

pub fn press_shading(world:&mut World,mode:u8)->Result<Entity,String> {
    let _=probe(world)?;
    crate::viewport::shading::validation_press_mode(world,mode)
}

pub fn toggle_grid(world:&mut World)->Result<Entity,String> {
    let _=probe(world)?;
    crate::viewport::snapping::validation_toggle_grid(world)
}

pub fn render_probe(world:&mut World)->Result<Value,String> {
    let _=probe(world)?;
    let view=crate::standalone::viewport_projection::capture_render(world).ok_or("原作者相机显示组件未就绪")?;
    Ok(json!({"shading":view.shading,"wire":view.wire,"grid":view.grid,"terrain_flat":view.terrain_flat,
        "terrain_grid":view.terrain_grid,"terrain_backfaces":view.terrain_backfaces,"physics_debug":view.physics_debug}))
}
