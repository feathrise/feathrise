//! 真实GPU回读、原按钮布局及精确Cargo子进程句柄取证；磁盘和图片工作均在单飞后台槽。

use super::{resources::Plan, types::BuildWitness, fixture};
use bevy::{asset::RenderAssetUsages, image::{CompressedImageFormats, ImageSampler, ImageType}, prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured}, tasks::IoTaskPool};
use serde_json::{Value, json};
use std::{os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle}, path::{Path, PathBuf}, time::{Duration, Instant}};
use windows_sys::Win32::{Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT}, System::Threading::{
    GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    QueryFullProcessImageNameW, WaitForSingleObject,
}};

pub(super) fn original_button(world: &mut World, label: &str) -> Result<Option<(Entity, Value)>, String> {
    let labels: Vec<_> = world.query::<(Entity, &Text)>().iter(world).filter_map(|(entity,text)|(text.as_str() == label).then_some(entity)).collect();
    let mut found = Vec::new();
    for text in labels {
        let mut current = text;
        for _ in 0..24 {
            if world.get::<Button>(current).is_some() {
                if world.get::<bevy::ui::InteractionDisabled>(current).is_none()
                    && visible_ancestry(world, text)
                    && world.get::<ComputedNode>(current).is_some_and(|node|node.size().min_element() > 0.) {
                    let node = world.get::<ComputedNode>(current).unwrap();
                    found.push((current, json!({"entity":current.to_bits(),"label":label,"text_entity":text.to_bits(),
                        "computed_size":node.size().to_array(),"visible":true,"disabled":false})));
                }
                break;
            }
            let Some(parent) = world.get::<ChildOf>(current) else { break; };
            current = parent.parent();
        }
    }
    found.sort_by_key(|(entity,_)|entity.to_bits());
    found.dedup_by_key(|(entity,_)|entity.to_bits());
    if found.len() > 1 { return Err(format!("原按钮“{label}”并非唯一可见入口")); }
    Ok(found.pop())
}

/// Display::None可能保留上轮ComputedNode；祖先隐藏时不能把旧尺寸当作当前可点入口。
fn visible_ancestry(world: &World, entity: Entity) -> bool {
    let mut cursor = Some(entity);
    for _ in 0..128 {
        let Some(entity) = cursor else { return true; };
        if world.get::<Node>(entity).is_some_and(|node|node.display == Display::None)
            || world.get::<Visibility>(entity).is_some_and(|visibility|*visibility == Visibility::Hidden)
            || world.get::<InheritedVisibility>(entity).is_some_and(|visible|!visible.get()) { return false; }
        cursor = world.get::<ChildOf>(entity).map(ChildOf::parent);
    }
    false
}

pub(super) fn pulse(world: &mut World, entity: Entity) {
    world.entity_mut(entity).insert((Interaction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
}

pub(super) fn screenshot(world: &mut World, plan: &mut Plan, name: &str) -> Result<Option<Value>, String> {
    if let Some(receiver) = &plan.screenshot {
        return match receiver.try_recv() {
            Ok(result) => { plan.screenshot = None; result.map(Some) }
            Err(async_channel::TryRecvError::Closed) => Err("停止编译GPU取证未返回结果".into()),
            Err(async_channel::TryRecvError::Empty) => Ok(None),
        };
    }
    let path = plan.directory.join(name);
    let (sender, receiver) = async_channel::bounded(1);
    plan.screenshot = Some(receiver);
    world.spawn(Screenshot::primary_window()).observe(move |event: On<ScreenshotCaptured>| {
        let image = event.image.clone();
        let sender = sender.clone();
        let path = path.clone();
        IoTaskPool::get().spawn(async move {
            let result = (|| -> Result<Value, String> {
                if path.try_exists().map_err(|error|error.to_string())? { return Err("停止编译GPU输出已存在，拒绝覆盖".into()); }
                let raw = image.data.as_ref().ok_or("实际GPU截图缺少回读像素")?;
                if image.width() < 320 || image.height() < 240 || raw.len() < 320 * 240 * 3 { return Err("实际GPU截图没有有效画面".into()); }
                let readback = fixture::hash_bytes(raw);
                image.try_into_dynamic().map_err(|error|error.to_string())?.to_rgba8().save(&path).map_err(|error|error.to_string())?;
                let mut proof = png_proof(&path, &fixture::read_bounded(&path, 64 * 1024 * 1024)?)?;
                proof["readback_sha256"] = json!(readback);
                Ok(proof)
            })();
            let _ = sender.try_send(result);
        }).detach();
    });
    Ok(None)
}

fn png_proof(path: &Path, bytes: &[u8]) -> Result<Value, String> {
    if bytes.len() < 24 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" { return Err("实际截图并非完整PNG".into()); }
    let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 16 * 1024 * 1024 { return Err("实际PNG尺寸超过固定验收预算".into()); }
    let pixels = Image::from_buffer(bytes, ImageType::Extension("png"), CompressedImageFormats::NONE, true,
        ImageSampler::nearest(), RenderAssetUsages::default()).map_err(|error|error.to_string())?
        .try_into_dynamic().map_err(|error|error.to_string())?.to_rgba8();
    Ok(json!({"source":"Bevy ScreenshotCaptured","path":path,"width":pixels.width(),"height":pixels.height(),
        "png_sha256":fixture::hash_bytes(bytes),"rgba_sha256":fixture::hash_bytes(pixels.as_raw())}))
}

pub(super) fn verify_png(path: &Path, proof: &Value) -> Result<Value, String> {
    let actual = png_proof(path, &fixture::read_bounded(path, 64 * 1024 * 1024)?)?;
    for field in ["source", "path", "width", "height", "png_sha256", "rgba_sha256"] {
        if actual[field] != proof[field] { return Err(format!("{}的真实PNG与{field}证据不符", path.display())); }
    }
    Ok(actual)
}

/// 只轮询本轮编译脚本创建的有界记录；等待与Win32查询均不占作者帧。
pub(super) fn build_witness(root: PathBuf, target: PathBuf, project: String, nonce: String,
    control: crate::build_process::BuildControl) -> Result<BuildWitness, String> {
    let started = Instant::now();
    let marker = root.join(super::START_FILE);
    loop {
        control.check()?;
        if marker.try_exists().map_err(|error|error.to_string())? {
            let Ok(record) = fixture::read_json(&marker) else {
                if started.elapsed() > Duration::from_secs(1500) { return Err("真实Cargo标记持续不完整".into()); }
                std::thread::sleep(Duration::from_millis(25));
                continue;
            };
            if record["schema"] != 1 || record["project_id"] != project || record["nonce"] != nonce || record["released"] != false {
                return Err("真实Cargo构建标记与本轮夹具身份不符".into());
            }
            let pid = record["pid"].as_u64().and_then(|pid|u32::try_from(pid).ok()).filter(|pid|*pid != 0).ok_or("真实Cargo构建标记缺少有效PID")?;
            // SAFETY：只申请精确标记PID的查询和等待权限，绝不借进程名扫描或结束其他进程。
            let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE, 0, pid) };
            if handle.is_null() { return Err(format!("未能持有真实build.rs进程：{}", std::io::Error::last_os_error())); }
            let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
            let mut buffer = [0_u16; 32768];
            let mut length = buffer.len() as u32;
            if unsafe { QueryFullProcessImageNameW(handle.as_raw_handle(), 0, buffer.as_mut_ptr(), &mut length) } == 0 {
                return Err(format!("不能核对真实Cargo子进程映像：{}", std::io::Error::last_os_error()));
            }
            let image = PathBuf::from(String::from_utf16(&buffer[..length as usize]).map_err(|error|error.to_string())?)
                .canonicalize().map_err(|error|error.to_string())?;
            if !image.starts_with(&target) || image.file_name().is_none_or(|name|name != "build-script-build.exe")
                || unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) } != WAIT_TIMEOUT {
                return Err("未取得本轮缓存内仍真实运行的Cargo构建脚本句柄，不能声称编译取消通过".into());
            }
            return Ok(BuildWitness { handle, proof:json!({"source":"owned build.rs record + held Windows process handle",
                "marker":record,"image":image,"image_in_current_target":true,"process_alive":true}) });
        }
        if started.elapsed() > Duration::from_secs(1500) { return Err("1500秒未见真实Cargo构建脚本执行标记".into()); }
        std::thread::sleep(Duration::from_millis(25));
    }
}

pub(super) fn process_exit(witness: &BuildWitness) -> Result<Option<Value>, String> {
    // timeout=0只查询同一已持有句柄，既不阻塞也不会因PID被复用误判。
    match unsafe { WaitForSingleObject(witness.handle.as_raw_handle(), 0) } {
        WAIT_TIMEOUT => Ok(None),
        WAIT_OBJECT_0 => {
            let mut code = 0;
            if unsafe { GetExitCodeProcess(witness.handle.as_raw_handle(), &mut code) } == 0 { return Err("真实Cargo子进程退出码查询失败".into()); }
            Ok(Some(json!({"source":"same held Windows process handle","pid":witness.proof["marker"]["pid"],"signaled":true,"exit_code":code})))
        }
        _ => Err("真实Cargo子进程句柄状态查询失败".into()),
    }
}

pub(super) fn guest_capture(directory: PathBuf, pid: u32, revision: u64, author: Value,
    geometry: crate::viewport_mode::ViewportGeometry) -> Result<Option<Value>, String> {
    let game = directory.join("game");
    let Some(mut evidence) = super::super::code_iteration_capture(&game, pid, revision, 0, &author, &geometry)? else { return Ok(None); };
    let source = game.join(format!("{pid}-{revision}-editing.png"));
    let bytes = fixture::read_bounded(&source, 64 * 1024 * 1024)?;
    let png = png_proof(&directory.join("retry-game.png"), &bytes)?;
    // SDK已把真实RGBA摘要绑定该帧身份；独立解码同一PNG后再存不可变副本，拒收重拍窗口的错配。
    if png["rgba_sha256"] != evidence["game"]["author_tools"]["rgba_sha256"] { return Ok(None); }
    fixture::write_bytes_new(&directory.join("retry-game.png"), &bytes)?;
    fixture::write_new(&directory.join("retry-game.json"), &evidence["game"])?;
    evidence["png"] = png;
    Ok(Some(evidence))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_button_geometry_cannot_bypass_hidden_ancestor_or_label() {
        let mut world = World::new();
        // 无渲染World不运行可见性传播，显式给出当前可见基线，再验证隐藏祖先会拒绝旧布局。
        let parent = world.spawn((Node::default(), InheritedVisibility::VISIBLE)).id();
        let button = world.spawn((Button, Node::default(), InheritedVisibility::VISIBLE,
            ComputedNode { size:Vec2::new(64.,26.), ..default() }, ChildOf(parent))).id();
        let label = world.spawn((Text::new("停止编译"), Node::default(), InheritedVisibility::VISIBLE, ChildOf(button))).id();
        assert_eq!(original_button(&mut world, "停止编译").unwrap().unwrap().0, button);
        world.get_mut::<Node>(parent).unwrap().display = Display::None;
        assert!(original_button(&mut world, "停止编译").unwrap().is_none());
        world.get_mut::<Node>(parent).unwrap().display = Display::Flex;
        world.get_mut::<Node>(label).unwrap().display = Display::None;
        assert!(original_button(&mut world, "停止编译").unwrap().is_none());
    }

    #[test]
    fn cancelled_witness_does_not_keep_waiting_for_a_missing_cargo_marker() {
        let control = crate::build_process::BuildControl::default();
        control.cancel();
        let result = build_witness(PathBuf::new(), PathBuf::new(), "owned".into(), "nonce".into(), control);
        assert!(result.is_err(), "取证任务取消后必须立即结束，而不能等待1500秒标记预算");
    }
}
