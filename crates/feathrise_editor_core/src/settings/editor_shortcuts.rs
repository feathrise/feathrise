//! 地图工作区的快捷键契约：精确修饰键、冲突校验及与设置文件往返的稳定名称。
//!
//! 【功能设计】只配置地图操作；文档编辑、鼠标手势和全局功能键保留各自输入所有权。
//! 固定动作的检查不随地图规模增长，左右修饰键等价，额外修饰键不会误触发裸键动作。

use std::collections::BTreeMap;

use bevy::prelude::{ButtonInput, KeyCode};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(usize)]
pub enum EditorShortcutAction {
    #[default]
    FocusSelected,
    FrameMap,
    Save,
    Undo,
    Redo,
    DecreaseRadius,
    IncreaseRadius,
    DecreaseStrength,
    IncreaseStrength,
    MoveSelected, // 追加在旧动作之后，保持旧配置与索引的解释顺序。
    RotateSelected,
    ScaleSelected,
    ToggleSnapping,
    AddObjectMenu, // 追加保持已有数组索引；默认采用Blender的Shift+A。
}

impl EditorShortcutAction {
    pub const ALL: [Self; 14] = [
        Self::FocusSelected,
        Self::FrameMap,
        Self::Save,
        Self::Undo,
        Self::Redo,
        Self::DecreaseRadius,
        Self::IncreaseRadius,
        Self::DecreaseStrength,
        Self::IncreaseStrength,
        Self::MoveSelected,
        Self::RotateSelected,
        Self::ScaleSelected,
        Self::ToggleSnapping,
        Self::AddObjectMenu,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::FocusSelected => "focus_selected",
            Self::FrameMap => "frame_map",
            Self::Save => "save_map",
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::DecreaseRadius => "decrease_radius",
            Self::IncreaseRadius => "increase_radius",
            Self::DecreaseStrength => "decrease_strength",
            Self::IncreaseStrength => "increase_strength",
            Self::MoveSelected => "move_selected",
            Self::RotateSelected => "rotate_selected",
            Self::ScaleSelected => "scale_selected",
            Self::ToggleSnapping => "toggle_snapping",
            Self::AddObjectMenu => "add_object_menu",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::FocusSelected => "聚焦所选",
            Self::FrameMap => "查看全图",
            Self::Save => "保存地图",
            Self::Undo => "撤销",
            Self::Redo => "重做",
            Self::DecreaseRadius => "缩小笔刷半径",
            Self::IncreaseRadius => "放大笔刷半径",
            Self::DecreaseStrength => "降低笔刷强度",
            Self::IncreaseStrength => "提高笔刷强度",
            Self::MoveSelected => "移动模式",
            Self::RotateSelected => "旋转模式",
            Self::ScaleSelected => "缩放模式",
            Self::ToggleSnapping => "切换吸附",
            Self::AddObjectMenu => "添加网格",
        }
    }
}

// 显式白名单同时承担保存名称与显示名称；功能键、Esc、删除和文本控制键属于固定入口。
macro_rules! supported_keys {
    ($($key:ident => $label:literal),* $(,)?) => {
        const SUPPORTED_KEYS: &[(KeyCode, &str, &str)] = &[
            $((KeyCode::$key, stringify!($key), $label)),*
        ];
    };
}
supported_keys! {
    KeyA => "A", KeyB => "B", KeyC => "C", KeyD => "D", KeyE => "E", KeyF => "F",
    KeyG => "G", KeyH => "H", KeyI => "I", KeyJ => "J", KeyK => "K", KeyL => "L",
    KeyM => "M", KeyN => "N", KeyO => "O", KeyP => "P", KeyQ => "Q", KeyR => "R",
    KeyS => "S", KeyT => "T", KeyU => "U", KeyV => "V", KeyW => "W", KeyX => "X",
    KeyY => "Y", KeyZ => "Z",
    Digit0 => "0", Digit1 => "1", Digit2 => "2", Digit3 => "3", Digit4 => "4",
    Digit5 => "5", Digit6 => "6", Digit7 => "7", Digit8 => "8", Digit9 => "9",
    Numpad0 => "小键盘0", Numpad1 => "小键盘1", Numpad2 => "小键盘2",
    Numpad3 => "小键盘3", Numpad4 => "小键盘4", Numpad5 => "小键盘5",
    Numpad6 => "小键盘6", Numpad7 => "小键盘7", Numpad8 => "小键盘8", Numpad9 => "小键盘9",
    NumpadAdd => "小键盘加", NumpadSubtract => "小键盘减", NumpadMultiply => "小键盘乘",
    NumpadDivide => "小键盘除", NumpadDecimal => "小键盘点",
    BracketLeft => "[", BracketRight => "]", Minus => "-", Equal => "=",
    Comma => ",", Period => ".", Slash => "/", Backslash => "\\",
    Semicolon => ";", Quote => "'", Backquote => "`",
    Space => "Space", Home => "Home", End => "End", Insert => "Insert",
    PageUp => "PageUp", PageDown => "PageDown", ArrowUp => "↑", ArrowDown => "↓",
    ArrowLeft => "←", ArrowRight => "→",
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditorKeyBinding {
    pub key: KeyCode,
    pub control: bool,
    pub shift: bool,
    pub alt: bool,
}

impl EditorKeyBinding {
    pub const fn plain(key: KeyCode) -> Self {
        Self {
            key,
            control: false,
            shift: false,
            alt: false,
        }
    }

    pub fn from_input(key: KeyCode, input: &ButtonInput<KeyCode>) -> Self {
        Self {
            key,
            control: input.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]),
            shift: input.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]),
            alt: input.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]),
        }
    }

    pub fn is_supported(self) -> bool {
        SUPPORTED_KEYS.iter().any(|&(key, _, _)| key == self.key)
    }

    pub fn is_modifier(key: KeyCode) -> bool {
        matches!(
            key,
            KeyCode::ControlLeft
                | KeyCode::ControlRight
                | KeyCode::ShiftLeft
                | KeyCode::ShiftRight
                | KeyCode::AltLeft
                | KeyCode::AltRight
                | KeyCode::SuperLeft
                | KeyCode::SuperRight
        )
    }

    pub fn just_pressed(self, input: &ButtonInput<KeyCode>) -> bool {
        input.just_pressed(self.key)
            && !input.any_pressed([KeyCode::SuperLeft, KeyCode::SuperRight])
            && Self::from_input(self.key, input) == self
    }

    fn formatted(self, saved: bool) -> String {
        let mut result = String::new();
        if self.control {
            result.push_str("Ctrl+");
        }
        if self.shift {
            result.push_str("Shift+");
        }
        if self.alt {
            result.push_str("Alt+");
        }
        if let Some(&(_, name, label)) = SUPPORTED_KEYS.iter().find(|&&(key, _, _)| key == self.key)
        {
            result.push_str(if saved { name } else { label });
        }
        result
    }

    pub fn display(self) -> String {
        self.formatted(false)
    }

    fn from_saved(value: &str) -> Option<Self> {
        let mut parts = value.split('+');
        let mut binding = Self::plain(KeyCode::KeyA);
        let mut key = None;
        for part in &mut parts {
            match part {
                "Ctrl" if !binding.control && key.is_none() => binding.control = true,
                "Shift" if !binding.shift && key.is_none() => binding.shift = true,
                "Alt" if !binding.alt && key.is_none() => binding.alt = true,
                name if key.is_none() => {
                    key = SUPPORTED_KEYS
                        .iter()
                        .find(|&&(_, saved, _)| saved == name)
                        .map(|&(key, _, _)| key);
                    key?;
                }
                _ => return None,
            }
        }
        binding.key = key?;
        Some(binding)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorShortcutConflict {
    ReservedKey,
    InUse(EditorShortcutAction),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorKeyBindings {
    bindings: [Option<EditorKeyBinding>; 14],
}

impl Default for EditorKeyBindings {
    fn default() -> Self {
        use KeyCode::*;
        let control = |key| {
            Some(EditorKeyBinding {
                control: true,
                ..EditorKeyBinding::plain(key)
            })
        };
        Self {
            bindings: [
                Some(EditorKeyBinding::plain(KeyF)),
                Some(EditorKeyBinding::plain(Home)),
                control(KeyS),
                control(KeyZ),
                control(KeyY),
                Some(EditorKeyBinding::plain(BracketLeft)),
                Some(EditorKeyBinding::plain(BracketRight)),
                Some(EditorKeyBinding::plain(Minus)),
                Some(EditorKeyBinding::plain(Equal)),
                Some(EditorKeyBinding::plain(KeyW)),
                Some(EditorKeyBinding::plain(KeyE)),
                Some(EditorKeyBinding::plain(KeyR)),
                Some(EditorKeyBinding::plain(KeyY)),
                Some(EditorKeyBinding { key: KeyA, control: false, shift: true, alt: false }),
            ],
        }
    }
}

impl EditorKeyBindings {
    pub fn get(&self, action: EditorShortcutAction) -> Option<EditorKeyBinding> {
        self.bindings[action as usize]
    }

    pub fn assign(
        &mut self,
        action: EditorShortcutAction,
        binding: Option<EditorKeyBinding>,
    ) -> Result<(), EditorShortcutConflict> {
        if let Some(binding) = binding {
            if !binding.is_supported() {
                return Err(EditorShortcutConflict::ReservedKey);
            }
            if let Some(other) = EditorShortcutAction::ALL
                .into_iter()
                .find(|&other| other != action && self.get(other) == Some(binding))
            {
                return Err(EditorShortcutConflict::InUse(other));
            }
        }
        self.bindings[action as usize] = binding;
        Ok(())
    }

    pub fn pressed(&self, action: EditorShortcutAction, input: &ButtonInput<KeyCode>) -> bool {
        self.get(action)
            .is_some_and(|binding| binding.just_pressed(input))
    }

    pub fn to_saved(&self) -> BTreeMap<String, Option<String>> {
        EditorShortcutAction::ALL
            .into_iter()
            .map(|action| {
                (
                    action.name().into(),
                    self.get(action).map(|binding| binding.formatted(true)),
                )
            })
            .collect()
    }

    pub fn from_saved(data: &BTreeMap<String, Option<String>>) -> Self {
        let mut result = Self::default();
        for action in EditorShortcutAction::ALL {
            match data.get(action.name()) {
                Some(None) => result.bindings[action as usize] = None, // 显式解绑与旧文件缺字段区分。
                Some(Some(value)) => {
                    if let Some(binding) = EditorKeyBinding::from_saved(value) {
                        result.bindings[action as usize] = Some(binding);
                    } // 未知或保留按键只回退本项，不丢弃其他显示偏好或合法绑定。
                }
                None => {}
            }
        }
        for action in EditorShortcutAction::ALL {
            let index = action as usize;
            if result.bindings[index].is_some()
                && result.bindings[..index].contains(&result.bindings[index])
            {
                let fallback = Self::default().get(action);
                result.bindings[index] = if fallback.is_some()
                    && !result
                        .bindings
                        .iter()
                        .enumerate()
                        .any(|(i, binding)| i != index && *binding == fallback)
                {
                    fallback
                } else {
                    None
                }; // 手改重复配置按固定动作顺序保留首项，不能一次按键执行两个动作。
            }
        }
        result
    }
}

#[cfg(test)]
#[path = "editor_shortcuts_tests.rs"]
mod tests;
