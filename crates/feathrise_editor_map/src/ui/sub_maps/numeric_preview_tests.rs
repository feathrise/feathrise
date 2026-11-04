//! 原子地图数值事务发布真实引用 Arc；桥只读取显示源，不替代作者提交和取消。
use super::*;
use crate::standalone::{embedded_author_preview as bridge, embedded_snapshot::EmbeddedPoseTargets};
use feathrise_map_content::{MapId, MapInstanceAuthorPreview, MapInstanceId, MapInstanceTransform,
    embedded_author_preview::{AuthorPreview,decode}};
use feathrise_editor_ui::NumericValue;
use std::time::{Duration,Instant};

fn app()->(App,Entity) {
    let (mut app,_,_)=crate::viewport::placement_move::interaction_tests::app();
    let id=MapInstanceId::parse("submap-preview").unwrap();
    let map={let mut session=app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft=session.draft.as_mut().unwrap();
        draft.manifest.sub_maps=Arc::from([MapInstancePlacement {
            id:id.clone(),source_map_id:MapId::parse("source-preview").unwrap(),label:String::new(),
            transform:MapInstanceTransform {translation:[1.,2.,3.],..default()}}]);
        draft.map_id.clone()};
    let mut state=SubMapEditorState::default();
    state.map_id=Some(map.clone()); state.selected=Some(id.clone());
    app.insert_resource(state).init_resource::<MapInstanceAuthorPreview>()
        .add_message::<NumericInputChanged>().add_message::<NumericInputCommitted>();
    app.edit_schedule(Update,|schedule|{
        *schedule=Schedule::new(Update);
        schedule.add_systems((input,systems::publish).chain().in_set(crate::MapEditorSystemSet::Tasks));
    });
    app.add_plugins(bridge::AuthorPreviewPlugin);
    {
        let mut state=app.world_mut().resource_mut::<bridge::EmbeddedAuthorPreviewState>();
        state.enabled=true;state.session=3;state.generation=1;
    }
    bridge::snapshot_sent(app.world_mut(),3,1,1,map,Arc::new(EmbeddedPoseTargets {
        sub_maps:[id].into_iter().collect(),..default()}));
    assert!(bridge::snapshot_applied(app.world_mut(),3,1,1));
    let field=app.world_mut().spawn(Field(0)).id();
    (app,field)
}
fn change(app:&mut App,field:Entity,x:f32){
    app.world_mut().write_message(NumericInputChanged {entity:field,value:NumericValue::F32(x)});
    app.update();
}
fn finish(app:&mut App,field:Entity,x:f32,cancel:bool){
    app.world_mut().write_message(NumericInputCommitted {entity:field,value:NumericValue::F32(x),
        source:NumericInputCommitSource::Scrub,outcome:if cancel {NumericInputCommitOutcome::Canceled}
        else {NumericInputCommitOutcome::Accepted},changed:true,edited:true});
    app.update();
}
fn wait(app:&mut App,active:bool,x:f32)->u64 {
    let deadline=Instant::now()+Duration::from_secs(4);
    loop {
        app.update();
        let state=app.world().resource::<bridge::EmbeddedAuthorPreviewState>();
        assert!(state.error.is_none(),"{:?}",state.error);
        if let Some(ready)=state.ready.clone(){
            let matched=matches!(decode(&ready.bytes).unwrap(),AuthorPreview::SubMapPose {
                pose:Some(pose),active:value,..} if value==active && pose.translation[0]==x);
            app.world_mut().resource_mut::<bridge::EmbeddedAuthorPreviewState>().acknowledged_sequence=ready.sequence;
            if matched{return ready.sequence;}
        }
        assert!(Instant::now()<deadline,"原子地图发布结果未到达实时桥");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn embedded_object_pose_original_submap_numeric_commit_cancel_and_late_ack_preserve_authority(){
    let (mut app,field)=app();
    change(&mut app,field,4.);
    let active=wait(&mut app,true,4.);
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    finish(&mut app,field,4.,false);
    let committed=wait(&mut app,false,4.);
    assert!(committed>active);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(),1);
    change(&mut app,field,9.);
    let next=wait(&mut app,true,9.);
    // 旧首笔回执不能吃掉第二笔取消；正文始终用原数值事务恢复后的当前权威值。
    app.world_mut().resource_mut::<bridge::EmbeddedAuthorPreviewState>().acknowledged_sequence=active;
    finish(&mut app,field,9.,true);
    app.world_mut().resource_mut::<bridge::EmbeddedAuthorPreviewState>().acknowledged_sequence=next;
    let cancelled=wait(&mut app,false,4.);
    assert!(cancelled>next);
    let session=app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.undo.len(),1);
    assert!(session.redo.is_empty());
    assert_eq!(session.draft.as_ref().unwrap().manifest.sub_maps[0].transform.translation,[4.,2.,3.]);
    assert!(app.world().resource::<MapEditorState>().dirty);
}
