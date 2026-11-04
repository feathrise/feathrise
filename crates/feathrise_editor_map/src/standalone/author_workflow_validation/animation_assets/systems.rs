//! 原动画动作/数值契约和磁盘热更的串行推进；每帧只轮询一个后台槽并核对小型结果。

use super::{io, resources::Fixture, types::*};
use crate::{
    resources::{EditorTextInputGuard, MapEditorNumericScrubMouseGuard, MapEditorState},
    ui::{
        asset_preview::{OpenAssetPreview, animation::workflow_validation as bridge},
        sidebars::MapEditorSidebarState,
    },
};
use bevy::{
    gltf::Gltf,
    prelude::*,
    tasks::{IoTaskPool, futures::check_ready},
};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

pub(super) fn begin(world: &mut World) -> Result<(), String> {
    if !world.contains_resource::<Fixture>() {
        return Err("动画资产作者验收未显式启用".into());
    }
    if world.resource::<Fixture>().phase != Phase::Idle {
        return Err("动画资产作者验收已经开始，拒绝重复改写".into());
    }
    let paths = feathrise_editor_core::editor_project_paths().ok_or("动画验收缺少独立工程来源")?;
    let paths = Paths {
        project: paths.project_root().to_owned(),
        assets: paths.asset_root().to_owned(),
    };
    let server = world.resource::<AssetServer>();
    let animation = server.load::<Gltf>(ANIMATION);
    let model = server.load::<Gltf>(MODEL);
    let mesh = server.load::<Mesh>(format!("{MODEL}#Mesh0/Primitive0"));
    let material = server.load::<StandardMaterial>(format!("{MODEL}#Material0/std"));
    let image = server.load::<Image>(TEXTURE);
    let background = paths.clone();
    let mut state = world.resource_mut::<Fixture>();
    state.paths = Some(paths);
    state.started = Some(Instant::now());
    state.phase = Phase::Preparing;
    state.animation_handle = Some(animation);
    state.model_handle = Some(model);
    state.mesh_handle = Some(mesh);
    state.material_handle = Some(material);
    state.image_handle = Some(image);
    state.io = Some(IoTaskPool::get().spawn(async move { io::prepare(&background) }));
    Ok(())
}

pub(super) fn advance(world: &mut World) -> Option<Result<Value, String>> {
    if !world.contains_resource::<Fixture>() {
        return None;
    }
    world.resource_scope(|world,mut state:Mut<Fixture>|{
        if let Some((action,before))=state.pending_action.take(){
            let after=bridge::diagnostic_value(world);
            trace(&mut state,json!({"kind":"original_action_observed","action":action,"before":before,"after":after,"acceptance":"verified by the following selection/history/save phase, never inferred from pulse existence"}));
        }
        if let Some(pulse)=state.pulse.take(){let _=world.despawn(pulse);}
        if state.phase==Phase::Complete {return Some(Ok(state.report.clone().expect("完成阶段必须有完整报告")));}
        if state.phase==Phase::Failed {return Some(Err(state.error.clone().unwrap_or_else(||"动画资产作者验收未完成".into())));}
        if state.phase==Phase::Idle{return None;}
        if state.trace_phase!=Some(state.phase){
            let phase=state.phase;state.trace_phase=Some(phase);
            let expected=state.before.as_ref().map(|before|before.key_identity);
            trace(&mut state,json!({"kind":"phase","phase":format!("{phase:?}"),"expected_key":expected,"actual":bridge::diagnostic_value(world)}));
        }
        let step=(||->Result<(),String>{
            if state.started.is_some_and(|start|start.elapsed()>Duration::from_secs(180)){return Err(format!("动画资产验收超时：{:?}；{}",state.phase,bridge::diagnostic(world)));}
            observe_events(world,&mut state)?;
            let paths=state.paths.as_ref().ok_or("动画资产验收缺少工程路径")?.clone();
            let source=paths.assets.join(ANIMATION);
            let identity=state.before.as_ref().map(|before|before.key_identity);
            match state.phase {
                Phase::Preparing=>{
                    let Some(result)=state.io.as_mut().and_then(check_ready)else{return Ok(());};state.io=None;
                    let IoResult::Prepared(prepared)=result?else{return Err("动画资产准备回执身份不符".into());};
                    state.disk_before=prepared.animation_sha256;state.model_bytes=Some(prepared.model);state.texture_bytes=Some(prepared.texture);
                    open_asset(world,ANIMATION)?;state.phase=Phase::OpenAnimation;
                }
                Phase::OpenAnimation=>{
                    let Some(probe)=bridge::animation_probe(world,&source,None)?else{return Ok(());};
                    if probe.dirty||probe.cursor!=0||probe.key.value.len()!=3{return Err("动画验收必须从本工程干净平移通道开始".into());}
                    if !bridge::input_ready(world){return Ok(());}
                    record_action(world,&mut state,"SelectChannel");state.pulse=Some(bridge::select_channel(world,probe.key_identity)?);state.before=Some(probe);state.phase=Phase::SelectChannel;
                }
                Phase::SelectChannel=>{
                    let Some(probe)=bridge::animation_probe(world,&source,identity)?else{return Ok(());};
                    if !probe.channel_selected||!bridge::input_ready(world){return Ok(());}
                    state.pulse=Some(command(world,&mut state,bridge::AnimationCommand::NextKey)?);state.phase=Phase::LocateKey;
                }
                Phase::LocateKey=>{
                    let Some(probe)=bridge::animation_probe(world,&source,identity)?else{return Ok(());};
                    if !probe.channel_selected||!probe.key_selected||!bridge::input_ready(world){return Ok(());}
                    let value=state.before.as_ref().unwrap().key.value[0]+0.25;
                    record_action(world,&mut state,"NumericCommitX");bridge::commit_x(world,value)?;state.phase=Phase::Edit;
                }
                Phase::Edit=>{
                    let Some(probe)=bridge::animation_probe(world,&source,identity)?else{return Ok(());};
                    let before=state.before.as_ref().unwrap();
                    if probe.cursor==before.cursor{return Ok(());}
                    let mut expected=before.key.clone();expected.value[0]+=0.25;
                    if probe.cursor!=before.cursor+1||probe.history_len!=before.history_len+1||!probe.dirty||probe.key!=expected{return Err("原动画数值提交没有形成精确一笔历史".into());}
                    if !bridge::input_ready(world){return Ok(());}
                    state.edited=Some(probe);state.pulse=Some(command(world,&mut state,bridge::AnimationCommand::Undo)?);state.phase=Phase::Undo;
                }
                Phase::Undo=>{
                    let Some(probe)=bridge::animation_probe(world,&source,identity)?else{return Ok(());};
                    let before=state.before.as_ref().unwrap();if probe.cursor!=before.cursor{return Ok(());}
                    if probe.key!=before.key||probe.dirty||probe.history_len!=before.history_len+1{return Err("原动画撤销未恢复完整关键帧及保存点".into());}
                    if !bridge::input_ready(world){return Ok(());}
                    state.undone=Some(probe);state.pulse=Some(command(world,&mut state,bridge::AnimationCommand::Redo)?);state.phase=Phase::Redo;
                }
                Phase::Redo=>{
                    let Some(probe)=bridge::animation_probe(world,&source,identity)?else{return Ok(());};
                    let edited=state.edited.as_ref().unwrap();if probe.cursor!=edited.cursor{return Ok(());}
                    if probe.key!=edited.key||!probe.dirty{return Err("原动画重做未恢复同一关键帧修改".into());}
                    if !bridge::input_ready(world){return Ok(());}
                    state.redone=Some(probe);state.animation_event_baseline=state.events.animation;state.pulse=Some(command(world,&mut state,bridge::AnimationCommand::Save)?);state.phase=Phase::Save;
                }
                Phase::Save=>{
                    let Some(probe)=bridge::animation_probe(world,&source,identity)?else{return Ok(());};
                    if let Some(Err(error))=probe.save_receipt.as_ref(){return Err(format!("原动画保存失败：{error}"));}
                    if probe.save_receipt!=Some(Ok(()))||probe.dirty{return Ok(());}
                    if probe.key!=state.edited.as_ref().unwrap().key{return Err("原动画保存回执与当前关键帧不一致".into());}
                    let identity=probe.key_identity;state.saved=Some(probe);
                    state.io=Some(IoTaskPool::get().spawn(async move{io::saved(&paths,identity)}));state.phase=Phase::ReadSaved;
                }
                Phase::ReadSaved=>{
                    let Some(result)=state.io.as_mut().and_then(check_ready)else{return Ok(());};state.io=None;
                    let IoResult::SavedAnimation{sha256,key}=result?else{return Err("动画落盘回执身份不符".into());};
                    if sha256==state.disk_before||key!=serde_json::to_value(&state.edited.as_ref().unwrap().key).map_err(|error|error.to_string())?{return Err("真实动画GLB没有保存原编辑后的关键帧".into());}
                    state.disk_after=sha256;state.disk_key=Some(key);open_asset(world,MODEL)?;state.phase=Phase::OpenModel;
                }
                Phase::OpenModel=>{
                    let Some(probe)=bridge::model_probe(world,MODEL)?else{return Ok(());};
                    state.model_before=Some(probe);launch_write(&mut state,Mutation::Mesh)?;state.phase=Phase::MeshWrite;
                }
                Phase::MeshWrite|Phase::MaterialWrite|Phase::TextureWrite=>{
                    let Some(result)=state.io.as_mut().and_then(check_ready)else{return Ok(());};state.io=None;
                    let IoResult::Written{bytes,sha256}=result?else{return Err("资产热更写入回执身份不符".into());};
                    if state.phase==Phase::TextureWrite{state.texture_bytes=Some(bytes);state.texture_sha256=Some(sha256);state.phase=Phase::TextureReload;}
                    else{state.model_bytes=Some(bytes);state.model_sha256.push(sha256);state.phase=if state.phase==Phase::MeshWrite{Phase::MeshReload}else{Phase::MaterialReload};}
                }
                Phase::MeshReload=>{
                    let Some(probe)=bridge::model_probe(world,MODEL)?else{return Ok(());};let before=state.model_before.as_ref().unwrap();
                    let expected=before.positions.iter().map(|point|[point[0]*1.375,point[1],point[2]]).collect::<Vec<_>>();
                    if probe.generation<=before.generation||probe.positions!=expected||state.events.mesh<=state.event_baseline.mesh||state.events.gltf<=state.event_baseline.gltf{return Ok(());}
                    state.model_mesh=Some(probe);launch_write(&mut state,Mutation::Material)?;state.phase=Phase::MaterialWrite;
                }
                Phase::MaterialReload=>{
                    let Some(probe)=bridge::model_probe(world,MODEL)?else{return Ok(());};let mesh=state.model_mesh.as_ref().unwrap();
                    if probe.generation<=mesh.generation||!near_color(probe.base_color_linear,COLOR)||(probe.roughness-0.45).abs()>0.00001||state.events.material<=state.event_baseline.material{return Ok(());}
                    if probe.positions!=mesh.positions{return Err("材质热更意外改变原顶点".into());}
                    state.model_material=Some(probe);launch_write(&mut state,Mutation::Texture)?;state.phase=Phase::TextureWrite;
                }
                Phase::TextureReload=>{
                    let Some(probe)=bridge::model_probe(world,MODEL)?else{return Ok(());};
                    if probe.image_size!=[2,2]||probe.first_pixel!=PIXEL||state.events.image<=state.event_baseline.image{return Ok(());}
                    if probe.positions!=state.model_mesh.as_ref().unwrap().positions||!near_color(probe.base_color_linear,COLOR){return Err("贴图热更意外回退Mesh或材质".into());}
                    state.model_texture=Some(probe);
                    state.pulse=Some(crate::standalone::validation_fixture::request_map_tab(world)?);state.phase=Phase::ReturnMap;
                }
                Phase::ReturnMap=>{
                    if !crate::standalone::preview_ready_for(world,"M1-SAMPLE"){return Ok(());}
                    if state.events.animation<=state.animation_event_baseline{return Ok(());} // 保存后原动画源必须真实换代，不能借初载补标签的Modified冒充。
                    state.report=Some(json!({
                        "passed":true,"source_asset_root":paths.assets,"animation_path":ANIMATION,"stage_and_action_trace":state.trace,
                        "animation":{"input":"original channel/key/action pulses and original numeric commit contract; not physical input","before":state.before,"edited":state.edited,"undone":state.undone,"redone":state.redone,"saved":state.saved,"disk_key":state.disk_key,"sha256_before":state.disk_before,"sha256_after":state.disk_after,"modified_events_before_save":state.animation_event_baseline,"modified_events_after_save":state.events.animation},
                        "asset_hot_reload":{"input":"atomic writes to this owned project's GLB/PNG files; original watcher and loaders; no Assets mutation","model_path":MODEL,"texture_path":TEXTURE,"before":state.model_before,"mesh_reloaded":state.model_mesh,"material_reloaded":state.model_material,"texture_reloaded":state.model_texture,"model_sha256":state.model_sha256,"texture_sha256":state.texture_sha256,"modified_events":state.events},
                        "map_preview_restored":true,"not_verified":["physical timeline/pose dragging","Blender save callback","other formats and large assets","runtime consumption of probe unless a saved placement references it"]
                    }));state.phase=Phase::Complete;
                }
                _=>{}
            }
            Ok(())
        })();
        if let Err(error)=step{
            let recent=state.trace.iter().rev().take(4).collect::<Vec<_>>();
            let error=format!("{error}；最近原动作/阶段：{}",json!(recent));
            state.error=Some(error.clone());state.phase=Phase::Failed;return Some(Err(error));
        }
        if state.phase==Phase::Complete{Some(Ok(state.report.clone().unwrap()))}else{None}
    })
}

fn trace(state: &mut Fixture, value: Value) {
    // 有限输入证据固定64条；不积累逐帧日志，正常作者会话不会安装本夹具。
    eprintln!("动画作者验收：{value}");
    if state.trace.len() == 64 {
        state.trace.remove(0);
    }
    state.trace.push(value);
}

fn record_action(world: &mut World, state: &mut Fixture, action: &str) {
    let before = bridge::diagnostic_value(world);
    trace(
        state,
        json!({"kind":"original_action_requested","action":action,"before":before}),
    );
    state.pending_action = Some((action.into(), before));
}

fn command(
    world: &mut World,
    state: &mut Fixture,
    action: bridge::AnimationCommand,
) -> Result<Entity, String> {
    record_action(world, state, &format!("{action:?}"));
    bridge::command(world, action)
}

fn open_asset(world: &mut World, path: &str) -> Result<(), String> {
    if world
        .get_resource::<EditorTextInputGuard>()
        .is_some_and(|guard| guard.block_shortcuts)
        || world
            .get_resource::<MapEditorNumericScrubMouseGuard>()
            .is_some_and(|guard| guard.blocked())
    {
        return Err("原作者输入尚未静默，不能打开动画验收资产".into());
    }
    world
        .resource_mut::<MapEditorSidebarState>()
        .reveal_asset_properties();
    world.resource_mut::<MapEditorState>().selected_asset = Some(path.into());
    world.write_message(OpenAssetPreview { path: path.into() });
    Ok(())
}

fn launch_write(state: &mut Fixture, mutation: Mutation) -> Result<(), String> {
    if state.io.is_some() {
        return Err("资产热更单槽仍忙，拒绝重叠IO".into());
    }
    let expected = if matches!(mutation, Mutation::Texture) {
        state.texture_bytes.clone()
    } else {
        state.model_bytes.clone()
    }
    .ok_or("资产热更缺少原字节")?;
    let paths = state.paths.as_ref().unwrap().clone();
    state.event_baseline = state.events.clone();
    state.io = Some(IoTaskPool::get().spawn(async move { io::write(&paths, mutation, expected) }));
    Ok(())
}

fn near_color(left: [f32; 4], right: [f32; 4]) -> bool {
    left.into_iter()
        .zip(right)
        .all(|(left, right)| (left - right).abs() < 0.00001)
}

fn observe_events(world: &World, state: &mut Fixture) -> Result<(), String> {
    if let Some(messages) = world.get_resource::<Messages<AssetEvent<Gltf>>>() {
        for (index, event) in state.gltf_events.read(messages).enumerate() {
            if index >= 8192 {
                return Err("有限GLTF热更通知超出8192条预算".into());
            }
            if let AssetEvent::Modified { id } = event {
                if state
                    .animation_handle
                    .as_ref()
                    .is_some_and(|handle| handle.id() == *id)
                {
                    state.events.animation += 1;
                }
                if state
                    .model_handle
                    .as_ref()
                    .is_some_and(|handle| handle.id() == *id)
                {
                    state.events.gltf += 1;
                }
            }
        }
    }
    macro_rules! count {
        ($cursor:ident,$handle:ident,$field:ident,$ty:ty) => {
            if let Some(messages) = world.get_resource::<Messages<AssetEvent<$ty>>>() {
                for (index, event) in state.$cursor.read(messages).enumerate() {
                    if index >= 8192 {
                        return Err("有限资产热更通知超出8192条预算".into());
                    }
                    if let AssetEvent::Modified { id } = event {
                        if state
                            .$handle
                            .as_ref()
                            .is_some_and(|handle| handle.id() == *id)
                        {
                            state.events.$field += 1;
                        }
                    }
                }
            }
        };
    }
    count!(mesh_events, mesh_handle, mesh, Mesh);
    count!(material_events, material_handle, material, StandardMaterial);
    count!(image_events, image_handle, image, Image);
    Ok(())
}
