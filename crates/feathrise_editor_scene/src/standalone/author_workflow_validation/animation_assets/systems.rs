//! 原动画动作/数值契约和磁盘热更的串行推进；每帧只轮询一个后台槽并核对小型结果。

use super::{io, resources::Fixture, types::*};
use crate::{
    asset_catalog::{EditorAssetCatalog, request_asset_reveal},
    resources::{EditorTextInputGuard, SceneEditorNumericScrubMouseGuard, SceneEditorState},
    ui::{
        asset_preview::{
            AnimationEditorState, AssetPreviewState, AssetPreviewWorkspace, OpenAssetPreview, PickProjectAnimationClip,
            animation::workflow_validation as bridge,
        },
        sidebars::{EditorRightSidebarTab, SceneEditorSidebarState},
    },
};
use feathrise_editor_animation_model::{media::{AssetPreviewContent, AssetPreviewPhase}, preview::PreviewRuntime, types::PoseTool};
use bevy::{
    gltf::Gltf,
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    tasks::{IoTaskPool, Task, futures::check_ready},
};
#[cfg(feature = "validation")]
use {crate::ui::file_manager_validation::pointer_click, bevy::window::PrimaryWindow};
use serde_json::{Value, json};
use std::{path::PathBuf, time::{Duration, Instant}};

#[derive(Resource)]
struct ModeScreenshot(Task<Result<(), String>>);

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
                    state.unanimated_model_sha256 = prepared.unanimated_model_sha256;
                    reveal_asset(world, ANIMATION)?;
                    state.phase=Phase::OpenAnimation;
                }
                Phase::OpenAnimation=>{
                    if !expanded_model_ready(world, ANIMATION) { return Ok(()); }
                    let Some(probe)=bridge::animation_probe(world,&source,None)?else{return Ok(());};
                    if probe.dirty||probe.cursor!=0||probe.key.value.len()!=3{return Err("动画验收必须从本工程干净平移通道开始".into());}
                    let author = world.resource::<AnimationEditorState>();
                    if author.editable() || author.can_edit_selected() {
                        return Err("直接打开 GLB 时已有动画编辑资格".into());
                    }
                    let runtime = world.resource::<PreviewRuntime>();
                    if !runtime.document_ready(author, world.resource::<AssetPreviewState>())
                        || runtime.published_weight_time(author).is_none()
                    {
                        return Ok(());
                    }
                    if !bridge::input_ready(world) || bridge::timeline_ruler(world).is_none() { return Ok(()); }
                    state.before = Some(probe);
                    state.phase = Phase::CaptureReadOnly;
                }
                Phase::CaptureReadOnly => {
                    if bridge::pose_toolbar_visible(world)? {
                        return Err("只读 GLB 预览显示了动画姿势工具栏".into());
                    }
                    // 中央视图已真实启用，等它经过一帧渲染后再截图。
                    capture_mode(world, "animation-read-only.png")?;
                    state.phase = Phase::ReadOnlyScreenshot;
                }
                Phase::ReadOnlyScreenshot => {
                    if !bridge::input_ready(world) || !mode_captured(world)? { return Ok(()); }
                    reveal_asset(world, ANIMATION)?;
                    state.phase = Phase::PickClip;
                }
                Phase::PickClip => {
                    if !bridge::input_ready(world) || !world.resource::<SceneEditorSidebarState>().assets_browser_visible() { return Ok(()); }
                    // 与普通文件列表的动作行提交同一入口，打开文件本身仍然只提供预览。
                    record_action(world, &mut state, "PickProjectClip");
                    world.write_message(PickProjectAnimationClip { asset_path: ANIMATION.into(), clip_index: 0, target: None });
                    state.phase = Phase::SelectClip;
                }
                Phase::SelectClip => {
                    let author = world.resource::<AnimationEditorState>();
                    if author.editable() { return Err("选择动作不应自动进入编辑".into()); }
                    if !author.can_edit_selected() || !bridge::input_ready(world) { return Ok(()); }
                    state.pulse = Some(command(world, &mut state, bridge::AnimationCommand::FirstFrame)?);
                    state.phase = Phase::PauseSelectedClip;
                }
                Phase::PauseSelectedClip => {
                    let author = world.resource::<AnimationEditorState>();
                    if author.playing || author.position() != 0.0 || !bridge::input_ready(world) { return Ok(()); }
                    record_action(world, &mut state, "SelectChannel");
                    state.pulse = Some(bridge::select_channel(world, identity.unwrap())?);
                    state.phase = Phase::SelectChannel;
                }
                Phase::SelectChannel=>{
                    let Some(probe)=bridge::animation_probe(world,&source,identity)?else{return Ok(());};
                    if !probe.channel_selected||!bridge::input_ready(world){return Ok(());}
                    state.pulse = Some(command(world, &mut state, bridge::AnimationCommand::NextFrame)?);
                    state.phase = Phase::LocateInsertFrame;
                }
                Phase::LocateInsertFrame => {
                    let identity = identity.unwrap();
                    let Some(probe) = bridge::animation_probe(world, &source, Some(identity))? else { return Ok(()); };
                    if !probe.channel_selected || !bridge::input_ready(world) { return Ok(()); }
                    let Some(second) = bridge::animation_probe(world, &source, Some(bridge::KeyIdentity { key: 1, ..identity }))?
                        else { return Ok(()); };
                    if probe.position <= probe.key.time || probe.position >= second.key.time {
                        return Err("后帧未定位到自有夹具的两个真实关键帧之间".into());
                    }
                    state.insert_time = Some(probe.position);
                    // 经原动作输入提交一次预览期间的插帧，证明隐藏的写入入口也会拒绝。
                    state.pulse = Some(command(world, &mut state, bridge::AnimationCommand::InsertKey)?);
                    state.phase = Phase::VerifyReadOnly;
                }
                Phase::VerifyReadOnly => {
                    let Some(probe) = bridge::animation_probe(world, &source, identity)? else { return Ok(()); };
                    let before = state.before.as_ref().unwrap();
                    if probe.cursor != before.cursor || probe.history_len != before.history_len
                        || probe.dirty || probe.key != before.key
                        || world.resource::<AnimationEditorState>().editable()
                    {
                        return Err("预览中的插帧命令修改了动画".into());
                    }
                    if !bridge::input_ready(world) { return Ok(()); }
                    state.phase = Phase::ClickEnterEditMode;
                }
                Phase::ClickEnterEditMode => {
                    if click_animation_button(world, &mut state, bridge::AnimationCommand::ToggleEditing)? {
                        state.phase = Phase::EnterEditMode;
                    }
                }
                Phase::EnterEditMode => {
                    // 原窗口依次点击旋转和移动，核对两个图标控制的是当前骨骼姿势工具。
                    if world.resource::<AnimationEditorState>().editable()
                        && bridge::pose_toolbar_visible(world)?
                        && click_animation_button(world, &mut state, bridge::AnimationCommand::RotatePose)?
                    {
                        state.phase = Phase::RotatePoseTool;
                    }
                }
                Phase::RotatePoseTool => {
                    if world.resource::<AnimationEditorState>().pose_tool == PoseTool::Rotate {
                        // 可见状态在释放时改变，后续点击阶段必须允许原鼠标流程完成收尾。
                        state.phase = Phase::ClickMovePoseTool;
                    }
                }
                Phase::ClickMovePoseTool => {
                    if click_animation_button(world, &mut state, bridge::AnimationCommand::MovePose)? {
                        state.phase = Phase::MovePoseTool;
                    }
                }
                Phase::MovePoseTool => {
                    if world.resource::<AnimationEditorState>().pose_tool != PoseTool::Move { return Ok(()); }
                    let Some(probe) = bridge::animation_probe(world, &source, identity)? else { return Ok(()); };
                    let before = state.before.as_ref().unwrap();
                    if probe.dirty || probe.cursor != before.cursor || probe.history_len != before.history_len
                        || Some(probe.position) != state.insert_time
                    {
                        return Err("切换动画姿势工具后，原播放位置或编辑历史发生变化".into());
                    }
                    if click_animation_button(world, &mut state, bridge::AnimationCommand::ToggleEditing)? {
                        state.phase = Phase::ExitPoseToolMode;
                    }
                }
                Phase::ExitPoseToolMode => {
                    if world.resource::<AnimationEditorState>().editable() { return Ok(()); }
                    if bridge::pose_toolbar_visible(world)? {
                        return Err("退出动画编辑后，动画姿势工具栏仍然显示".into());
                    }
                    state.phase = Phase::ReenterAfterPoseTools;
                }
                Phase::ReenterAfterPoseTools => {
                    if click_animation_button(world, &mut state, bridge::AnimationCommand::ToggleEditing)? {
                        state.phase = Phase::InsertAfterPoseTools;
                    }
                }
                Phase::InsertAfterPoseTools => {
                    if world.resource::<AnimationEditorState>().editable()
                        && bridge::pose_toolbar_visible(world)?
                        && click_animation_button(world, &mut state, bridge::AnimationCommand::InsertKey)?
                    {
                        state.phase = Phase::VerifyInsertion;
                    }
                }
                Phase::VerifyInsertion => {
                    let identity = bridge::KeyIdentity { key: 1, ..identity.unwrap() };
                    let Some(probe) = bridge::animation_probe(world, &source, Some(identity))? else { return Ok(()); };
                    if !probe.dirty || probe.cursor != 1 || probe.history_len != 1
                        || Some(probe.key.time) != state.insert_time
                        || !world.resource::<AnimationEditorState>().editable()
                    {
                        return Err("显式进入动画编辑后，原插帧未在同一播放位置产生历史".into());
                    }
                    if !bridge::input_ready(world) || bridge::timeline_ruler(world).is_none() { return Ok(()); }
                    capture_mode(world, "animation-edit-mode.png")?;
                    state.phase = Phase::EditModeScreenshot;
                }
                Phase::EditModeScreenshot => {
                    if !bridge::input_ready(world) || !mode_captured(world)? { return Ok(()); }
                    state.pulse = Some(command(world, &mut state, bridge::AnimationCommand::Undo)?);
                    state.phase = Phase::UndoInsertion;
                }
                Phase::UndoInsertion => {
                    let identity = bridge::KeyIdentity { key: 1, ..identity.unwrap() };
                    let Some(probe) = bridge::animation_probe(world, &source, Some(identity))? else { return Ok(()); };
                    if probe.cursor != 0 { return Ok(()); }
                    if probe.dirty || Some(probe.key.time) == state.insert_time { return Err("撤销未移除新插入的帧".into()); }
                    if !bridge::input_ready(world) { return Ok(()); }
                    state.pulse = Some(command(world, &mut state, bridge::AnimationCommand::NextKey)?);
                    state.phase = Phase::LocateKey;
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
                    state.disk_after=sha256;state.disk_key=Some(key);
                    reveal_asset(world, MODEL)?;
                    state.phase = if super::super::animation_edit_only() { Phase::OpenUnanimatedModel } else { Phase::OpenModel };
                }
                Phase::OpenUnanimatedModel => {
                    if !expanded_model_ready(world, MODEL) { return Ok(()); }
                    let author = world.resource::<AnimationEditorState>();
                    if author.source() != Some(paths.assets.join(MODEL).as_path()) || author.busy() {
                        return Ok(());
                    }
                    let Some(document) = author.active_document() else { return Ok(()); };
                    if !document.clips.is_empty() || !author.can_create_clip() || author.editable() || author.can_edit_selected() {
                        return Err("打开零动画 GLB 应可新建动画，并保持只读".into());
                    }
                    state.phase = Phase::ClickCreateClip;
                }
                Phase::ClickCreateClip => {
                    if click_animation_button(world, &mut state, bridge::AnimationCommand::CreateClip)? {
                        state.phase = Phase::VerifyCreatedClip;
                    }
                }
                Phase::VerifyCreatedClip => {
                    let Some(probe) = bridge::animation_probe(world, &paths.assets.join(MODEL), None)? else { return Ok(()); };
                    let author = world.resource::<AnimationEditorState>();
                    if author.active_document().is_none_or(|document| document.clips.len() != 1)
                        || !probe.dirty || probe.history_len != 1 || author.editable() || !author.can_edit_selected()
                    {
                        return Err("新建动画应选中唯一的草稿，并等待用户进入编辑".into());
                    }
                    state.phase = Phase::ClickEditCreatedClip;
                }
                Phase::ClickEditCreatedClip => {
                    if click_animation_button(world, &mut state, bridge::AnimationCommand::ToggleEditing)? {
                        state.phase = Phase::SaveCreatedClip;
                    }
                }
                Phase::SaveCreatedClip => {
                    if world.resource::<AnimationEditorState>().editable()
                        && click_animation_button(world, &mut state, bridge::AnimationCommand::Save)?
                    {
                        state.phase = Phase::ReadCreatedClip;
                    }
                }
                Phase::ReadCreatedClip => {
                    let Some(probe) = bridge::animation_probe(world, &paths.assets.join(MODEL), None)? else { return Ok(()); };
                    if let Some(Err(error)) = probe.save_receipt.as_ref() {
                        return Err(format!("原新建动画保存失败：{error}"));
                    }
                    if probe.save_receipt != Some(Ok(())) || probe.dirty { return Ok(()); }
                    state.io = Some(IoTaskPool::get().spawn(async move { io::saved_created_clip(&paths) }));
                    state.phase = Phase::VerifyCreatedClipOnDisk;
                }
                Phase::VerifyCreatedClipOnDisk => {
                    let Some(result) = state.io.as_mut().and_then(check_ready) else { return Ok(()); };
                    state.io = None;
                    let IoResult::SavedCreatedClip { sha256, clip_count } = result? else {
                        return Err("新建动画落盘回执身份不符".into());
                    };
                    let before = state.unanimated_model_sha256.as_ref().ok_or("缺少零动画原始摘要")?;
                    if clip_count != 1 || sha256 == *before {
                        return Err("保存后自有 GLB 应新增一个动画".into());
                    }
                    state.created_clip_proof = Some(json!({
                        "asset_path": MODEL, "disk_clip_count_before": 0, "disk_clip_count_after": clip_count,
                        "created_read_only": true, "entered_edit_mode": true, "save_receipt": {"Ok": null},
                        "sha256_before": before, "sha256_after": sha256,
                    }));
                    state.pulse = Some(crate::standalone::validation_fixture::request_scene_tab(world)?);
                    state.phase = Phase::ReturnScene;
                }
                Phase::OpenModel=>{
                    if !expanded_model_ready(world, MODEL) { return Ok(()); }
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
                    state.pulse=Some(crate::standalone::validation_fixture::request_scene_tab(world)?);state.phase=Phase::ReturnScene;
                }
                Phase::ReturnScene=>{
                    if !crate::standalone::preview_ready_for(world,"M1-SAMPLE"){return Ok(());}
                    // 全量验收另核对文件观察器；动画专项只核对真实保存回执及写后文件。
                    if !super::super::animation_edit_only() && state.events.animation <= state.animation_event_baseline {
                        return Ok(());
                    }
                    state.report=Some(json!({
                        "passed":true,"source_asset_root":paths.assets,"animation_path":ANIMATION,"stage_and_action_trace":state.trace,
                        "animation":{"input":"Bevy WindowEvent clicks on original mode, pose tools and key insert buttons; original hidden-action pulses and numeric commit contract","before":state.before,"edited":state.edited,"undone":state.undone,"redone":state.redone,"saved":state.saved,"disk_key":state.disk_key,"sha256_before":state.disk_before,"sha256_after":state.disk_after,"modified_events_before_save":state.animation_event_baseline,"modified_events_after_save":state.events.animation,
                            "edit_gate":{"opened_read_only":true,"selected_read_only":true,"read_only_insert_rejected":true,"entered_edit_mode":true,"inserted_key":true,"insert_time":state.insert_time},
                            "pose_toolbar":{"hidden_in_preview":true,"rotate_selected":true,"move_selected":true,"history_unchanged":true,"hidden_after_exit":true}},
                        "created_clip":state.created_clip_proof,
                        "asset_hot_reload":(!super::super::animation_edit_only()).then(|| json!({"input":"atomic writes to this owned project's GLB/PNG files; original watcher and loaders; no Assets mutation","model_path":MODEL,"texture_path":TEXTURE,"before":state.model_before,"mesh_reloaded":state.model_mesh,"material_reloaded":state.model_material,"texture_reloaded":state.model_texture,"model_sha256":state.model_sha256,"texture_sha256":state.texture_sha256,"modified_events":state.events})),
                        "scene_preview_restored":true,"not_verified":["physical timeline/pose dragging","Blender save callback","other formats and large assets","runtime consumption of probe unless a saved placement references it"]
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

#[cfg(feature = "validation")]
fn click_animation_button(world: &mut World, state: &mut Fixture, action: bridge::AnimationCommand) -> Result<bool, String> {
    let position = if let Some((position, _)) = state.click {
        position
    } else {
        if !bridge::input_ready(world) { return Ok(false); }
        let Some(entity) = bridge::original_button(world, action) else { return Ok(false); };
        let Some(transform) = world.get::<UiGlobalTransform>(entity) else { return Ok(false); };
        let position = transform.translation.as_dvec2();
        record_action(world, state, &format!("Click{action:?}"));
        position
    };
    let window = world.query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world).map_err(|error| error.to_string())?;
    // 采用正常窗口鼠标路径。释放后由随后阶段确认状态变更，不把发出事件当作成功。
    Ok(pointer_click(world, &mut state.click, window, position, MouseButton::Left))
}

#[cfg(not(feature = "validation"))]
fn click_animation_button(_: &mut World, _: &mut Fixture, _: bridge::AnimationCommand) -> Result<bool, String> {
    Err("动画原窗口按钮需要显式 validation 构建".into())
}

fn capture_mode(world: &mut World, filename: &'static str) -> Result<(), String> {
    let output = PathBuf::from(std::env::var_os("NEW_EDITOR_NATIVE_SMOKE_DIR").ok_or("动画截图缺少自有目录")?);
    let path = output.join(filename);
    // 冻结原画布与编辑面板，等真实窗口回读写完再切换阶段。
    world.spawn(Screenshot::primary_window()).observe(move |event: On<ScreenshotCaptured>, mut commands: Commands| {
        let image = event.image.clone();
        let path = path.clone();
        commands.insert_resource(ModeScreenshot(IoTaskPool::get().spawn(async move {
            image.try_into_dynamic().map_err(|error| error.to_string())?
                .save(path).map_err(|error| error.to_string())
        })));
    });
    Ok(())
}

fn mode_captured(world: &mut World) -> Result<bool, String> {
    let Some(result) = world.get_resource_mut::<ModeScreenshot>().and_then(|mut screenshot| check_ready(&mut screenshot.0))
        else { return Ok(false); };
    result?;
    world.remove_resource::<ModeScreenshot>();
    Ok(true)
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

fn reveal_asset(world: &mut World, path: &str) -> Result<(), String> {
    world.resource_scope(|world, mut catalog: Mut<EditorAssetCatalog>| {
        world.resource_scope(|world, mut editor: Mut<SceneEditorState>| {
            request_asset_reveal(&mut catalog, &mut editor, &mut world.resource_mut::<SceneEditorSidebarState>(), path)
        })
    })
}

fn expanded_model_ready(world: &mut World, path: &str) -> bool {
    let media = world.resource::<AssetPreviewWorkspace>();
    if media.central_active && media.expanded.as_ref().is_some_and(|tab| tab.path == path) {
        let preview = world.resource::<AssetPreviewState>();
        return preview.selected_path.as_deref() == Some(path) && matches!(preview.content,
            AssetPreviewContent::Model { phase: AssetPreviewPhase::Ready, .. })
            && preview.render_visible;
    }
    // 等文件定位与属性页采用后提交双击入口。旧中央标签可能仍在展示上一 GLB。
    if world.resource::<SceneEditorState>().selected_asset.as_deref() == Some(path)
        && world.resource::<SceneEditorSidebarState>().right == EditorRightSidebarTab::Assets
        && !world.resource::<EditorTextInputGuard>().block_shortcuts
        && !world.resource::<SceneEditorNumericScrubMouseGuard>().blocked()
    {
        world.write_message(OpenAssetPreview { path: path.into() });
    }
    false
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
