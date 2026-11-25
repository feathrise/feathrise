//! 导入角色动作复用原预览骨架映射。直接复制和明确要求的原地适配使用同一可撤销事务。

use super::{
    data::{AnimationDocument, Clip, ContentKind, ContentSpan, Keyframe},
    resources::Edit,
};
use bevy::prelude::*;
use feathrise_editor_animation_model::retarget::RigRetarget;
use std::collections::{HashMap, HashSet};

mod panel;
#[cfg(test)]
mod tests;
#[cfg(feature = "validation")]
pub(crate) use panel::original_button;
pub(super) use panel::{install, scene};

pub(super) fn import(
    target: &AnimationDocument,
    source: &AnimationDocument,
    rows: &[ContentSpan],
    adapt: bool,
) -> Result<Edit, String> {
    if rows.is_empty()
        || rows.iter().any(|s| s.kind != ContentKind::Animation)
        || target.read_only
        || source.read_only
    {
        return Err("导入动作需要选择独立 GLB 中的动画片段及可写目标".into());
    }
    let mapping = adapt
        .then(|| super::retarget::mapping(source, target))
        .transpose()?;
    let mut direct = mapping
        .is_none()
        .then(|| target.direct_clip_mapping(source));
    let target_joints: HashMap<_, _> = if mapping.is_some() {
        let (skin, _) = target.scene_skin();
        target
            .nodes
            .iter()
            .enumerate()
            .filter(|(i, _)| skin[*i])
            .map(|(i, n)| (n.name.as_str(), i))
            .collect()
    } else {
        HashMap::new()
    };
    let mut names: HashSet<_> = target.clips.iter().map(|c| c.name.clone()).collect();
    let mut clips = Vec::new();
    for item in source.resolve_content(rows)? {
        let source_clip = source.selected_clip(item.id)?;
        let mut clip = if let Some(rig) = mapping.as_ref().and_then(|m| m.retarget.as_deref()) {
            adapted_clip(target, source, source_clip, rig, &target_joints)?
        } else {
            direct
                .as_mut()
                .ok_or("来源动作的骨架映射不可用")?
                .copy(source_clip)?
        };
        let name = &source_clip.name;
        clip.name = if names.insert(name.clone()) {
            name.clone()
        } else {
            (2..)
                .map(|i| format!("{name}_{i}"))
                .find(|s| names.insert(s.clone()))
                .unwrap()
        };
        clip.source_index += clips.len();
        clips.push(clip);
    }
    Ok(Edit::ImportClips {
        position: target.clips.len(),
        clips: clips.into(),
    })
}

fn adapted_clip(
    target: &AnimationDocument,
    source: &AnimationDocument,
    clip: &Clip,
    rig: &RigRetarget,
    targets: &HashMap<&str, usize>,
) -> Result<Clip, String> {
    let mut result = target.new_imported_clip(&clip.name);
    let duration = clip.duration();
    let mut channels = HashMap::new();
    for channel in clip.channels.iter().filter(|ch| ch.path == "rotation") {
        if rig
            .corrections
            .get(channel.node)
            .and_then(Option::as_ref)
            .is_none()
        {
            return Err(format!(
                "旋转通道「{}」在目标骨架中缺少映射",
                channel.node_name
            ));
        }
        channels.insert(channel.node, channel);
    }
    if channels.is_empty() {
        return Err("所选动作没有可按预览适配的旋转通道".into());
    }
    for (i, correction) in rig.corrections.iter().enumerate() {
        let Some(correction) = correction else {
            continue;
        };
        let target_node = *targets
            .get(source.nodes[i].name.as_str())
            .ok_or("适配目标关节已改变")?;
        let name = &target.nodes[target_node].name;
        if !target.can_animate_node(target_node) {
            return Err(format!("适配目标「{name}」使用只读矩阵"));
        }
        // 左、右乘单位四元数都是四维线性变换。对三次切线用同一矩阵而不归一化，保留导数。
        let basis = [Vec4::X, Vec4::Y, Vec4::Z, Vec4::W].map(|q| {
            Vec4::from_array(
                (correction.before * Quat::from_array(q.to_array()) * correction.after).to_array(),
            )
        });
        let transform = Mat4::from_cols(basis[0], basis[1], basis[2], basis[3]);
        if let Some(channel) = channels.get(&i) {
            let mut channel = (*channel).clone();
            channel.node = target_node;
            channel.node_name.clone_from(name);
            for key in &mut channel.keys {
                for (values, is_rotation) in [
                    (&mut key.value, true),
                    (&mut key.in_tangent, false),
                    (&mut key.out_tangent, false),
                ] {
                    if values.is_empty() {
                        continue;
                    }
                    let value = transform * Vec4::from_slice(values);
                    values.copy_from_slice(
                        &if is_rotation {
                            value.normalize()
                        } else {
                            value
                        }
                        .to_array(),
                    );
                }
            }
            if let Some(last) = channel.keys.last_mut().filter(|key| key.time < duration) {
                last.out_tangent.fill(0.);
                let mut hold = last.clone();
                hold.in_tangent.fill(0.);
                hold.time = duration;
                channel.keys.push(hold);
            }
            result.push_existing_channel(channel)?;
        } else {
            // 预览也校正静止关节。写入常量旋转，独立播放器不再依赖这套映射。
            let rotation = (transform * Vec4::from_array(source.nodes[i].rotation)).normalize();
            let key = Keyframe {
                time: 0.,
                value: rotation.to_array().into(),
                in_tangent: vec![],
                out_tangent: vec![],
            };
            result.push_transform_channel(target_node, name, "rotation", vec![key])?;
        }
    }
    Ok(result)
}
