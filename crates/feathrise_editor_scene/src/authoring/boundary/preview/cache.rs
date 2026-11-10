//! 空气墙辅助线按视口异步筛选；显示预算不限制作者数据、碰撞或命中列表。

use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, futures::check_ready},
};
use feathrise_scene_content::{SceneBoundariesAsset, SceneBoundaryCapture, SceneId};
use feathrise_scene::{ClipmapGroundHit, ClipmapGroundSnapshot};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, PartialEq)]
struct Identity {
    epoch: u64,
    scene_id: SceneId,
    revision: u64,
    center: Vec2,
    radius: f32,
    selected: Option<usize>,
    ground: Option<([u64; 7], Option<u64>)>, // 基础高度内容及局部页到达身份；相机显示LOD不改写同一端点Y。
    drag: Option<(usize, usize, [u32; 2])>, // 原Arc文档尚未提交的一个点，worker只覆盖这个固定位置。
}

struct Output {
    identity: Identity,
    paths: BTreeMap<usize, Vec<usize>>,
    points: Vec<usize>,
    heights: Arc<BTreeMap<[u32; 2], Vec3>>, // 同源完成点由worker复用；最多现有4096段三条轮廓端点及256控制点。
}

#[derive(Default)]
pub(crate) struct PreviewCache {
    epoch: u64,
    source: Option<(SceneId, u64)>,
    capture: SceneBoundaryCapture,
    document: Option<Arc<SceneBoundariesAsset>>,
    desired: Option<Identity>,
    installed: Option<Output>,
    task: Option<Task<Output>>, // 旧任务完成前保留槽，移动相机不堆积后台任务。
}

impl PreviewCache {
    pub(super) fn invalidate(&mut self) {
        if self.source.take().is_none() && self.desired.is_none() {
            return;
        }
        self.epoch = self.epoch.wrapping_add(1);
        self.desired = None;
        self.document = None;
        self.installed = None;
        self.capture = default(); // 离开工具/场景后旧CPU任务仍持有槽，迟到结果由epoch拒收。
    }
    pub(super) fn update(
        &mut self,
        document: &SceneBoundariesAsset,
        revision: u64,
        center: Vec2,
        radius: f32,
        selected: Option<usize>,
        ground: Option<&ClipmapGroundSnapshot>,
        drag: Option<(usize, usize, [f32; 2])>,
    ) {
        let identity = Identity {
            epoch: self.epoch,
            scene_id: document.scene_id.clone(),
            revision,
            center,
            radius,
            selected,
            ground: ground.map(|ground| {
                (
                    ground.content_revision_words(),
                    ground.authoritative_page_revision(),
                )
            }),
            drag: drag.map(|(path, point, value)| (path, point, value.map(f32::to_bits))),
        };
        self.desired = Some(identity.clone());
        if let Some(output) = self.task.as_mut().and_then(check_ready) {
            self.task = None;
            if self.desired.as_ref() == Some(&output.identity) {
                self.installed = Some(output);
            }
        }
        if self
            .source
            .as_ref()
            .is_none_or(|(map, old)| map != &document.scene_id || *old != revision)
        {
            self.source = Some((document.scene_id.clone(), revision));
            self.capture = default();
            self.document = None;
            self.installed = None; // 旧索引不能访问新数组，后台任务仍保留single-flight槽。
        }
        if self.document.is_none() {
            if !self.capture.step(document, 4096, 64) {
                return;
            }
            self.document = self.capture.take().map(Arc::new);
        }
        if self.task.is_some()
            || self
                .installed
                .as_ref()
                .is_some_and(|output| output.identity == identity)
        {
            return;
        }
        let document = Arc::clone(self.document.as_ref().expect("完整预览快照"));
        let known = self
            .installed
            .as_ref()
            .filter(|output| {
                output.identity.scene_id == identity.scene_id
                    && output.identity.ground.map(|(content, _)| content)
                        == identity.ground.map(|(content, _)| content)
            })
            .map(|output| Arc::clone(&output.heights)); // 主线程只复制Arc，不能为每次页到达深拷贝端点数组。
        let ground = ground.cloned(); // Arc共享至多64张局部基础页，采样及需求合并仍使用场景唯一服务。
        self.task = Some(AsyncComputeTaskPool::get().spawn(async move {
            let mut output = build(&document, identity);
            if let Some(ground) = ground {
                sample_heights(&document, &mut output, known.as_deref(), |point| {
                    match ground.sample_authoritative_surface(Vec2::from_array(point)) {
                        ClipmapGroundHit::Hit(point) => Some(point + Vec3::Y * 0.35),
                        _ => None, // 缺页不画零高度，也不把粗显示面Y缓存为基础作者高度。
                    }
                });
            }
            output
        })); // 已有可见段单飞槽同时派生高度，不新增每段任务或主线程逐点IO。
    }

    pub(super) fn paths(&self) -> impl Iterator<Item = (&usize, &Vec<usize>)> {
        self.installed.iter().flat_map(|output| output.paths.iter())
    }

    pub(super) fn points(&self, path: usize) -> &[usize] {
        self.installed
            .as_ref()
            .filter(|output| output.identity.selected == Some(path))
            .map_or(&[], |output| output.points.as_slice())
    }

    pub(super) fn display_point(&self, point: [f32; 2]) -> Option<Vec3> {
        let desired = self.desired.as_ref()?;
        self.installed
            .as_ref()
            .filter(|output| {
                output.identity.ground.map(|(content, _)| content)
                    == desired.ground.map(|(content, _)| content)
            })
            .and_then(|output| output.heights.get(&point.map(f32::to_bits)).copied())
    } // 同内容已有点不随页到达闪烁；内容换代立即隐藏，缺页端点等待后台补齐。
}

fn point(
    document: &SceneBoundariesAsset,
    identity: &Identity,
    path: usize,
    index: usize,
) -> [f32; 2] {
    if let Some((drag_path, drag_point, value)) = identity.drag
        && drag_path == path
        && drag_point == index
    {
        return value.map(f32::from_bits);
    }
    document.paths[path].points[index]
}

/// 所选墙优先；所有端点派生在现有worker中完成，不在Gizmos提交帧扫描高度页。
fn sample_heights(
    document: &SceneBoundariesAsset,
    output: &mut Output,
    known: Option<&BTreeMap<[u32; 2], Vec3>>,
    mut sample: impl FnMut([f32; 2]) -> Option<Vec3>,
) {
    let heights = Arc::make_mut(&mut output.heights); // 仅在worker物化本次有界可见集合。
    let mut sample = |point: [f32; 2]| {
        known
            .and_then(|known| known.get(&point.map(f32::to_bits)).copied())
            .or_else(|| sample(point))
    }; // 已完成基础Y不重复请求页，视口超过64页时也可逐批补齐辅助端点。
    for index in output.identity.selected.into_iter().chain(
        output
            .paths
            .keys()
            .copied()
            .filter(|index| Some(*index) != output.identity.selected),
    ) {
        let Some(segments) = output.paths.get(&index) else {
            continue;
        };
        let path = &document.paths[index];
        if output.identity.selected == Some(index) {
            for &point_index in &output.points {
                let point = point(document, &output.identity, index, point_index);
                if let Some(value) = sample(point) {
                    heights.insert(point.map(f32::to_bits), value);
                }
            } // 当前墙的可点击控制点先于厚度轮廓请求，避免轮廓端点挤占起拖需求。
        }
        for &segment in segments {
            let start = point(document, &output.identity, index, segment);
            let end = point(
                document,
                &output.identity,
                index,
                (segment + 1) % path.points.len(),
            );
            let Some(lines) = feathrise_scene_content::boundary_footprint_lines(start, end, path.thickness)
            else {
                continue;
            };
            for (start, end) in lines {
                for point in [start, end] {
                    let key = point.map(f32::to_bits);
                    if !heights.contains_key(&key)
                        && let Some(value) = sample(point)
                    {
                        heights.insert(key, value);
                    }
                }
            }
        }
    }
}

fn build(document: &SceneBoundariesAsset, identity: Identity) -> Output {
    let mut output = Output {
        identity,
        paths: BTreeMap::new(),
        points: vec![],
        heights: Arc::new(BTreeMap::new()),
    };
    let view = Rect::from_center_size(
        output.identity.center,
        Vec2::splat(output.identity.radius * 2.0),
    );
    let mut budget = 4096; // 稳定帧最多12k gizmo线；密集总览缩放后可查看其余细节。
    let selected = output.identity.selected;
    for index in selected
        .into_iter()
        .chain((0..document.paths.len()).filter(|index| Some(*index) != selected))
    {
        let Some(path) = document.paths.get(index).filter(|path| path.visible) else {
            continue;
        };
        let len = path.points.len();
        let count = len.saturating_sub(1) + usize::from(path.closed && len >= 3);
        for segment in 0..count {
            if budget == 0 {
                break;
            }
            let a = Vec2::from_array(point(document, &output.identity, index, segment));
            let b = Vec2::from_array(point(
                document,
                &output.identity,
                index,
                (segment + 1) % len,
            ));
            let bounds = Rect::from_corners(
                a.min(b) - Vec2::splat(path.thickness),
                a.max(b) + Vec2::splat(path.thickness),
            );
            if bounds.max.cmplt(view.min).any() || bounds.min.cmpgt(view.max).any() {
                continue;
            }
            output.paths.entry(index).or_default().push(segment);
            budget -= 1;
        }
        if budget == 0 {
            break;
        }
    }
    if let Some(path) = selected
        .and_then(|index| document.paths.get(index))
        .filter(|path| path.visible)
    {
        output
            .paths
            .entry(selected.expect("所选路径存在"))
            .or_default(); // 单点/空开放新墙仍能显示控制点。
        let mut points: Vec<_> = path
            .points
            .iter()
            .enumerate()
            .filter_map(|(index, _)| {
                let point =
                    Vec2::from_array(point(document, &output.identity, selected.unwrap(), index));
                view.contains(point)
                    .then_some((index, point.distance_squared(output.identity.center)))
            })
            .collect();
        points.sort_unstable_by(|a, b| a.1.total_cmp(&b.1)); // 控制点优先当前观察中心，不永久只显示前256个。
        output.points = points
            .into_iter()
            .take(256)
            .map(|(index, _)| index)
            .collect();
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terrain_document() -> (SceneBoundariesAsset, Identity) {
        let map = SceneId::parse("MAP-wall-height").unwrap();
        let mut doc =
            SceneBoundariesAsset::rectangular_default(&map, [-50.; 2], [50.; 2], -80., 160.).unwrap();
        doc.paths[0].closed = false;
        doc.paths[0].points = vec![[2., 2.], [8., 2.]];
        let identity = Identity {
            epoch: 0,
            scene_id: map,
            revision: 1,
            center: Vec2::ZERO,
            radius: 50.,
            selected: Some(0),
            ground: Some(([1; 7], Some(1))),
            drag: None,
        };
        (doc, identity)
    }

    #[test]
    fn clipmap_boundary_preview_uses_base_height_for_control_points_and_all_width_endpoints() {
        let (doc, mut identity) = terrain_document();
        identity.drag = Some((0, 0, [3., 4.].map(f32::to_bits)));
        let mut output = build(&doc, identity);
        sample_heights(&doc, &mut output, None, |[x, z]| {
            Some(Vec3::new(x, x * 2. + z + 0.35, z))
        });
        assert_eq!(
            output.heights.get(&[3., 4.].map(f32::to_bits)),
            Some(&Vec3::new(3., 10.35, 4.))
        );
        assert!(!output.heights.contains_key(&[2., 2.].map(f32::to_bits))); // Arc原点被未提交拖点覆盖。
        let lines =
            feathrise_scene_content::boundary_footprint_lines([3., 4.], [8., 2.], doc.paths[0].thickness)
                .unwrap();
        for (start, end) in lines {
            for [x, z] in [start, end] {
                assert_eq!(
                    output.heights.get(&[x, z].map(f32::to_bits)),
                    Some(&Vec3::new(x, x * 2. + z + 0.35, z))
                );
            }
        }
        assert_eq!(doc.paths[0].points[0], [2., 2.]); // worker不修改作者文档或墙体高度。
        let known = Arc::clone(&output.heights);
        let mut retry = build(&doc, output.identity.clone());
        sample_heights(&doc, &mut retry, Some(&known), |_| {
            panic!("同源完成端点不能再次请求基础页")
        });
        assert_eq!(retry.heights, known); // 分页补齐不会反复争用已经完成的页。
    }

    #[test]
    fn clipmap_boundary_preview_has_no_zero_height_fallback_and_rejects_replaced_content() {
        let (doc, identity) = terrain_document();
        let mut output = build(&doc, identity.clone());
        sample_heights(&doc, &mut output, None, |_| None);
        assert!(output.heights.is_empty());
        sample_heights(&doc, &mut output, None, |[x, z]| {
            Some(Vec3::new(x, 12.35, z))
        });
        let mut cache = PreviewCache {
            desired: Some(identity.clone()),
            installed: Some(output),
            ..default()
        };
        assert_eq!(
            cache.display_point([2., 2.]),
            Some(Vec3::new(2., 12.35, 2.))
        );
        cache.desired.as_mut().unwrap().ground = Some(([1; 7], Some(2)));
        assert!(cache.display_point([2., 2.]).is_some()); // 同一基础内容分页到达保留已经正确的辅助线。
        cache.desired.as_mut().unwrap().ground = Some(([2; 7], Some(2)));
        assert_eq!(cache.display_point([2., 2.]), None);
        cache.desired.as_mut().unwrap().ground = None;
        assert_eq!(cache.display_point([2., 2.]), None);
    }
    #[test]
    fn preview_budget_prioritizes_selected_wall_and_keeps_single_point_editable() {
        let map = SceneId::parse("MAP-wall-preview").unwrap();
        let mut document = SceneBoundariesAsset::rectangular_default(
            &map,
            [-10_000.0; 2],
            [10_000.0; 2],
            -10.0,
            100.0,
        )
        .unwrap();
        document.paths[0].closed = false;
        document.paths[0].points = (0..9000).map(|index| [index as f32, 0.0]).collect();
        let mut selected = document.paths[0].clone();
        selected.id = "selected".to_owned();
        selected.points = vec![[1.0, 1.0]];
        document.paths.push(selected);
        let identity = Identity {
            epoch: 0,
            scene_id: map,
            revision: 0,
            center: Vec2::ZERO,
            radius: 20_000.0,
            selected: Some(1),
            ground: None,
            drag: None,
        };
        let output = build(&document, identity.clone());
        assert!(output.paths.contains_key(&1)); // 尚未形成线段的新墙仍显示白色点。
        assert_eq!(output.points, vec![0]);
        assert_eq!(output.paths.values().map(Vec::len).sum::<usize>(), 4096);
        document.paths[0].visible = false;
        let output = build(&document, identity);
        assert!(!output.paths.contains_key(&0)); // 隐藏显示不纳入Gizmos预算。
    }
}
