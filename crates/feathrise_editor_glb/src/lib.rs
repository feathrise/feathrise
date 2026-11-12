//! GLB 人形绑骨和蒙皮权重的后台实现，由应用入口与场景库并列装配。

use bevy::prelude::*;

mod files;
mod rigging;
mod weights;

pub struct GlbEditorPlugin;

impl Plugin for GlbEditorPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(rigging::operations())
            .insert_resource(weights::operations());
    }
}
