//! 连续输入的自有窗口验收。按住时等待真实子进程GPU取证，松手/取消只由原作者业务收口。

use bevy::{
    prelude::*,
    tasks::{IoTaskPool, Task, futures::check_ready},
};
use feathrise_editor_map::standalone::embedded_validation::{self as author, live, tool_previews as tools};
use serde_json::{Value, json};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Focus,
    Layer,
    PrepareGpu,
    Hover,
    FirstGrass,
    SecondGrass,
    ReleaseGrass,
    Select,
    FirstMove,
    SecondMove,
    Commit,
    CancelMove,
    Cancel,
    PoseStart,
    PoseFirst,
    PoseSecond,
    PoseCancel,
    PoseRestore,
    Complete,
}

#[derive(Clone)]
enum Expected {
    PrepareGpu,
    Grass {
        after: u64,
        source: u64,
        previous: Option<Value>,
        tools: Value,
    },
    Placement {
        after: u64,
        id: String,
        x: f32,
        edge: &'static str,
        cancelled: bool,
    },
    ObjectPose {
        after: u64,
        active: bool,
        translation: [f32; 3],
    },
}

#[derive(Resource)]
struct Driver {
    phase: Phase,
    started: Instant,
    stage: Instant,
    pulse: Option<Entity>,
    cube: String,
    field: Option<Entity>,
    undo: usize,
    save: u64,
    first: Option<Value>,
    last_sequence: u64,
    proofs: Vec<Value>,
    expected: Option<Expected>,
    reader: Option<Task<Result<Option<Value>, String>>>,
    retry: Instant,
}

impl Driver {
    fn advance(&mut self, phase: Phase) {
        self.phase = phase;
        self.stage = Instant::now();
        self.expected = None;
        self.reader = None;
    }
    fn proof(&mut self, world: &World, name: &str, capture: Value) -> Result<(), String> {
        self.last_sequence = capture["preview_sequence"]
            .as_u64()
            .ok_or("实时截图缺少序号")?;
        self.proofs
            .push(json!({"stage":name,"author":author::probe(world)?,"capture":capture}));
        println!(
            "FEATHRISE_EMBEDDED_LIVE_STEP {}",
            json!({"stage":name,"sequence":self.last_sequence})
        );
        Ok(())
    }
}

pub(super) fn exercise(
    world: &mut World,
    directory: &Path,
    cube: &str,
) -> Result<Option<Value>, String> {
    if !world.contains_resource::<Driver>() {
        let author = author::probe(world)?;
        live::focus_grass_terrain(world)?;
        world.insert_resource(Driver {
            phase: Phase::Focus,
            started: Instant::now(),
            stage: Instant::now(),
            pulse: None,
            cube: cube.into(),
            field: None,
            undo: author.undo_len,
            save: author.save_sequence,
            first: None,
            last_sequence: 0,
            proofs: Vec::new(),
            expected: None,
            reader: None,
            retry: Instant::now(),
        });
    }
    world.resource_scope(|world,mut driver:Mut<Driver>| {
        // 首次真实驱动编译独立限时；它不构成预览成功，也不能吃掉后续按住/拖动的业务预算。
        let stage_budget=if driver.phase==Phase::PrepareGpu {600}else{90};
        if driver.started.elapsed()>Duration::from_secs(840) || driver.stage.elapsed()>Duration::from_secs(stage_budget) {
            return Err(format!("连续预览真实窗口验收超时：{:?}；作者 {:?}；草 {}；数值 {}；整体姿态 {:?}；协议 {}",driver.phase,author::probe(world),live::grass_probe(world)?,live::numeric_input_probe(world)?,live::object_pose::probe(world),super::super::previews::diagnostic(world)));
        }
        if let Some(entity)=driver.pulse.take() { let _=world.despawn(entity); }
        let current=author::probe(world)?;
        if current.save_sequence!=driver.save {return Err("连续预览期间发生隐式保存".into())}
        match driver.phase {
            Phase::Focus if live::grass_terrain_ready(world)=> {
                driver.pulse=Some(live::request_grass_layer(world)?); driver.advance(Phase::Layer);
            }
            Phase::Layer if live::activate_grass(world)?=>driver.advance(Phase::PrepareGpu),
            Phase::PrepareGpu=> {
                driver.expected=Some(Expected::PrepareGpu);
                if let Some(preparation)=poll_capture(world,&mut driver,directory)? {
                    let elapsed=driver.stage.elapsed().as_millis();
                    driver.proofs.push(json!({"stage":"original_compute_pipelines_prepared","elapsed_ms":elapsed,"preparation":preparation}));
                    println!("FEATHRISE_EMBEDDED_LIVE_STEP {}",json!({"stage":"original_compute_pipelines_prepared","elapsed_ms":elapsed}));
                    driver.advance(Phase::Hover);
                }
            }
            Phase::Hover=> {
                let grass=live::grass_probe(world)?;
                if grass["terrain"]=="starter-ground" && !grass["cursor"].is_null()
                    && super::current_snapshot_applied(world,world.resource::<super::EmbeddedStatus>().applied_revision) {
                    driver.undo=current.undo_len; driver.last_sequence=0;
                    live::grass_pointer(world,true,false)?; driver.advance(Phase::FirstGrass);
                }
            }
            Phase::FirstGrass|Phase::SecondGrass=> {
                let grass=live::grass_probe(world)?;
                if current.undo_len!=driver.undo {return Err("按住原笔刷时提前提交了历史".into())}
                if grass["active"]==true && grass["mouse_down"]==true && grass["nonzero"]==true {
                    if driver.expected.is_none() {
                        let published=tools::probe(world)?;
                        let held_color=json!(Color::srgb(1.0,0.78,0.12).to_linear().to_f32_array());
                        // 驱动与原工具发布并非同一系统：先等按住颜色及第二落点真正发布，不能锁住上一帧预期。
                        if !published["preview"]["items"].as_array().is_some_and(|items|items.iter()
                            .any(|item|item["kind"]=="brush"&&item["preview"]["color"]==held_color))
                            || driver.first.as_ref().is_some_and(|first|first["author_tools"]["semantic_sha256"]==published["semantic_sha256"])
                        {return Ok(None)}
                        driver.expected=Some(Expected::Grass{after:driver.last_sequence,
                            source:grass["source_revision"].as_u64().ok_or("按住时没有完整生产密度版本")?,
                            previous:driver.first.clone(),tools:published});
                    }
                    if let Some(capture)=poll_capture(world,&mut driver,directory)? {
                        let first=driver.phase==Phase::FirstGrass;
                        let required_source=match driver.expected.as_ref() {Some(Expected::Grass{source,..})=>*source,_=>return Err("草预览取证目标丢失".into())};
                        driver.proof(world,if first {"grass_first_point_while_held"}else{"grass_second_point_while_held"},capture.clone())?;
                        driver.proofs.last_mut().unwrap()["brush"]=grass;
                        driver.proofs.last_mut().unwrap()["required_source_revision"]=json!(required_source);
                        driver.proofs.last_mut().unwrap()["tools"]=match driver.expected.as_ref() {
                            Some(Expected::Grass{tools,..})=>tools.clone(),_=>return Err("草刷圈取证目标丢失".into())};
                        if first {
                            driver.first=Some(capture); live::grass_pointer(world,true,true)?; driver.advance(Phase::SecondGrass);
                        } else {
                            live::grass_pointer(world,false,true)?; driver.advance(Phase::ReleaseGrass);
                        }
                    }
                }
            }
            Phase::ReleaseGrass=> {
                let grass=live::grass_probe(world)?;
                if grass["active"]==false && current.undo_len==driver.undo+1
                    && super::current_snapshot_applied(world,world.resource::<super::EmbeddedStatus>().applied_revision) {
                    driver.proofs.push(json!({"stage":"grass_release_one_history","author":current,"brush":grass,"history_delta":1}));
                    let cube=driver.cube.clone(); live::select_numeric_placement(world,&cube)?;
                    driver.undo=current.undo_len; driver.last_sequence=0; driver.first=None; driver.advance(Phase::Select);
                }
            }
            Phase::Select=> {
                if let Some(field)=live::numeric_field(world) {
                    driver.field=Some(field); live::numeric_change(world,field,8.)?; driver.advance(Phase::FirstMove);
                }
            }
            Phase::FirstMove|Phase::SecondMove|Phase::CancelMove=> {
                if current.undo_len!=driver.undo {return Err("原数值拖动还在继续时提前提交了历史".into())}
                let input=live::numeric_input_probe(world)?;
                if !live::numeric_active(world) {return Ok(None)}
                let x=match driver.phase {Phase::FirstMove=>8.,Phase::SecondMove=>9.,_=>10.};
                // 游戏画面之外也必须证明原共享控件捕获了真正的鼠标手势，排队消息不能冒充已执行。
                if input["captured"]!=true || input["mouse_down"]!=true || input["queued_actions"]!=0
                    || input["widget_value"].as_f64().is_none_or(|value|(value-f64::from(x)).abs()>0.002) {return Ok(None)}
                if driver.expected.is_none() {driver.expected=Some(Expected::Placement{after:driver.last_sequence,id:driver.cube.clone(),x,edge:"update",cancelled:false});}
                if let Some(capture)=poll_capture(world,&mut driver,directory)? {
                    let phase=driver.phase;
                    driver.proof(world,match phase {Phase::FirstMove=>"numeric_first_value_while_dragging",Phase::SecondMove=>"numeric_second_value_while_dragging",_=>"numeric_cancel_value_while_dragging"},capture)?;
                    driver.proofs.last_mut().unwrap()["input"]=input;
                    let field=driver.field.unwrap();
                    match phase {
                        Phase::FirstMove=>{live::numeric_change(world,field,9.)?;driver.advance(Phase::SecondMove);}
                        Phase::SecondMove=>{live::numeric_finish(world,field,false)?;driver.advance(Phase::Commit);}
                        _=>{live::numeric_finish(world,field,true)?;driver.advance(Phase::Cancel);}
                    }
                }
            }
            Phase::Commit=> {
                let input=live::numeric_input_probe(world)?;
                if live::numeric_idle(world) && !live::numeric_active(world) && current.undo_len==driver.undo+1 && position(&current,&driver.cube)==Some(9.)
                    && super::current_snapshot_applied(world,world.resource::<super::EmbeddedStatus>().applied_revision) {
                    // 第二笔也等待原提交后的值、文字和布局同步，不能复用第一笔之前的起点。
                    let Some(field)=live::numeric_field(world) else {return Ok(None)};
                    driver.proofs.push(json!({"stage":"numeric_release_one_history","author":current,"history_delta":1,"input":input}));
                    driver.undo=current.undo_len; driver.field=Some(field);
                    live::numeric_change(world,field,10.)?;driver.advance(Phase::CancelMove);
                }
            }
            Phase::Cancel=> {
                if current.undo_len!=driver.undo {return Err("原数值取消错误地产生历史".into())}
                let input=live::numeric_input_probe(world)?;
                if live::numeric_idle(world) && !live::numeric_active(world) && position(&current,&driver.cube)==Some(9.) {
                    if driver.expected.is_none() {driver.expected=Some(Expected::Placement{after:driver.last_sequence,id:driver.cube.clone(),x:9.,edge:"end",cancelled:true});}
                    if let Some(capture)=poll_capture(world,&mut driver,directory)? {
                        driver.proof(world,"numeric_cancel_restored_without_history",capture)?;
                        driver.proofs.last_mut().unwrap()["input"]=input;
                        live::object_pose::start(world)?;
                        driver.last_sequence=0;
                        driver.advance(Phase::PoseStart);
                    }
                }
            }
            Phase::PoseStart=> {
                if live::object_pose::probe(world)?["ready_to_move"]==true {
                    live::object_pose::move_pointer(world,false)?;
                    driver.advance(Phase::PoseFirst);
                }
            }
            Phase::PoseFirst|Phase::PoseSecond|Phase::PoseCancel=> {
                let input=live::object_pose::probe(world)?;
                let active=driver.phase!=Phase::PoseCancel;
                if current.undo_len!=driver.undo {return Err("整体地形拖动/取消错误地登记了历史".into())}
                if (active && (input["active"]!=true || input["mouse_down"]!=true))
                    || (!active && input["idle"]!=true) {return Ok(None)}
                let translation=translation(&input["expected_translation"])?;
                if driver.expected.is_none() {
                    driver.expected=Some(Expected::ObjectPose{after:driver.last_sequence,active,translation});
                }
                if let Some(capture)=poll_capture(world,&mut driver,directory)? {
                    let phase=driver.phase;
                    driver.proof(world,match phase {Phase::PoseFirst=>"terrain_first_pose_while_held",Phase::PoseSecond=>"terrain_second_pose_while_held",_=>"terrain_cancel_restored_without_history"},capture)?;
                    driver.proofs.last_mut().unwrap()["input"]=input;
                    match phase {
                        Phase::PoseFirst=>{live::object_pose::move_pointer(world,true)?;driver.advance(Phase::PoseSecond);}
                        Phase::PoseSecond=>{live::object_pose::cancel(world)?;driver.advance(Phase::PoseCancel);}
                        _=>driver.advance(Phase::PoseRestore),
                    }
                }
            }
            Phase::PoseRestore=> {
                if live::object_pose::restore(world)? {driver.advance(Phase::Complete);}
            }
            Phase::Complete=> {
                if super::current_snapshot_applied(world,world.resource::<super::EmbeddedStatus>().applied_revision) {
                    let Some(view_controls)=super::view_controls::exercise(world,directory)? else {return Ok(None)};
                    return Ok(Some(json!({"passed":true,"input":"grass: original RelativeCursorPosition+MouseLeft; numeric: WindowEvent + CursorMoved/MouseButtonInput/KeyboardInput through original PointerInputPlugin and numeric widget",
                        "physical_pointer_or_escape_not_verified":true,"numeric_escape_input_path_verified":true,"terrain_input":"original scene-row selection + MoveCamera + RelativeCursorPosition + MouseLeft + Escape","proofs":driver.proofs,"view_controls":view_controls})));
                }
            }
            _=>{}
        }
        Ok(None)
    })
}

fn position(author: &author::AuthorProbe, id: &str) -> Option<f32> {
    author
        .placements
        .iter()
        .find(|placement| placement.id == id)
        .map(|placement| placement.translation[0])
}

fn translation(value: &Value) -> Result<[f32; 3], String> {
    let values = value
        .as_array()
        .filter(|values| values.len() == 3)
        .ok_or("整体姿态取证缺少三维位置")?;
    let mut result = [0.; 3];
    for (target, value) in result.iter_mut().zip(values) {
        *target = value
            .as_f64()
            .filter(|value| value.is_finite())
            .ok_or("整体姿态位置不是有限数值")? as f32;
        if !target.is_finite() {
            return Err("整体姿态位置超出有限范围".into());
        }
    }
    Ok(result)
}

fn poll_capture(
    world: &World,
    driver: &mut Driver,
    directory: &Path,
) -> Result<Option<Value>, String> {
    if let Some(task) = driver.reader.as_mut() {
        if let Some(result) = check_ready(task) {
            driver.reader = None;
            driver.retry = Instant::now() + Duration::from_millis(100);
            return result;
        }
        return Ok(None);
    }
    if Instant::now() < driver.retry {
        return Ok(None);
    }
    let status = world.resource::<super::EmbeddedStatus>();
    let pid = status.pid;
    let world_id = status.world_id;
    let generation = world
        .non_send::<super::super::resources::Runtime>()
        .identity
        .as_ref()
        .ok_or("预览验收缺少当前协议身份")?
        .generation;
    let directory = directory.join("game");
    let expected = driver.expected.clone().ok_or("缺少预览取证目标")?;
    driver.reader = Some(
        IoTaskPool::get()
            .spawn(async move { read_capture(&directory, pid, world_id, generation, &expected) }),
    );
    Ok(None)
}

fn read_capture(
    directory: &Path,
    pid: u32,
    world_id: u64,
    generation: u64,
    expected: &Expected,
) -> Result<Option<Value>, String> {
    if matches!(expected, Expected::PrepareGpu) {
        let path = directory.join(format!("{pid}-preview-wait.json"));
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        let mut bytes = Vec::new();
        file.take(128 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() > 128 * 1024 {
            return Err("首次计算管线准备记录超预算".into());
        }
        let report: Value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        if report["source"] != "real-project-preview-wait"
            || report["not_capture"] != true
            || report["pid"] != pid
            || report["world_id"] != world_id
            || report["generation"] != generation
        {
            return Err("首次计算管线准备身份不符".into());
        }
        if report["gates"]["failed"] == true
            || report["render"]["compute_pipeline_error_stages"]
                .as_u64()
                .unwrap_or(0)
                > 0
        {
            return Err("真实草计算管线准备失败".into());
        }
        return Ok(compute_prepared(&report).then_some(report));
    }
    let (kind, after) = match expected {
        Expected::Grass { after, .. } => ("vegetation", *after),
        Expected::Placement { after, .. } => ("placement", *after),
        Expected::ObjectPose { after, .. } => ("author-domain", *after),
        Expected::PrepareGpu => unreachable!(),
    };
    let prefix = format!("{pid}-preview-{kind}-");
    let mut paths: Vec<(u64, PathBuf)> = Vec::new();
    let mut count = 0;
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        count += 1;
        if count > 2048 {
            return Err("有限预览夹具证据目录超预算".into());
        }
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(sequence) = name
            .strip_prefix(&prefix)
            .and_then(|name| name.strip_suffix(".json"))
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|sequence| *sequence > after)
        {
            paths.push((sequence, entry.path()));
        }
    }
    paths.sort_unstable_by_key(|(sequence, _)| std::cmp::Reverse(*sequence));
    for (sequence, path) in paths.into_iter().take(64) {
        let mut bytes = Vec::new();
        fs::File::open(&path)
            .map_err(|error| error.to_string())?
            .take(128 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() > 128 * 1024 {
            return Err("实时预览证据超过128KiB预算".into());
        }
        let capture: Value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        if capture["pid"] != pid
            || capture["world_id"] != world_id
            || capture["kind"] != kind
            || capture["preview_sequence"] != sequence
            || capture["source"] != "real-project-preview"
            || capture["mode"] != "editing"
        {
            return Err("连续预览截图身份不符".into());
        }
        if capture["map_generation"] != generation {
            continue;
        }
        if !matches_expected(&capture, expected)? {
            continue;
        }
        if capture["capture"]["source"] != "Bevy ScreenshotCaptured"
            || capture["capture"]["sha256"]
                .as_str()
                .is_none_or(|hash| hash.len() != 64)
        {
            return Err("预览缺少实际GPU像素回读".into());
        }
        let image = path.with_extension("png");
        let mut signature = [0; 8];
        fs::File::open(&image)
            .and_then(|mut file| file.read_exact(&mut signature))
            .map_err(|error| error.to_string())?;
        if signature != *b"\x89PNG\r\n\x1a\n" {
            return Err("实时预览截图不是PNG".into());
        }
        if matches!(expected, Expected::Grass {..}) { super::tool_previews::verify_pixels(&capture,&image)?; }
        return Ok(Some(capture));
    }
    Ok(None)
}

fn matches_expected(capture: &Value, expected: &Expected) -> Result<bool, String> {
    match expected {
        Expected::PrepareGpu => Ok(false), // 准备状态绝不作为真实像素或手势证据。
        Expected::ObjectPose {
            active,
            translation,
            ..
        } => {
            // 采用回执不能替代原实例消费者：同一稳定地形根的局部/世界矩阵必须实际到位。
            let pose = &capture["content"]["object_pose"];
            let matches = |key: &str| {
                pose["actual"][key].as_array().is_some_and(|matrix| {
                    matrix.len() == 16
                        && (0..3).all(|axis| {
                            matrix[12 + axis].as_f64().is_some_and(|actual| {
                                (actual - f64::from(translation[axis])).abs() < 0.002
                            })
                        })
                })
            };
            Ok(pose["last"][0] == "M1-SAMPLE"
                && pose["last"][2] == *active
                && pose["last"][3] == "terrain:starter-ground"
                && pose["actual"]["entity"].as_u64().is_some()
                && matches("local")
                && matches("world"))
        }
        Expected::Placement {
            id,
            x,
            edge,
            cancelled,
            ..
        } => Ok(super::tool_previews::brush_cleared(capture)
            && capture["content"]["edge"] == *edge
            && capture["content"]["cancelled"] == *cancelled
            && capture["content"]["placements"]
                .as_array()
                .is_some_and(|placements| {
                    placements.iter().any(|placement| {
                        placement["id"] == *id
                            && placement["active"] == true
                            && placement["local"][12]
                                .as_f64()
                                .is_some_and(|value| (value - f64::from(*x)).abs() < 0.002)
                            && placement["world"][12]
                                .as_f64()
                                .is_some_and(|value| (value - f64::from(*x)).abs() < 0.002)
                    })
                })),
        Expected::Grass {
            source, previous, tools, ..
        } => {
            if !super::tool_previews::matches_brush(capture, tools) { return Ok(false); }
            let gpu = &capture["gpu"];
            if gpu["sample_sequence"].as_u64().unwrap_or(0)
                <= gpu["sample_baseline"].as_u64().unwrap_or(u64::MAX)
                || gpu["rendered_instances"].as_u64().unwrap_or(0) == 0
                || gpu["drawn_draws"].as_u64().unwrap_or(0) == 0
                || gpu["density_waiting_windows"] != 0
                || gpu["map_errors"] != 0
            {
                return Ok(false);
            }
            let Some(canvas) = capture["content"]["canvases"]
                .as_array()
                .and_then(|canvases| {
                    canvases
                        .iter()
                        .find(|canvas| canvas["terrain_id"] == "starter-ground")
                })
            else {
                return Ok(false);
            };
            if canvas["source_revision"].as_u64().unwrap_or(0) < *source
                || canvas["source_revision"] != canvas["adopted_source_revision"]
                || canvas["draft_revision"] != gpu["source_revision"]
            {
                return Ok(false);
            }
            let Some(density) = canvas["densities"].as_array().and_then(|values| {
                values
                    .iter()
                    .find(|density| density["id"] == "embedded-live-blade")
            }) else {
                return Ok(false);
            };
            if density["nonzero"].as_u64().unwrap_or(0) == 0 {
                return Ok(false);
            }
            if let Some(previous) = previous {
                let earlier = &previous["author_tools"];
                let now = &capture["author_tools"];
                // 位移必须反映到实际材质或真实Gizmo，不能只由密度变化或协议消息证明。
                if earlier["clipmap_render"]["visible"] == true {
                    if earlier["clipmap_render"]["brush"] == now["clipmap_render"]["brush"] { return Ok(false); }
                } else if earlier["geometry_sha256"] == now["geometry_sha256"] { return Ok(false); }
                let previous_canvas = previous["content"]["canvases"]
                    .as_array()
                    .and_then(|values| {
                        values
                            .iter()
                            .find(|value| value["terrain_id"] == "starter-ground")
                    })
                    .ok_or("首点截图缺少地形")?;
                let previous_density = previous_canvas["densities"]
                    .as_array()
                    .and_then(|values| {
                        values
                            .iter()
                            .find(|value| value["id"] == "embedded-live-blade")
                    })
                    .ok_or("首点截图缺少密度")?;
                if previous_density["sha256"] == density["sha256"]
                    || previous["capture"]["sha256"] == capture["capture"]["sha256"]
                {
                    return Ok(false);
                }
            }
            Ok(true)
        }
    }
}

fn compute_prepared(report: &Value) -> bool {
    let Some(stages) = report["compute_stages"].as_array() else {
        return false;
    };
    stages.len() == 4
        && [
            "cull_tiles",
            "finalize_tiles",
            "classify_and_compact",
            "finalize_indirect",
        ]
        .into_iter()
        .all(|name| {
            stages
                .iter()
                .filter(|stage| {
                    stage["stage"] == name && stage["state"] == "ready" && stage["code"] == 3
                })
                .count()
                == 1
        })
}

/// 协调器在进程结束后重新读取独立截图回执，并复核手势边界与历史；不能只相信passed字段。
pub(super) fn verify(directory: &Path, report: &Value, cube: &str) -> Result<(), String> {
    let proof = &report["continuous_previews"];
    super::view_controls::verify(directory, &proof["view_controls"])?;
    let entries = proof["proofs"].as_array().ok_or("连续预览缺少原输入阶段")?;
    let find = |name: &str| {
        entries
            .iter()
            .find(|entry| entry["stage"] == name)
            .ok_or_else(|| format!("缺少连续预览阶段{name}"))
    };
    let first = find("grass_first_point_while_held")?;
    let second = find("grass_second_point_while_held")?;
    for entry in [first, second] {
        if entry["brush"]["active"] != true || entry["brush"]["mouse_down"] != true {
            return Err("草截图发生在松手之后".into());
        }
    }
    let undo = first["author"]["undo_len"]
        .as_u64()
        .ok_or("原笔刷缺少历史基线")?;
    if second["author"]["undo_len"] != undo
        || find("grass_release_one_history")?["author"]["undo_len"] != undo + 1
    {
        return Err("草笔划没有只在松手形成一条历史".into());
    }
    for name in [
        "numeric_first_value_while_dragging",
        "numeric_second_value_while_dragging",
    ] {
        if find(name)?["author"]["undo_len"] != undo + 1 {
            return Err("连续数值拖动提前提交历史".into());
        }
    }
    for name in [
        "numeric_release_one_history",
        "numeric_cancel_value_while_dragging",
        "numeric_cancel_restored_without_history",
    ] {
        if find(name)?["author"]["undo_len"] != undo + 2 {
            return Err("数值提交/取消历史数量不符".into());
        }
    }
    verify_numeric_inputs(entries)?;
    let terrain_first = find("terrain_first_pose_while_held")?;
    let terrain_second = find("terrain_second_pose_while_held")?;
    let terrain_cancel = find("terrain_cancel_restored_without_history")?;
    let entity = &terrain_first["capture"]["content"]["object_pose"]["actual"]["entity"];
    for entry in [terrain_first, terrain_second, terrain_cancel] {
        if entry["author"]["undo_len"] != undo + 2
            || entry["input"]["author_pose"] != terrain_first["input"]["author_pose"]
            || entry["capture"]["content"]["object_pose"]["actual"]["entity"] != *entity
        {
            return Err("整体地形预览更换了实际根、污染来源或增加了历史".into());
        }
    }
    if [terrain_first, terrain_second]
        .into_iter()
        .any(|entry| entry["input"]["active"] != true || entry["input"]["mouse_down"] != true)
        || terrain_cancel["input"]["idle"] != true
        || terrain_first["input"]["expected_translation"]
            == terrain_second["input"]["expected_translation"]
        || translation(&terrain_cancel["input"]["expected_translation"])?
            != translation(&terrain_first["input"]["author_pose"]["translation"])?
    {
        return Err("整体地形缺少按住两处预览及Esc恢复原姿态证据".into());
    }
    for entry in entries.iter().filter(|entry| !entry["capture"].is_null()) {
        let capture = &entry["capture"];
        let stage = entry["stage"].as_str().unwrap_or_default();
        let expected = match stage {
            "terrain_first_pose_while_held"
            | "terrain_second_pose_while_held"
            | "terrain_cancel_restored_without_history" => Expected::ObjectPose {
                after: 0,
                active: stage != "terrain_cancel_restored_without_history",
                translation: translation(&entry["input"]["expected_translation"])?,
            },
            "grass_first_point_while_held" | "grass_second_point_while_held" => Expected::Grass {
                after: 0,
                source: entry["required_source_revision"]
                    .as_u64()
                    .ok_or("草截图缺少作者生产版本")?,
                previous: (stage == "grass_second_point_while_held")
                    .then(|| first["capture"].clone()),
                tools: entry["tools"].clone(),
            },
            _ => Expected::Placement {
                after: 0,
                id: cube.into(),
                x: match stage {
                    "numeric_first_value_while_dragging" => 8.,
                    "numeric_cancel_value_while_dragging" => 10.,
                    _ => 9.,
                },
                edge: if stage == "numeric_cancel_restored_without_history" {
                    "end"
                } else {
                    "update"
                },
                cancelled: stage == "numeric_cancel_restored_without_history",
            },
        };
        if !matches_expected(capture, &expected)? {
            return Err(format!("{stage}实际运行实例来源/GPU/矩阵未通过复核"));
        }
        let pid = capture["pid"].as_u64().ok_or("实时截图没有PID")?;
        let kind = capture["kind"].as_str().ok_or("实时截图没有类别")?;
        let sequence = capture["preview_sequence"]
            .as_u64()
            .ok_or("实时截图没有序号")?;
        if !matches!(kind, "placement" | "vegetation" | "author-domain") {
            return Err("未知实时截图类别".into());
        }
        let base = format!("{pid}-preview-{kind}-{sequence}");
        let stored: Value = serde_json::from_slice(
            &fs::read(directory.join(format!("{base}.json"))).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        if stored != *capture {
            return Err(format!("{stage}落盘的真实运行实例回执与持有期间取证不一致"));
        }
        let mut signature = [0; 8];
        fs::File::open(directory.join(format!("{base}.png")))
            .and_then(|mut file| file.read_exact(&mut signature))
            .map_err(|error| error.to_string())?;
        if signature != *b"\x89PNG\r\n\x1a\n" {
            return Err("连续预览真实PNG缺失".into());
        }
        if matches!(expected, Expected::Grass {..}) {
            super::tool_previews::verify_pixels(capture,&directory.join(format!("{base}.png")))?;
        }
    }
    Ok(())
}

/// 公共控件捕获、真实 PointerInput 边沿与 Escape 原始消息缺一不可；领域结果单独正确不代表输入链通过。
fn verify_numeric_inputs(entries: &[Value]) -> Result<(), String> {
    for (stage, gesture, value, held) in [
        ("numeric_first_value_while_dragging", 1, 8., true),
        ("numeric_second_value_while_dragging", 1, 9., true),
        ("numeric_release_one_history", 1, 9., false),
        ("numeric_cancel_value_while_dragging", 2, 10., true),
        ("numeric_cancel_restored_without_history", 2, 9., false),
    ] {
        let input = &entries
            .iter()
            .find(|entry| entry["stage"] == stage)
            .ok_or("数值原始输入缺少阶段")?["input"];
        let releases = if held { gesture - 1 } else { gesture };
        if input["source"] != "WindowEvent and independent Bevy input messages"
            || input["physical_input"] != false
            || input["gesture"] != gesture
            || input["pointer_press"] != gesture
            || input["pointer_release"] != releases
            || input["captured"] != held
            || input["mouse_down"] != held
            || input["idle"] != !held
            || input["queued_actions"] != 0
            || input["escape_down"] != false
            || input["widget_value"]
                .as_f64()
                .is_none_or(|actual| (actual - value).abs() > 0.002)
        {
            return Err(format!("{stage}未通过公共数值控件与窗口原始输入复核"));
        }
        if stage == "numeric_cancel_restored_without_history"
            && (input["escape_press"] != 1 || input["escape_release"] != 1)
        {
            return Err("数值取消没有实际 Escape 按下和释放消息".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numeric_evidence_rejects_domain_only_success_without_public_capture_and_escape() {
        let mut entries:Vec<Value>=[
            ("numeric_first_value_while_dragging",1,8.,true),("numeric_second_value_while_dragging",1,9.,true),
            ("numeric_release_one_history",1,9.,false),("numeric_cancel_value_while_dragging",2,10.,true),
            ("numeric_cancel_restored_without_history",2,9.,false),
        ].into_iter().map(|(stage,gesture,value,held)|json!({"stage":stage,"input":{
            "source":"WindowEvent and independent Bevy input messages","physical_input":false,
            "gesture":gesture,"pointer_press":gesture,"pointer_release":if held{gesture-1}else{gesture},
            "captured":held,"mouse_down":held,"idle":!held,"queued_actions":0,"escape_down":false,
            "widget_value":value,"escape_press":1,"escape_release":1}})).collect();
        assert!(verify_numeric_inputs(&entries).is_ok());
        entries[0]["input"]["captured"] = json!(false);
        assert!(verify_numeric_inputs(&entries).is_err());
        entries[0]["input"]["captured"] = json!(true);
        entries[4]["input"]["escape_press"] = json!(0);
        assert!(verify_numeric_inputs(&entries).is_err());
    }
    #[test]
    fn first_pipeline_preparation_requires_all_actual_stages_and_is_not_pixel_evidence() {
        let mut report = json!({"compute_stages":[
            {"stage":"cull_tiles","state":"ready","code":3},{"stage":"finalize_tiles","state":"ready","code":3},
            {"stage":"classify_and_compact","state":"creating","code":2},{"stage":"finalize_indirect","state":"ready","code":3}]});
        assert!(!compute_prepared(&report));
        report["compute_stages"][2]["state"] = json!("ready");
        report["compute_stages"][2]["code"] = json!(3);
        assert!(compute_prepared(&report));
        assert!(!matches_expected(&report, &Expected::PrepareGpu).unwrap());
        report["compute_stages"][2]["stage"] = json!("cull_tiles");
        assert!(!compute_prepared(&report));
    }
    #[test]
    fn preview_acks_or_old_gpu_samples_cannot_prove_held_blade_render() {
        let expected = Expected::Grass {
            after: 0,
            source: 7,
            previous: None,
            tools: json!({"semantic_sha256":"brush","preview":{"items":[{"kind":"brush","preview":{"clipmap":null}}]}}),
        };
        let mut capture = json!({"gpu":{"source_revision":3,"sample_sequence":12,"sample_baseline":10,"rendered_instances":9,"drawn_draws":1,"density_waiting_windows":0,"map_errors":0},
            "author_tools":{"source":"actual-project-GizmoAssets-after-GizmoMeshSystems","error":null,
                "mode":"Editing","pending":false,"visible":true,"brushes":1,"semantic_sha256":"brush","visible_segments":114},
            "content":{"canvases":[{"terrain_id":"starter-ground","source_revision":7,"adopted_source_revision":7,"draft_revision":3,"densities":[{"id":"embedded-live-blade","nonzero":30,"sha256":"new"}]}]}});
        assert!(matches_expected(&capture, &expected).unwrap());
        capture["gpu"]["sample_sequence"] = json!(10);
        assert!(!matches_expected(&capture, &expected).unwrap());
        capture["gpu"]["sample_sequence"] = json!(12);
        capture["gpu"]["source_revision"] = json!(2);
        assert!(!matches_expected(&capture, &expected).unwrap());
        capture["gpu"]["source_revision"] = json!(3);
        capture["gpu"]["rendered_instances"] = json!(0);
        assert!(!matches_expected(&capture, &expected).unwrap());
    }
}
