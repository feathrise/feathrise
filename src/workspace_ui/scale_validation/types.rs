//! 首页缩放专项的固定动作与磁盘快照；这些类型不进入普通编辑器会话。
use serde::Serialize;
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode { Exercise, Reopen }
impl Mode {
    pub fn name(self) -> &'static str { match self { Self::Exercise => "exercise", Self::Reopen => "reopen" } }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Target { Open, Scale(u16), Auto, Plus, Minus, Close }
impl Target {
    pub fn label(self) -> String {
        match self {
            Self::Open => "设置".into(), Self::Scale(value) => format!("{value}%"), Self::Auto => "自动".into(),
            Self::Plus => "+".into(), Self::Minus => "−".into(), Self::Close => "完成".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub(super) struct Values { pub manual: f32, pub automatic: bool, pub offset: f32 }
impl Values {
    pub fn manual(value: f32) -> Self { Self { manual: value, automatic: false, offset: 0. } }
    pub fn same(self, other: Self) -> bool {
        (self.manual - other.manual).abs() < 0.001 && self.automatic == other.automatic && (self.offset - other.offset).abs() < 0.001
    }
}

#[derive(Clone, Copy)]
pub(super) struct Step {
    pub name: &'static str, pub target: Option<Target>, pub values: Values, pub settings_open: bool,
    pub no_write: bool, pub resize: Option<(f32, f32)>, pub capture: bool,
}

pub(super) fn step(mode: Mode, index: usize) -> Option<Step> {
    let manual = |value| Values::manual(value);
    let auto = |offset| Values { manual: 1.5, automatic: true, offset };
    let mut result = Step { name: "", target: None, values: manual(1.), settings_open: true,
                            no_write: false, resize: None, capture: true };
    match (mode, index) {
        (Mode::Reopen, 0) => { result.name = "reopened-before-first-input"; result.values = manual(1.25); result.settings_open = false; }
        (Mode::Reopen, 1) => { result.name = "reopened-settings"; result.target = Some(Target::Open); result.values = manual(1.25); result.no_write = true; }
        (Mode::Reopen, 2) => { result.name = "reopened-complete"; result.target = Some(Target::Close); result.values = manual(1.25); result.settings_open = false; result.no_write = true; }
        (Mode::Exercise, 0) => { result.name = "home-baseline"; result.settings_open = false; }
        (Mode::Exercise, 1) => { result.name = "settings-open"; result.target = Some(Target::Open); result.no_write = true; }
        (Mode::Exercise, 2..=5) => {
            let percent = [75, 100, 125, 150][index - 2];
            result.name = ["manual-75", "manual-100", "manual-125", "manual-150"][index - 2];
            result.target = Some(Target::Scale(percent)); result.values = manual(percent as f32 / 100.);
        }
        (Mode::Exercise, 6) => { result.name = "automatic"; result.target = Some(Target::Auto); result.values = auto(0.); }
        (Mode::Exercise, 7) => { result.name = "automatic-plus"; result.target = Some(Target::Plus); result.values = auto(0.05); }
        (Mode::Exercise, 8) => { result.name = "automatic-minus"; result.target = Some(Target::Minus); result.values = auto(0.); }
        (Mode::Exercise, 9) => { result.name = "automatic-resized"; result.values = auto(0.); result.no_write = true; result.resize = Some((1440., 900.)); }
        (Mode::Exercise, 10) => { result.name = "manual-restored-125"; result.target = Some(Target::Scale(125)); result.values = manual(1.25); }
        (Mode::Exercise, 11) => { result.name = "manual-plus-130"; result.target = Some(Target::Plus); result.values = manual(1.30); }
        (Mode::Exercise, 12) => { result.name = "manual-minus-125"; result.target = Some(Target::Minus); result.values = manual(1.25); }
        (Mode::Exercise, 13) => { result.name = "repeat-125-no-write"; result.target = Some(Target::Scale(125)); result.values = manual(1.25); result.no_write = true; result.capture = false; }
        (Mode::Exercise, 14) => { result.name = "complete-no-write"; result.target = Some(Target::Close); result.values = manual(1.25); result.settings_open = false; result.no_write = true; }
        (Mode::Exercise, 15) => { result.name = "idle-no-write"; result.values = manual(1.25); result.settings_open = false; result.no_write = true; result.capture = false; }
        _ => return None,
    }
    Some(result)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FileStamp { pub bytes: Vec<u8>, pub modified: SystemTime, pub sha256: String }
