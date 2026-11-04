//! 地图摆放与手柄共用的吸附偏好；网格点对齐世界原点，固定步长保留操作起点偏移。

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditorSnapMode {
    #[default]
    WorldGrid,
    Relative,
}

impl EditorSnapMode {
    pub fn as_str(self) -> &'static str {
        match self { Self::WorldGrid => "WorldGrid", Self::Relative => "Relative" }
    }
    pub fn from_saved(value: &str) -> Self {
        match value { "Relative" => Self::Relative, _ => Self::WorldGrid }
    }
    pub fn label(self) -> &'static str {
        match self { Self::WorldGrid => "网格点", Self::Relative => "固定步长" }
    }
}

/// 位模式冻结一笔落点的参数与基准；Ctrl松开不改写已经确认的后台查询。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditorTranslationSnap {
    pub mode: EditorSnapMode,
    step: u32,
    enabled: bool,
    temporary: bool,
    origin: [u32; 3],
}

impl EditorTranslationSnap {
    pub fn active(self) -> bool { self.enabled != self.temporary }
    pub fn step(self) -> f32 { f32::from_bits(self.step) }
    pub fn origin(self) -> [f32; 3] { self.origin.map(f32::from_bits) }
    pub fn same_preferences(self, other: Self) -> bool {
        self.mode == other.mode && self.step == other.step && self.enabled == other.enabled
            && self.origin == other.origin // 临时修饰键只在点击时冻结，释放不能取消已确认落点。
    }
    pub fn axis(self, value: f32, axis: usize) -> Option<f32> {
        if !value.is_finite() || axis >= 3 { return None; }
        if !self.active() { return Some(value); }
        let origin = if self.mode == EditorSnapMode::Relative { self.origin()[axis] } else { 0.0 };
        let result = origin + ((value - origin) / self.step()).round() * self.step();
        result.is_finite().then_some(result)
    }
    /// 地表点放只吸附水平坐标；Y须由当前同层真实地形重新核验。
    pub fn horizontal(self, point: [f32; 3]) -> Option<[f32; 3]> {
        if !point[1].is_finite() { return None; }
        Some([self.axis(point[0], 0)?, point[1], self.axis(point[2], 2)?])
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditorSnapSettings {
    pub enabled: bool,
    pub show_grid: bool,
    pub mode: EditorSnapMode,
    pub translation_meters: f32,
    pub rotation_degrees: f32,
    pub scale_factor: f32,
}

impl Default for EditorSnapSettings {
    fn default() -> Self {
        Self {
            enabled: false, // 旧操作继续按Ctrl临时启用，常开由作者显式选择。
            show_grid: true,
            mode: EditorSnapMode::WorldGrid,
            translation_meters: 1.0,
            rotation_degrees: 15.0,
            scale_factor: 0.1,
        }
    }
}

impl EditorSnapSettings {
    pub fn active(self, control: bool) -> bool { self.enabled != control }
    pub fn translation_snap(self, control: bool, origin: [f32; 3]) -> EditorTranslationSnap {
        let snap = self.sanitized();
        EditorTranslationSnap {
            mode: snap.mode, step: snap.translation_meters.to_bits(), enabled: snap.enabled,
            temporary: control, origin: origin.map(|value| if value.is_finite() { value } else { 0.0 }).map(f32::to_bits),
        }
    }
    /// 旧配置缺项继承默认；非法或非有限值逐项回退，正数只限制到可操作范围。
    pub fn sanitized(self) -> Self {
        fn value(value: f32, default: f32, min: f32, max: f32) -> f32 {
            if value.is_finite() && value > 0.0 {
                value.clamp(min, max)
            } else {
                default
            }
        }
        let defaults = Self::default();
        Self {
            enabled: self.enabled,
            show_grid: self.show_grid,
            mode: self.mode,
            translation_meters: value(
                self.translation_meters,
                defaults.translation_meters,
                0.001,
                1000.0,
            ),
            rotation_degrees: value(self.rotation_degrees, defaults.rotation_degrees, 0.1, 180.0),
            scale_factor: value(self.scale_factor, defaults.scale_factor, 0.001, 1.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grid_and_increment_have_distinct_phases_and_keep_surface_height() {
        let settings = EditorSnapSettings { enabled: true, translation_meters: 1.0, ..Default::default() };
        let grid = settings.translation_snap(false, [2.3, 0.0, -1.2]);
        let relative = EditorSnapSettings { mode: EditorSnapMode::Relative, ..settings }.translation_snap(false, [2.3, 0.0, -1.2]);
        assert_eq!(grid.horizontal([2.9, -43.25, -1.8]), Some([3.0, -43.25, -2.0]));
        let point = relative.horizontal([2.9, -43.25, -1.8]).unwrap();
        assert!((point[0] - 3.3).abs() < 0.00001);
        assert!((point[2] + 2.2).abs() < 0.00001);
        assert_eq!(point[1], -43.25);
        assert!(relative.horizontal([2.9, f32::NAN, 0.0]).is_none());
    }
    #[test]
    fn control_temporarily_inverts_persistent_snap_without_changing_preferences() {
        for enabled in [false, true] {
            let settings = EditorSnapSettings { enabled, ..Default::default() };
            let clicked = settings.translation_snap(true, [0.0; 3]);
            let released = settings.translation_snap(false, [0.0; 3]);
            assert_eq!(clicked.active(), !enabled);
            assert_eq!(released.active(), enabled);
            assert!(clicked.same_preferences(released));
            assert!(!clicked.same_preferences(EditorSnapSettings { mode: EditorSnapMode::Relative, ..settings }.translation_snap(false, [0.0; 3])));
        }
    }
    #[test]
    fn unknown_saved_mode_retains_world_grid_and_relative_round_trips() {
        assert_eq!(EditorSnapMode::from_saved("unknown"), EditorSnapMode::WorldGrid);
        assert_eq!(EditorSnapMode::from_saved(EditorSnapMode::Relative.as_str()), EditorSnapMode::Relative);
    }
    #[test]
    fn snapping_sanitizes_each_field_without_changing_valid_custom_values() {
        assert_eq!(
            EditorSnapSettings {
                translation_meters: 0.25,
                rotation_degrees: 30.0,
                scale_factor: 0.05,
                ..Default::default()
            }
            .sanitized(),
            EditorSnapSettings {
                translation_meters: 0.25,
                rotation_degrees: 30.0,
                scale_factor: 0.05,
                ..Default::default()
            }
        );
        assert_eq!(
            EditorSnapSettings {
                translation_meters: f32::NAN,
                rotation_degrees: -1.0,
                scale_factor: 0.0,
                ..Default::default()
            }
            .sanitized(),
            EditorSnapSettings::default()
        );
        assert_eq!(
            EditorSnapSettings {
                translation_meters: 10000.0,
                rotation_degrees: 360.0,
                scale_factor: 2.0,
                ..Default::default()
            }
            .sanitized(),
            EditorSnapSettings {
                translation_meters: 1000.0,
                rotation_degrees: 180.0,
                scale_factor: 1.0,
                ..Default::default()
            }
        );
    }
}
