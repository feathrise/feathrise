//! 原四球和网格按钮的真实窗口取证；CPU请求、客体相机组件和GPU截图三者必须一致。
use bevy::{prelude::*, tasks::{IoTaskPool, Task, futures::check_ready}};
use feathrise_editor_map::standalone::embedded_validation::{self as author, live};
use serde_json::{Value,json};
use std::{fs, io::Read, path::Path, time::{Duration,Instant}};

#[derive(Resource)]
struct Driver {
    original:Value, author:Value, step:usize, started:Instant, pulse:Option<Entity>,
    proofs:Vec<Value>, expected:Option<Value>, task:Option<Task<Result<Option<Value>,String>>>, retry:Instant,
    selection:Option<String>, restoring:bool,
}

pub(super) fn exercise(world:&mut World,directory:&Path)->Result<Option<Value>,String> {
    if !world.contains_resource::<Driver>() {
        let original=live::render_probe(world)?;
        let author=serde_json::to_value(author::probe(world)?).map_err(|error|error.to_string())?;
        let selection=live::render_selection(world);
        live::set_render_selection(world,None)?;
        world.insert_resource(Driver {original,author,step:0,started:Instant::now(),pulse:None,
            proofs:Vec::new(),expected:None,task:None,retry:Instant::now(),selection,restoring:false});
    }
    world.resource_scope(|world,mut driver:Mut<Driver>| {
        if driver.started.elapsed()>Duration::from_secs(90) {return Err(format!("原四球/网格实际窗口取证超时：{}",driver.step))}
        if let Some(entity)=driver.pulse.take() {
            if let Ok(mut entity)=world.get_entity_mut(entity) {entity.insert(feathrise_editor_ui::EditorButtonClick::default());}
        }
        if serde_json::to_value(author::probe(world)?).map_err(|error|error.to_string())?!=driver.author {
            return Err("原显示控件改变了作者草稿或历史".into())
        }
        let actual=live::render_probe(world)?;
        if driver.step>5 {return finish(world,&mut driver,&actual)}
        if live::render_selection(world).is_some() || !live::render_overlay_empty(world) {return Ok(None)}
        let original_mode=driver.original["shading"][0].as_f64().ok_or("缺少原四球状态")? as u8;
        let (mode,grid)=match driver.step {
            0..=2=>(driver.step as u8,!driver.original["grid"].is_null()),
            3=>(3,false), 4=>(3,true), _=>(original_mode,!driver.original["grid"].is_null()),
        };
        if actual["shading"][0].as_f64()!=Some(f64::from(mode)) {
            driver.pulse=Some(live::press_shading(world,mode)?);return Ok(None)
        }
        if (!actual["grid"].is_null())!=grid {
            driver.pulse=Some(live::toggle_grid(world)?);return Ok(None)
        }
        if driver.step==5 && actual==driver.original && driver.proofs.last().is_some_and(|proof|proof["requested"]==actual) {
            driver.step=6;
            return finish(world,&mut driver,&actual)
        }
        if driver.expected.is_none() {driver.expected=Some(actual.clone());}
        if driver.expected.as_ref()!=Some(&actual) {return Err("显示控件取证时作者显示参数自行变化".into())}
        let capture=if let Some(task)=driver.task.as_mut() {
            if let Some(result)=check_ready(task) {driver.task=None;driver.retry=Instant::now()+Duration::from_millis(100);result?} else {None}
        } else {
            if Instant::now()>=driver.retry {
                let status=world.resource::<super::EmbeddedStatus>();
                let (pid,world_id,revision)=(status.pid,status.world_id,status.applied_revision);
                let directory=directory.join("game");let expected=actual.clone();let author=driver.author.clone();
                driver.task=Some(IoTaskPool::get().spawn(async move {read(&directory,pid,world_id,revision,&expected,&author)}));
            }
            None
        };
        if let Some(capture)=capture {
            let name=["wireframe","solid","material_preview","rendered_without_grid","rendered_with_grid","restored"][driver.step];
            driver.proofs.push(json!({"stage":name,"requested":actual,"capture":capture}));
            println!("FEATHRISE_EMBEDDED_LIVE_STEP {}",json!({"stage":name,"source":"actual project author-view GPU capture"}));
            driver.step+=1;driver.expected=None;
            if driver.step>5 {
                return finish(world,&mut driver,&actual)
            }
        }
        Ok(None)
    })
}

fn finish(world:&mut World,driver:&mut Driver,actual:&Value)->Result<Option<Value>,String> {
    if !driver.restoring {
        live::set_render_selection(world,driver.selection.as_deref())?;
        driver.restoring=true;
        return Ok(None); // 原高亮和布局至少经历一次生产帧后，才交给后续 F12 现场基线。
    }
    let restored=live::render_selection(world);
    if restored!=driver.selection || (restored.is_some()&&live::render_overlay_empty(world)) {return Ok(None)}
    Ok(Some(json!({"source":"original visible buttons -> production camera -> Bevy screenshot",
        "original":driver.original,"restored":actual,"proofs":driver.proofs,
        "selection_before":driver.selection,"selection_restored":restored})))
}

fn components_match(capture:&Value,expected:&Value,playing:bool)->bool {
    let view=&capture["author_view"];
    if view["source"]!="production-PrimarySceneView-components" || view["requested"]!=*expected || view["camera"]["entity"].is_null() {return false}
    let camera=&view["camera"];
    let toolbar=&capture["preview_toolbar"]["runtime"];
    let preview_display=playing && toolbar["active"]==true;
    if playing {
        if !camera["grid"].is_null() {return false}
        if preview_display {
            // 游戏预览的“网格”是模型三角线框，只有真实开关开启时才能持有对应相机租约。
            if toolbar["source"]!=super::preview_toolbar::RUNTIME_SOURCE || toolbar["grid_kind"]!="mesh_triangles"
                || camera["entity"]!=toolbar["primary_camera"] || !toolbar["actual_grid"].is_null()
                || camera["shading"]!=toolbar["actual_shading"]
                || camera["prepass_lease"]!=toolbar["actual_prepass_lease"]
                || camera["depth_prepass"]!=toolbar["actual_depth_prepass"]
                || camera["normal_prepass"]!=toolbar["actual_normal_prepass"] || camera["msaa"]!=toolbar["actual_msaa"]
                || toolbar["requested_collision"]!=toolbar["actual_collision"] {return false}
            if toolbar["requested_grid"]==true {
                if camera["shading"]!=json!({"options":[0.,1.,0.,1.],"wire":[0.,0.,1.,0.]})
                    || camera["prepass_lease"]!=true || camera["depth_prepass"]!=true
                    || camera["normal_prepass"]!=true || camera["msaa"]!="Off" {return false}
            } else if toolbar["requested_grid"]!=false || !camera["shading"].is_null() || camera["prepass_lease"]!=false {return false}
        } else if !camera["shading"].is_null()||camera["prepass_lease"]!=false {return false}
    } else {
        if camera["shading"]["options"]!=expected["shading"] || camera["shading"]["wire"]!=expected["wire"] || camera["grid"]!=expected["grid"] {return false}
        let prepass=expected["shading"][0].as_f64().is_some_and(|mode|mode<=1.)||expected["shading"][3].as_f64().is_some_and(|background|background<0.5);
        if camera["prepass_lease"]!=prepass || prepass&&(camera["depth_prepass"]!=true||camera["normal_prepass"]!=true||camera["msaa"]!="Off") {return false}
    }
    ["terrain_flat","terrain_grid","terrain_backfaces"].into_iter().all(|field|view[field]==expected[field])
        && &view["physics_debug"]==if preview_display {&toolbar["actual_collision"]} else {&expected["physics_debug"]}
}

fn read(directory:&Path,pid:u32,world_id:u64,revision:u64,expected:&Value,author:&Value)->Result<Option<Value>,String> {
    let prefix=format!("{pid}-{revision}-editing-author-view-");
    for (index,entry) in fs::read_dir(directory).map_err(|error|error.to_string())?.enumerate() {
        if index>=2048 {return Err("显示控件验收证据目录超预算".into())}
        let entry=entry.map_err(|error|error.to_string())?;let name=entry.file_name();let name=name.to_string_lossy();
        if !name.starts_with(&prefix)||!name.ends_with(".json") {continue}
        let mut bytes=Vec::new();fs::File::open(entry.path()).map_err(|error|error.to_string())?.take(128*1024+1)
            .read_to_end(&mut bytes).map_err(|error|error.to_string())?;
        if bytes.len()>128*1024 {return Err("显示控件回执超预算".into())}
        let Ok(mut capture)=serde_json::from_slice::<Value>(&bytes) else {continue};
        if capture["pid"]!=pid||capture["world_id"]!=world_id||capture["revision"]!=revision||capture["mode"]!="editing"||capture["source"]!="real-project-world" {return Err("显示控件截图身份不符".into())}
        if !components_match(&capture,expected,false) {continue}
        if !super::tool_previews::capture_empty(&capture) {continue}
        super::placement_evidence::verify(&capture,author)?;
        verify_pixels(&capture,&entry.path().with_extension("png"))?;
        capture["immutable_evidence"]=json!(name.as_ref());
        return Ok(Some(capture))
    }
    Ok(None)
}

fn verify_pixels(capture:&Value,image:&Path)->Result<(),String> {
    if !super::tool_previews::capture_empty(capture) {return Err("原选中框或工具预览仍覆盖线框取证画面".into())}
    if capture["capture"]["source"]!="Bevy ScreenshotCaptured"||capture["capture"]["sha256"].as_str().is_none_or(|hash|hash.len()!=64)
        || capture["capture"]["width"].as_u64().unwrap_or(0)==0 ||capture["capture"]["height"].as_u64().unwrap_or(0)==0 {return Err("显示控件缺少真实GPU像素回读".into())}
    verify_render_readiness(capture)?;
    let mut signature=[0;8];fs::File::open(image).and_then(|mut file|file.read_exact(&mut signature)).map_err(|error|error.to_string())?;
    if signature!=*b"\x89PNG\r\n\x1a\n" {return Err("显示控件实际截图不是PNG".into())}
    super::wire_evidence::verify(capture,image)
}

pub(super) fn verify_render_readiness(capture:&Value)->Result<(),String> {
    let ready=&capture["render_readiness"];
    if ready["source"]!="RenderApp-after-RenderSystems-Render" || ready["ready"]!=true
        || ready["view_matches"]!=true || ready["ticket"].as_u64().unwrap_or(0)==0
        || ready["ticket"]!=ready["render_ticket"]
        || ready["stable_render_submissions"].as_u64().unwrap_or(0)<2
        || ready["camera"]!=capture["author_view"]["camera"]["entity"]
        || ready["size"]!=json!([capture["capture"]["width"],capture["capture"]["height"]]) {
        return Err("显示控件截图缺少当前真实渲染视图的就绪回执".into());
    }
    let pipelines=&ready["pipelines"];
    if pipelines["overflow"]!=false || pipelines["total"].as_u64().unwrap_or(0)==0
        || ["waiting","queued","creating"].iter().any(|field|pipelines[*field]!=0)
        || pipelines["errors"].as_array().is_none_or(|errors|!errors.is_empty()) {
        return Err("显示控件截图时实际GPU管线尚未就绪或编译失败".into());
    }
    Ok(())
}

pub(super) fn verify(directory:&Path,evidence:&Value)->Result<(),String> {
    if evidence["original"]!=evidence["restored"] {return Err("原显示控件验收后未恢复原设置".into())}
    if evidence["selection_before"].is_null() || evidence["selection_before"]!=evidence["selection_restored"] {
        return Err("线框取证后未恢复原对象选择，不能进入 F12 现场验收".into())
    }
    let proofs=evidence["proofs"].as_array().ok_or("缺少原显示控件实际取证")?;
    for name in ["wireframe","solid","material_preview","rendered_without_grid","rendered_with_grid"] {
        let proof=proofs.iter().find(|proof|proof["stage"]==name).ok_or_else(||format!("缺少原显示控件阶段{name}"))?;
        if !components_match(&proof["capture"],&proof["requested"],false) {return Err(format!("{name}没有采用原真实相机组件"))}
        let file=proof["capture"]["immutable_evidence"].as_str().ok_or("显示控件缺少独立截图路径")?;
        if Path::new(file).components().count()!=1||!file.ends_with(".json") {return Err("显示控件取证路径越界".into())}
        let stored:Value=serde_json::from_slice(&fs::read(directory.join(file)).map_err(|error|error.to_string())?).map_err(|error|error.to_string())?;
        let mut recorded=proof["capture"].clone();recorded.as_object_mut().unwrap().remove("immutable_evidence");
        if stored!=recorded {return Err(format!("{name}实际落盘回执不一致"))}
        verify_pixels(&stored,&directory.join(file).with_extension("png"))?;
    }
    let hash=|stage:&str|proofs.iter().find(|proof|proof["stage"]==stage).unwrap()["capture"]["capture"]["sha256"].clone();
    if hash("wireframe")==hash("solid") || hash("solid")==hash("rendered_without_grid") || hash("rendered_without_grid")==hash("rendered_with_grid") {
        return Err("原四球/网格的实际GPU像素没有产生应有区别".into())
    }
    Ok(())
}

pub(super) fn verify_mode(capture:&Value,playing:bool)->Result<(),String> {
    let expected=&capture["author_view"]["requested"];
    if !components_match(capture,expected,playing) {return Err("实际工程相机没有按F12采用/释放原作者显示组件".into())}
    super::tool_previews::verify_visibility(capture,playing)?;
    Ok(())
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn preview_mesh_triangles_require_real_shading_and_reject_reference_grid_or_stale_lease() {
        let expected=json!({"shading":[3.,1.,0.,1.],"wire":[0.,1.,0.,0.],"grid":null,
            "terrain_flat":false,"terrain_grid":false,"terrain_backfaces":false,"physics_debug":false});
        let mesh=json!({"options":[0.,1.,0.,1.],"wire":[0.,0.,1.,0.]});
        let mut capture=json!({"author_view":{"source":"production-PrimarySceneView-components","requested":expected,
            "camera":{"entity":1,"shading":mesh,"grid":null,"prepass_lease":true,
                "depth_prepass":true,"normal_prepass":true,"msaa":"Off"},
            "terrain_flat":false,"terrain_grid":false,"terrain_backfaces":false,"physics_debug":false},
            "preview_toolbar":{"runtime":{"source":super::super::preview_toolbar::RUNTIME_SOURCE,
                "grid_kind":"mesh_triangles","active":true,"primary_camera":1,"requested_grid":true,
                "actual_grid":null,"actual_shading":mesh,"actual_prepass_lease":true,"actual_depth_prepass":true,
                "actual_normal_prepass":true,"actual_msaa":"Off","requested_collision":false,"actual_collision":false}}});
        assert!(components_match(&capture,&expected,true));
        capture["preview_toolbar"]["runtime"]["actual_grid"]=json!([[0.,0.,0.,1.],[0.,1.,0.,0.],[1.,0.,0.,0.],[0.,0.,1.,0.]]);
        assert!(!components_match(&capture,&expected,true),"无限参考网格不能冒充模型三角网格");
        capture["preview_toolbar"]["runtime"]["actual_grid"]=Value::Null;
        capture["author_view"]["camera"]["shading"]["wire"][2]=json!(0.);
        assert!(!components_match(&capture,&expected,true),"实际相机必须使用三角拓扑语义");
        capture["author_view"]["camera"]["shading"]=mesh;
        capture["preview_toolbar"]["runtime"]["requested_grid"]=json!(false);
        assert!(!components_match(&capture,&expected,true),"关闭后旧线框与租约不能残留");
        capture["author_view"]["camera"]["shading"]=Value::Null;
        capture["author_view"]["camera"]["prepass_lease"]=json!(false);
        capture["preview_toolbar"]["runtime"]["actual_shading"]=Value::Null;
        capture["preview_toolbar"]["runtime"]["actual_prepass_lease"]=json!(false);
        assert!(components_match(&capture,&expected,true));
    }
    #[test] fn author_view_request_without_actual_components_or_play_release_is_rejected() {
        let expected=json!({"shading":[1.,1.,0.,1.],"wire":[1.,1.,0.,0.],"grid":null,"terrain_flat":false,"terrain_grid":false,"terrain_backfaces":false,"physics_debug":false});
        let mut capture=json!({"author_view":{"source":"production-PrimarySceneView-components","requested":expected,"camera":{
            "entity":1,"shading":{"options":expected["shading"],"wire":expected["wire"]},"grid":null,"prepass_lease":true,"depth_prepass":true,"normal_prepass":true,"msaa":"Off"},
            "terrain_flat":false,"terrain_grid":false,"terrain_backfaces":false,"physics_debug":false}});
        assert!(components_match(&capture,&expected,false));assert!(!components_match(&capture,&expected,true));
        capture["author_view"]["camera"]["shading"]=Value::Null;capture["author_view"]["camera"]["prepass_lease"]=json!(false);
        assert!(components_match(&capture,&expected,true));assert!(!components_match(&capture,&expected,false));
    }
}
