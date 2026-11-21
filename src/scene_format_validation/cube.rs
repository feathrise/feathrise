//! 对照真实编辑器3D视口保存前、热接管后和重开的同一普通立方体。

use super::{driver::click_text, fixture::SavedCube};
use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    tasks::IoTaskPool,
};
use feathrise_editor_scene::standalone::{
    self,
    scene_format_validation::{self, Observation, cube_save as author},
};
use feathrise_scene_content::{SceneId, ScenePlacementAssetTarget, ScenePlacementRecord};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{path::Path, time::{Duration, Instant}};

pub(super) const BEFORE: &str = "cube-wire-before-save";
pub(super) const AFTER: &str = "cube-wire-after-save";
pub(super) const REOPENED: &str = "cube-wire-reopened";

#[derive(Clone, Copy, Debug)]
enum Phase {
    Frame,
    SwitchWire,
    BeforeSave,
    Save,
    AwaitAdoption,
    ReopenWire,
}

pub(super) struct Probe {
    pub reopen: Option<SavedCube>,
    phase: Phase,
    record: Option<ScenePlacementRecord>,
    pending: Option<async_channel::Receiver<Result<(bool, Value), String>>>,
    capture_attempt: usize,
    stable_frames: u8,
    rev_before: u64,
    save_before: u64,
    adopted_since: Option<Instant>,
    after_framed: bool,
}

impl Probe {
    pub(super) fn new(reopen: Option<SavedCube>) -> Self {
        Self {
            reopen, phase: Phase::Frame, record: None,
            pending: None, capture_attempt: 0, stable_frames: 0,
            rev_before: 0, save_before: 0, adopted_since: None, after_framed: false,
        }
    }

    pub(super) fn advance(
        &mut self,
        world: &mut World,
        scene: &SceneId,
        observed: &Observation,
        pulse: &mut Option<Entity>,
        directory: &Path,
        proof: &mut Value,
    ) -> Result<bool, String> {
        if !standalone::preview_ready_for(world, scene.as_str()) {
            return Ok(false);
        }
        let record = author::draft_record(world, scene).ok_or("目标草稿没有恰好一个立方体")?;
        if record.asset_target != ScenePlacementAssetTarget::DEFAULT_CUBOID
            || record.translation != [0.0; 3]
            || record.editor.hidden
            || self.record.as_ref().is_some_and(|old| old != &record)
            || self.reopen.as_ref().is_some_and(|saved| saved.record != record)
        {
            return Err("作者记录不再是已冻结的未细分普通立方体".into());
        }
        proof["phase"] = json!(format!("{:?}", self.phase));
        if matches!(self.phase, Phase::BeforeSave | Phase::ReopenWire)
            && !scene_display_ready(world, scene, &record.id, observed, pulse, proof)?
        {
            return Ok(false);
        }
        let state = author::cube_state(world, scene, &record);
        proof["last"] = state.details.clone();
        match self.phase {
            Phase::Frame => {
                if self.reopen.is_none() && !observed.dirty {
                    return Ok(false);
                }
                self.record = Some(record.clone());
                author::frame_cube(world, &record, false)?;
                self.phase = Phase::SwitchWire;
            }
            Phase::SwitchWire => {
                if author::wireframe_capture(world, &record).is_none() {
                    if let Ok(entity) = author::press_wireframe(world) { *pulse = Some(entity); }
                } else {
                    self.phase = if self.reopen.is_some() { Phase::ReopenWire } else { Phase::BeforeSave };
                }
            }
            Phase::BeforeSave => {
                if state.visible_preview
                    && self.capture(world, &record, directory, BEFORE, proof)?
                {
                    proof["before"] = state.details;
                    self.phase = Phase::Save;
                }
            }
            Phase::Save => {
                self.rev_before = state.runtime_revision;
                self.save_before = observed.save_sequence;
                if let Some(entity) = author::press_save(world)? {
                    *pulse = Some(entity);
                    self.phase = Phase::AwaitAdoption;
                }
            }
            Phase::AwaitAdoption => {
                if observed.save_sequence <= self.save_before || observed.dirty {
                    return Ok(false);
                }
                proof["after"] = state.details.clone();
                proof["save_sequence"] = json!(observed.save_sequence);
                if !self.after_framed {
                    author::frame_cube(world, &record, true)?;
                    self.after_framed = true;
                    return Ok(false);
                }
                // 等待新一版正式摆放，同步保存回执不等于渲染采用。失败时仍回读原相机的实际画面。
                let adopted = state.runtime_ready && state.runtime_revision > self.rev_before
                    && state.placement_count == 1 && state.visible_formal && !state.visible_preview;
                let clock = self.adopted_since.get_or_insert_with(Instant::now);
                if adopted || clock.elapsed() >= Duration::from_secs(9) {
                    if self.capture(world, &record, directory, AFTER, proof)? {
                        if !adopted {
                            return Err("原保存已完成，普通立方体未以同身份可见正式根接替作者预览".into());
                        }
                        return Ok(true);
                    }
                }
            }
            Phase::ReopenWire => {
                let adopted = state.runtime_ready && state.placement_count == 1
                    && state.visible_formal && !state.visible_preview && !observed.dirty;
                let clock = self.adopted_since.get_or_insert_with(Instant::now);
                if (adopted || clock.elapsed() >= Duration::from_secs(9))
                    && self.capture(world, &record, directory, REOPENED, proof)?
                {
                    proof["reopened"] = state.details;
                    if !adopted {
                        return Err("已保存记录重开后未生成同身份可见正式网格".into());
                    }
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn capture(
        &mut self, world: &mut World, record: &ScenePlacementRecord,
        directory: &Path, name: &'static str, proof: &mut Value,
    ) -> Result<bool, String> {
        if let Some(receiver) = &self.pending {
            match receiver.try_recv() {
                Ok(result) => {
                    let (passed, evidence) = result?;
                    proof[name] = evidence;
                    self.pending = None;
                    self.stable_frames = 0;
                    if passed {
                        self.capture_attempt = 0;
                    } else if self.capture_attempt >= 5 {
                        return Err(format!("实际 GPU 画面缺少普通立方体的投影边：{name}"));
                    }
                    return Ok(passed);
                }
                Err(async_channel::TryRecvError::Empty) => return Ok(false),
                Err(async_channel::TryRecvError::Closed) => return Err("视口截图未返回 GPU 回读".into()),
            }
        }
        let Some(capture) = author::wireframe_capture(world, record) else { return Ok(false); };
        self.stable_frames = self.stable_frames.saturating_add(1);
        if self.stable_frames < 8 {
            return Ok(false);
        }
        self.capture_attempt += 1;
        let attempt = self.capture_attempt;
        let directory = directory.to_path_buf();
        let (sender, receiver) = async_channel::bounded(1);
        self.pending = Some(receiver);
        let probes = capture.edge_samples;
        let viewport = capture.viewport;
        let camera_current = capture.camera_current;
        let previous_viewport = (name == AFTER).then(|| {
            proof[BEFORE]["viewport_sha256"].as_str().map(str::to_owned)
        }).flatten();
        // 从实际主窗口而非离屏目标读回数据，主窗中的真实场景区域另算摘要，顶栏写盘状态的改变不算新视口。
        world.spawn(Screenshot::primary_window()).observe(move |event: On<ScreenshotCaptured>| {
            let image = event.image.clone();
            let (directory, probes, sender) = (directory.clone(), probes.clone(), sender.clone());
            let previous_viewport = previous_viewport.clone();
            IoTaskPool::get().spawn(async move {
                let result = (|| -> Result<(bool, Value), String> {
                    let pixels = image.data.as_ref().ok_or("实际截图缺少 GPU 数据")?;
                    let sha256 = Sha256::digest(pixels).iter().map(|byte| format!("{byte:02x}")).collect::<String>();
                    let image = image.try_into_dynamic().map_err(|error| error.to_string())?.to_rgb8();
                    let viewport_sha256 = viewport_digest(image.as_raw(), image.width(), image.height(), viewport)?;
                    let fresh = camera_current && previous_viewport.as_ref().is_none_or(|previous| previous != &viewport_sha256);
                    let visible = visible_edges(&probes, |point| {
                        let point = point.round().as_uvec2();
                        image.get_pixel_checked(point.x, point.y).map(|pixel| pixel.0)
                    });
                    let passed = fresh && visible * 2 >= probes.len() && probes.len() == 36;
                    let filename = if passed { format!("{name}.png") } else { format!("{name}-attempt-{attempt}.png") };
                    let path = directory.join(filename);
                    image.save(&path).map_err(|error| error.to_string())?;
                    Ok((passed, json!({"source":"Bevy ScreenshotCaptured","path":path,
                        "width":image.width(),"height":image.height(),"sha256":sha256,
                        "viewport":viewport,"viewport_sha256":viewport_sha256,"camera_current":camera_current,"fresh_viewport":fresh,
                        "visible_author_edge_samples":visible,"author_edge_samples":probes.len()})))
                })();
                let _ = sender.try_send(result);
            }).detach();
        });
        Ok(false)
    }
}

fn scene_display_ready(
    world: &mut World,
    scene: &SceneId,
    placement: &str,
    observed: &Observation,
    pulse: &mut Option<Entity>,
    proof: &mut Value,
) -> Result<bool, String> {
    if !observed.tree_visible {
        *pulse = click_text(world, "场景树");
        return Ok(false);
    }
    let Some(labels) = scene_format_validation::empty_terrain_placement_tree_labels(world, scene, placement)? else {
        return Ok(false);
    };
    let visible = |label: &str| labels.iter().any(|text| text == label);
    if visible("搜索") {
        return Err("场景树的摆放列表上方仍显示搜索框".into());
    }
    // 展开原按钮后等待真实布局，与同一立方体的主窗口截图一起取证。
    const COLLAPSED: &str = "▶ 场景显示 · 网格 / 碰撞 / 着色";
    if visible(COLLAPSED) {
        *pulse = click_text(world, COLLAPSED);
    }
    let ready = ["▼ 场景显示 · 网格 / 碰撞 / 着色", "平滑", "平直", "显示三角线", "显示碰撞"]
        .into_iter().all(visible);
    proof["scene_tree"] = json!({"scene_id": scene, "placement_id": placement,
        "terrain_count": 0, "display_controls_visible": ready, "visible_labels": labels});
    Ok(ready)
}

fn viewport_digest(bytes: &[u8], width: u32, height: u32, [left, top, right, bottom]: [u32; 4]) -> Result<String, String> {
    if !(left < right && right <= width && top < bottom && bottom <= height) {
        return Err("主窗口截图与场景视口布局不匹配".into());
    }
    let mut hasher = Sha256::new();
    for y in top as usize..bottom as usize {
        let row = y * width as usize;
        hasher.update(&bytes[(row + left as usize) * 3..(row + right as usize) * 3]);
    }
    Ok(hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

fn visible_edges(probes: &[(Vec2, Vec2)], sample: impl Fn(Vec2) -> Option<[u8; 3]>) -> usize {
    let light = |[r, g, b]: [u8; 3]| (u16::from(r) + u16::from(g) + u16::from(b)) / 3;
    // 以每条边旁侧的实际像素作对照，忽略灰背景的绝对色与选中状态。
    probes.iter().filter(|&&(point, normal)| {
        let (Some(a), Some(b)) = (sample(point + normal), sample(point - normal)) else { return false; };
        let adjacent = light(a).min(light(b));
        (-1..=1).any(|y| (-1..=1).any(|x| {
            sample(point + Vec2::new(x as f32, y as f32)).is_some_and(|pixel| {
                let [r, g, b] = pixel;
                light(pixel) + 14 < adjacent
                    || (r > 135 && g >= 66 && r.saturating_sub(g) > 40 && g.saturating_sub(b) > 24)
            })
        }))
    }).count()
}
