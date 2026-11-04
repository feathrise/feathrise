//! 导出目标的单槽监听租约：只排除已核对的精确目录，已发布标记失效后恢复正常源码事件。

use crate::build_process::OutputLease;
use std::{fs, io::Read, path::{Path, PathBuf}, sync::{Arc, Mutex}};

#[derive(Default)]
pub(super) struct OutputScope {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    ticket: u64,
    target: Option<Target>,
}

struct Target {
    path: PathBuf,
    comparison: PathBuf,
    project_id: String,
    ticket: u64,
    export_id: Option<String>,
    dirty: bool,
    checking: bool,
}

impl OutputScope {
    /// 调用方必须先验证目标路径和现有内容；本槽只负责事件归属，不替代文件事务的安全检查。
    pub(super) fn register(self: &Arc<Self>, path: &Path, project_id: &str,
        invalidate: impl Fn() + Send + Sync + 'static) -> Result<OutputLease, String> {
        if !path.is_absolute() { return Err("导出监听租约要求已验证的绝对目录".into()); }
        {
            let mut state = self.state.lock().map_err(|_| "导出目标监听槽异常")?;
            if let Some(target) = &mut state.target {
                if target.export_id.is_none() { return Err("已有导出目标正在发布，不能登记第二个目录".into()); }
                if target.checking { return Err("前次导出目标正在后台核验，请完成后重新导出".into()); }
                // 更换目标不能抹掉旧标记失效或尚未处理的通知；先在后台有界重核旧槽。
                target.dirty = true;
            }
        }
        if self.refresh() {
            invalidate();
            return Err("前次导出目标的标记已变化，请等待源码监听同步后重新导出".into());
        }
        let mut state = self.state.lock().map_err(|_| "导出目标监听槽异常")?;
        if state.target.as_ref().is_some_and(|target| target.export_id.is_none() || target.checking || target.dirty) {
            return Err("前次导出目标有待核验的通知，请完成后重新导出".into());
        }
        state.ticket = state.ticket.wrapping_add(1).max(1);
        let ticket = state.ticket;
        state.target = Some(Target { path: path.to_owned(), comparison: super::source::comparison_path(path),
            project_id: project_id.to_owned(), ticket, export_id: None, dirty: false, checking: false });
        let scope = self.clone();
        Ok(OutputLease::new(move |export_id| {
            let Ok(mut state) = scope.state.lock() else { return; };
            let Some(target) = state.target.as_mut().filter(|target| target.ticket == ticket) else { return; };
            if let Some(export_id) = export_id {
                target.export_id = Some(export_id);
                target.dirty = true;
            } else {
                // 活动期若吞过事件，失败／取消也必须推进一次代次，不能随清槽永久丢掉外来Rust保存。
                let changed = target.dirty;
                state.target = None;
                drop(state);
                if changed { invalidate(); }
            }
        }))
    }

    /// 原生回调只比较内存路径；发布中的暂时缺标记由活动租约覆盖，其他源码事件始终照常推进。
    pub(super) fn ignores(&self, path: &Path, relevant_change: bool) -> bool {
        let Ok(mut state) = self.state.lock() else { return false; };
        let Some(target) = state.target.as_mut() else { return false; };
        if !super::source::comparison_path(path).starts_with(&target.comparison) { return false; }
        // 普通非源码通知不触发反复读取；标记自身的增删改另外属于租约生命周期事件。
        // Access 已由上游排除，后台只读标记不会再给自己制造 dirty。
        if relevant_change || path.file_name().is_some_and(|name| name == "feathrise-export.json") {
            target.dirty = true;
        }
        true
    }

    /// 后台监听或登记前读取最多64KiB标记；无效时撤销排除并要求推进代次，防止后来源码被吞掉。
    pub(super) fn refresh(&self) -> bool {
        let check = {
            let Ok(mut state) = self.state.lock() else { return true; };
            let Some(target) = state.target.as_mut().filter(|target| target.dirty && !target.checking) else { return false; };
            let Some(export_id) = target.export_id.clone() else { return false; };
            target.dirty = false;
            target.checking = true;
            (target.path.clone(), target.project_id.clone(), export_id, target.ticket)
        };
        let valid = valid_marker(&check.0, &check.1, &check.2);
        let Ok(mut state) = self.state.lock() else { return true; };
        if let Some(target) = state.target.as_mut().filter(|target| target.ticket == check.3) { target.checking = false; }
        if !valid && state.target.as_ref().is_some_and(|target| target.ticket == check.3 && target.export_id.as_ref() == Some(&check.2)) {
            state.target = None;
            return true;
        }
        false
    }
}

fn ordinary(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 { return false; }
    }
    !metadata.file_type().is_symlink()
}

fn valid_marker(root: &Path, project_id: &str, export_id: &str) -> bool {
    let Ok(metadata) = fs::symlink_metadata(root) else { return false; };
    if !ordinary(&metadata) || !metadata.is_dir()
        || root.canonicalize().ok().is_none_or(|canonical|
            super::source::comparison_path(&canonical) != super::source::comparison_path(root)) { return false; }
    let marker = root.join("feathrise-export.json");
    let Ok(metadata) = fs::symlink_metadata(&marker) else { return false; };
    if !ordinary(&metadata) || !metadata.is_file() || metadata.len() > 64 * 1024 { return false; }
    let Ok(file) = fs::File::open(marker) else { return false; };
    let mut bytes = Vec::new();
    if file.take(64 * 1024 + 1).read_to_end(&mut bytes).is_err() || bytes.len() > 64 * 1024 { return false; }
    let Ok(marker) = serde_json::from_slice::<serde_json::Value>(&bytes) else { return false; };
    marker["format"] == 1 && marker["kind"] == "feathrise-export" && marker["project_id"] == project_id
        && marker["export_id"] == export_id && marker["manifest"] == "export-manifest.json"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacing_a_published_target_cannot_discard_an_in_flight_marker_check() {
        let previous = PathBuf::from("D:/owned-output-scope/previous");
        // 精确构造锁外标记读取阶段：dirty已消费，但checking仍代表尚无最终身份回执。
        let scope = Arc::new(OutputScope { state: Mutex::new(State { ticket: 1, target: Some(Target {
            comparison: super::super::source::comparison_path(&previous), path: previous.clone(),
            project_id: "owned".into(), ticket: 1, export_id: Some("previous-export".into()), dirty: false, checking: true,
        }) }) });
        let result = scope.register(Path::new("D:/owned-output-scope/next"), "owned", || {});
        assert!(result.is_err());
        let state = scope.state.lock().unwrap();
        let target = state.target.as_ref().unwrap();
        assert_eq!(target.path, previous);
        assert_eq!(target.ticket, 1);
        assert!(target.checking, "新登记不能抹掉旧published的在途检查");
    }
}
