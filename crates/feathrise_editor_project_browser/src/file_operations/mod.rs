//! 文件整理的唯一后台写入槽；窗口生命周期不拥有已提交任务，队列和回执都有固定上限。

mod filesystem;
mod recycle;
#[cfg(test)]
mod tests;

use bevy::tasks::IoTaskPool;
use feathrise_editor_project_browser_model::file_operations::*;
use std::{collections::VecDeque, sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU64, Ordering}}};

const MAX_QUEUE: usize = 8;
const MAX_RECEIPTS: usize = 32;
const CANCELLED: &str = "文件任务已取消；已完成的条目保留，请查看任务回执";

pub(super) fn service() -> FileOperationService { FileOperationService(Arc::new(Backend::default())) }

#[derive(Default)]
struct Backend { shared: Arc<Shared> }

#[derive(Default)]
struct Shared {
    queue: Mutex<Queue>,
    next_id: AtomicU64,
    cancelled_context: AtomicU64,
}

#[derive(Default)]
struct Queue {
    pending: VecDeque<Job>,
    running: bool,
    active: Option<(FileOperationProgress, Arc<AtomicBool>)>,
    completed: VecDeque<Arc<FileOperationReceipt>>,
    revision: u64,
    clipboard: Option<FileClipboard>,
    active_move: Option<u64>,
}

impl Queue {
    fn clipboard_has_receipts(&self, clipboard: &FileClipboard) -> bool {
        self.completed.iter().rev().take_while(|receipt| receipt.id > clipboard.through)
            .any(|receipt| receipt.context == clipboard.context && receipt.kind == FileOperationKind::Move)
    }
}

struct Job {
    id: u64,
    request: FileOperationRequest,
    cancel: Arc<AtomicBool>,
}

impl FileOperationBackend for Backend {
    fn enqueue(&self, request: FileOperationRequest) -> Result<u64, String> {
        let source_count = match &request.operation {
            FileOperation::Copy { sources, .. } | FileOperation::Move { sources, .. } | FileOperation::Delete { sources } => sources.len(),
            _ => 1,
        };
        if source_count == 0 || request.allowed_roots.is_empty() || request.allowed_roots.len() > 32 {
            return Err("请选择要操作的文件，并先在编辑器中打开所属目录".into());
        }
        if request.context == 0 || self.shared.cancelled_context.load(Ordering::Acquire) == request.context {
            return Err("文件操作所属工程会话已经结束".into());
        }
        let mut queue = self.shared.queue.try_lock().map_err(|_| "文件任务正在更新，请重试")?;
        if queue.pending.len() >= MAX_QUEUE { return Err("文件任务队列已满，请等待当前任务完成".into()); }
        let id = self.shared.next_id.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
        queue.pending.push_back(Job { id, request, cancel: Arc::new(AtomicBool::new(false)) });
        queue.revision = queue.revision.wrapping_add(1);
        self.start_worker(&mut queue);
        Ok(id)
    }

    fn snapshot(&self) -> Option<FileOperationSnapshot> {
        let queue = self.shared.queue.try_lock().ok()?;
        Some(FileOperationSnapshot { revision: queue.revision,
            active: queue.active.as_ref().map(|(progress, cancel)| {
                let mut progress = progress.clone();
                progress.cancel_requested = cancel.load(Ordering::Acquire);
                progress
            }),
            queued: queue.pending.iter().map(|job| progress(job)).collect(),
            completed: queue.completed.iter().cloned().collect(),
            clipboard: queue.clipboard.as_ref().filter(|clipboard| !queue.clipboard_has_receipts(clipboard)
                && queue.active_move != Some(clipboard.context)
                && !queue.pending.iter().any(|job| job.request.context == clipboard.context && job.request.operation.kind() == FileOperationKind::Move))
                .cloned(),
        })
    }

    fn cancel(&self, id: u64) -> bool {
        let Ok(queue) = self.shared.queue.try_lock() else { return false; };
        let cancel = queue.active.as_ref().filter(|(progress, _)| progress.id == id).map(|(_, cancel)| cancel)
            .or_else(|| queue.pending.iter().find(|job| job.id == id).map(|job| &job.cancel));
        if let Some(cancel) = cancel { cancel.store(true, Ordering::Release); true } else { false }
    }

    fn cancel_context(&self, context: u64) {
        // 切工程/退出不等待后台锁；worker在下一块复制或下一顶层条目前撤回未完成内容。
        self.shared.cancelled_context.store(context, Ordering::Release);
        if let Ok(queue) = self.shared.queue.try_lock() {
            if let Some((progress, cancel)) = &queue.active {
                if progress.context == context { cancel.store(true, Ordering::Release); }
            }
            for job in &queue.pending {
                if job.request.context == context { job.cancel.store(true, Ordering::Release); }
            }
        }
    }

    fn is_busy(&self) -> bool {
        self.shared.queue.try_lock().map_or(true, |queue| queue.running || !queue.pending.is_empty())
    }

    fn publish_clipboard(&self, clipboard: Option<FileClipboard>) -> bool {
        let Ok(mut queue) = self.shared.queue.try_lock() else { return false; };
        let needs_worker = clipboard.as_ref().is_some_and(|clipboard| queue.clipboard_has_receipts(clipboard));
        let old = std::mem::replace(&mut queue.clipboard, clipboard);
        queue.revision = queue.revision.wrapping_add(1);
        if needs_worker { self.start_worker(&mut queue); }
        drop(queue);
        drop(old);
        true
    }
}

impl Backend {
    fn start_worker(&self, queue: &mut Queue) {
        if !queue.running {
            queue.running = true;
            let shared = Arc::clone(&self.shared);
            // 文件事务和剪切板交集共用一个后台工作者，按真实发布顺序收口。
            IoTaskPool::get().spawn(async move { run(shared); }).detach();
        }
    }
}

fn progress(job: &Job) -> FileOperationProgress {
    let label = match job.request.operation.kind() {
        FileOperationKind::Copy => "复制文件", FileOperationKind::Move => "移动文件",
        FileOperationKind::Delete => "移到回收站", FileOperationKind::CreateFolder => "新建文件夹",
        FileOperationKind::CreateFile => "新建文件", FileOperationKind::Rename => "重命名",
    };
    FileOperationProgress { id: job.id, context: job.request.context, label: label.into(), completed: 0,
        bytes: 0, current: None, cancel_requested: job.cancel.load(Ordering::Acquire) }
}

fn run(shared: Arc<Shared>) {
    loop {
        let job = {
            let mut queue = shared.queue.lock().expect("文件任务队列不跨用户代码持锁");
            if let Some(mut clipboard) = queue.clipboard.as_ref().filter(|clipboard| queue.clipboard_has_receipts(clipboard)).cloned() {
                let through = clipboard.through;
                let receipts: Vec<_> = queue.completed.iter().filter(|receipt| {
                    receipt.id > through && receipt.context == clipboard.context && receipt.kind == FileOperationKind::Move
                }).cloned().collect();
                drop(queue);
                for receipt in receipts {
                    if let Some(remaining) = receipt.remaining_clipboard(&clipboard.sources) { clipboard.sources = remaining; }
                    clipboard.through = receipt.id;
                }
                let mut queue = shared.queue.lock().expect("采用剪切板只替换已完成快照");
                if queue.clipboard.as_ref().is_some_and(|current| (current.context, current.generation) == (clipboard.context, clipboard.generation)) {
                    let old = queue.clipboard.replace(clipboard);
                    queue.revision = queue.revision.wrapping_add(1);
                    drop(queue);
                    drop(old);
                }
                continue;
            }
            let Some(job) = queue.pending.pop_front() else { queue.running = false; return; };
            queue.active = Some((progress(&job), Arc::clone(&job.cancel)));
            queue.active_move = (job.request.operation.kind() == FileOperationKind::Move).then_some(job.request.context);
            queue.revision = queue.revision.wrapping_add(1);
            job
        };
        let control = Control { shared: &shared, job: &job };
        let mut receipt = FileOperationReceipt::new(job.id, job.request.context, job.request.operation.kind());
        let mut current = None;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            filesystem::execute(&job.request, &control, &mut receipt, &mut current)
        }));
        match result {
            Ok(Err(error)) => {
                if control.cancelled() { receipt.cancelled = true; }
                if !receipt.cancelled || error != CANCELLED { receipt.error = Some(error); }
            }
            Err(_) => receipt.error = Some("文件任务异常结束；已完成条目见回执，未发布暂存内容保留供检查".into()),
            Ok(Ok(())) => {}
        }
        receipt.finish(&job.request.operation, current);
        let mut queue = shared.queue.lock().expect("文件任务完成只发布一次完整回执");
        queue.active = None;
        queue.active_move = None;
        let expired = (queue.completed.len() == MAX_RECEIPTS).then(|| queue.completed.pop_front()).flatten();
        queue.completed.push_back(Arc::new(receipt));
        queue.revision = queue.revision.wrapping_add(1);
        drop(queue);
        drop(expired);
    }
}

pub(super) struct Control<'a> { shared: &'a Shared, job: &'a Job }

impl Control<'_> {
    pub(super) fn cancelled(&self) -> bool {
        self.job.cancel.load(Ordering::Acquire)
            || self.shared.cancelled_context.load(Ordering::Acquire) == self.job.request.context
    }
    pub(super) fn check(&self) -> Result<(), String> {
        if self.cancelled() { Err(CANCELLED.into()) } else { Ok(()) }
    }
    pub(super) fn update(&self, current: Option<&std::path::Path>, completed: u64, bytes: u64) {
        let mut queue = self.shared.queue.lock().expect("后台进度更新不包含文件IO");
        if let Some((progress, _)) = queue.active.as_mut() {
            if progress.id == self.job.id {
                if let Some(current) = current { progress.current = Some(current.to_owned()); }
                progress.completed = progress.completed.saturating_add(completed);
                progress.bytes = progress.bytes.saturating_add(bytes);
                queue.revision = queue.revision.wrapping_add(1);
            }
        }
    }
    pub(super) fn id(&self) -> u64 { self.job.id }
}
