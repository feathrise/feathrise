//! 编辑器本地场景只按已登记的摆放 ID 更新。应用和恢复均摊到帧；快速拖帧只保留一个正处理的样本。
use super::*;
use feathrise_scene_content::SceneId;
use std::time::{Duration, Instant};

struct Original {
    entity: Entity,
    before: Transform,
    applied: Transform,
}
type Roots<'w, 's, 'a, 'b> = Query<
    'w,
    's,
    (&'a ScenePlacementInstance, &'b mut Transform),
    Without<crate::components::SceneEditorCamera>,
>;

#[derive(Default)]
pub(super) struct ObjectPreview {
    key: Option<(usize, u32, u64)>,
    scene: Option<SceneId>,
    pass: Option<(Arc<Picture>, usize)>,
    previous: Vec<Original>,
    applied: Vec<Original>,
}
impl ObjectPreview {
    pub(super) fn apply(
        &mut self,
        frame: Option<&(Arc<Timeline>, Arc<Picture>)>,
        scene: Option<&SceneId>,
        index: Option<&PlacementInstanceIndex>,
        roots: &mut Roots<'_, '_, '_, '_>,
    ) {
        let desired = frame.zip(scene).zip(index);
        let identity = desired.map(|(((film, pic), _), idx)| {
            (
                Arc::as_ptr(film) as usize,
                pic.time.to_bits(),
                idx.membership_revision(),
            )
        });
        let interrupted =
            identity.map(|k| k.0) != self.key.map(|k| k.0) || scene != self.scene.as_ref();
        if interrupted {
            self.pass = None;
        }
        let started = Instant::now();
        // 仅恢复本预览最后写入的姿态，避免覆盖在外部流程中已换身份或已移动的对象。
        while let Some(o) = self
            .previous
            .pop()
            .or_else(|| interrupted.then(|| self.applied.pop()).flatten())
        {
            if let Ok((_, mut t)) = roots.get_mut(o.entity)
                && *t == o.applied
            {
                *t = o.before;
            }
            if started.elapsed() >= Duration::from_micros(500) {
                return;
            }
        }
        if self.pass.is_none() && (self.key != identity || interrupted) {
            self.key = identity;
            self.scene = scene.cloned();
            std::mem::swap(&mut self.previous, &mut self.applied);
            self.pass = frame
                .filter(|_| desired.is_some())
                .map(|(_, p)| (p.clone(), 0));
            if !self.previous.is_empty() {
                return;
            }
        }
        let (Some((pic, cursor)), Some(id), Some(index)) = (&mut self.pass, scene, index) else {
            return;
        };
        while let Some(o) = pic.objects.get(*cursor) {
            *cursor += 1;
            if o.instance
                .as_deref()
                .is_none_or(|instance| instance == id.as_str())
            {
                for &entity in index.roots_for(id, &o.target) {
                    if let Ok((owner, mut tr)) = roots.get_mut(entity)
                        && owner.is_active()
                    {
                        let pose = as_transform(o.pose);
                        self.applied.push(Original {
                            entity,
                            before: *tr,
                            applied: pose,
                        });
                        *tr = pose;
                    }
                }
            }
            if started.elapsed() >= Duration::from_micros(500) {
                return;
            }
        }
        self.pass = None;
    }
}
