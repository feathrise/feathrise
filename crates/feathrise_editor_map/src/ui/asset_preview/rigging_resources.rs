//! 一次只保留一个模型的关节草稿和一个真正运行中的后台槽。

use std::sync::atomic::Ordering;

use bevy::prelude::*;

use super::{
    backend::{AnimationAsset, Landmarks, RigInput},
    context::Candidate,
    types::*,
};

pub(super) struct MarkerDraft {
    pub input: RigInput,
    pub suggested: Landmarks,
    pub points: Landmarks,
    pub selected: Option<usize>,
}

pub(super) struct Session {
    pub source: SourceKey,
    pub entry: Entry,
    pub draft: Option<MarkerDraft>,
    pub result: Option<String>,
    pub animations: Vec<AnimationAsset>,
    pub reference_indices: Vec<usize>,
    pub selected_file: Option<usize>,
    pub clip: usize,
    pub picker: Option<Picker>,
    pub page: usize,
    pub notice: Notice,
    pub warnings: Vec<String>,
    pub error: Option<String>,
}

impl Session {
    pub(super) fn new(source: SourceKey, entry: Entry) -> Self {
        let notice = match entry {
            Entry::Unskinned => Notice::Inspecting,
            Entry::Skinned => Notice::Cataloguing,
        };
        Self {
            source,
            entry,
            draft: None,
            result: None,
            animations: Vec::new(),
            reference_indices: Vec::new(),
            selected_file: None,
            clip: 0,
            picker: None,
            page: 0,
            notice,
            warnings: Vec::new(),
            error: None,
        }
    }

    pub(super) fn matches(&self, source: Candidate<'_>) -> bool {
        if !source.same_file(&self.source) || source.entry != self.entry {
            return false;
        }
        // 回到模型标签时 Scene0 会重新实例化。已发布的新 GLB 可继续选动作，
        // 但新一代源模型不能继续使用旧的、尚未发布的关节点。
        source.generation == self.source.generation
            || self.result.is_some()
            || self.entry == Entry::Skinned
                && matches!(self.notice, Notice::ChooseAnimation | Notice::NoClips)
    }

    pub(super) fn reference_mode(&self) -> bool {
        self.entry == Entry::Unskinned && self.result.is_none()
    }

    pub(super) fn model_for_preview(&self) -> Option<&str> {
        self.result
            .as_deref()
            .or_else(|| (self.entry == Entry::Skinned).then_some(self.source.path.as_str()))
    }

    pub(super) fn file(&self) -> Option<&AnimationAsset> {
        self.animations.get(self.selected_file?)
    }

    fn file_index(&self, index: usize) -> Option<usize> {
        if self.reference_mode() {
            self.reference_indices.get(index).copied()
        } else {
            self.animations.get(index).map(|_| index)
        }
    }

    pub(super) fn choice_count(&self) -> usize {
        match self.picker {
            Some(Picker::File) if self.reference_mode() => self.reference_indices.len(),
            Some(Picker::File) => self.animations.len(),
            Some(Picker::Clip) => self.file().map_or(0, |asset| asset.clips.len()),
            None => 0,
        }
    }

    pub(super) fn choice(&self, row: usize) -> Option<&str> {
        let index = self.page * PICKER_ROWS + row;
        match self.picker? {
            Picker::File => Some(self.animations.get(self.file_index(index)?)?.path.as_str()),
            Picker::Clip => Some(self.file()?.clips.get(index)?.as_str()),
        }
    }

    pub(super) fn choose(&mut self, row: usize) {
        let index = self.page * PICKER_ROWS + row;
        match self.picker {
            Some(Picker::File) => {
                let Some(index) = self.file_index(index) else {
                    return;
                };
                self.selected_file = Some(index);
                self.clip = 0;
            }
            Some(Picker::Clip) if index < self.choice_count() => self.clip = index,
            _ => return,
        }
        self.picker = None;
    }

    pub(super) fn toggle_picker(&mut self, picker: Picker) {
        self.page = 0;
        self.picker = (self.picker != Some(picker)).then_some(picker);
        if self.choice_count() == 0 {
            self.picker = None;
        }
    }
}

#[derive(Resource, Default)]
pub(crate) struct RiggingState {
    pub(super) session: Option<Session>,
    pub(super) work: Option<Work>,
    pub(super) drag: Option<LandmarkDrag>,
    pub(super) panel_open: bool,
    pub(super) mouse_owned: bool,
}

impl RiggingState {
    pub(crate) fn owns_mouse(&self) -> bool {
        self.mouse_owned || self.drag.is_some()
    }

    pub(crate) fn exit_pending(&self) -> bool {
        self.work.is_some()
    }

    pub(super) fn cancel_work(&self) {
        if let Some(work) = &self.work {
            work.cancelled.store(true, Ordering::Release);
        }
    }
}

impl Drop for RiggingState {
    fn drop(&mut self) {
        self.cancel_work();
    }
}

#[cfg(feature = "validation")]
impl RiggingState {
    /// 验收只读真实采用状态，不开放任何跳过按钮、Blender 或发布事务的写入口。
    pub(crate) fn validation_status(&self) -> RigValidationStatus<'_> {
        let session = self.session.as_ref();
        RigValidationStatus {
            open: self.panel_open,
            busy: self.work.is_some(),
            source: session.map(|session| session.source.path.as_str()),
            output: session.and_then(|session| session.result.as_deref()),
            landmarks: session
                .and_then(|session| session.draft.as_ref())
                .map_or(0, |draft| draft.points.len()),
            animation: session
                .and_then(Session::file)
                .map(|asset| asset.path.as_str()),
            clip: session.map_or(0, |session| session.clip),
            error: session.and_then(|session| session.error.as_deref()),
        }
    }

    pub(crate) fn validation_landmark(&self, index: usize) -> Option<[f32; 3]> {
        let (key, _) = LANDMARKS.get(index)?;
        self.session
            .as_ref()?
            .draft
            .as_ref()?
            .points
            .get(*key)
            .copied()
    }
}

#[cfg(feature = "validation")]
pub(crate) struct RigValidationStatus<'a> {
    pub open: bool,
    pub busy: bool,
    pub source: Option<&'a str>,
    pub output: Option<&'a str>,
    pub landmarks: usize,
    pub animation: Option<&'a str>,
    pub clip: usize,
    pub error: Option<&'a str>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn candidate(root: &str, generation: u64) -> Candidate<'_> {
        Candidate {
            root: Path::new(root),
            path: "models/humanoid.glb",
            generation,
            entry: Entry::Unskinned,
        }
    }

    #[test]
    fn unbound_landmarks_expire_on_model_reload_and_published_assets_can_preview_again() {
        let first = candidate("project-a/assets", 1);
        let mut session = Session::new(first.key(), first.entry);
        assert!(session.matches(first));
        assert!(!session.matches(candidate("project-a/assets", 2)));
        session.result = Some("models/humanoid_rigged.glb".into());
        assert!(session.matches(candidate("project-a/assets", 2)));
        assert!(!session.matches(candidate("project-b/assets", 1)));
    }

    #[test]
    fn reference_picker_filters_files_and_motion_picker_preserves_clip_ordinals() {
        let source = candidate("assets", 1);
        let mut session = Session::new(source.key(), source.entry);
        session.animations = vec![
            AnimationAsset {
                path: "other.glb".into(),
                clips: vec!["Other".into()],
                can_reference: false,
            },
            AnimationAsset {
                path: "humanoid.glb".into(),
                clips: vec!["Idle".into(), "Walk".into()],
                can_reference: true,
            },
        ];
        session.reference_indices.push(1);
        session.toggle_picker(Picker::File);
        assert_eq!(session.choice_count(), 1);
        assert_eq!(session.choice(0), Some("humanoid.glb"));
        session.choose(0);
        assert_eq!(session.selected_file, Some(1));

        session.result = Some("rigged.glb".into());
        session.toggle_picker(Picker::File);
        assert_eq!(session.choice_count(), 2);
        session.choose(1);
        session.toggle_picker(Picker::Clip);
        assert_eq!(session.choice(1), Some("Walk"));
        session.choose(1);
        assert_eq!(session.clip, 1);
    }
}
