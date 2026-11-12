//! 原动画控件和真实模型预览的有限桥；只投递既有输入契约，不改文档、相机或Assets。

mod types;
pub use types::*;

use super::{components::*, data::AnimationDocument, resources::AnimationEditorState};
use feathrise_editor_animation_model::{host::{AnimationHost, SceneEditorNumericScrubMouseGuard}, media::{AssetPreviewContent, AssetPreviewPhase, AssetPreviewState}};
use feathrise_editor_core::EditorTextInputGuard;
use bevy::{mesh::VertexAttributeValues, prelude::*};
use feathrise_editor_ui::{
    EditorButtonClick, NumericInputCommitOutcome, NumericInputCommitSource, NumericInputCommitted,
    NumericValue,
};
use serde_json::{Value, json};
use std::path::Path;

fn enabled() -> bool {
    std::env::var("NEW_EDITOR_NATIVE_SMOKE_AUTHOR_TOOLS").as_deref() == Ok("1")
        && std::env::var_os("NEW_EDITOR_NATIVE_SMOKE_DIR").is_some()
        && std::env::var("NEW_EDITOR_NATIVE_SMOKE_SAVE").as_deref() == Ok("1")
}

#[cfg(feature = "validation")]
pub fn timeline_ruler(world: &mut World) -> Option<(Entity, f32)> {
    let state = world.get_resource::<AnimationEditorState>()?;
    if !state.visible() || state.busy() {
        return None;
    }
    let session = state.current()?;
    let fps = state.fps as f32;
    let midpoint = ((session.window_start + session.window_span * 0.5) * fps).round() / fps;
    let binding = state.revision;
    // 返回真实可见帧尺。验收随后仍通过窗口事件定位并读取已发布姿态。
    world
        .query::<(Entity, &AnimationTrack, &ComputedNode, &InheritedVisibility)>()
        .iter(world)
        .find_map(|(entity, track, node, visibility)| {
            (track.row.is_none()
                && track.binding == binding
                && node.size().min_element() > 1.0
                && visibility.get())
                .then_some((entity, midpoint))
        })
}

pub fn animation_probe(
    world: &World,
    source: &Path,
    identity: Option<KeyIdentity>,
) -> Result<Option<AnimationProbe>, String> {
    let state = world.resource::<AnimationEditorState>();
    if state.failed.as_deref() == Some(source) && state.work.is_none() {
        return Err(format!("原动画加载失败：{}", state.status));
    }
    if state.source() != Some(source) || !state.visible() || state.busy() {
        return Ok(None);
    }
    let Some(session) = state.current() else {
        return Ok(None);
    };
    let identity = match identity {
        Some(identity) => identity,
        None => {
            let channel = session
                .document
                .clips
                .first()
                .and_then(|clip| {
                    clip.channels.iter().position(|channel| {
                        channel.path == "translation" && !channel.keys.is_empty()
                    })
                })
                .ok_or("本工程待机动画没有可编辑平移关键帧")?;
            KeyIdentity {
                clip: 0,
                channel,
                key: 0,
            }
        }
    };
    let channel = session
        .document
        .clips
        .get(identity.clip)
        .and_then(|clip| clip.channels.get(identity.channel))
        .ok_or("动画验收通道身份已变化")?;
    let key = channel
        .keys
        .get(identity.key)
        .ok_or("动画验收关键帧已变化")?;
    Ok(Some(AnimationProbe {
        key_identity: identity,
        key: KeyProof {
            time: key.time,
            value: key.value.clone(),
            in_tangent: key.in_tangent.clone(),
            out_tangent: key.out_tangent.clone(),
        },
        revision: state.revision,
        cursor: session.cursor,
        saved_cursor: session.saved_cursor,
        history_len: session.history.len(),
        dirty: session.dirty(),
        actual_clip: session.clip,
        actual_channel: session.channel,
        actual_key: session.key,
        position: session.position,
        channel_selected: session.clip == identity.clip
            && session.channel == Some(identity.channel),
        key_selected: session.key == Some(identity.key) && session.position == key.time,
        save_receipt: state
            .project_save_receipt
            .as_ref()
            .filter(|(path, _)| path == source)
            .map(|(_, result)| result.clone()),
    }))
}

pub fn select_channel(world: &mut World, identity: KeyIdentity) -> Result<Entity, String> {
    if !enabled() {
        return Err("动画有限验收未启用".into());
    }
    let binding = world.resource::<AnimationEditorState>().revision;
    Ok(world
        .spawn((
            AnimationChannelButton {
                slot: 0,
                binding,
                index: Some(identity.channel),
            },
            EditorButtonClick::triggered(),
        ))
        .id())
}

pub fn command(world: &mut World, command: AnimationCommand) -> Result<Entity, String> {
    if !enabled() {
        return Err("动画有限验收未启用".into());
    }
    let binding = world.resource::<AnimationEditorState>().revision;
    let action = match command {
        AnimationCommand::NextKey => AnimationAction::NextKey,
        AnimationCommand::Undo => AnimationAction::Undo,
        AnimationCommand::Redo => AnimationAction::Redo,
        AnimationCommand::Save => AnimationAction::Save,
    };
    Ok(world
        .spawn((
            AnimationButton { action, binding },
            EditorButtonClick::triggered(),
        ))
        .id())
}

pub fn commit_x(world: &mut World, value: f32) -> Result<(), String> {
    if !enabled() {
        return Err("动画有限验收未启用".into());
    }
    let binding = world.resource::<AnimationEditorState>().revision;
    let entity = world
        .query::<(Entity, &AnimationNumericInput)>()
        .iter(world)
        .find_map(|(entity, field)| {
            (field.binding == binding && matches!(field.field, AnimationNumericField::Value(0)))
                .then_some(entity)
        })
        .ok_or("原动画X数值控件尚未生成或已换代")?;
    // 该消息就是原公共数值控件接受后的契约；不冒充物理键盘/鼠标，也不直接写入曲线。
    world.write_message(NumericInputCommitted {
        entity,
        value: NumericValue::F32(value),
        outcome: NumericInputCommitOutcome::Accepted,
        source: NumericInputCommitSource::Text,
        changed: true,
        edited: true,
    });
    Ok(())
}

/// 仅后台IO任务调用；复用原GLB解析器核对真实落盘关键帧，不用当前内存草稿代替保存。
pub fn read_saved(source: &Path, identity: KeyIdentity) -> Result<Value, String> {
    let document = AnimationDocument::load(source)?;
    let channel = document
        .clips
        .get(identity.clip)
        .and_then(|clip| clip.channels.get(identity.channel))
        .ok_or("保存动画缺少原通道")?;
    let key = channel
        .keys
        .get(identity.key)
        .ok_or("保存动画缺少原关键帧")?;
    Ok(
        json!({"time":key.time,"value":key.value,"in_tangent":key.in_tangent,"out_tangent":key.out_tangent}),
    )
}

/// 只检查自有四顶点probe的真实预览后代；固定32节点预算，不扫描场景或改造加载产物。
pub fn model_probe(world: &World, path: &str) -> Result<Option<ModelProbe>, String> {
    let state = world.resource::<AssetPreviewState>();
    if state.selected_path.as_deref() != Some(path) {
        return Ok(None);
    }
    if matches!(
        state.content,
        AssetPreviewContent::Model {
            phase: AssetPreviewPhase::Failed,
            ..
        }
    ) {
        return Err(format!("原模型预览失败：{}", state.status));
    }
    if !matches!(
        state.content,
        AssetPreviewContent::Model {
            phase: AssetPreviewPhase::Ready,
            ..
        }
    ) {
        return Ok(None);
    }
    let Some(model) = state.model.as_ref().filter(|model| model.hot_reload_armed) else {
        return Ok(None);
    };
    let Some(root) = state.current_model_root() else {
        return Ok(None);
    };
    let mut stack = vec![root];
    let mut found = None;
    for _ in 0..32 {
        let Some(entity) = stack.pop() else {
            break;
        };
        if let Some(children) = world.get::<Children>(entity) {
            stack.extend(children.iter());
        }
        if let (Some(mesh), Some(material)) = (
            world.get::<Mesh3d>(entity),
            world.get::<MeshMaterial3d<StandardMaterial>>(entity),
        ) {
            if found.replace((entity, mesh, material)).is_some() {
                return Err("热更probe意外含多个网格，拒绝猜测目标".into());
            }
        }
    }
    if !stack.is_empty() {
        return Err("热更probe超出32节点有限范围".into());
    }
    let Some((entity, mesh_handle, material_handle)) = found else {
        return Ok(None);
    };
    let meshes = world.resource::<Assets<Mesh>>();
    let materials = world.resource::<Assets<StandardMaterial>>();
    let images = world.resource::<Assets<Image>>();
    let Some(mesh) = meshes.get(&mesh_handle.0) else {
        return Ok(None);
    };
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return Err("热更probe缺少Float32x3顶点".into());
    };
    if positions.len() != 4 {
        return Err("热更probe必须仍为原四顶点网格".into());
    }
    let Some(material) = materials.get(&material_handle.0) else {
        return Ok(None);
    };
    let Some(image_handle) = material.base_color_texture.as_ref() else {
        return Err("热更probe原材质贴图引用丢失".into());
    };
    let Some(image) = images.get(image_handle) else {
        return Ok(None);
    };
    let Some(data) = image.data.as_ref().filter(|data| data.len() >= 4) else {
        return Err("热更probe贴图没有可核对的真实CPU像素".into());
    };
    let color = material.base_color.to_linear();
    Ok(Some(ModelProbe {
        generation: model.generation,
        root: format!("{root:?}"),
        mesh_entity: format!("{entity:?}"),
        mesh_id: format!("{:?}", mesh_handle.0.id()),
        material_id: format!("{:?}", material_handle.0.id()),
        image_id: format!("{:?}", image_handle.id()),
        positions: positions.clone(),
        base_color_linear: [color.red, color.green, color.blue, color.alpha],
        roughness: material.perceptual_roughness,
        image_size: [image.width(), image.height()],
        first_pixel: data[..4].to_vec(),
    }))
}

pub fn diagnostic(world: &mut World) -> String {
    diagnostic_value(world).to_string()
}

/// 读取原输入所有者；前一按钮受理后的释放门也属于忙碌，不把下一动作投进必被丢弃的一帧。
pub fn input_ready(world: &World) -> bool {
    let animation = world.resource::<AnimationEditorState>();
    animation.visible()
        && !animation.busy()
        && !world
            .get_resource::<SceneEditorNumericScrubMouseGuard>()
            .is_some_and(|guard| guard.blocked())
        && !world
            .get_resource::<feathrise_editor_ui::ActiveNumericInput>()
            .is_some_and(|state| state.entity.is_some())
        && !world
            .get_resource::<feathrise_editor_ui::ActiveNumericInputScrub>()
            .is_some_and(|state| state.entity.is_some())
        && !world
            .get_resource::<AnimationHost>()
            .is_some_and(|state| state.scene_dragging)
        && !world
            .get_resource::<EditorTextInputGuard>()
            .is_some_and(|guard| guard.block_shortcuts)
}

/// 保存真实选择和输入门；脉冲前后由夹具记录，失败日志不能只剩“已载入动画”。
pub fn diagnostic_value(world: &mut World) -> Value {
    let windows = world
        .query_filtered::<(Entity, &Window), With<bevy::window::PrimaryWindow>>()
        .iter(world)
        .take(2)
        .map(|(entity, window)| json!({"entity":format!("{entity:?}"),"focused":window.focused}))
        .collect::<Vec<_>>();
    let animation = world.resource::<AnimationEditorState>();
    let preview = world.resource::<AssetPreviewState>();
    let session = animation.current();
    let focus = world
        .get_resource::<bevy::input_focus::InputFocus>()
        .and_then(|focus| focus.get());
    json!({
        "source":animation.source(),"visible":animation.visible(),"busy":animation.busy(),"revision":animation.revision,
        "status":animation.status,"clip":session.map(|session|session.clip),"channel":session.and_then(|session|session.channel),
        "key":session.and_then(|session|session.key),"position":session.map(|session|session.position),"cursor":session.map(|session|session.cursor),
        "pending":animation.pending.is_some(),"work":animation.work.as_ref().map(|work|json!({"path":work.path,"saving":work.saving})),
        "mouse_owned":animation.owns_mouse(),"numeric_mouse_blocked":world.get_resource::<SceneEditorNumericScrubMouseGuard>().is_some_and(|guard|guard.blocked()),
        "active_numeric":world.get_resource::<feathrise_editor_ui::ActiveNumericInput>().and_then(|state|state.entity).map(|entity|format!("{entity:?}")),
        "active_scrub":world.get_resource::<feathrise_editor_ui::ActiveNumericInputScrub>().and_then(|state|state.entity).map(|entity|format!("{entity:?}")),
        "text_guard":world.get_resource::<EditorTextInputGuard>().is_some_and(|guard|guard.block_shortcuts),
        "focus":focus.map(|entity|format!("{entity:?}")),"focus_is_text":focus.is_some_and(|entity|world.get::<bevy::text::EditableText>(entity).is_some()),
        "preview_selected":preview.selected_path,"preview_status":preview.status,"input_ready":input_ready(world),"primary_windows":windows
    })
}
