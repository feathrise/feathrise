//! 模式偏好使用用户配置旁的独立侧文件，避免作者进程与工作台最近工程记录争写同一原文。
//! 最多一个后台写入和一份最新选择；只有成功回执才能更新磁盘身份，失败保留最后有效文件。

use super::{ViewportMode, ViewportModeState};
use crate::user_preferences::{Preferences, publish_checked, read_optional};
use async_channel::{Receiver, TryRecvError};
use bevy::{prelude::*, tasks::IoTaskPool};
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, path::PathBuf, rc::Rc};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    format: u32,
    #[serde(default)]
    mode: ViewportMode,
}

struct Store {
    path: PathBuf,
    original: Option<Vec<u8>>,
    mode: ViewportMode,
}

impl Store {
    fn load(path: PathBuf) -> Result<Self, String> {
        let original = read_optional(&path)?;
        let mode = if let Some(bytes) = original.as_deref() {
            let stored: Stored = serde_json::from_slice(bytes)
                .map_err(|error| format!("视口偏好损坏，原文保留：{error}"))?;
            if stored.format != 1 {
                return Err(format!("不支持视口偏好版本{}，原文保留", stored.format));
            }
            stored.mode
        } else {
            ViewportMode::default()
        };
        Ok(Self {
            path,
            original,
            mode,
        })
    }

    fn save(&mut self) -> Result<(), String> {
        let mut bytes = serde_json::to_vec_pretty(&Stored {
            format: 1,
            mode: self.mode,
        })
        .map_err(|error| error.to_string())?;
        bytes.push(b'\n');
        publish_checked(&self.path, &self.original, &bytes)?;
        self.original = Some(bytes);
        Ok(())
    }
}

struct Written {
    store: Store,
    revision: u64,
    result: Result<(), String>,
}

struct State {
    store: Option<Store>,
    current: ViewportMode,
    committed: ViewportMode,
    revision: u64,
    job: Option<Receiver<Written>>,
    failure: Option<(u64, String)>,
}

impl State {
    fn capture(&mut self, mode: ViewportMode) {
        if self.current != mode {
            self.current = mode;
            self.revision = self.revision.wrapping_add(1).max(1);
            self.failure = None;
        }
    }

    fn dirty(&self) -> bool {
        self.current != self.committed
    }

    fn error(&self) -> Option<&str> {
        self.failure
            .as_ref()
            .filter(|(revision, _)| *revision == self.revision)
            .map(|(_, error)| error.as_str())
    }

    fn adopt(&mut self, written: Written) {
        match written.result {
            Ok(()) => {
                self.committed = written.store.mode;
                self.failure = None;
            }
            Err(error) => {
                self.failure = Some((written.revision, error));
            }
        }
        self.store = Some(written.store);
        self.job = None;
    }

    fn advance(&mut self) {
        match self.job.as_ref().map(|job| job.try_recv()) {
            Some(Ok(written)) => self.adopt(written),
            Some(Err(TryRecvError::Closed)) => {
                self.job = None;
                self.failure = Some((self.revision, "视口偏好写入任务异常结束，原文保留".into()));
            }
            Some(Err(TryRecvError::Empty)) | None => (),
        }
        if self.job.is_some() || !self.dirty() || self.error().is_some() {
            return;
        }
        let Some(mut store) = self.store.take() else {
            self.failure = Some((
                self.revision,
                "视口偏好写入槽未归还原文身份，请重新打开编辑器".into(),
            ));
            return;
        };
        store.mode = self.current;
        let revision = self.revision;
        let (sender, receiver) = async_channel::bounded(1);
        self.job = Some(receiver);
        IoTaskPool::get()
            .spawn(async move {
                let result = store.save();
                let _ = sender.try_send(Written {
                    store,
                    revision,
                    result,
                });
            })
            .detach();
    }
}

struct Runtime(Rc<RefCell<State>>);

pub(crate) struct Completion(Rc<RefCell<State>>);

pub(super) fn install(app: &mut App, preferences: &Preferences) -> Completion {
    let path = preferences.file().with_extension("viewport.json");
    let (store, error) = match Store::load(path.clone()) {
        Ok(store) => (store, None),
        Err(error) => (
            Store {
                path,
                original: None,
                mode: ViewportMode::default(),
            },
            Some(error),
        ),
    };
    let mode = store.mode;
    let state = Rc::new(RefCell::new(State {
        store: Some(store),
        current: mode,
        committed: mode,
        revision: 1,
        job: None,
        failure: None,
    }));
    app.insert_resource(ViewportModeState {
        selected: mode,
        revision: 1,
        status: error.unwrap_or_default(),
    })
    .insert_non_send(Runtime(Rc::clone(&state)))
    .add_systems(Last, capture_and_poll);
    Completion(state)
}

fn capture_and_poll(mut mode: ResMut<ViewportModeState>, runtime: NonSend<Runtime>) {
    let mut state = runtime.0.borrow_mut();
    state.capture(mode.selected);
    state.advance();
    if let Some(error) = state.error() {
        let message = format!("视口选择尚未保存：{error}");
        if mode.status != message {
            mode.status = message;
        }
    }
}

/// 作者退出已经获准后再排空；帧中只检查真实回执，不等待线程或磁盘。
pub(crate) fn ready_to_exit(world: &mut World, retry: bool) -> Result<bool, String> {
    let Some(runtime) = world.get_non_send::<Runtime>() else {
        return Ok(true);
    };
    let mut state = runtime.0.borrow_mut();
    state.capture(world.resource::<ViewportModeState>().selected);
    if retry {
        state.failure = None;
    }
    state.advance();
    if state.dirty()
        && let Some(error) = state.error()
    {
        return Err(format!("视口选择未保存：{error}"));
    }
    Ok(!state.dirty() && state.job.is_none())
}

impl Completion {
    /// 仅 App::run 返回后调用；候选作者被退役时也保留最后用户选择，正常关闭应已异步排空。
    pub(crate) fn finish(self) -> Result<(), String> {
        let mut state = self.0.borrow_mut();
        if let Some(job) = state.job.take() {
            let written = job
                .recv_blocking()
                .map_err(|_| "视口偏好后台任务没有返回结果".to_owned())?;
            state.adopt(written);
        }
        if !state.dirty() {
            return Ok(());
        }
        if let Some(error) = state.error() {
            return Err(error.to_owned());
        }
        let mode = state.current;
        let store = state.store.as_mut().ok_or("视口偏好写入槽未归还")?;
        store.mode = mode;
        store.save()?;
        state.committed = mode;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "feathrise-viewport-preferences-{}-{}-{}.json",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            assert!(!path.exists());
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[test]
    fn missing_selection_keeps_original_mode_and_choice_survives_reopen() {
        let fixture = Fixture::new();
        let mut store = Store::load(fixture.0.clone()).unwrap();
        assert_eq!(store.mode, ViewportMode::SavedWorld);
        assert!(!fixture.0.exists());
        store.mode = ViewportMode::EmbeddedWorld;
        store.save().unwrap();
        assert_eq!(
            Store::load(fixture.0.clone()).unwrap().mode,
            ViewportMode::EmbeddedWorld
        );
        store.mode = ViewportMode::SavedWorld;
        store.save().unwrap();
        assert_eq!(
            Store::load(fixture.0.clone()).unwrap().mode,
            ViewportMode::SavedWorld
        );
    }

    #[test]
    fn corrupt_and_external_preferences_are_not_overwritten() {
        let fixture = Fixture::new();
        let mut store = Store::load(fixture.0.clone()).unwrap();
        fs::write(&fixture.0, b"broken").unwrap();
        assert!(Store::load(fixture.0.clone()).is_err());
        assert!(store.save().is_err());
        assert_eq!(fs::read(&fixture.0).unwrap(), b"broken");
        fs::remove_file(&fixture.0).unwrap();
        store.save().unwrap();
        let mut stale = Store::load(fixture.0.clone()).unwrap();
        store.mode = ViewportMode::EmbeddedWorld;
        store.save().unwrap();
        assert!(stale.save().is_err());
        assert_eq!(
            Store::load(fixture.0.clone()).unwrap().mode,
            ViewportMode::EmbeddedWorld
        );
    }
}
