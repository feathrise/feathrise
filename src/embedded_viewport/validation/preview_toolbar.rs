//! 工程预览三个真实按钮的闭环验收：原生点击、主相机/碰撞配置、独立GPU像素及原状态恢复。
//! 只读取本轮夹具；文件和PNG工作放入唯一后台槽，主线程不扫描目录或等待GPU。

use super::resources::{Plan, PreviewToolbarDriver as Driver};
use bevy::{
    asset::RenderAssetUsages,
    image::{CompressedImageFormats, ImageSampler, ImageType},
    prelude::*,
    tasks::{IoTaskPool, futures::check_ready},
};
use feathrise_editor_map::standalone::embedded_validation as author;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::Path, time::{Duration, Instant}};

const SOURCE: &str = "owned HWND mouse -> production preview toolbar -> actual camera/config -> GPU PNG";
pub(super) const RUNTIME_SOURCE: &str = "production-PrimarySceneView-MeshTriangles-and-PhysicsGizmos";
const STAGES: [(&str, bool, bool); 5] = [
    ("initial", false, false),
    ("grid_on", true, false),
    ("grid_collision_on", true, true),
    ("collision_on_grid_off", false, true),
    ("restored", false, false),
];
const CLICKS: [&str; 4] = ["显示网格", "显示碰撞网格", "显示网格", "显示碰撞网格"];

pub(super) fn exercise(world: &mut World, plan: &mut Plan) -> Result<Option<Value>, String> {
    if !world.contains_resource::<Driver>() {
        let baseline = author::probe(world)?;
        if baseline.placements.len() > 64 { return Err("预览工具条验收作者夹具超出64个实例预算".into()); }
        let status = world.resource::<super::EmbeddedStatus>();
        let session = (status.pid, status.world_id, status.applied_revision);
        world.insert_resource(Driver {
            author: serde_json::to_value(baseline).map_err(|error| error.to_string())?,
            session,
            camera: None, step: 0, started: Instant::now(), minimum_frame: 0,
            retry: Instant::now(), task: None, proofs: Vec::new(),
        });
    }
    world.resource_scope(|world, mut driver: Mut<Driver>| {
        if driver.started.elapsed() > Duration::from_secs(90) {
            return Err(format!("预览工具条真实点击/截图验收超时：{}", STAGES[driver.step.min(4)].0));
        }
        let current_author = serde_json::to_value(author::probe(world)?).map_err(|error| error.to_string())?;
        if current_author != driver.author { return Err("预览显示按钮改变了作者草稿、历史或保存身份".into()); }
        let status = world.resource::<super::EmbeddedStatus>();
        if (status.pid, status.world_id, status.applied_revision) != driver.session || !status.playing {
            return Err("预览显示按钮改变了实际PID、World、修订或预览模式".into());
        }
        // 上一个按下沿必须先被原游戏UI采样并松开，不能把多次点击压成同一次输入。
        if plan.game_mouse_release.is_some() { return Ok(None); }
        let geometry = world.resource::<crate::viewport_mode::ViewportGeometry>();
        // 本轮操作仍须真实前台许可；失焦只等待，不点击被生产规则禁用的子窗口。
        if !geometry.focused { return Ok(None); }
        if geometry.excluded_rects.len() > feathrise_editor_protocol::MAX_VIEWPORT_EXCLUDED_RECTS
            || geometry.excluded_shapes.len() > feathrise_editor_protocol::MAX_VIEWPORT_EXCLUDED_SHAPES {
            return Err("预览工具条窗口几何超过正式协议预算".into());
        }
        let proof = if let Some(task) = driver.task.as_mut() {
            if let Some(result) = check_ready(task) {
                driver.task = None;
                driver.retry = Instant::now() + Duration::from_millis(100);
                result?
            } else { None }
        } else {
            if Instant::now() >= driver.retry {
                let directory = plan.directory.join("game");
                let session = driver.session;
                let expected = STAGES[driver.step];
                let minimum_frame = driver.minimum_frame;
                let camera = driver.camera;
                let author = driver.author.clone();
                let geometry = geometry.clone();
                driver.task = Some(IoTaskPool::get().spawn(async move {
                    read(&directory, session, expected, minimum_frame, camera, &author, &geometry)
                }));
            }
            None
        };
        let Some(mut proof) = proof else { return Ok(None); };
        if proof["host_window_region"]["host_expected"]["revision"] != geometry.revision {
            return Ok(None); // 后台读跨过布局修订，重新独立核验当前窗口后才允许点击。
        }
        let capture = &proof["capture"];
        let frame = capture["frame"].as_u64().ok_or("预览截图缺少实际帧")?;
        let camera = capture["preview_toolbar"]["runtime"]["primary_camera"].as_u64().ok_or("预览截图缺少主相机")?;
        if let Some(previous) = driver.proofs.last()
            && previous["scene_rgba_sha256"] == proof["scene_rgba_sha256"] {
            return Err("预览显示切换后，排除工具栏的实际GPU场景像素未变化".into());
        }
        if let Some(previous) = driver.proofs.last()
            && previous["capture"]["preview_toolbar"]["physical_rect"] != capture["preview_toolbar"]["physical_rect"] {
            return Err("显示开关取证期间HUD位置改变，不能把遮罩变化当成模型显示变化".into());
        }
        proof["stage"] = json!(STAGES[driver.step].0);
        proof["trigger_button"] = if driver.step == 0 { Value::Null } else { json!(CLICKS[driver.step - 1]) };
        driver.camera = Some(camera);
        driver.minimum_frame = frame.saturating_add(1);
        plan.region_evidence = Some(proof["host_window_region"].clone());
        println!("FEATHRISE_EMBEDDED_LIVE_STEP {}", json!({"stage":format!("preview_toolbar_{}", STAGES[driver.step].0),
            "source":SOURCE,"frame":frame}));
        driver.proofs.push(proof);
        if driver.step == STAGES.len() - 1 {
            return Ok(Some(json!({"source":SOURCE,"author_before":driver.author,"author_after":current_author,
                "pid":driver.session.0,"world_id":driver.session.1,"revision":driver.session.2,
                "grid_kind":"mesh_triangles",
                "scene_pixel_scope":"PNG outside actual preview HUD rectangle; independent change evidence, not line classification",
                "proofs":driver.proofs})));
        }
        // 坐标来自当前实际ComputedNode，HWND归属由宿主刚独立确认；不注入组件或UI状态。
        plan.game_mouse_release = Some(super::window_evidence::request_game_toolbar_button(
            plan.region_evidence.as_ref().ok_or("预览工具条缺少实时窗口证明")?, driver.session.0, CLICKS[driver.step],
        )?);
        driver.step += 1;
        Ok(None)
    })
}

fn verify_controls(capture: &Value, grid: bool, collision: bool) -> Result<(), String> {
    let toolbar = &capture["preview_toolbar"];
    let runtime = &toolbar["runtime"];
    let buttons = toolbar["buttons"].as_array().filter(|buttons| buttons.len() == 3)
        .ok_or("预览工具条缺少三个真实按钮")?;
    if toolbar["source"] != "production-Bevy-UI-ComputedNode" || toolbar["active"] != true
        || toolbar["root_visible"] != true || toolbar["camera_active"] != true || toolbar["f12_bound"] != false
        || toolbar["grid"] != grid || toolbar["collision"] != collision
        || runtime["source"] != RUNTIME_SOURCE || runtime["grid_kind"] != "mesh_triangles" || runtime["active"] != true
        || runtime["requested_grid"] != grid || runtime["requested_collision"] != collision
        || runtime["primary_camera"].as_u64().unwrap_or(0) == 0
        || toolbar["target_camera"] != runtime["primary_camera"]
        || runtime["target_camera"] != runtime["primary_camera"]
        || toolbar["fallback_camera_active"] != false || runtime["fallback_camera_active"] != false
        || !runtime["actual_grid"].is_null() || runtime["actual_collision"] != collision
        || capture["author_view"]["camera"]["entity"] != runtime["primary_camera"]
        || capture["author_view"]["camera"]["grid"] != runtime["actual_grid"]
        || capture["author_view"]["camera"]["shading"] != runtime["actual_shading"]
        || capture["author_view"]["camera"]["prepass_lease"] != runtime["actual_prepass_lease"]
        || capture["author_view"]["camera"]["depth_prepass"] != runtime["actual_depth_prepass"]
        || capture["author_view"]["camera"]["normal_prepass"] != runtime["actual_normal_prepass"]
        || capture["author_view"]["camera"]["msaa"] != runtime["actual_msaa"]
        || capture["author_view"]["physics_debug"] != runtime["actual_collision"] {
        return Err("预览按钮请求未被真实主相机网格或PhysicsGizmos采用".into());
    }
    for label in ["返回", "显示网格", "显示碰撞网格"] {
        let mut matching = buttons.iter().filter(|button| button["label"] == label && button["visible"] == true);
        let button = matching.next().ok_or_else(|| format!("预览工具条缺少可见{label}按钮"))?;
        if matching.next().is_some() { return Err("预览工具条出现重复按钮".into()); }
        for field in ["physical_center", "physical_size"] {
            if button[field].as_array().is_none_or(|values| values.len() != 2
                || values.iter().any(|value| value.as_f64().is_none_or(|number| !number.is_finite() || number <= 0.))) {
                return Err("预览按钮缺少真实有效布局".into());
            }
        }
    }
    if grid {
        if runtime["actual_shading"] != json!({"options":[0.,1.,0.,1.],"wire":[0.,0.,1.,0.]})
            || runtime["actual_prepass_lease"] != true || runtime["actual_depth_prepass"] != true
            || runtime["actual_normal_prepass"] != true || runtime["actual_msaa"] != "Off" {
            return Err("模型网格未采用真实三角线框与预通道租约".into());
        }
        if capture["render_readiness"]["pipelines"]["ready_author_pipelines"]["wire"].as_u64().unwrap_or(0) == 0 {
            return Err("实际模型三角线框GPU管线尚未就绪".into());
        }
    } else if !runtime["actual_shading"].is_null() || runtime["actual_prepass_lease"] != false {
        return Err("关闭模型网格后没有归还相机线框或预通道租约".into());
    }
    super::view_controls::verify_mode(capture, true)?;
    super::view_controls::verify_render_readiness(capture)
}

fn read(
    directory: &Path, session: (u32, u64, u64), expected: (&str, bool, bool), minimum_frame: u64,
    camera: Option<u64>, author: &Value, geometry: &crate::viewport_mode::ViewportGeometry,
) -> Result<Option<Value>, String> {
    let (pid, world_id, revision) = session;
    let prefix = format!("{pid}-{revision}-playing-author-view-");
    for (index, entry) in fs::read_dir(directory).map_err(|error| error.to_string())?.enumerate() {
        if index >= 2048 { return Err("预览工具条证据目录超过固定预算".into()); }
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name(); let name = name.to_string_lossy();
        if !name.starts_with(&prefix) || !name.ends_with(".json") { continue; }
        let Some(mut capture) = read_json(&entry.path())? else { continue; };
        if capture["pid"] != pid || capture["world_id"] != world_id || capture["revision"] != revision
            || capture["mode"] != "playing" || capture["source"] != "real-project-world" {
            return Err("预览工具条截图的PID、World、修订或来源不符".into());
        }
        if capture["frame"].as_u64().unwrap_or(0) < minimum_frame
            || capture["preview_toolbar"]["runtime"]["requested_grid"] != expected.1
            || capture["preview_toolbar"]["runtime"]["requested_collision"] != expected.2 { continue; }
        verify_controls(&capture, expected.1, expected.2)?;
        if camera.is_some_and(|camera| capture["preview_toolbar"]["runtime"]["primary_camera"] != camera) {
            return Err("预览工具条切换意外更换实际主相机".into());
        }
        super::placement_evidence::verify(&capture, author)?;
        let scene_hash = verify_pixels(&capture, &entry.path().with_extension("png"))?;
        let Some(window) = super::window_evidence::query_live(&capture, geometry)? else { continue; };
        capture["immutable_evidence"] = json!(name.as_ref());
        return Ok(Some(json!({"capture":capture,"host_window_region":window,"scene_rgba_sha256":scene_hash})));
    }
    Ok(None)
}

fn read_json(path: &Path) -> Result<Option<Value>, String> {
    let mut bytes = Vec::new();
    fs::File::open(path).map_err(|error| error.to_string())?.take(128 * 1024 + 1)
        .read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    if bytes.len() > 128 * 1024 { return Err("预览工具条JSON超过128KiB预算".into()); }
    Ok(serde_json::from_slice(&bytes).ok())
}

fn verify_pixels(capture: &Value, path: &Path) -> Result<String, String> {
    let mut bytes = Vec::new();
    fs::File::open(path).map_err(|error| error.to_string())?.take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    if bytes.len() < 24 || bytes.len() > 64 * 1024 * 1024 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
        return Err("预览工具条真实PNG缺失或超过64MiB预算".into());
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    if width == 0 || height < 5 || u64::from(width) * u64::from(height) > 16 * 1024 * 1024 {
        return Err("预览工具条PNG尺寸超过固定预算".into());
    }
    let image = Image::from_buffer(&bytes, ImageType::Extension("png"), CompressedImageFormats::NONE,
        true, ImageSampler::nearest(), RenderAssetUsages::default()).map_err(|error| error.to_string())?
        .try_into_dynamic().map_err(|error| error.to_string())?.to_rgba8();
    let hash = |pixels: &[u8]| Sha256::digest(pixels).iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    if capture["capture"]["source"] != "Bevy ScreenshotCaptured" || capture["capture"]["width"] != width
        || capture["capture"]["height"] != height || capture["author_tools"]["rgba_sha256"] != hash(image.as_raw())
        || capture["capture"]["sha256"].as_str().is_none_or(|hash| hash.len() != 64) {
        return Err("预览工具条PNG与本次实际GPU回读身份不符".into());
    }
    // HUD可拖动，按实际根节点范围排除整个背景和拖动条，不沿用固定顶部20%的裁剪。
    let rect = hud_bounds(capture, width, height)?;
    let pixels = image.as_raw();
    let stride = width as usize * 4;
    let mut scene = Sha256::new();
    scene.update(width.to_le_bytes()); scene.update(height.to_le_bytes());
    for value in rect { scene.update(value.to_le_bytes()); }
    for y in 0..height {
        let row = &pixels[y as usize * stride..(y as usize + 1) * stride];
        if y < rect[1] || y >= rect[3] { scene.update(row); }
        else {
            scene.update(&row[..rect[0] as usize * 4]);
            scene.update(&row[rect[2] as usize * 4..]);
        }
    }
    Ok(scene.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

fn hud_bounds(capture: &Value, width: u32, height: u32) -> Result<[u32; 4], String> {
    if capture["preview_toolbar"]["physical_rect"] != capture["preview_toolbar"]["runtime"]["hud_rect"] {
        return Err("预览HUD根节点矩形与实际运行身份不符".into());
    }
    let values = capture["preview_toolbar"]["physical_rect"].as_array().filter(|values| values.len() == 4)
        .ok_or("预览HUD缺少实际根节点矩形")?;
    let mut rect = [0.; 4];
    for (index, value) in values.iter().enumerate() {
        rect[index] = value.as_f64().filter(|value| value.is_finite()).ok_or("预览HUD矩形坐标无效")?;
    }
    if rect[0] < 0. || rect[1] < 0. || rect[2] > f64::from(width) || rect[3] > f64::from(height)
        || rect[2] <= rect[0] || rect[3] <= rect[1]
        || (rect[2] - rect[0]) * (rect[3] - rect[1]) * 2. >= f64::from(width) * f64::from(height) {
        return Err("预览HUD范围越界或遮住了大部分场景".into());
    }
    for button in capture["preview_toolbar"]["buttons"].as_array().ok_or("缺少实际预览按钮布局")? {
        let cx = button["physical_center"][0].as_f64().ok_or("按钮中心无效")?;
        let cy = button["physical_center"][1].as_f64().ok_or("按钮中心无效")?;
        let w = button["physical_size"][0].as_f64().ok_or("按钮尺寸无效")?;
        let h = button["physical_size"][1].as_f64().ok_or("按钮尺寸无效")?;
        if ![cx,cy,w,h].into_iter().all(f64::is_finite) || w <= 0. || h <= 0.
            || cx - w / 2. < rect[0] - 1. || cy - h / 2. < rect[1] - 1.
            || cx + w / 2. > rect[2] + 1. || cy + h / 2. > rect[3] + 1. {
            return Err("实际预览按钮不在HUD根节点范围内".into());
        }
    }
    Ok([rect[0].floor() as u32, rect[1].floor() as u32, rect[2].ceil() as u32, rect[3].ceil() as u32])
}

pub(super) fn verify(directory: &Path, report: &Value) -> Result<(), String> {
    let evidence = &report["preview_toolbar_controls"];
    let playing = report["steps"].as_array().ok_or("缺少工程预览阶段")?.iter()
        .find(|step| step["stage"] == "f12_play_same_world").ok_or("缺少工程预览同World阶段")?;
    if evidence["source"] != SOURCE || evidence["grid_kind"] != "mesh_triangles" || evidence["author_before"] != evidence["author_after"]
        || evidence["author_before"] != playing["author"] || evidence["pid"] != playing["pid"]
        || evidence["world_id"] != playing["world_id"] || evidence["revision"] != playing["applied_revision"] {
        return Err("预览工具条闭环改变作者数据或会话身份".into());
    }
    let proofs = evidence["proofs"].as_array().filter(|proofs| proofs.len() == STAGES.len())
        .ok_or("预览工具条缺少完整五阶段实际证据")?;
    let mut previous_frame = None;
    let mut previous_scene = None;
    let mut previous_hud = None;
    let mut camera = None;
    for (index, (proof, (stage, grid, collision))) in proofs.iter().zip(STAGES).enumerate() {
        let recorded = &proof["capture"];
        let file = recorded["immutable_evidence"].as_str().ok_or("预览工具条缺少独立截图路径")?;
        if Path::new(file).components().count() != 1 || !file.ends_with(".json") { return Err("预览工具条截图路径越界".into()); }
        let stored = read_json(&directory.join(file))?.ok_or("预览工具条独立JSON无效")?;
        let mut original = recorded.clone(); original.as_object_mut().ok_or("预览截图对象无效")?.remove("immutable_evidence");
        let trigger = if index == 0 { Value::Null } else { json!(CLICKS[index - 1]) };
        if stored != original || proof["stage"] != stage || proof["trigger_button"] != trigger || stored["pid"] != evidence["pid"]
            || stored["world_id"] != evidence["world_id"] || stored["revision"] != evidence["revision"]
            || stored["mode"] != "playing" || stored["source"] != "real-project-world"
            || stored["screenshot"] != Path::new(file).with_extension("png").to_string_lossy().as_ref() {
            return Err(format!("预览工具条阶段{stage}与本次独立落盘截图不一致"));
        }
        let frame = stored["frame"].as_u64().filter(|frame| *frame > 0).ok_or("预览截图实际帧无效")?;
        if previous_frame.is_some_and(|previous| frame <= previous) { return Err("预览工具条重复使用点击前的旧截图".into()); }
        let actual_camera = stored["preview_toolbar"]["runtime"]["primary_camera"].clone();
        if camera.as_ref().is_some_and(|camera| camera != &actual_camera) { return Err("预览工具条闭环更换主相机".into()); }
        verify_controls(&stored, grid, collision)?;
        let hud = stored["preview_toolbar"]["physical_rect"].clone();
        if previous_hud.as_ref().is_some_and(|previous| previous != &hud) {
            return Err("预览显示切换期间HUD遮罩变化，不能作为模型网格证据".into());
        }
        super::placement_evidence::verify(&stored, &evidence["author_before"])?;
        let scene = verify_pixels(&stored, &directory.join(file).with_extension("png"))?;
        if proof["scene_rgba_sha256"] != scene || previous_scene.as_ref() == Some(&scene) {
            return Err("预览工具条实际场景像素身份或切换差异不符".into());
        }
        let step = json!({"pid":evidence["pid"],"world_id":evidence["world_id"],
            "applied_revision":evidence["revision"],"mode":"playing","host_window_region":proof["host_window_region"]});
        super::window_evidence::verify_record(&step, &stored, &report["host_pid"])?;
        if proof["host_window_region"]["capture_frame"] != frame
            || proof["host_window_region"]["preview_toolbar"] != stored["preview_toolbar"] {
            return Err("预览工具条原生窗口证明没有绑定本次实际帧和按钮布局".into());
        }
        previous_frame = Some(frame); previous_scene = Some(scene); camera = Some(actual_camera);
        previous_hud = Some(hud);
    }
    Ok(())
}
