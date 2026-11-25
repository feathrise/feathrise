use super::*;

#[derive(Clone, Copy, PartialEq)]
enum Handle {
    Center,
    Left,
    Right,
    Back,
    Front,
}

impl Handle {
    const ALL: [Self; 5] = [
        Self::Center,
        Self::Left,
        Self::Right,
        Self::Back,
        Self::Front,
    ];

    fn position(self, area: MotionRegion) -> Vec3 {
        let offset = match self {
            Self::Center => Vec3::ZERO,
            Self::Left => Vec3::NEG_X,
            Self::Right => Vec3::X,
            Self::Back => Vec3::NEG_Z,
            Self::Front => Vec3::Z,
        };
        area.center + area.half_extents * offset
    }

    /// 拖边时固定对边与垂直范围；指针越过对边时保留上次有效预览。
    fn moved(self, initial: MotionRegion, delta: Vec3) -> Option<MotionRegion> {
        let mut area = initial;
        match self {
            Self::Center => area.center += Vec3::new(delta.x, 0.0, delta.z),
            Self::Left | Self::Right => {
                area.center.x += delta.x * 0.5;
                area.half_extents.x += if self == Self::Right {
                    delta.x
                } else {
                    -delta.x
                } * 0.5;
            }
            Self::Back | Self::Front => {
                area.center.z += delta.z * 0.5;
                area.half_extents.z += if self == Self::Front {
                    delta.z
                } else {
                    -delta.z
                } * 0.5;
            }
        }
        area.valid().then_some(area)
    }
}

pub(super) struct Drag {
    handle: Handle,
    anchor: Vec3,
    camera: Entity,
    pub(super) preview: MotionRegion,
}

impl Pointer<'_, '_> {
    fn project(&self, point: Vec3) -> Option<Vec2> {
        let view = self.viewport.as_ref()?;
        let (camera, pose) = self.cameras.get(view.camera).ok()?;
        camera
            .world_to_viewport(&GlobalTransform::from(*pose), point)
            .ok()
    }
}

/// 每次拖动只在松开时返回一个最终值。草稿修订、焦点或视口改变都会丢弃临时值。
pub(super) fn update(
    drag: &mut Option<Drag>,
    initial: MotionRegion,
    pointer: &Pointer,
    mouse: &ButtonInput<MouseButton>,
    over_controls: bool,
) -> Option<MotionRegion> {
    let view = pointer.viewport.as_ref()?;
    let point = pointer.on_plane(initial.center.y);
    if over_controls
        || point.is_none()
        || pointer.windows.iter().any(|window| !window.focused)
        || drag.as_ref().is_some_and(|d| d.camera != view.camera)
    {
        *drag = None;
        return None;
    }
    let point = point?;
    if mouse.just_pressed(MouseButton::Left) {
        let pixel = pointer.project(point)?;
        let (handle, _) = Handle::ALL
            .into_iter()
            .filter_map(|handle| {
                let distance = pointer.project(handle.position(initial))?.distance(pixel);
                (distance <= 12.0).then_some((handle, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))?;
        *drag = Some(Drag {
            handle,
            anchor: point,
            camera: view.camera,
            preview: initial,
        });
    }
    let current = drag.as_mut()?;
    if let Some(area) = current.handle.moved(initial, point - current.anchor) {
        current.preview = area;
    }
    if mouse.just_released(MouseButton::Left) {
        return drag.take().map(|d| d.preview);
    }
    if !mouse.pressed(MouseButton::Left) {
        *drag = None;
    }
    None
}

pub(super) fn draw(
    area: MotionRegion,
    pointer: &Pointer,
    gizmos: &mut Gizmos<SceneEditorBrushGizmos>,
) {
    let Some(view) = pointer.viewport.as_ref() else {
        return;
    };
    let Ok((camera, pose)) = pointer.cameras.get(view.camera) else {
        return;
    };
    let pose = GlobalTransform::from(*pose);
    for handle in Handle::ALL {
        let world = handle.position(area);
        let Ok(pixel) = camera.world_to_viewport(&pose, world) else {
            continue;
        };
        let Ok(a) = camera.viewport_to_world(&pose, pixel + Vec2::X * 6.0) else {
            continue;
        };
        let normal = InfinitePlane3d::new(pose.forward());
        let Some(t) = a.intersect_plane(world, normal) else {
            continue;
        };
        let radius = world.distance(a.get_point(t));
        gizmos.sphere(
            Isometry3d::from_translation(world),
            radius,
            Color::srgb(1.0, 0.78, 0.28),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_region_handles_preserve_opposite_edges_height_and_actions() {
        let area = MotionRegion {
            center: Vec3::new(1., 5., 3.),
            half_extents: Vec3::new(2., 3., 4.),
            enter: MotionRegionAction::Reverse,
            exit: MotionRegionAction::Pause,
        };
        for (handle, opposite, delta) in [
            (Handle::Left, Handle::Right, Vec3::NEG_X),
            (Handle::Right, Handle::Left, Vec3::X),
            (Handle::Back, Handle::Front, Vec3::NEG_Z),
            (Handle::Front, Handle::Back, Vec3::Z),
        ] {
            let next = handle.moved(area, delta).unwrap();
            assert_eq!(opposite.position(area), opposite.position(next));
            assert_eq!(handle.position(next) - handle.position(area), delta);
            assert_eq!(
                (next.center.y, next.half_extents.y, next.enter, next.exit),
                (5., 3., area.enter, area.exit)
            );
            assert!(handle.moved(area, -delta * 20.).is_none());
        }
        let translated = Handle::Center
            .moved(area, Vec3::new(4., 100., -3.))
            .unwrap();
        assert_eq!(translated.center, Vec3::new(5., 5., 0.));
        assert_eq!(translated.half_extents, area.half_extents);
    }
}
