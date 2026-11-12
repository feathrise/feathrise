//! 动画作者插件，负责 GLB 动作、时间轴、姿势编辑与有界双容器预览。

use bevy::prelude::*;
use feathrise_editor_animation_model::{
    assembly::{AnimationOperations, AnimationScenes, AnimationSystems},
    host::AnimationHost,
    resources::AnimationEditorState,
};
use feathrise_editor_animation_model::{data, resources, types};
use feathrise_editor_core::SceneEditorSystemSet;

mod binding;
mod components;
mod context;
mod control;
mod docking;
mod drive;
mod editing;
#[cfg(test)]
mod editor_tests;
mod pose;
mod presentation;
mod preview;
mod request;
mod retarget;
mod scene;
mod timeline;
mod timeline_dock;

#[cfg(feature = "runtime_test_support")]
pub mod test_support;
#[cfg(any(feature = "runtime_test_support", feature = "validation"))]
pub mod workflow_validation;

#[cfg(feature = "validation")]
pub use docking::{ProjectClipProperties, project_clip_properties, properties_panel_visible};
#[cfg(feature = "validation")]
pub use preview::{RigAnimationProbe, rig_animation_diagnostic, rig_animation_probe};
#[cfg(feature = "validation")]
pub use request::{active_project_clip, rig_animation_identity};
#[cfg(feature = "validation")]
pub use timeline_dock::{AnimationTimelineDockProbe, timeline_dock_probe};

pub struct AnimationEditorPlugin;

impl Plugin for AnimationEditorPlugin {
    fn build(&self, app: &mut App) {
        install_controls(app);
        app.init_resource::<AnimationEditorState>()
            .insert_resource(scenes())
            .insert_resource(AnimationOperations {
                select: request::select,
                save: drive::launch_save,
            })
            .add_systems(
                Update,
                drive::drive
                    .in_set(AnimationSystems::Drive)
                    .in_set(SceneEditorSystemSet::Tasks),
            )
            .add_systems(
                Update,
                (
                    scene::setup_numeric_inputs,
                    docking::sync,
                    timeline_dock::sync,
                    presentation::sync,
                )
                    .chain()
                    .in_set(AnimationSystems::Presentation)
                    .in_set(SceneEditorSystemSet::Presentation),
            );
        preview::install(app);
        binding::install(app);
        pose::install(app);
    }
}

fn install_controls(app: &mut App) {
    app.init_resource::<AnimationHost>()
        .init_resource::<timeline_dock::Layout>()
        .add_systems(
            Update,
            (timeline_dock::input, control::input, timeline::input)
                .chain()
                .in_set(AnimationSystems::Input)
                .in_set(SceneEditorSystemSet::Input),
        );
}

/// 兼容测试与实际窗口采用同一组原位挂载布局。
pub fn scenes() -> AnimationScenes {
    AnimationScenes {
        pose_toolbar: || Box::new(scene::pose_toolbar()),
        inline_panel: || {
            Box::new(bsn! {
                docking::InlineAnimationHost
                Node { width: percent(100), min_width: px(0), flex_shrink: 0.0, flex_direction: FlexDirection::Column }
                Children [@scene::panel()]
            })
        },
        timeline_dock: || Box::new(timeline_dock::scene()),
        properties_host: || Box::new(docking::properties_host()),
        binding_properties_host: || Box::new(docking::binding_properties_host()),
    }
}
