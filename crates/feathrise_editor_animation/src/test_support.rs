//! 场景宿主的交互夹具借用原动画系统与控件身份。

use bevy::{ecs::system::IntoSystem, prelude::*};
use std::any::TypeId;

pub use crate::{
    components::{
        AnimationAction, AnimationButton, AnimationChannelButton, AnimationKeyButton,
        AnimationNumericField, AnimationNumericInput, AnimationPanel, AnimationPoseToolbar,
        AnimationTrack,
    },
    editing::copy,
    scene::{panel, pose_toolbar},
};

pub fn install_controls(app: &mut App) {
    crate::install_controls(app);
}

pub fn timeline_input_type() -> TypeId {
    crate::timeline::input.system_type_id()
}

pub fn install_ui(app: &mut App) {
    app.add_systems(
        Update,
        (
            crate::scene::setup_numeric_inputs,
            crate::presentation::sync,
        )
            .chain(),
    );
}
