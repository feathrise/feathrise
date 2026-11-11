//! 传送箭头按显式外框派生：有界捕获、single-flight后台计算、同身份原子发布。
use super::{EditorAuthoringSession, EditorPreviewAssets, refresh_owned_portal_preview};
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, futures::check_ready};
use feathrise_scene_content::{
    SCENE_SCHEMA_VERSION, SceneBoundariesAsset, SceneBoundaryPath, SceneBoundaryRole, SceneId,
    ScenePortalRecord,
};
use std::collections::VecDeque;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Signature {
    scene_id: SceneId,
    portals: u64,
    boundaries: u64,
}
struct Capture {
    signature: Signature,
    scan: usize,
    outer_index: Option<usize>,
    scanned: bool,
    outer: Option<SceneBoundaryPath>,
    points: usize,
    portals: Vec<ScenePortalRecord>,
}
struct Completed {
    signature: Signature,
    outer: Option<Arc<SceneBoundaryPath>>,
    portals: Option<Vec<ScenePortalRecord>>,
    refresh: VecDeque<ScenePortalRecord>,
}
#[derive(Default)]
pub(crate) struct PortalDirectionState {
    capture: Option<Capture>,
    task: Option<Task<Completed>>,
    pending: Option<Completed>,
    applied: Option<Signature>,
    outer: Option<Arc<SceneBoundaryPath>>,
    refresh: VecDeque<ScenePortalRecord>,
    retirement: Option<Task<()>>, // 大旧数组后台回收，最多一个在途任务。
}

/// 只有几何或开放状态变化才改变外框方向，标签、厚度和内部墙不触发重算。
fn same_outer(left: Option<&SceneBoundaryPath>, right: Option<&SceneBoundaryPath>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => left.closed == right.closed && left.points == right.points,
        _ => false,
    }
}
fn compute(
    capture: Capture,
    previous: Option<Arc<SceneBoundaryPath>>,
    previous_portals: Option<u64>,
) -> Completed {
    let Capture {
        signature,
        outer,
        mut portals,
        ..
    } = capture;
    if previous_portals == Some(signature.portals)
        && same_outer(previous.as_deref(), outer.as_ref())
    {
        return Completed {
            signature,
            outer: previous,
            portals: None,
            refresh: VecDeque::new(),
        }; // 内部墙修改不重复全部射线计算。
    }
    let outer = outer.map(Arc::new);
    let mut refresh = VecDeque::new();
    if let Some(path) = outer.as_ref() {
        let boundaries = SceneBoundariesAsset {
            schema_version: SCENE_SCHEMA_VERSION,
            scene_id: signature.scene_id.clone(),
            paths: vec![(**path).clone()],
        }; // 深复制只发生在CPU后台。
        for record in &mut portals {
            let position = Vec3::from_array(record.position);
            if !position.is_finite() {
                continue;
            }
            if let Some(direction) =
                boundaries.nearest_main_boundary_direction_degrees(position.xz())
            {
                if record.arrow_degrees.to_bits() != direction.to_bits() {
                    record.arrow_degrees = direction;
                    refresh.push_back(record.clone()); // 预览稍后分帧刷新，作者数组先完整发布。
                }
            }
        }
    }
    Completed {
        signature,
        outer,
        portals: Some(portals),
        refresh,
    }
}

/// 主线程只复制固定输入切片、非阻塞探测任务，并交换完整同修订结果。
pub(crate) fn sync_portal_directions_from_boundary(
    mut commands: Commands,
    preview_assets: Res<EditorPreviewAssets>,
    mut session: ResMut<EditorAuthoringSession>,
    mut state: Local<PortalDirectionState>,
    edit: Option<Res<super::BoundaryEditState>>, // 活动拖点尚未推进边界修订。
    numeric: Option<Res<super::boundary::BoundaryThicknessInputState>>, // 数值输入收口后再观察稳定外框。
) {
    if let Some(retirement) = state.retirement.as_mut() {
        if check_ready(retirement).is_some() {
            state.retirement = None;
        }
    }
    if edit.as_ref().is_some_and(|edit| edit.dragging())
        || numeric.as_ref().is_some_and(|numeric| numeric.busy())
    {
        return; // 不捕获或发布尚未收口的作者中间态。
    }
    let signature = session.draft.as_ref().map(|draft| Signature {
        scene_id: draft.scene_id.clone(),
        portals: session.portal_revision,
        boundaries: session.boundary_revision,
    });
    if let Some(task) = state.task.as_mut() {
        if let Some(completed) = check_ready(task) {
            state.task = None; // 真正完成才归还CPU槽。
            state.pending = Some(completed); // 唯一完成值等待回收槽，不重新启动任务。
        }
    }
    if state.retirement.is_none() {
        if let Some(completed) = state.pending.take() {
            if signature.as_ref() == Some(&completed.signature) {
                if let Some(portals) = completed.portals {
                    let old = std::mem::replace(
                        &mut session.draft.as_mut().unwrap().portals.portals,
                        portals,
                    ); // O1发布完整作者方向。
                    state.retirement = Some(AsyncComputeTaskPool::get().spawn(async move {
                        drop(old);
                    }));
                }
                state.outer = completed.outer;
                state.refresh = completed.refresh;
                state.applied = Some(completed.signature);
            } else {
                state.retirement = Some(AsyncComputeTaskPool::get().spawn(async move {
                    drop(completed);
                })); // 迟到数组仅后台回收。
            }
        }
    }
    let Some(signature) = signature else {
        state.applied = None;
        if state.retirement.is_none() && (state.capture.is_some() || !state.refresh.is_empty()) {
            let capture = state.capture.take();
            let refresh = std::mem::take(&mut state.refresh);
            state.retirement = Some(AsyncComputeTaskPool::get().spawn(async move {
                drop((capture, refresh));
            })); // 切换场景回收不在交互帧遍历记录。
        }
        return;
    };
    if state.applied.as_ref() == Some(&signature) {
        for _ in 0..8 {
            let Some(record) = state.refresh.pop_front() else {
                break;
            };
            refresh_owned_portal_preview(&mut commands, &preview_assets, &mut session, &record); // 固定预算避免重建大量实体。
        }
        return;
    }
    if !state.refresh.is_empty() {
        if state.retirement.is_some() {
            return;
        } // 有界回收槽背压。
        let refresh = std::mem::take(&mut state.refresh);
        state.retirement = Some(AsyncComputeTaskPool::get().spawn(async move {
            drop(refresh);
        })); // 新分支不继续安装旧预览。
    }
    if state.task.is_some() || state.pending.is_some() {
        return;
    } // 不靠drop抢占CPU任务，不无限启动替代任务。
    if state
        .capture
        .as_ref()
        .is_none_or(|capture| capture.signature != signature)
    {
        if state.capture.is_some() {
            if state.retirement.is_some() {
                return;
            } // 中途换代的捕获同样有界回收。
            let stale = state.capture.take();
            state.retirement = Some(AsyncComputeTaskPool::get().spawn(async move {
                drop(stale);
            }));
        }
        state.capture = Some(Capture {
            signature: signature.clone(),
            scan: 0,
            outer_index: None,
            scanned: false,
            outer: None,
            points: 0,
            portals: Vec::new(),
        });
    }
    let capture = state.capture.as_mut().unwrap();
    let draft = session.draft.as_ref().unwrap();
    if !capture.scanned {
        if let Some(document) = draft.boundaries.as_ref() {
            for index in capture.scan..(capture.scan + 64).min(document.paths.len()) {
                capture.scan = index + 1;
                let path = &document.paths[index];
                if path.enabled && path.role == SceneBoundaryRole::Outer {
                    capture.outer_index = Some(index);
                    capture.outer = Some(SceneBoundaryPath {
                        id: path.id.clone(),
                        label: path.label.clone(),
                        role: path.role,
                        enabled: path.enabled,
                        visible: path.visible,
                        closed: path.closed,
                        points: Vec::new(),
                        bottom_y: path.bottom_y,
                        top_y: path.top_y,
                        thickness: path.thickness,
                    });
                    capture.scanned = true;
                    break;
                }
            }
            capture.scanned |= capture.scan == document.paths.len();
        } else {
            capture.scanned = true;
        }
        if !capture.scanned {
            return;
        }
    }
    if let Some(index) = capture.outer_index {
        let path = &draft.boundaries.as_ref().unwrap().paths[index];
        let end = (capture.points + 4096).min(path.points.len());
        capture
            .outer
            .as_mut()
            .unwrap()
            .points
            .extend_from_slice(&path.points[capture.points..end]);
        capture.points = end;
        if end != path.points.len() {
            return;
        }
    }
    let start = capture.portals.len();
    let end = (start + 64).min(draft.portals.portals.len());
    capture
        .portals
        .extend_from_slice(&draft.portals.portals[start..end]);
    if end != draft.portals.portals.len() {
        return;
    }
    let capture = state.capture.take().unwrap();
    let previous = state.outer.clone();
    let previous_portals = state
        .applied
        .as_ref()
        .filter(|old| old.scene_id == signature.scene_id)
        .map(|old| old.portals);
    state.task = Some(
        AsyncComputeTaskPool::get()
            .spawn(async move { compute(capture, previous, previous_portals) }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    fn capture() -> Capture {
        let map = SceneId::parse("MAP-direction-worker").unwrap();
        let document =
            SceneBoundariesAsset::rectangular_default(&map, [-50.0; 2], [50.0; 2], 0.0, 20.0)
                .unwrap();
        let record = ScenePortalRecord {
            terrain_binding: None,
            arrival_terrain_binding: None,
            id: "portal".into(),
            label: "出口".into(),
            label_customized: true,
            position: [40.0, 0.0, 0.0],
            arrival: [35.0, 0.0, 0.0],
            target: feathrise_scene_content::ScenePortalTarget::default_spawn(map.clone()),
            color_srgba: [1.0; 4],
            arrow_degrees: 0.0,
        };
        Capture {
            signature: Signature {
                scene_id: map,
                portals: 1,
                boundaries: 1,
            },
            scan: 1,
            outer_index: Some(0),
            scanned: true,
            outer: document.paths.into_iter().next(),
            points: 4,
            portals: vec![record],
        }
    }
    #[test]
    fn background_direction_produces_complete_records() {
        let result = compute(capture(), None, None);
        assert_eq!(result.portals.unwrap()[0].arrow_degrees, 90.0); // 靠近东边外框朝向东方。
        assert_eq!(result.refresh.len(), 1); // 只有真实方向变化需要刷新视觉。
    }
    #[test]
    fn unchanged_outer_skips_work_even_when_boundary_revision_changed() {
        let mut next = capture();
        let previous = Arc::new(next.outer.as_ref().unwrap().clone());
        next.signature.boundaries = 2;
        next.outer.as_mut().unwrap().label = "只改名称或内部墙".into();
        let result = compute(next, Some(previous), Some(1));
        assert!(result.portals.is_none()); // 相同外框几何不重复P×N派生。
        assert!(result.refresh.is_empty());
    }
}
