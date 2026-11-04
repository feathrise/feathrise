//! 自有SDK工程的真实root→author→game预览三键专项；仅显式场景安装，不改生产状态。
mod resources;
mod types;
mod snapshot_evidence;
pub(crate) use snapshot_evidence::verify as snapshot_pixel_identity;
#[path="../workspace_ui/scale_validation/io.rs"]
mod io;

use resources::*;
use super::{components::ChromeButton,types::WindowAction};
use bevy::{prelude::*,input::{ButtonState,mouse::MouseButtonInput},tasks::IoTaskPool,
    render::view::screenshot::{Screenshot,ScreenshotCaptured},window::{PrimaryWindow,RawHandleWrapper},
    ui::InteractionDisabled};
use crate::{embedded_viewport::{EmbeddedStatus,validation as evidence},viewport_mode::{RequestViewportMode,ViewportMode,ViewportGeometry}};
use feathrise_editor_map::standalone::{embedded_validation as author,embedded_snapshot};
use feathrise_editor_protocol::Mode;
use serde_json::{Value,json};
use std::{path::Path,time::{Duration,Instant}};
use windows_sys::Win32::{Foundation::{HWND,RECT},UI::WindowsAndMessaging::*};

fn ensure(value:bool,message:&str)->Result<(),String>{value.then_some(()).ok_or_else(||message.into())}
fn poll<T>(reply:&Reply<T>)->Result<Option<T>,String>{match reply.try_recv(){Ok(value)=>value.map(Some),Err(async_channel::TryRecvError::Empty)=>Ok(None),Err(_)=>Err("预览三键后台回执丢失".into())}}

/// 作者panic可能早于其Last诊断；仅此显式自有三层专项把原stderr直接持久化，不新增管道或改stdout协议。
pub(crate) fn capture_author_stderr(command:&mut std::process::Command,project:&Path)->Result<(),String>{
    if std::env::var("FEATHRISE_EMBEDDED_SCENARIO").as_deref()!=Ok("preview-chrome")
        ||std::env::var("FEATHRISE_PREVIEW_CHROME_NATIVE_INPUT").as_deref()!=Ok("1"){return Ok(());}
    let owned=evidence::code_iteration_owner(project)?;
    let directory=std::path::PathBuf::from(std::env::var_os("FEATHRISE_EMBEDDED_SMOKE_DIR").ok_or("作者stderr专项缺少独占输出目录")?);
    ensure(directory.is_absolute(),"作者stderr输出必须为绝对路径")?;
    // canonical身份核对不能代替原输入路径审计：逐层拒绝junction/symlink，避免日志借别名写到其他位置。
    for path in [project,directory.as_path()]{
        for (depth,parent) in path.ancestors().enumerate(){
            ensure(depth<64,"作者stderr路径层数超出专项固定预算")?;
            if !parent.as_os_str().is_empty(){ensure(io::ordinary(parent)?.is_dir(),"作者stderr路径父级不是普通目录")?;}
        }
    }
    let directory=directory.canonicalize().map_err(|error|error.to_string())?;
    let expected=owned.root.parent().ok_or("自有工程缺少owner工作根")?.join("embedded-comparison");
    ensure(directory==expected,"作者stderr目录必须是当前owner工作根下的embedded-comparison")?;
    let file=std::fs::OpenOptions::new().create_new(true).write(true).open(directory.join("author-stderr.log"))
        .map_err(|error|format!("无法独占创建本轮作者stderr日志：{error}"))?;
    command.stderr(std::process::Stdio::from(file));
    Ok(())
}

pub(crate) fn install(app:&mut App,project:&Path,directory:std::path::PathBuf)->Result<(),String>{
    let owned=evidence::code_iteration_owner(project)?;
    ensure(directory.is_absolute()&&io::ordinary(&directory)?.is_dir()&&!directory.join("preview-chrome.json").exists(),"预览三键必须使用本轮独占输出")?;
    let directory=directory.canonicalize().map_err(|error|error.to_string())?;
    let prefs=std::path::PathBuf::from(std::env::var_os("FEATHRISE_PREFERENCES_FILE").ok_or("预览三键缺少隔离偏好")?);
    ensure(prefs.parent().and_then(|value|value.canonicalize().ok()).as_ref()==Some(&directory),"拒绝读写真实用户偏好")?;
    let global=std::path::PathBuf::from(std::env::var_os("FEATHRISE_VALIDATION_GLOBAL_PLUGINS").ok_or("预览三键缺少隔离全局插件根")?).canonicalize().map_err(|error|error.to_string())?;
    ensure(global.starts_with(&directory)&&io::read_now(&global.join(".feathrise-plugin-fixture-owner"))?.is_some_and(|value|value.bytes==b"feathrise-single-binary-validation-v1"),"全局插件根不归本轮所有")?;
    let embed=crate::workspace_window::EmbedTarget::from_env()?.ok_or("预览三键必须通过真实工作台宿主打开作者子窗口")?;
    let native_token=if std::env::var("FEATHRISE_PREVIEW_CHROME_NATIVE_INPUT").as_deref()==Ok("1"){
        Some(std::env::var("FEATHRISE_PREVIEW_CHROME_NATIVE_TOKEN").ok().filter(|value|!value.is_empty()&&value.len()<=128).ok_or("真实NC补验缺少本轮token")?)
    }else{None};
    ensure(native_token.is_some(),"预览三键专项必须由真实NC输入观察器启动；官方ECS输入不能替代原生caption_down")?;
    author::install(app);
    app.insert_resource(Plan{directory,project:owned.root,started:Instant::now(),changed:Instant::now(),stage:0,frames:0,map_requested:false,mouse:0,target:None,clicked:false,
        root:embed.parent,root_pid:embed.pid,author_hwnd:0,pid:0,world_id:0,baseline:None,baseline_chrome:None,min_frame:0,
        game_task:None,game:None,retry:Instant::now(),image:None,image_ready:false,checkpoint_done:false,attempts:0,minimize:None,minimize_proof:None,
        proofs:Vec::new(),captures:Vec::new(),write:None,report_written:false,failure:None,native_token,gate_written:false,gate_write:None,
        gate_read:None,gate_ack:false,gate_retry:Instant::now(),diagnostic:None,diagnostic_count:0,
        diagnostic_next:Instant::now()+Duration::from_secs(10),game_last_result:None,diagnostic_error_seen:false,
        failure_status:None,failure_log:None,
        native_sync_trace:std::env::var("FEATHRISE_PREVIEW_CHROME_NATIVE_TRACE").as_deref()==Ok("1"),
        native_sync_write:None,native_sync_count:0,
        native_sync_last:None,native_sync_last_gate:None})
        .add_systems(First,input).add_systems(Last,drive);
    Ok(())
}

fn name(stage:u8)->&'static str{match stage{0=>"author-ready",1=>"embedded-editing",2=>"playing-original-captions",3=>"playing-maximized",4=>"playing-restored",5=>"playing-minimized-and-restored",6=>"returned-editing",7=>"playing-before-close",8=>"close-confirm-restored-editor",_=>"confirm-owned-exit"}}
fn action(stage:u8)->Option<WindowAction>{match stage{3|4=>Some(WindowAction::Maximize),5=>Some(WindowAction::Minimize),8=>Some(WindowAction::Close),_=>None}}
fn rect(world:&World,entity:Entity)->Option<Rect>{
    if world.get::<InteractionDisabled>(entity).is_some()||!world.get::<InheritedVisibility>(entity)?.get(){return None;}
    let node=world.get::<ComputedNode>(entity)?;
    let transform=world.get::<UiGlobalTransform>(entity)?;
    (!node.is_empty()).then(||Rect::from_center_size(transform.translation,node.size()))
}
fn button(world:&mut World,stage:u8)->Result<Option<Entity>,String>{
    if stage==9{return crate::native_project_close::embedded_validation::fixture_finish_target(world);}
    if matches!(stage,2|7){return Ok(world.query_filtered::<Entity,With<feathrise_editor_core::ProjectLaunchButton>>().iter(world).find(|entity|rect(world,*entity).is_some()));}
    let Some(action)=action(stage)else{return Ok(None);};
    Ok(world.query::<(Entity,&ChromeButton)>().iter(world).find_map(|(entity,value)|(matches!(value,ChromeButton::Window(current) if *current==action)&&rect(world,entity).is_some()).then_some(entity)))
}
fn valid_root(plan:&Plan)->bool{let mut pid=0;unsafe{IsWindow(plan.root as HWND)!=0&&GetWindowThreadProcessId(plan.root as HWND,&mut pid)!=0&&pid==plan.root_pid}}

fn input(world:&mut World){let Some(mut plan)=world.remove_resource::<Plan>()else{return;};
    if (plan.failure.is_none()||plan.report_written)&&let Err(error)=issue(world,&mut plan){plan.failure=Some(error);}
    world.insert_resource(plan);
}
fn issue(world:&mut World,plan:&mut Plan)->Result<(),String>{
    if !matches!(plan.stage,2|3|4|5|7|8|9)||plan.mouse>=3{return Ok(());}
    if plan.stage==8&&plan.native_token.is_some(){return Ok(());}
    if plan.stage==9&&!plan.report_written{return Ok(());}
    ensure(valid_root(plan),"自有宿主PID/HWND身份已失效，停止输入")?;
    if plan.stage==5&&plan.minimize.is_none()&&plan.minimize_proof.is_none(){plan.minimize=Some(observe_minimize(plan.root,plan.root_pid));}
    if plan.target.is_none(){plan.target=button(world,plan.stage)?;}
    let Some(entity)=plan.target else{return Ok(());};let Some(bounds)=rect(world,entity)else{return Ok(());};
    let Ok(window)=world.query_filtered::<Entity,With<PrimaryWindow>>().single(world)else{return Ok(());};
    world.get_mut::<Window>(window).ok_or("作者主窗已消失")?.set_physical_cursor_position(Some(bounds.center().as_dvec2()));
    match plan.mouse{0=>plan.mouse=1,1 if world.get::<Interaction>(entity)==Some(&Interaction::Hovered)=>{
        world.write_message(MouseButtonInput{button:MouseButton::Left,state:ButtonState::Pressed,window});plan.mouse=2;
    },2=>{
        ensure(world.get::<Interaction>(entity)==Some(&Interaction::Pressed),"原三键或预览按钮没有接收官方按下")?;
        world.write_message(MouseButtonInput{button:MouseButton::Left,state:ButtonState::Released,window});plan.mouse=3;
    },_=>{}}
    Ok(())
}

fn observe_minimize(root:usize,pid:u32)->Reply<Value>{
    let(sender,receiver)=async_channel::bounded(1);
    IoTaskPool::get().spawn(async move{
        let result=(||{let start=Instant::now();let mut samples=Vec::new();
            while start.elapsed()<Duration::from_secs(12){let mut actual=0;unsafe{GetWindowThreadProcessId(root as HWND,&mut actual);}
                ensure(actual==pid&&unsafe{IsWindow(root as HWND)}!=0,"最小化观察器宿主身份变化")?;
                let iconic=unsafe{IsIconic(root as HWND)}!=0;
                samples.push(json!({"elapsed_us":start.elapsed().as_micros() as u64,"iconic":iconic}));
                if iconic{
                    // 最小化会暂停UI帧；仅已实测IsIconic=true后用原系统恢复命令归还自有窗口。
                    ensure(unsafe{PostMessageW(root as HWND,WM_SYSCOMMAND,SC_RESTORE as usize,0)}!=0,"原系统恢复命令发送失败")?;
                    return Ok(json!({"source":"owned root HWND IsIconic polling","root_hwnd":root,"root_pid":pid,"observed_minimized":true,
                        "restore":"WM_SYSCOMMAND/SC_RESTORE after actual IsIconic","sample_interval_ms":10,"samples":samples}));
                }
                std::thread::sleep(Duration::from_millis(10));
            }Err("12秒内未实测宿主最小化，不能用按钮事件代替系统结果".into())})();let _=sender.try_send(result);
    }).detach();receiver
}

fn next(world:&World,plan:&mut Plan){plan.stage+=1;plan.changed=Instant::now();plan.mouse=0;plan.target=None;plan.clicked=false;plan.game=None;plan.game_task=None;
    plan.image_ready=false;plan.checkpoint_done=false;plan.image=None;plan.attempts=0;plan.retry=Instant::now();plan.min_frame=world.resource::<EmbeddedStatus>().frames.saturating_add(1);
    plan.gate_written=false;plan.gate_write=None;plan.gate_read=None;plan.gate_ack=false;plan.gate_retry=Instant::now();
    plan.diagnostic_count=0;plan.diagnostic_next=Instant::now()+Duration::from_secs(10);plan.game_last_result=None;}

fn native(world:&mut World,plan:&mut Plan)->Result<Value,String>{
    let raw=world.query_filtered::<&RawHandleWrapper,With<PrimaryWindow>>().single(world).map_err(|error|error.to_string())?;
    let own=crate::workspace_window::primary_hwnd(raw).ok_or("作者没有原生HWND")?;
    ensure(valid_root(plan)&&unsafe{GetParent(own as HWND)}==plan.root as HWND,"必须保持同一root→author父链")?;
    plan.author_hwnd=own;
    let mut root=RECT::default();let mut client=RECT::default();
    ensure(unsafe{GetWindowRect(plan.root as HWND,&mut root)}!=0&&unsafe{GetClientRect(own as HWND,&mut client)}!=0,"原生宿主/作者矩形查询失败")?;
    Ok(json!({"root_hwnd":plan.root,"root_pid":plan.root_pid,"author_hwnd":own,"author_pid":std::process::id(),
        "root_rect":[root.left,root.top,root.right,root.bottom],"author_client":[client.left,client.top,client.right,client.bottom],
        "maximized":unsafe{IsZoomed(plan.root as HWND)}!=0,"minimized":unsafe{IsIconic(plan.root as HWND)}!=0}))
}
fn chrome(world:&World,plan:&Plan,is_playing:bool)->Result<Option<Value>,String>{
    let Some(probe)=super::preview_chrome_probe(world)else{return Ok(None);};
    if is_playing{
        let bottom=probe["layout"]["bottom"].as_i64().unwrap_or(0);
        if probe["layout"]["visible"]!=true||bottom<=0||probe["camera"]["active"]!=true{return Ok(None);}
        let Some(captions)=probe["captions"].as_array().filter(|items|items.len()==3)else{return Ok(None);};
        for action in ["minimize","maximize","close"]{
            let Some(caption)=captions.iter().find(|item|item["action"]==action)else{return Ok(None);};
            let Some(bounds)=caption["rect"].as_array().filter(|items|items.len()==4)else{return Ok(None);};
            if caption["visible"]!=true||bounds[3].as_f64().unwrap_or(f64::MAX).ceil()>bottom as f64{return Ok(None);}
            if let Some(before)=&plan.baseline_chrome{ensure(before["captions"].as_array().is_some_and(|items|items.iter().any(|item|item["action"]==action&&item["entity"]==caption["entity"])),"预览必须移动原三键实体，不能复制按钮")?;}
        }
        let geometry=world.resource::<ViewportGeometry>();
        if geometry.rect[0]!=0||geometry.rect[1]!=bottom as i32{return Ok(None);}
    }else if probe["layout"]["visible"]!=false{return Ok(None);}
    Ok(Some(probe))
}

fn game(world:&mut World,plan:&mut Plan,is_playing:bool)->Result<bool,String>{
    let status=world.resource::<EmbeddedStatus>();
    if !status.ready||status.playing!=is_playing||status.observed_mode!=Some(if is_playing{Mode::Playing}else{Mode::Editing})
        ||!embedded_snapshot::ready_is_current(world){return Ok(false);}
    if plan.pid==0{plan.pid=status.pid;plan.world_id=status.world_id;}
    ensure(status.pid==plan.pid&&status.world_id==plan.world_id,"窗口操作/预览往返更换了真实游戏PID或World")?;
    if is_playing&&!world.resource::<ViewportGeometry>().focused{return Ok(false);}
    if plan.game.is_some(){return Ok(true);}
    if let Some(reply)=&plan.game_task&&let Some(proof)=poll(reply)?{plan.game_task=None;plan.retry=Instant::now()+Duration::from_millis(100);
        plan.game_last_result=Some(json!({"elapsed_ms":plan.changed.elapsed().as_millis() as u64,"returned_capture":proof.is_some()}));
        if let Some(proof)=proof{plan.game=Some(proof);return Ok(true);}}
    if plan.game_task.is_none()&&Instant::now()>=plan.retry{
        let directory=plan.directory.join("game");let snapshot=plan.directory.join(format!("{}-game.png",name(plan.stage)));let pid=plan.pid;let revision=status.applied_revision;let min_frame=plan.min_frame;
        let geometry=world.resource::<ViewportGeometry>().clone();let author=serde_json::to_value(author::probe(world)?).map_err(|error|error.to_string())?;
        let(sender,receiver)=async_channel::bounded(1);plan.game_task=Some(receiver);
        IoTaskPool::get().spawn(async move{let result=(||{
            let Some(mut proof)=evidence::code_iteration_capture_mode(&directory,pid,revision,is_playing,min_frame,&author,&geometry)? else{return Ok(None);};
            // SDK同修订会刷新同名图；冻结这一阶段的真实字节，并核hash防止将后一帧冒充前一帧。
            use std::io::Write;
            let source=directory.join(format!("{pid}-{revision}-{}.png",if is_playing{"playing"}else{"editing"}));
            let metadata=std::fs::metadata(&source).map_err(|error|error.to_string())?;
            ensure(metadata.len()<=128*1024*1024,"游戏PNG超过专项128MiB预算")?;
            let bytes=std::fs::read(source).map_err(|error|error.to_string())?;
            let Some(pixels)=snapshot_pixel_identity(&bytes,&proof["game"])?else{return Ok(None);};
            let mut file=std::fs::OpenOptions::new().write(true).create_new(true).open(&snapshot).map_err(|error|error.to_string())?;
            file.write_all(&bytes).and_then(|_|file.sync_all()).map_err(|error|error.to_string())?;
            proof["snapshot_png"]=json!(snapshot);proof["snapshot_pixels"]=pixels;Ok(Some(proof))
        })();let _=sender.try_send(result);}).detach();
    }Ok(false)
}

fn checkpoint(world:&mut World,plan:&mut Plan,is_playing:bool)->Result<bool,String>{
    if plan.checkpoint_done{return Ok(true);}
    let Some(chrome)=chrome(world,plan,is_playing)?else{return Ok(false);};
    let author=serde_json::to_value(author::probe(world)?).map_err(|error|error.to_string())?;
    if let Some(before)=&plan.baseline{ensure(*before==author,"窗口三键与预览切换改变了作者草稿/历史")?;}
    let native=native(world,plan)?;
    if is_playing{
        let proof=plan.game.as_ref().ok_or("预览缺少独立原生游戏证明")?;
        let rect=proof["window"]["physical_rect"].as_array().ok_or("缺少实际游戏矩形")?;
        let bottom=chrome["layout"]["bottom"].as_i64().ok_or("标题栏未测量")?;
        let client=&native["author_client"];
        ensure(*rect==vec![json!(0),json!(bottom),client[2].clone(),json!(client[3].as_i64().ok_or("作者高度缺失")?-bottom)],"真实游戏HWND没有精确填满标题栏以下客户区")?;
    }
    if let Some(reply)=&plan.image&&let Some(proof)=poll(reply)?{plan.image=None;plan.image_ready=proof["accepted"]==true;plan.captures.push(proof);plan.retry=Instant::now()+Duration::from_millis(250);}
    if !plan.image_ready{
        if plan.image.is_none()&&Instant::now()>=plan.retry{
            ensure(plan.attempts<16,"预览GPU16次内未产生有效文字像素")?;plan.attempts+=1;
            let physical=world.query_filtered::<&Window,With<PrimaryWindow>>().single(world).map_err(|error|error.to_string()).map(|window|[window.physical_width(),window.physical_height()])?;
            let view=json!({"stage":name(plan.stage),"native":native,"window_chrome":chrome});
            let path=plan.directory.join(format!("{}-author-{:02}.png",name(plan.stage),plan.attempts));let(sender,receiver)=async_channel::bounded(1);plan.image=Some(receiver);
            world.spawn(Screenshot::primary_window()).observe(move|capture:On<ScreenshotCaptured>|{io::image(capture.image.clone(),path.clone(),physical,view.clone(),sender.clone());});
        }return Ok(false);
    }
    plan.proofs.push(json!({"stage":name(plan.stage),"native":native,"window_chrome":chrome,"author":author,"game":plan.game,
        "official_click":plan.clicked,"button_entity":plan.target.map(Entity::to_bits),"minimize":if plan.stage==5{plan.minimize_proof.clone()}else{None},
        "author_gpu":plan.captures.last()}));plan.checkpoint_done=true;Ok(true)
}

fn native_gate(world:&mut World,plan:&mut Plan,stage:&str)->Result<bool,String>{
    let Some(token)=plan.native_token.clone()else{return Ok(true);};
    if plan.native_sync_trace&&!native_sync_evidence(world,plan,stage,&token)?{return Ok(false);}
    if plan.gate_ack{return Ok(true);}
    if let Some(reply)=&plan.gate_write&&poll(reply)?.is_some(){plan.gate_write=None;plan.gate_written=true;}
    if !plan.gate_written&&plan.gate_write.is_none(){
        let actual=native(world,plan)?;let probe=super::preview_chrome_probe(world).ok_or("原生门缺少真实标题栏")?;
        ensure(probe["layout"]["visible"]==true,"真实NC门必须仍在预览标题栏")?;
        let source=plan.game.as_ref().or_else(||plan.proofs.iter().rev().find_map(|proof|proof.get("game").filter(|value|!value.is_null()))).ok_or("原生门缺少实际游戏HWND")?;
        let ready=json!({"schema":1,"token":token,"stage":stage,"root_hwnd":plan.root,"root_pid":plan.root_pid,
            "author_hwnd":plan.author_hwnd,"author_pid":std::process::id(),"game_hwnd":source["window"]["hwnd"],"game_pid":plan.pid,
            "layout":probe["layout"],"captions":probe["captions"],"source_probe":probe,"native":actual,
            "purpose":"bounded real SendInput on owned foreground root; preserve original NC queue"});
        plan.gate_write=Some(io::report(plan.directory.join(format!("{stage}.ready.json")),ready));return Ok(false);
    }
    if !plan.gate_written{return Ok(false);}
    if let Some(reply)=&plan.gate_read&&let Some(stamp)=poll(reply)?{plan.gate_read=None;plan.gate_retry=Instant::now()+Duration::from_millis(80);if let Some(stamp)=stamp{
        let ack:Value=serde_json::from_slice(&stamp.bytes).map_err(|error|error.to_string())?;
        ensure(ack["schema"]==1&&ack["token"]==token&&ack["stage"]==stage&&ack["passed"]==true,"真实NC ack身份或实际操作结果不匹配")?;
        plan.proofs.push(json!({"native_input_gate":stage,"external_real_input_ack":ack}));plan.gate_ack=true;return Ok(true);
    }}
    if plan.gate_read.is_none()&&Instant::now()>=plan.gate_retry{plan.gate_read=Some(io::read(plan.directory.join(format!("{stage}.ack.json"))));}
    Ok(false)
}

/// Last读取上帧afterLast原生同步的固定DTO；仅显式gate观察，文件写入始终在单个后台槽。
fn native_sync_evidence(world:&World,plan:&mut Plan,stage:&str,token:&str)->Result<bool,String>{
    if let Some(reply)=&plan.native_sync_write{
        if poll(reply)?.is_none(){return Ok(false);}
        plan.native_sync_write=None;
    }
    // 计数不随gate或业务阶段重置；达到固定总额后仍需先消费末份真实IO回执。
    if plan.native_sync_count>=32{return Ok(true);}
    let current=world.get_resource::<super::resources::NativeSyncDiagnostics>().copied()
        .ok_or("原生按压诊断资源没有安装")?;
    if plan.native_sync_last.as_ref()==Some(&current)&&plan.native_sync_last_gate.as_deref()==Some(stage){return Ok(true);}
    plan.native_sync_count+=1;
    let sequence=plan.native_sync_count;
    let value=json!({"schema":1,"scope":"preview-chrome-native-sync-readonly","token":token,"stage":stage,"sequence":sequence,
        "maximum_files_for_both_gates":32,"stage_elapsed_ms":plan.changed.elapsed().as_millis() as u64,
        "observation":"Last reads the previous frame afterLast ChromeNativeSync DTO; semantic changes only, not every frame",
        "native_sync":current});
    plan.native_sync_write=Some(io::report(plan.directory.join(format!("{stage}-native-sync-{sequence:02}.json")),value));
    plan.native_sync_last=Some(current);plan.native_sync_last_gate=Some(stage.to_owned());
    // 完成落盘才能继续发布/采用gate回执；诊断IO失败会正常失败，不把缺证据当成功。
    Ok(false)
}

fn drive(world:&mut World){let Some(mut plan)=world.remove_resource::<Plan>()else{return;};
    diagnostic(world,&mut plan);
    if plan.failure.is_none()&&let Err(error)=advance(world,&mut plan){plan.failure=Some(error);}
    if plan.failure.is_some()&&!plan.report_written&&plan.write.is_none(){
        plan.failure_status=Some(embedded_status(world));
        plan.failure_log=world.get_resource::<crate::project_log::ProjectLogState>().map(|log|log.validation_tail());
        let observed=native(world,&mut plan).ok();
        eprintln!("FEATHRISE_PREVIEW_CHROME_FAILED {}",json!({"stage":name(plan.stage),"error":plan.failure,"native":observed,
            "embedded_status":plan.failure_status,"project_log_tail":plan.failure_log,"chrome":super::preview_chrome_probe(world)}));
        plan.write=Some(io::report(plan.directory.join("preview-chrome.json"),report(&plan,false)));
    }
    if plan.failure.is_some()&&let Some(reply)=&plan.write&&let Ok(Some(()))=poll(reply){plan.report_written=true;plan.write=None;
        // 失败也只对本轮归属root走原关闭入口；不写AppExit或假冒成功退出。
        if valid_root(&plan){unsafe{PostMessageW(plan.root as HWND,WM_CLOSE,0,0);}plan.stage=9;plan.mouse=0;plan.target=None;plan.clicked=false;}
    }
    world.insert_resource(plan);
}

/// 只读等待诊断不参与业务通过条件；每阶段10秒一份、最多三份，帧内只复制固定小DTO。
fn diagnostic(world:&mut World,plan:&mut Plan){
    if !plan.diagnostic_error_seen&&world.resource::<EmbeddedStatus>().error.is_some(){
        plan.diagnostic_error_seen=true;plan.diagnostic_next=Instant::now();
    }
    if let Some(reply)=&plan.diagnostic{match poll(reply){Ok(Some(()))=>plan.diagnostic=None,Err(error)=>{
        eprintln!("FEATHRISE_PREVIEW_CHROME_DIAGNOSTIC_IO {error}");plan.diagnostic=None;
    },Ok(None)=>return,}}
    if plan.diagnostic.is_some()||plan.diagnostic_count>=3||Instant::now()<plan.diagnostic_next{return;}
    plan.diagnostic_count+=1;plan.diagnostic_next=Instant::now()+Duration::from_secs(10);
    let status=world.resource::<EmbeddedStatus>();let pid=status.pid;let revision=status.applied_revision;
    let snapshot=world.resource::<embedded_snapshot::EmbeddedSnapshotState>();let geometry=world.resource::<ViewportGeometry>();
    let mut value=json!({"schema":1,"scope":"preview-chrome-readonly-wait","stage":name(plan.stage),"sequence":plan.diagnostic_count,
        "stage_elapsed_ms":plan.changed.elapsed().as_millis() as u64,"min_frame":plan.min_frame,
        "status":{"active":status.active,"ready":status.ready,"pid":pid,"world_id":status.world_id,"playing":status.playing,
            "applied_revision":revision,"frames":status.frames,"ticks":status.ticks,"observed_mode":status.observed_mode,"error":status.error},
        "snapshot":{"ready_current":embedded_snapshot::ready_is_current(world),"revision":snapshot.revision,"map_id":snapshot.map_id,
            "pending":snapshot.pending,"error":snapshot.error,"ready_revision":snapshot.ready.as_ref().map(|ready|ready.author_revision),
            "waiting_reason":embedded_snapshot::waiting_reason(world)},
        "geometry":{"parent_hwnd":geometry.parent_hwnd,"parent_pid":geometry.parent_pid,"rect":geometry.rect,"visible":geometry.visible,
            "focused":geometry.focused,"revision":geometry.revision,"excluded_rects":geometry.excluded_rects,"excluded_shapes":geometry.excluded_shapes},
        "io":{"game_task_pending":plan.game_task.is_some(),"game_ready":plan.game.is_some(),"last_game_result":plan.game_last_result,
            "image_pending":plan.image.is_some(),"image_ready":plan.image_ready,"gate_write_pending":plan.gate_write.is_some(),"gate_read_pending":plan.gate_read.is_some()}});
    value["window_chrome"]=super::preview_chrome_probe(world).unwrap_or(Value::Null);
    value["project_log_tail"]=world.get_resource::<crate::project_log::ProjectLogState>().map(|log|log.validation_tail()).unwrap_or(Value::Null);
    value["native"]=native(world,plan).map_or_else(|error|json!({"error":error}),|native|native);
    let directory=plan.directory.clone();let stage=name(plan.stage);let sequence=plan.diagnostic_count;
    let(sender,receiver)=async_channel::bounded(1);plan.diagnostic=Some(receiver);
    IoTaskPool::get().spawn(async move{let result=(||{
        let path=directory.join("game").join(format!("{pid}-{revision}-{}.json",if matches!(stage,"playing-original-captions"|"playing-maximized"|"playing-restored"|"playing-minimized-and-restored"|"playing-before-close"){"playing"}else{"editing"}));
        value["capture_metadata"]=match io::read_now(&path){Ok(Some(stamp))=>match serde_json::from_slice::<Value>(&stamp.bytes){Ok(capture)=>json!({
            "path":path,"pid":capture["pid"],"world_id":capture["world_id"],"revision":capture["revision"],"frame":capture["frame"],"mode":capture["mode"],
            "capture":capture["capture"],"window_region":capture["window_region"],"capture_input":capture["capture_input"]}),Err(error)=>json!({"path":path,"error":error.to_string()})},
            Ok(None)=>json!({"path":path,"exists":false}),Err(error)=>json!({"path":path,"error":error})};
        use std::io::Write;
        let mut file=std::fs::OpenOptions::new().create_new(true).write(true).open(directory.join(format!("{stage}-wait-{sequence:02}.json"))).map_err(|error|error.to_string())?;
        file.write_all(&serde_json::to_vec_pretty(&value).map_err(|error|error.to_string())?).and_then(|_|file.sync_all()).map_err(|error|error.to_string())
    })();let _=sender.try_send(result);}).detach();
}
fn embedded_status(world:&World)->Value{
    let status=world.resource::<EmbeddedStatus>();
    json!({"active":status.active,"ready":status.ready,"pid":status.pid,"world_id":status.world_id,"playing":status.playing,
        "applied_revision":status.applied_revision,"frames":status.frames,"ticks":status.ticks,"observed_mode":status.observed_mode,"error":status.error})
}
fn report(plan:&Plan,passed:bool)->Value{json!({"schema":1,"scope":"preview-original-caption-owned-three-level-window","passed":passed,
    "owner_checked":true,"project_root":plan.project,"root_pid":plan.root_pid,"author_pid":std::process::id(),"game_pid":plan.pid,"world_id":plan.world_id,
    "native_input":plan.native_token.is_some(),
    "native_sync_trace_enabled":plan.native_sync_trace,
    "native_sync_diagnostic_files_scheduled":plan.native_sync_count,"native_sync_diagnostic_limit":32,
    "steps":plan.proofs,"captures":plan.captures,"error":plan.failure,"embedded_status":plan.failure_status,
    "project_log_tail":plan.failure_log,"final_exit_requires_process_result":true,
    "input":"official cursor + MouseButtonInput on original buttons; original RequestViewportMode command; owned native game Return click",
    "not_verified":["physical keyboard/mouse feel","native taskbar restore click","all preview gameplay"]})}

fn advance(world:&mut World,plan:&mut Plan)->Result<(),String>{
    plan.frames=plan.frames.saturating_add(1);
    ensure(plan.started.elapsed()<Duration::from_secs(2200)&&plan.changed.elapsed()<Duration::from_secs(if plan.stage<=1{1800}else{120}),"预览三键阶段超时")?;
    if let Some(error)=&world.resource::<EmbeddedStatus>().error{return Err(format!("真实游戏会话失败：{error}"));}
    if plan.mouse==3&&!plan.clicked&&plan.target.is_some_and(|entity|world.get::<feathrise_editor_ui::EditorButtonClick>(entity).is_some_and(|click|click.is_clicked())){plan.clicked=true;}
    match plan.stage{
        0=>{
            if plan.frames<30{return Ok(());}
            if !plan.map_requested{
                world.write_message(RequestViewportMode(ViewportMode::SavedWorld));
                feathrise_editor_map::standalone::request_open_map(world,"M1-SAMPLE")?;
                plan.map_requested=true;return Ok(());
            }
            if !feathrise_editor_map::standalone::preview_ready_for(world,"M1-SAMPLE"){return Ok(());}
            let Some(chrome)=chrome(world,plan,false)?else{return Ok(());};
            if !crate::native_project_close::embedded_validation::idle(world){return Ok(());}
            let probe=serde_json::to_value(author::probe(world)?).map_err(|error|error.to_string())?;
            plan.baseline=Some(probe);plan.baseline_chrome=Some(chrome);
            plan.proofs.push(json!({"stage":name(0),"mode_input":"production RequestViewportMode(EmbeddedWorld); command, no selector click claim"}));
            world.write_message(RequestViewportMode(ViewportMode::EmbeddedWorld));next(world,plan);
        },
        1=>{if game(world,plan,false)?&&checkpoint(world,plan,false)?{next(world,plan);}},
        2|3|4|5|7=>{
            if !plan.clicked{return Ok(());}
            if plan.stage==3&&unsafe{IsZoomed(plan.root as HWND)}==0{return Ok(());}
            if matches!(plan.stage,4|5)&&unsafe{IsZoomed(plan.root as HWND)}!=0{return Ok(());}
            if plan.stage==5{
                if let Some(reply)=&plan.minimize&&let Some(proof)=poll(reply)?{plan.minimize_proof=Some(proof);plan.minimize=None;}
                if plan.minimize_proof.is_none()||unsafe{IsIconic(plan.root as HWND)}!=0{return Ok(());}
            }
            if game(world,plan,true)?&&checkpoint(world,plan,true)?{
                if plan.stage==2&&plan.native_token.is_some(){
                    if !native_gate(world,plan,"native-controls")?{return Ok(());}
                    // 外层已实测原NC三键，不重复合成三键；native sync会按真实鼠标覆盖其Interaction。
                    plan.stage=5;next(world,plan);return Ok(());
                }
                next(world,plan);
            }
        },
        6=>{
            let proof=plan.proofs.iter().rev().find_map(|value|value.get("game").filter(|value|!value.is_null())).ok_or("返回缺少本轮游戏来源")?.clone();
            if !evidence::code_iteration_return_to_edit(world,&proof["window"],plan.pid)?{return Ok(());}
            if game(world,plan,false)?&&checkpoint(world,plan,false)?{next(world,plan);}
        },
        8=>{
            if !native_gate(world,plan,"native-close")?{return Ok(());}
            if (!plan.clicked&&!plan.gate_ack)||crate::native_project_close::embedded_validation::fixture_finish_target(world)?.is_none(){return Ok(());}
            if world.resource::<EmbeddedStatus>().playing{return Ok(());}
            if checkpoint(world,plan,false)?{plan.write=Some(io::report(plan.directory.join("preview-chrome.json"),report(plan,true)));next(world,plan);}
        },
        9=>{if let Some(reply)=&plan.write&&poll(reply)?.is_some(){plan.write=None;plan.report_written=true;}
            // 最后原确认只在真实证据文件完成后放行；父协调器还必须证明root/author/game正常退出。
        },_=>{}
    }Ok(())
}
