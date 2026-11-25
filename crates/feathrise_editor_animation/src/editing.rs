//! 框选、移动、复制和姿势插帧的后台事务；整组冲突检查通过后才发布一份新文档。

pub(super) use super::types::{Clipboard, Command};
use super::{data::*, resources::Edit, types::{KeySelection, PoseProperty}};
use bevy::prelude::*;
use std::{collections::HashSet, path::PathBuf, sync::Arc};

pub fn copy(
    document: &AnimationDocument,
    clip: usize,
    selection: &KeySelection,
    source: PathBuf,
) -> Result<Arc<Clipboard>, String> {
    let clip = document.clips.get(clip).ok_or("动作已改变")?;
    let keys: Vec<_> = clip
        .channels
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let keys: Vec<_> = c
                .keys
                .iter()
                .filter(|k| selection.contains(i, k.time))
                .cloned()
                .collect();
            (!keys.is_empty()).then(|| (c.node, c.path.clone(), keys))
        })
        .collect();
    let origin = keys
        .iter()
        .flat_map(|(_, _, keys)| keys.iter().map(|k| k.time))
        .reduce(f32::min)
        .ok_or("请先选择关键帧")?;
    Ok(Arc::new(Clipboard {
        source,
        source_identity: document.source_identity.clone(),
        origin,
        keys,
    }))
}

pub(super) fn build(document: &AnimationDocument, command: Command) -> Result<Edit, String> {
    let clip_position = |id| document.clips.binary_search_by_key(&id, |clip| clip.source_index)
        .map_err(|_| "动画片段已改变，请重新选择".to_owned());
    let clip = match &command {
        Command::ImportClips { source, rows, adapt } => return super::composition::import(document, source, rows, *adapt),
        Command::CreateClip => return document.create_clip().map(|clip| Edit::AppendClip(Arc::new(clip))),
        Command::RenameClip { id, name } => {
            let clip = clip_position(*id)?;
            let before = document.clips[clip].name.clone();
            let after = document.renamed_clip_name(*id, name)?;
            if before == after {
                return Err("动画名称没有改变".into());
            }
            return Ok(Edit::RenameClip { clip, id: *id, before, after });
        }
        Command::DuplicateClip { id } => {
            let snapshot = document.duplicate_clip(clip_position(*id)?)?;
            return Ok(Edit::ClipSlot { position: document.clips.len(), snapshot, insert_forward: true });
        }
        Command::DeleteClip { id } => {
            let position = clip_position(*id)?;
            let snapshot = document.snapshot_clip(position)?;
            return Ok(Edit::ClipSlot { position, snapshot, insert_forward: false });
        }
        Command::Copy { clip, .. }
        | Command::Move { clip, .. }
        | Command::Delete { clip, .. }
        | Command::Paste { clip, .. }
        | Command::Pose { clip, .. }
        | Command::CapturePose { clip, .. } => *clip,
    };
    let original = document.clips.get(clip).ok_or("动作已改变")?;
    if let Command::Pose { draft, .. } = &command
        && draft.iter().any(|(node, _)| !document.can_animate_node(node))
    {
        return Err("目标骨骼的变换不可写入动画，整组姿势未插入".into());
    }
    if let Command::Paste { clipboard, .. } = &command {
        // 同路径重导出也可能重新编号骨骼；后台入口独立拒绝旧命令，不能仅靠清空UI剪贴板。
        if !Arc::ptr_eq(&document.source_identity, &clipboard.source_identity) {
            return Err("动画来源已重新载入，请重新复制关键帧后再粘贴".into());
        }
        if clipboard.keys.iter().any(|(node, path, _)| {
            !original
                .channels
                .iter()
                .any(|c| c.node == *node && c.path == *path)
        }) {
            return Err("当前动作缺少剪贴板中的通道，粘贴已取消".into());
        }
    }
    let mut values = Vec::new();
    for (index, channel) in original.channels.iter().enumerate() {
        if let Command::Pose { draft, .. } = &command
            && !PoseProperty::from_path(&channel.path).is_some_and(|property|
                draft.get(channel.node).is_some_and(|pose| pose.contains(property)))
        {
            continue; // 批量插帧只复制本次实际修改的曲线。
        }
        let mut after = channel.clone();
        match &command {
            Command::Move {
                selection, delta, ..
            } => {
                if !delta.is_finite() {
                    return Err("移动时间必须有限".into());
                }
                for key in &mut after.keys {
                    if selection.contains(index, key.time) {
                        key.time += delta;
                    }
                }
                // 先整体移动再排序和验冲突，允许相邻选中键一起平移，禁止覆盖未选中的键。
                after.keys.sort_by(|a, b| a.time.total_cmp(&b.time));
                if after.keys.iter().any(|k| k.time < 0.0) {
                    return Err("选区不能移到第0帧之前".into());
                }
                if after
                    .keys
                    .windows(2)
                    .any(|w| (w[1].time - w[0].time).abs() < 1e-6)
                {
                    return Err("目标帧已有未选中关键帧，移动已取消".into());
                }
            }
            Command::Delete { selection, .. } => {
                after.keys.retain(|k| !selection.contains(index, k.time))
            }
            Command::Paste {
                source,
                clipboard,
                at,
                ..
            } => {
                if *source != clipboard.source {
                    return Err("剪贴板只支持同一动画文件的动作，避免误写不同骨架".into());
                }
                if let Some((_, _, keys)) = clipboard
                    .keys
                    .iter()
                    .find(|(node, path, _)| *node == channel.node && *path == channel.path)
                {
                    for key in keys {
                        let mut key = key.clone();
                        key.time = key.time - clipboard.origin + at;
                        if after.keys.iter().any(|k| (k.time - key.time).abs() < 1e-6) {
                            return Err("粘贴位置已有关键帧，请移动播放头后重试".into());
                        }
                        after.keys.push(key);
                    }
                    after.keys.sort_by(|a, b| a.time.total_cmp(&b.time));
                }
            }
            Command::Pose { draft, time, .. } => {
                let transform = draft.get(channel.node).expect("已核对改动通道").transform;
                let property = PoseProperty::from_path(&channel.path).expect("已核对改动属性");
                // 精确秒值决定已有键的身份，近邻的合法关键帧仍各自保留。
                after.insert_key(*time, pose_key(channel, transform, property, *time))?;
            }
            Command::CapturePose { node, time, .. } if channel.node == *node => {
                after.insert_key(*time, channel.sample(*time))?;
            }
            _ => {}
        }
        if after.keys != channel.keys {
            after.validate()?;
            values.push((index, channel.keys.clone(), after.keys));
        }
    }
    let mut appended = Vec::new();
    if let Command::Pose { draft, time, .. } = command {
        let existing: HashSet<_> = original.channels.iter().filter_map(|channel| {
            Some((channel.node, PoseProperty::from_path(&channel.path)?))
        }).collect();
        let new_tracks: Vec<_> = draft.iter().flat_map(|(node, pose)| {
            PoseProperty::ALL.into_iter().filter(move |&property| pose.contains(property))
                .map(move |property| (node, property))
        }).filter(|target| !existing.contains(target)).collect();
        appended = document.create_pose_tracks(original, &new_tracks)?;
        for (channel, (node, property)) in appended.iter_mut().zip(new_tracks) {
            let transform = draft.get(node).expect("新增通道来自当前草稿").transform;
            channel.insert_key(time, pose_key(channel, transform, property, time))?;
        }
    }
    if values.is_empty() && appended.is_empty() {
        return Err("没有可修改的关键帧；请选择通道中的关键帧或改变姿势".into());
    }
    Ok(Edit::Channels {
        clip,
        values: Arc::new(values),
        appended: appended.into(),
        first_appended: original.channels.len(),
    })
}

fn pose_key(channel: &Channel, transform: Transform, property: PoseProperty, time: f32) -> Vec<f32> {
    match property {
        PoseProperty::Translation => transform.translation.to_array().to_vec(),
        PoseProperty::Scale => transform.scale.to_array().to_vec(),
        PoseProperty::Rotation => {
            let value = transform.rotation.normalize();
            let reference = channel.sample(time);
            let reference = Quat::from_array(reference.as_slice().try_into().expect("旋转通道固定四维"));
            // 等价四元数也保持原取值的半球，避免三次样条在写键后反向绕行。
            (if value.dot(reference) < 0.0 { -value } else { value }).to_array().to_vec()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adjacent_keys_move_as_one_transaction_and_undo_restores_tangents() {
        let mut doc = super::super::data::tests::test_document();
        doc.clips[0].channels[0]
            .set_interpolation("CUBICSPLINE")
            .unwrap();
        let before = doc.clips[0].channels[0].keys.clone();
        let edit = build(
            &doc,
            Command::Move {
                clip: 0,
                selection: KeySelection(vec![(0, 0.0, 100.0)]),
                delta: 2.0,
            },
        )
        .unwrap();
        edit.apply(&mut doc, true).unwrap();
        for (a, b) in before.iter().zip(&doc.clips[0].channels[0].keys) {
            assert_eq!(a.time + 2.0, b.time);
            assert_eq!(a.in_tangent, b.in_tangent);
        }
        edit.apply(&mut doc, false).unwrap();
        assert_eq!(doc.clips[0].channels[0].keys, before);
    }
    #[test]
    fn move_collision_and_negative_time_preserve_document() {
        let doc = super::super::data::tests::test_document();
        let keys = &doc.clips[0].channels[0].keys;
        assert!(
            build(
                &doc,
                Command::Move {
                    clip: 0,
                    selection: KeySelection::single(0, keys[0].time),
                    delta: keys[1].time - keys[0].time
                }
            )
            .is_err()
        );
        assert!(
            build(
                &doc,
                Command::Move {
                    clip: 0,
                    selection: KeySelection::single(0, 0.0),
                    delta: -1.0
                }
            )
            .is_err()
        );
    }
    #[test]
    fn reloaded_source_rejects_clipboard_when_node_indices_now_name_other_bones() {
        let original = super::super::data::tests::clipboard_document(false);
        let clipboard = copy(
            &original,
            0,
            &KeySelection::single(0, 0.0),
            PathBuf::from("a.glb"),
        )
        .unwrap();
        let reloaded = super::super::data::tests::clipboard_document(true);
        let wrong_target = reloaded.clips[0]
            .channels
            .iter()
            .find(|channel| channel.node == 0 && channel.path == "translation")
            .unwrap();
        assert_ne!(wrong_target.node_name, original.clips[0].channels[0].node_name);
        let error = build(
            &reloaded,
            Command::Paste {
                clip: 0,
                at: 5.0,
                source: PathBuf::from("a.glb"),
                clipboard,
            },
        )
        .err()
        .expect("同路径但节点已重排，后台必须拒绝旧剪贴板");
        assert!(error.contains("重新复制"));
    }

    #[test]
    fn clipboard_remains_valid_after_an_edit_and_pastes_into_another_clip() {
        let original = super::super::data::tests::test_document();
        let clipboard = copy(
            &original,
            0,
            &KeySelection::single(0, 0.0),
            PathBuf::from("a.glb"),
        )
        .unwrap();
        let copied_value = original.clips[0].channels[0].keys[0].value.clone();
        let mut draft = original.clone();
        draft.clips[0].channels[0].keys[0].value[0] = 19.0;
        let before = draft.clips[1].channels[0].clone();
        let edit = build(
            &draft,
            Command::Paste {
                clip: 1,
                at: 5.0,
                source: PathBuf::from("a.glb"),
                clipboard,
            },
        )
        .expect("普通编辑不改变骨骼身份，同源跨动作复制应继续有效");
        edit.apply(&mut draft, true).unwrap();
        let key = draft.clips[1].channels[0].keys.last().unwrap();
        assert_eq!(key.time, 5.0);
        assert_eq!(key.value, copied_value);
        assert_eq!(draft.clips[0].channels[0].keys[0].value[0], 19.0);
        edit.apply(&mut draft, false).unwrap();
        assert_eq!(draft.clips[1].channels[0], before);
    }

    #[test]
    fn pose_updates_the_exact_key_without_overwriting_a_nearby_time() {
        use super::super::types::PoseDraft;
        let mut doc = super::super::data::tests::test_document();
        let near_end = 0.000_000_5;
        let channel = &mut doc.clips[0].channels[0];
        let node = channel.node;
        channel.insert_key(near_end, vec![2.0, 3.0, 4.0]).unwrap();
        let original = channel.clone();
        let mut draft = PoseDraft::default();
        draft.set(node, Transform::from_xyz(5.0, 6.0, 7.0), PoseProperty::Translation);
        let edit = build(
            &doc,
            Command::Pose {
                clip: 0,
                time: near_end,
                draft,
            },
        ).unwrap();

        edit.apply(&mut doc, true).unwrap();
        let channel = &doc.clips[0].channels[0];
        assert_eq!(channel.keys.len(), original.keys.len());
        assert_eq!(channel.keys[0], original.keys[0]);
        assert_eq!(channel.keys[1].time, near_end);
        assert_eq!(channel.keys[1].value, vec![5.0, 6.0, 7.0]);
        assert_eq!(channel.keys[2], original.keys[2]);
        edit.apply(&mut doc, false).unwrap();
        assert_eq!(doc.clips[0].channels[0], original);
    }

    #[test]
    fn clipboard_offsets_and_pose_upsert_preserve_untouched_keys() {
        use super::super::types::PoseDraft;
        let mut doc = super::super::data::tests::test_document();
        let clip = copy(
            &doc,
            0,
            &KeySelection::single(0, 0.0),
            PathBuf::from("a.glb"),
        )
        .unwrap();
        let edit = build(
            &doc,
            Command::Paste {
                clip: 0,
                at: 5.0,
                source: PathBuf::from("a.glb"),
                clipboard: clip.clone(),
            },
        )
        .unwrap();
        edit.apply(&mut doc, true).unwrap();
        assert_eq!(doc.clips[0].channels[0].keys.last().unwrap().time, 5.0);
        assert!(
            build(
                &doc,
                Command::Paste {
                    clip: 0,
                    at: 6.0,
                    source: PathBuf::from("other.glb"),
                    clipboard: clip
                }
            )
            .is_err()
        );
        let node = doc.clips[0].channels[0].node;
        let len = doc.clips[0].channels[0].keys.len();
        let mut draft = PoseDraft::default();
        draft.set(node, Transform::from_xyz(3., 4., 5.), PoseProperty::Translation);
        let edit = build(
            &doc,
            Command::Pose {
                clip: 0,
                time: 0.0,
                draft,
            },
        )
        .unwrap();
        edit.apply(&mut doc, true).unwrap();
        assert_eq!(doc.clips[0].channels[0].keys.len(), len);
        assert_eq!(doc.clips[0].channels[0].keys[0].value, vec![3., 4., 5.]);
    }
}
