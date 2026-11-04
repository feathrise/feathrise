//! 地形手柄原悬停输入到工程实际Gizmo与PNG像素的有限证据；只读作者内容，不模拟成功状态。

use bevy::{prelude::*,tasks::{IoTaskPool,Task,futures::check_ready}};
use feathrise_editor_map::standalone::embedded_validation::{self as author,live::object_pose,tool_previews as tools};
use serde_json::{Value,json};
use std::{path::Path,time::{Duration,Instant}};

const STAGES:[&str;4]=["idle","x_axis","xz_plane","cleared"];

#[derive(Resource)]
struct Driver {
    author:Value, original:Value, pose:Option<Value>, step:usize, started:Instant,
    expected:Option<Value>, minimum_frame:u64, retry:Instant,
    task:Option<Task<Result<Option<Value>,String>>>, proofs:Vec<Value>, ending:bool, pulse:Option<Entity>,
    setup_before:Value, setup:Option<Value>, hover_after:Option<Value>, restore_before:Option<Value>,
}

pub(super) fn exercise(world:&mut World,directory:&Path)->Result<Option<Value>,String>{
    if !world.contains_resource::<Driver>() {
        let author=serde_json::to_value(author::probe(world)?).map_err(|error|error.to_string())?;
        let original=tools::probe(world)?;
        object_pose::start_hover(world)?;
        let setup_before=object_pose::probe(world)?;
        world.insert_resource(Driver {author,original,pose:None,step:0,started:Instant::now(),
            expected:None,minimum_frame:0,retry:Instant::now(),task:None,proofs:Vec::new(),ending:false,pulse:None,
            setup_before,setup:None,hover_after:None,restore_before:None});
    }
    world.resource_scope(|world,mut driver:Mut<Driver>|{
        if driver.started.elapsed()>Duration::from_secs(90){return Err(format!("地形悬停真实窗口验收超时：阶段{}",driver.step))}
        if let Some(pulse)=driver.pulse.take(){author::release_pulse(world,pulse);}
        let current_author=serde_json::to_value(author::probe(world)?).map_err(|error|error.to_string())?;
        let state=object_pose::probe(world)?;
        if state["mouse_down"]!=false || state["transforming"]!=false || !state["preview_pose"].is_null(){
            return Err("悬停证据链意外启动了地形拖动或姿态预览".into());
        }
        if let Some(pose)=&driver.pose {
            if &state["author_pose"]!=pose{return Err("悬停改变了地形作者姿态".into())}
        } else {driver.pose=Some(state["author_pose"].clone());}
        if driver.setup.is_none(){
            // 选择会经原系统载入独立画布并推进绑定修订；先完整采用，再冻结四步悬停的严格基线。
            verify_rebind(&driver.author,&driver.setup_before["canvas"],&current_author,&state["canvas"])?;
            if state["hover_ready"]!=true || state["canvas"]["active"]!="starter-ground"
                || state["canvas"]["loading"]!=false || !snapshot_applied(world){return Ok(None)}
            driver.setup=Some(json!({"author_before":driver.author,"author_after":current_author,
                "before":driver.setup_before,"after":state,"snapshot_applied":true}));
            driver.author=current_author.clone();
        }
        if driver.step==STAGES.len(){
            if !driver.ending {
                if current_author!=driver.author{return Err("悬停结束前作者基线发生变化".into())}
                driver.hover_after=Some(current_author.clone());driver.restore_before=Some(state.clone());
                object_pose::end_hover(world)?;driver.ending=true;
            }
            verify_rebind(&driver.author,&driver.restore_before.as_ref().unwrap()["canvas"],&current_author,&state["canvas"])?;
            if !object_pose::restore(world)? {return Ok(None)}
            if state["canvas"]["active"]!=driver.setup_before["canvas"]["active"]
                || state["canvas"]["loading"]!=false || !snapshot_applied(world){return Ok(None)}
            let actual=tools::probe(world)?;
            let mode=driver.original["mode"].as_str().ok_or("悬停证据缺少原模式")?;
            if actual["mode"]!=mode {driver.pulse=Some(tools::press_mode(world,mode)?);return Ok(None)}
            if actual["semantic_sha256"]!=driver.original["semantic_sha256"] {return Ok(None)}
            return Ok(Some(json!({"source":"original scene row and pointer -> actual project GizmoAssets -> PNG pixels",
                "author_before":driver.author,"author_after":driver.hover_after,"terrain_pose":driver.pose,
                "selection_setup":driver.setup,"selection_restore":{"author_before":driver.hover_after,"author_after":current_author,
                    "before":driver.restore_before,"after":state,"snapshot_applied":true},
                "original":driver.original,"restored":actual,"proofs":driver.proofs})));
        }
        if current_author!=driver.author{return Err(format!("悬停改变了作者草稿、保存身份或撤销历史：before={} after={current_author}",driver.author))}
        if state["hover_ready"]!=true {return Ok(None)}
        let actual=tools::probe(world)?;
        let wanted_axes=if driver.step==1 {1}else{0};
        let wanted_plane=driver.step==2;
        let matches=actual["preview"]["items"].as_array().is_some_and(|items|items.iter().any(|item|{
            let preview=&item["preview"];
            item["kind"]=="transform" && preview["kind"]=="move" && preview["plane_cross"]==false
                && preview["active_axes"]==wanted_axes && preview["plane_active"]==wanted_plane && preview["plane"]==true
        }));
        if !matches {return Ok(None)}
        if driver.expected.as_ref().is_none_or(|expected|expected["generation"]!=actual["generation"]
            || expected["semantic_sha256"]!=actual["semantic_sha256"]) {
            if let Some(task)=driver.task.as_mut(){
                if let Some(result)=check_ready(task){driver.task=None;result?;}
                return Ok(None);
            }
            driver.expected=Some(actual.clone());
            driver.minimum_frame=world.resource::<super::EmbeddedStatus>().frames.saturating_add(1);
        }
        let capture=if let Some(task)=driver.task.as_mut(){
            if let Some(result)=check_ready(task){driver.task=None;driver.retry=Instant::now()+Duration::from_millis(100);result?}
            else {None}
        } else {
            if Instant::now()>=driver.retry {
                let status=world.resource::<super::EmbeddedStatus>();
                let (pid,id,revision)=(status.pid,status.world_id,status.applied_revision);
                let expected=driver.expected.clone().unwrap();let author=driver.author.clone();
                let minimum=driver.minimum_frame;let directory=directory.join("game");
                driver.task=Some(IoTaskPool::get().spawn(async move{
                    super::tool_previews::read(&directory,pid,id,revision,minimum,&expected,&author)
                }));
            }
            None
        };
        if let Some(capture)=capture {
            if matches!(driver.step,1|2) && !has_highlight_sample(&capture){
                return Err("悬停高亮未出现在实际Gizmo颜色和GPU样本中".into());
            }
            let stage=STAGES[driver.step];
            println!("FEATHRISE_EMBEDDED_LIVE_STEP {}",json!({"stage":format!("terrain_hover_{stage}")}));
            driver.proofs.push(json!({"stage":stage,"pointer":state,"requested":actual,"capture":capture}));
            driver.step+=1;driver.expected=None;
            if driver.step<STAGES.len(){object_pose::hover_target(world,match driver.step {1=>1,2=>2,_=>0})?;}
        }
        Ok(None)
    })
}

fn snapshot_applied(world:&World)->bool{
    super::current_snapshot_applied(world,world.resource::<super::EmbeddedStatus>().applied_revision)
}

/// 画布选择/停放只允许原绑定系统的单次代次推进；内容、保存回执及历史仍逐字段完全相同。
fn verify_rebind(before:&Value,before_canvas:&Value,after:&Value,after_canvas:&Value)->Result<(),String>{
    let mut content_before=before.clone();let mut content_after=after.clone();
    content_before.as_object_mut().ok_or("缺少选择前作者身份")?.remove("revision");
    content_after.as_object_mut().ok_or("缺少选择后作者身份")?.remove("revision");
    let revision=before["revision"].as_u64().ok_or("缺少画布绑定前修订")?;
    let expected=revision.wrapping_add(u64::from(before_canvas["active"]!=after_canvas["active"]));
    if content_before!=content_after || after["revision"]!=expected {
        return Err(format!("画布选择/恢复超出原绑定代次变化：before={before} after={after}"));
    }
    Ok(())
}

fn has_highlight_sample(capture:&Value)->bool{
    capture["author_tools"]["samples"].as_array().is_some_and(|samples|samples.iter().any(|sample|{
        sample["matched"]==true && sample["srgb"].as_array().is_some_and(|color|color.len()==3
            && color.iter().zip([0.92,0.72,0.40]).all(|(actual,wanted)|actual.as_f64().is_some_and(|actual|(actual-wanted).abs()<0.01)))
    }))
}

/// 汇总前重读独立JSON与PNG，不把驱动器的阶段完成标记当成截图通过。
pub(super) fn verify(directory:&Path,evidence:&Value)->Result<(),String>{
    if evidence["author_before"]!=evidence["author_after"]
        || evidence["original"]["mode"]!=evidence["restored"]["mode"]
        || evidence["original"]["selection"]!=evidence["restored"]["selection"]
        || evidence["original"]["semantic_sha256"]!=evidence["restored"]["semantic_sha256"] {
        return Err("悬停取证没有保持作者数据或恢复原工具选择".into());
    }
    for stage in ["selection_setup","selection_restore"]{
        let transition=&evidence[stage];
        verify_rebind(&transition["author_before"],&transition["before"]["canvas"],
            &transition["author_after"],&transition["after"]["canvas"])?;
        if transition["snapshot_applied"]!=true || transition["after"]["canvas"]["loading"]!=false
            || transition["before"]["author"]!=transition["author_before"]
            || transition["after"]["author"]!=transition["author_after"]
            || transition["before"]["author_pose"]!=transition["after"]["author_pose"] {
            return Err(format!("{stage}缺少完整画布采用或改变了作者姿态"));
        }
    }
    if evidence["selection_setup"]["author_after"]!=evidence["author_before"]
        || evidence["selection_restore"]["author_before"]!=evidence["author_after"]
        || evidence["selection_setup"]["after"]["canvas"]["active"]!="starter-ground"
        || evidence["selection_restore"]["after"]["canvas"]["active"]!=evidence["selection_setup"]["before"]["canvas"]["active"] {
        return Err("悬停基线未处于独立画布采用和恢复之间".into());
    }
    let proofs=evidence["proofs"].as_array().filter(|proofs|proofs.len()==4).ok_or("地形悬停缺少四个完整阶段")?;
    for (index,stage) in STAGES.into_iter().enumerate(){
        let proof=&proofs[index];let pointer=&proof["pointer"];let capture=&proof["capture"];
        if proof["stage"]!=stage || pointer["hover_ready"]!=true || pointer["mouse_down"]!=false
            || pointer["transforming"]!=false || !pointer["preview_pose"].is_null()
            || pointer["author"]!=evidence["author_before"]
            // 快照修订与单个作者会话修订属于不同身份域；对应本次实际发送的完整工具快照。
            || capture["revision"]!=proof["requested"]["revision"]
            || pointer["author_pose"]!=evidence["terrain_pose"]
            || !super::tool_previews::matches_request(capture,&proof["requested"]) {
            return Err(format!("悬停{stage}没有对应原输入及工程真实几何"));
        }
        if matches!(index,1|2)&&!has_highlight_sample(capture){return Err(format!("悬停{stage}缺少实际高亮像素"))}
        let file=capture["immutable_evidence"].as_str().ok_or("悬停缺少独立截图路径")?;
        if Path::new(file).components().count()!=1 || !file.ends_with(".json"){return Err("悬停取证路径越界".into())}
        let stored:Value=serde_json::from_slice(&std::fs::read(directory.join(file)).map_err(|error|error.to_string())?)
            .map_err(|error|error.to_string())?;
        let mut recorded=capture.clone();recorded.as_object_mut().ok_or("悬停截图回执无效")?.remove("immutable_evidence");
        if recorded!=stored {return Err("悬停汇总与独立落盘回执不同".into())}
        super::tool_previews::verify_pixels(&stored,&directory.join(file).with_extension("png"))?;
        if index>0 && (capture["frame"].as_u64().unwrap_or(0)<=proofs[index-1]["capture"]["frame"].as_u64().unwrap_or(0)
            || capture["author_tools"]["geometry_sha256"]==proofs[index-1]["capture"]["author_tools"]["geometry_sha256"]
            || capture["capture"]["sha256"]==proofs[index-1]["capture"]["capture"]["sha256"]) {
            return Err(format!("悬停{stage}没有更新真实帧、几何色或像素"));
        }
    }
    Ok(())
}

/// 定向重读已执行的原生悬停证据，独立于后续预览工具条/故障恢复的验收结果。
#[cfg(test)]
#[test]
#[ignore = "显式提供本轮自有夹具的FEATHRISE_HOVER_NATIVE_REPORT，重核真实JSON与PNG"]
fn stored_native_hover_proofs_match_actual_gpu_pixels() {
    let path=std::path::PathBuf::from(std::env::var_os("FEATHRISE_HOVER_NATIVE_REPORT").expect("缺少本轮原生报告"));
    assert!(path.is_absolute());
    assert!(std::fs::metadata(&path).unwrap().len()<=16*1024*1024,"有限验收报告超出预算");
    let report:Value=serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(report["scope"],"resident_project_view_and_preserved_saved_world_comparison");
    assert_eq!(report["owner_checked"],true);
    assert_eq!(report["all_author_asset_bytes_preserved"],true);
    super::orchestration::owner(Path::new(report["project_root"].as_str().unwrap())).unwrap();
    verify(&path.parent().unwrap().join("game"),&report["semantic_tools"]["terrain_hover"]).unwrap();
    println!("真实悬停四阶段JSON、PNG、几何颜色、原输入与作者身份独立核验通过；完整常驻流程结果不据此改写");
}
