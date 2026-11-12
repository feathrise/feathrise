//! 框选、移动、复制和姿势插帧的后台事务；整组冲突检查通过后才发布一份新文档。

pub(super) use super::types::{Clipboard, Command};
use super::{data::*, resources::Edit, types::KeySelection};
use bevy::prelude::*;
use std::{path::PathBuf, sync::Arc};

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
    let clip = match &command {
        Command::Copy { clip, .. }
        | Command::Move { clip, .. }
        | Command::Delete { clip, .. }
        | Command::Paste { clip, .. }
        | Command::Pose { clip, .. } => *clip,
    };
    let original = document.clips.get(clip).ok_or("动作已改变")?;
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
            Command::Pose {
                node,
                transform,
                time,
                ..
            } if channel.node == *node => {
                let value = match channel.path.as_str() {
                    "translation" => transform.translation.to_array().to_vec(),
                    "rotation" => transform.rotation.normalize().to_array().to_vec(),
                    "scale" => transform.scale.to_array().to_vec(),
                    _ => continue,
                };
                if let Some(key) = after.keys.iter_mut().find(|k| (k.time - time).abs() < 1e-6) {
                    key.value = value;
                } else {
                    after.insert_key(*time, value)?;
                }
            }
            _ => {}
        }
        if after.keys != channel.keys {
            after.validate()?;
            values.push((index, channel.keys.clone(), after.keys));
        }
    }
    if values.is_empty() {
        return Err("没有可修改的关键帧；请选择通道中的关键帧或改变姿势".into());
    }
    Ok(Edit::Channels {
        clip,
        values: Arc::new(values),
    })
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
    fn clipboard_offsets_and_pose_upsert_preserve_untouched_keys() {
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
        let edit = build(
            &doc,
            Command::Pose {
                clip: 0,
                node,
                time: 0.0,
                transform: Transform::from_xyz(3., 4., 5.),
            },
        )
        .unwrap();
        edit.apply(&mut doc, true).unwrap();
        assert_eq!(doc.clips[0].channels[0].keys.len(), len);
        assert_eq!(doc.clips[0].channels[0].keys[0].value, vec![3., 4., 5.]);
    }
}
