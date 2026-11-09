//! 显式自有工程完整边缘专项。原输入前arm桌面观察器，GPU整图对照外围文字/线条与连续过渡。
use feathrise_editor_ui::UiInteraction;
mod resources;
mod types;
// 复用首页已验证的有界读写与ScreenshotCaptured原图保存，避免复制另一套截图成功协议。
#[path = "../workspace_ui/scale_validation/io.rs"]
mod io;
use feathrise_editor_ui::InteractionButton;
use resources::*;
use types::*;
use bevy::{prelude::*,input::{ButtonState,mouse::MouseButtonInput},render::view::screenshot::{Screenshot,ScreenshotCaptured},
    ui::{ComputedStackIndex,ComputedUiTargetCamera,InteractionDisabled},window::{PrimaryWindow,RawHandleWrapper}};
use feathrise_editor_map::workbench::{WorkbenchHeader,WorkbenchHostPageState,WorkbenchSettingsBody};
use feathrise_editor_ui::{EditorDialogBackdrop,EditorPalette};
use serde_json::{Value,json};
use std::{fs::OpenOptions,io::Write,path::PathBuf,time::{Duration,Instant}};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetParent,GetWindowThreadProcessId,IsWindow,IsWindowVisible};

fn ensure(value:bool,message:&str)->Result<(),String>{value.then_some(()).ok_or_else(||message.into())}
fn poll<T>(receiver:&Reply<T>)->Result<Option<T>,String>{match receiver.try_recv(){Ok(result)=>result.map(Some),Err(async_channel::TryRecvError::Empty)=>Ok(None),Err(_)=>Err("边孔验收后台回执丢失".into())}}
pub(super) fn install(app:&mut App){
    let Some(directory)=std::env::var_os("FEATHRISE_FRAME_VALIDATION_DIR").map(PathBuf::from) else{return;};
    setup(app,directory).expect("边孔颜色专项只接受本轮独占夹具");
}
fn setup(app:&mut App,directory:PathBuf)->Result<(),String>{
    ensure(directory.is_absolute()&&io::ordinary(&directory)?.is_dir(),"先由观察器独占创建绝对目录")?;
    let directory=directory.canonicalize().map_err(|e|e.to_string())?;
    let owner=io::read_now(&directory.join("owner.json"))?.ok_or("缺少观察器owner")?;
    let owner:Value=serde_json::from_slice(&owner.bytes).map_err(|e|e.to_string())?;
    ensure(owner["kind"]=="native-frame-validation","owner用途不同")?;
    let token=owner["token"].as_str().filter(|v|!v.is_empty()).ok_or("owner缺少token")?.to_owned();
    let owner_root=PathBuf::from(owner["root"].as_str().ok_or("owner缺少root")?).canonicalize().map_err(|e|e.to_string())?;
    ensure(owner_root==directory,"owner真实路径不符")?;
    let project=PathBuf::from(std::env::var_os("FEATHRISE_FRAME_VALIDATION_PROJECT").ok_or("缺少自有空工程")?).canonicalize().map_err(|e|e.to_string())?;
    ensure(project.parent()==Some(directory.as_path())&&io::ordinary(&project)?.is_dir(),"工程不在本轮直接子目录")?;
    let prefs=PathBuf::from(std::env::var_os("FEATHRISE_PREFERENCES_FILE").ok_or("必须隔离偏好")?);
    ensure(prefs.parent().and_then(|p|p.canonicalize().ok()).as_ref()==Some(&directory),"偏好不在本轮目录")?;
    let global=PathBuf::from(std::env::var_os("FEATHRISE_VALIDATION_GLOBAL_PLUGINS").ok_or("必须隔离global")?).canonicalize().map_err(|e|e.to_string())?;
    ensure(global.starts_with(&directory)&&io::read_now(&global.join(".feathrise-plugin-fixture-owner"))?.is_some_and(|v|v.bytes==b"feathrise-single-binary-validation-v1"),"global归属不符")?;
    for other in ["FEATHRISE_HOME_SCALE_VALIDATION_DIR","FEATHRISE_ABOUT_VALIDATION_DIR","FEATHRISE_CHROME_VALIDATION_DIR","FEATHRISE_WORKSPACE_SMOKE_DIR","FEATHRISE_PLUGIN_HOST_VALIDATION_DIR"]{
        ensure(std::env::var_os(other).is_none(),"不能叠加其他自动驱动")?;
    }
    let embed=crate::workspace_window::EmbedTarget::from_env()?;
    let role=if embed.is_some(){Role::Author}else{Role::Root};
    let args:Vec<_>=std::env::args_os().collect();
    if role==Role::Root{ensure(!args.iter().any(|v|v=="--project"||v=="--native-editor"),"根实例必须从无工程首页启动")?;}
    else{
        let selected=args.iter().position(|v|v=="--project").and_then(|i|args.get(i+1)).ok_or("作者缺少工程参数")?;
        ensure(PathBuf::from(selected).canonicalize().map_err(|e|e.to_string())?==project,"作者不是本轮工程")?;
        let root=io::read_now(&directory.join("home-closed.ready.json"))?.ok_or("作者缺少根实例真实ready")?;
        let root:Value=serde_json::from_slice(&root.bytes).map_err(|e|e.to_string())?;
        let target=embed.unwrap();
        ensure(root["token"]==token&&root["root_pid"]==target.pid&&root["root_hwnd"]==target.parent,"作者父身份不同")?;
    }
    let mut marker=OpenOptions::new().write(true).create_new(true).open(directory.join(format!("{}.owner",role.name()))).map_err(|e|e.to_string())?;
    marker.write_all(std::process::id().to_string().as_bytes()).map_err(|e|e.to_string())?;
    let now=Instant::now();
    app.insert_resource(Plan{directory,project,token,role,stage:0,started:now,stage_started:now,mouse_phase:0,target:None,clicked:false,stable:0,saw_project:false,
        image:None,image_done:false,attempts:0,next_image:now,writing:None,ready_written:false,reading:None,next_read:now,arm_written:false,input_armed:false,input_rect:None,proofs:Vec::new(),captures:Vec::new(),failure:None,
        final_write:None,final_done:false,exit_issued:false}).add_systems(First,input.before(bevy::picking::PickingSystems::Input)).add_systems(Last,drive);
    Ok(())
}
fn below(world:&World,mut entity:Entity,root:Entity)->bool{for _ in 0..24{if entity==root{return true;}let Some(parent)=world.get::<ChildOf>(entity)else{return false;};entity=parent.parent();}false}
fn rect(world:&World,entity:Entity)->Option<Rect>{let node=world.get::<ComputedNode>(entity)?;let at=world.get::<UiGlobalTransform>(entity)?;(!node.is_empty()).then(||Rect::from_center_size(at.translation,node.size()))}
fn find_button(world:&mut World,label:&str)->Option<Entity>{
    if label=="设置"||label=="工程"{
        let parts=world.resource::<super::resources::ChromeParts>().0.as_ref()?;
        let slot=if label=="设置"{parts.settings}else{parts.project?};
        // ChromeParts保存原挂载槽，真正接收按下松开的是槽内Button；工程菜单不依赖变化的工程名。
        return super::scene::descendants(world,slot,3).into_iter().find(|entity|world.get::<InteractionButton>(*entity).is_some()&&world.get::<InheritedVisibility>(*entity).is_some_and(|v|v.get())&&world.get::<InteractionDisabled>(*entity).is_none()&&rect(world,*entity).is_some());
    }
    let frame=world.query_filtered::<Entity,With<WorkbenchSettingsBody>>().iter(world).next().and_then(|body|world.get::<ChildOf>(body).map(|p|p.parent()));
    let texts:Vec<_>=world.query::<(Entity,&Text)>().iter(world).filter_map(|(entity,text)|(text.0==label).then_some(entity)).collect();
    texts.into_iter().find_map(|mut entity|{for _ in 0..24{
        if world.get::<InteractionButton>(entity).is_some(){return (world.get::<InheritedVisibility>(entity).is_some_and(|v|v.get())&&world.get::<InteractionDisabled>(entity).is_none()&&rect(world,entity).is_some()&&(label!="完成"||frame.is_some_and(|frame|below(world,entity,frame)))).then_some(entity);}
        entity=world.get::<ChildOf>(entity)?.parent();
    }None})
}
/// 复现真实窗口后端的双通道投递，保证InputPlugin和现代拾取读取同一份输入。
fn window_input(world: &mut World, input: impl bevy::ecs::message::Message + Clone + Into<bevy::window::WindowEvent>) {
    let event: bevy::window::WindowEvent = input.clone().into();
    world.write_message(input);
    world.write_message(event);
}

fn input(world:&mut World){let Some(mut plan)=world.remove_resource::<Plan>()else{return;};
    if plan.failure.is_none()&&!plan.final_done&&plan.final_write.is_none(){if let Err(error)=issue(world,&mut plan){plan.failure=Some(error);}}
    world.insert_resource(plan);
}
fn issue(world:&mut World,plan:&mut Plan)->Result<(),String>{
    let Some(label)=target(plan.role,plan.stage)else{return Ok(());};if plan.mouse_phase>=3{return Ok(());}
    if checkpoint(plan.role,plan.stage).is_some()&&!plan.input_armed{return Ok(());}
    let Ok(window)=world.query_filtered::<Entity,With<PrimaryWindow>>().single(world)else{return Ok(());};
    if plan.target.is_none(){plan.target=find_button(world,label);}
    let Some(entity)=plan.target else{return Ok(());};let Some(bounds)=rect(world,entity)else{return Ok(());};
    plan.input_rect=Some([bounds.min.x,bounds.min.y,bounds.max.x,bounds.max.y]);
    world.get_mut::<Window>(window).unwrap().set_physical_cursor_position(Some(bounds.center().as_dvec2()));
    let position = bounds.center() / world.get::<Window>(window).unwrap().scale_factor();
    window_input(world, bevy::window::CursorMoved { window, position, delta: None });
    match plan.mouse_phase{0=>plan.mouse_phase=1,1 if world.get::<UiInteraction>(entity)==Some(&UiInteraction::Hovered)=>{window_input(world, MouseButtonInput{button:MouseButton::Left,state:ButtonState::Pressed,window});plan.mouse_phase=2;},2=>{
        ensure(world.get::<UiInteraction>(entity)==Some(&UiInteraction::Pressed),"原按钮未接收官方按下")?;window_input(world, MouseButtonInput{button:MouseButton::Left,state:ButtonState::Released,window});plan.mouse_phase=3;},_=>{}}
    Ok(())
}
fn next(plan:&mut Plan){plan.stage+=1;plan.stage_started=Instant::now();plan.mouse_phase=0;plan.target=None;plan.clicked=false;plan.stable=0;plan.image_done=false;plan.attempts=0;plan.next_image=Instant::now();plan.writing=None;plan.ready_written=false;plan.reading=None;plan.next_read=Instant::now();plan.arm_written=false;plan.input_armed=false;plan.input_rect=None;}
fn arm_input(world:&mut World,plan:&mut Plan,name:&str)->Result<bool,String>{
    if plan.input_armed{return Ok(true);}
    if let Some(receiver)=&plan.writing&&poll(receiver)?.is_some(){plan.arm_written=true;plan.writing=None;}
    if !plan.arm_written&&plan.writing.is_none(){
        let own=world.query_filtered::<&RawHandleWrapper,With<PrimaryWindow>>().single(world).ok().and_then(crate::workspace_window::primary_hwnd);
        let Some(own)=own else{return Ok(false);};
        let button=target(plan.role,plan.stage).and_then(|label|find_button(world,label));
        let Some(bounds)=button.and_then(|entity|rect(world,entity))else{return Ok(false);};
        let embed=crate::workspace_window::EmbedTarget::from_env()?;let(root,pid)=embed.map_or((own,std::process::id()),|v|(v.parent,v.pid));
        let arm=json!({"schema":1,"token":plan.token,"stage":name,"root_hwnd":root,"root_pid":pid,"window_hwnd":own,
            "pid":std::process::id(),"role":plan.role.name(),"button_rect":[bounds.min.x,bounds.min.y,bounds.max.x,bounds.max.y],
            "purpose":"start bounded desktop sampling before original button input"});
        plan.writing=Some(io::report(plan.directory.join(format!("{name}.arm.json")),arm));return Ok(false);
    }
    if !plan.arm_written{return Ok(false);}
    if let Some(receiver)=&plan.reading&&let Some(stamp)=poll(receiver)?{plan.reading=None;plan.next_read=Instant::now()+Duration::from_millis(80);if let Some(stamp)=stamp{
        let ack:Value=serde_json::from_slice(&stamp.bytes).map_err(|e|e.to_string())?;
        ensure(ack["token"]==plan.token&&ack["stage"]==name&&ack["sampling_active"]==true,"输入arm身份不同或桌面采样尚未启动")?;
        plan.input_armed=true;plan.proofs.push(json!({"input_arm":name,"sampler_before_input":ack}));return Ok(true);
    }}
    if plan.reading.is_none()&&Instant::now()>=plan.next_read{plan.reading=Some(io::read(plan.directory.join(format!("{name}.armed.json"))));}Ok(false)
}
fn view(world:&mut World,plan:&Plan,opened:bool)->Result<Option<Value>,String>{
    if world.resource::<WorkbenchHostPageState>().settings_open!=opened{return Ok(None);}
    let model=world.resource::<crate::workspace_ui::WorkspaceModel>();
    ensure(model.project.as_ref().is_none_or(|p|p.root==plan.project),"出现非本轮工程")?;
    if (plan.role==Role::Author)!=model.project.is_some(){return Ok(None);}
    let palette=world.resource::<EditorPalette>();ensure(palette.name()==EditorPalette::neutral_gray().name(),"专项只覆盖中性主题")?;
    let (base,camera)=match world.query_filtered::<(&BackgroundColor,&ComputedUiTargetCamera),With<WorkbenchHeader>>().single(world){Ok((color,camera))=>(color.0,camera.get()),Err(_)=>return Ok(None)};
    let mut layers:Vec<_>=world.query_filtered::<(&BackgroundColor,&ComputedNode,&InheritedVisibility,&ComputedUiTargetCamera,&ComputedStackIndex,Option<&UiInteraction>),With<EditorDialogBackdrop>>().iter(world)
        .filter(|(_,node,visible,target,_,_)|visible.get()&&!node.is_empty()&&camera.is_some()&&target.get()==camera).map(|(color,_,_,_,order,interaction)|(order.0,color.0,interaction.copied())).collect();
    layers.sort_unstable_by_key(|(order,_,_)|*order);if layers.len()!=usize::from(opened){return Ok(None);}
    let glyphs=world.query::<(&Text,&bevy::text::TextLayoutInfo,&InheritedVisibility)>().iter(world).filter(|(_,layout,visible)|visible.get()&&!layout.glyphs.is_empty()).count();
    if glyphs<8{return Ok(None);}
    let palette=world.resource::<EditorPalette>();let mut color=palette.resolve(base).to_linear();let base_rgb=Color::from(color).to_srgba().to_u8_array();
    for (_,layer,_) in &layers{let foreground=palette.resolve(*layer).to_linear();let kept=color.alpha*(1.-foreground.alpha);let alpha=foreground.alpha+kept;if alpha>0.{color.red=(foreground.red*foreground.alpha+color.red*kept)/alpha;color.green=(foreground.green*foreground.alpha+color.green*kept)/alpha;color.blue=(foreground.blue*foreground.alpha+color.blue*kept)/alpha;}color.alpha=alpha;}
    let expected=Color::from(color).to_srgba().to_u8_array();
    let (own,physical)=match world.query_filtered::<(&Window,&RawHandleWrapper),With<PrimaryWindow>>().single(world){Ok((window,raw))=>(crate::workspace_window::primary_hwnd(raw).ok_or("主窗口没有HWND")?,[window.physical_width(),window.physical_height()]),Err(_)=>return Ok(None)};
    if unsafe{IsWindowVisible(own as _)}==0{return Ok(None);}
    let embed=crate::workspace_window::EmbedTarget::from_env()?;let (root,pid)=embed.map_or((own,std::process::id()),|v|(v.parent,v.pid));
    if plan.role==Role::Author{let mut actual=0;unsafe{GetWindowThreadProcessId(root as _,&mut actual);if IsWindow(root as _)==0||actual!=pid||GetParent(own as _)!=root as _{return Ok(None);}}}
    let peripheral_text:Vec<_>=world.query::<(&Text,&bevy::text::TextLayoutInfo,&ComputedNode,&UiGlobalTransform,&InheritedVisibility,&ComputedUiTargetCamera)>().iter(world)
        .filter_map(|(text,layout,node,at,visible,target)|{let bounds=Rect::from_center_size(at.translation,node.size());
            (visible.get()&&!node.is_empty()&&!layout.glyphs.is_empty()&&target.get()==camera&&bounds.max.x>0.&&bounds.max.y>0.&&bounds.min.x<physical[0] as f32&&bounds.min.y<physical[1] as f32&&
                (bounds.min.x<64.||bounds.min.y<64.||bounds.max.x>physical[0] as f32-64.||bounds.max.y>physical[1] as f32-64.))
                .then(||json!({"text":text.0,"glyphs":layout.glyphs.len(),"rect":[bounds.min.x,bounds.min.y,bounds.max.x,bounds.max.y]}))}).collect();
    let footer_ancestry=footer_ancestry(world);
    let encode=|rect:super::types::PixelRect|[rect.left,rect.top,rect.right,rect.bottom];
    let geometry=world.resource::<super::types::ChromeGeometry>();
    Ok(Some(json!({"root_hwnd":root,"root_pid":pid,"window_hwnd":own,"pid":std::process::id(),"role":plan.role.name(),"physical":physical,
        "settings_open":opened,"visible_backdrops":layers.len(),"base_background_rgba":base_rgb,"expected_hole_rgba":expected,"visible_text_nodes_with_glyphs":glyphs,
        "legacy_uniform_color_diagnostic_only":true,"peripheral_text":peripheral_text,"footer_ancestry":footer_ancestry,
        "hit_geometry":{"drag":encode(geometry.drag),"minimize":encode(geometry.minimize),"maximize":encode(geometry.maximize),"close":encode(geometry.close),"visible":geometry.visible},
        "edge_pixel_policy":{"band_px":64,"exclude_outer_px":2,"corner_logical_px":12,"max_channel_error":3,"allowed_mismatch_pixels":0,"feature_delta_from_dominant":8},
        "backdrop_samples":layers.iter().map(|(order,color,interaction)|json!({"order":order,"logical_rgba":color.to_srgba().to_u8_array(),"interaction":interaction.map(|value|format!("{value:?}"))})).collect::<Vec<_>>(),
        "ui_scale":world.resource::<UiScale>().0,"neutral_theme":true})))
}
/// 布局占位不能证明实际可见；记录原底栏到工作台根的裁切和绘制顺序，配合整帧GPU原图取证。
fn footer_ancestry(world:&mut World)->Vec<Value>{
    let Some(mut entity)=world.query_filtered::<Entity,With<feathrise_editor_map::workbench::WorkbenchProjectLogButton>>()
        .iter(world).next()else{return Vec::new();};
    let mut result=Vec::new();
    for _ in 0..12{
        result.push(json!({"entity":format!("{entity:?}"),"text":world.get::<Text>(entity).map(|text|text.as_str()),
            "rect":rect(world,entity).map(|bounds|[bounds.min.x,bounds.min.y,bounds.max.x,bounds.max.y]),
            "display":world.get::<Node>(entity).map(|node|format!("{:?}",node.display)),
            "visible":world.get::<InheritedVisibility>(entity).map(|visible|visible.get()),
            "clip":world.get::<bevy::ui::CalculatedClip>(entity).map(|clip|format!("{clip:?}")),
            "global_z":world.get::<GlobalZIndex>(entity).map(|layer|layer.0),
            "stack_index":world.get::<ComputedStackIndex>(entity).map(|index|index.0),
            "target_camera":world.get::<ComputedUiTargetCamera>(entity).and_then(|camera|camera.get()).map(|camera|format!("{camera:?}")),
            "background":world.get::<BackgroundColor>(entity).map(|color|color.0.to_srgba().to_u8_array())}));
        let Some(parent)=world.get::<ChildOf>(entity)else{break;};entity=parent.parent();
    }
    result
}
fn capture(world:&mut World,plan:&mut Plan,name:&str,view:Value)->Result<(),String>{
    ensure(plan.attempts<16,"GPU像素16次内未就绪")?;plan.attempts+=1;
    let physical=world.query_filtered::<&Window,With<PrimaryWindow>>().single(world).map_err(|e|e.to_string()).map(|w|[w.physical_width(),w.physical_height()])?;
    let path=plan.directory.join(format!("{name}-gpu-{:02}.png",plan.attempts));let(sender,receiver)=async_channel::bounded(1);plan.image=Some(receiver);
    world.spawn(Screenshot::primary_window()).observe(move|event:On<ScreenshotCaptured>|{io::image(event.image.clone(),path.clone(),physical,view.clone(),sender.clone());});Ok(())
}
fn advance(world:&mut World,plan:&mut Plan)->Result<(),String>{
    ensure(plan.started.elapsed()<Duration::from_secs(150)&&plan.stage_started.elapsed()<Duration::from_secs(60),"边孔专项阶段超时")?;
    ensure(world.resource::<crate::workspace_ui::WorkspaceModel>().project.as_ref().is_none_or(|project|project.root==plan.project),"出现非本轮工程，停止验收输入")?;
    if let Some(entity)=plan.target&&plan.mouse_phase==3&&!plan.clicked&&world.get::<feathrise_editor_ui::EditorButtonClick>(entity).is_some_and(|v|v.is_clicked()){
        plan.clicked=true;plan.proofs.push(json!({"stage":plan.stage,"original_button":target(plan.role,plan.stage),"official_press_release_and_click":true}));
    }
    if plan.role==Role::Root&&plan.stage==3{world.write_message(crate::workspace_ui::WorkspaceCommand::Open(plan.project.clone()));plan.proofs.push(json!({"project_open":"production WorkspaceCommand::Open; direct command, no system picker or button claim","project":plan.project}));next(plan);return Ok(());}
    if plan.role==Role::Root&&plan.stage==4{
        let has=world.resource::<crate::workspace_ui::WorkspaceModel>().project.is_some();if has{plan.saw_project=true;return Ok(());}if !plan.saw_project{return Ok(());}
    }
    if plan.role==Role::Author&&plan.stage==3{if plan.clicked&&find_button(world,"关闭工程").is_some(){next(plan);}return Ok(());}
    if plan.role==Role::Author&&plan.stage==4{if plan.clicked{next(plan);}return Ok(());}
    let Some((name,opened))=checkpoint(plan.role,plan.stage)else{return Ok(());};
    if target(plan.role,plan.stage).is_some()&&!arm_input(world,plan,name)?{return Ok(());}
    if target(plan.role,plan.stage).is_some()&&!plan.clicked{return Ok(());}
    let Some(view)=view(world,plan,opened)?else{plan.stable=0;return Ok(());};plan.stable+=1;if plan.stable<10{return Ok(());}
    if let Some(receiver)=&plan.image&&let Some(proof)=poll(receiver)?{plan.image_done=proof["accepted"]==true;plan.captures.push(proof);plan.image=None;plan.next_image=Instant::now()+Duration::from_millis(250);}
    if !plan.image_done{if plan.image.is_none()&&Instant::now()>=plan.next_image{capture(world,plan,name,view)?;}return Ok(());}
    if let Some(receiver)=&plan.writing&&poll(receiver)?.is_some(){plan.ready_written=true;plan.writing=None;}
    if !plan.ready_written&&plan.writing.is_none(){let mut ready=view;ready["schema"]=json!(1);ready["token"]=json!(plan.token);ready["stage"]=json!(name);ready["gpu"]=plan.captures.last().cloned().unwrap();ready["input_target_rect"]=json!(plan.input_rect);plan.writing=Some(io::report(plan.directory.join(format!("{name}.ready.json")),ready));return Ok(());}
    if !plan.ready_written{return Ok(());}
    if let Some(receiver)=&plan.reading&&let Some(stamp)=poll(receiver)?{plan.reading=None;plan.next_read=Instant::now()+Duration::from_millis(80);if let Some(stamp)=stamp{
        let ack:Value=serde_json::from_slice(&stamp.bytes).map_err(|e|e.to_string())?;ensure(ack["token"]==plan.token&&ack["stage"]==name,"桌面ack身份不同")?;
        ensure(ack["captured"]==true&&ack["passed"]==true,"实际桌面颜色/父链检查未通过，原图保留")?;plan.proofs.push(json!({"checkpoint":name,"desktop_ack":ack}));next(plan);return Ok(());}}
    if plan.reading.is_none()&&Instant::now()>=plan.next_read{plan.reading=Some(io::read(plan.directory.join(format!("{name}.ack.json"))));}Ok(())
}
fn drive(world:&mut World){let Some(mut plan)=world.remove_resource::<Plan>()else{return;};
    if plan.failure.is_none()&&plan.stage<5&&plan.final_write.is_none()&&!plan.final_done{if let Err(error)=advance(world,&mut plan){plan.failure=Some(error);}}
    if plan.failure.is_some()&&plan.mouse_phase==2&&let Ok(window)=world.query_filtered::<Entity,With<PrimaryWindow>>().single(world){
        window_input(world, MouseButtonInput{button:MouseButton::Left,state:ButtonState::Released,window});plan.mouse_phase=3;
    }
    if (plan.stage>=5||plan.failure.is_some())&&plan.final_write.is_none()&&!plan.final_done{
        let report=json!({"schema":1,"passed":plan.failure.is_none(),"error":plan.failure,"role":plan.role.name(),"pid":std::process::id(),"project":plan.project,"proofs":plan.proofs,"gpu_captures":plan.captures,
            "input_diagnostics":{"stage":plan.stage,"mouse_phase":plan.mouse_phase,"target":plan.target.map(|entity|format!("{entity:?}")),"target_is_button":plan.target.is_some_and(|entity|world.get::<InteractionButton>(entity).is_some()),"interaction":plan.target.and_then(|entity|world.get::<UiInteraction>(entity)).map(|value|format!("{value:?}"))},
            "scope":"neutral theme/settings only; original button input and real desktop observer acknowledgement","physical_os_button_input_verified":false,"process_exit_observed_externally":true});
        plan.final_write=Some(io::report(plan.directory.join(format!("{}-evidence.json",plan.role.name())),report));
    }
    if let Some(receiver)=&plan.final_write{match poll(receiver){Ok(Some(()))=>{plan.final_done=true;plan.final_write=None;},Err(error)=>{eprintln!("边孔专项报告失败：{error}");plan.final_done=true;plan.final_write=None;plan.failure=Some(error);},_=>{}}}
    if plan.final_done&&world.resource::<crate::workspace_ui::WorkspaceModel>().project.as_ref().is_none_or(|project|project.root==plan.project){
        if !plan.exit_issued{
            if plan.role==Role::Root||plan.failure.is_some(){world.write_message(crate::workspace_ui::WorkspaceCommand::QuitApp);}
            plan.exit_issued=true;
        }
        if world.resource::<crate::native_project_close::NativeProjectCloseState>().active(){use bevy::ecs::system::RunSystemOnce;let _=world.run_system_once(crate::native_project_close::confirm_workspace_validation_close);}
    }
    world.insert_resource(plan);
}
