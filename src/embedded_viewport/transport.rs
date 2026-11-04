//! 常驻工程的专用双管道传输：后台读写、有界背压、显式关闭与可核实的线程收口。
//!
//! stdout/stderr 始终由进程协调器独立排空。调用方拥有 Job；关停时先请求 Stop，
//! 必要时终止该 Job，再 shutdown，最后轮询 is_finished，不能把 Drop 当退出确认。

use async_channel::{Receiver, Sender, TryRecvError, TrySendError};
use feathrise_editor_protocol::{Envelope, Limits, read_frame, write_frame};
use std::{
    fs::File,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
};

const OUTGOING_CAPACITY: usize = 4;
const INCOMING_CAPACITY: usize = 8;

pub(super) struct Transport {
    outgoing: Sender<Envelope>,
    incoming: Receiver<Envelope>,
    snapshot_pending: Arc<AtomicBool>,
    reader: Option<JoinHandle<Result<(), String>>>,
    writer: Option<JoinHandle<Result<(), String>>>,
    result: Option<Result<(), String>>,
}

impl Transport {
    /// 由生命周期后台工作槽创建。只有这两个线程可以读写专用控制管道。
    pub(super) fn spawn(mut read: File, mut write: File) -> Result<Self, String> {
        let (outgoing, send_queue) = async_channel::bounded::<Envelope>(OUTGOING_CAPACITY);
        let (receive_queue, incoming) = async_channel::bounded::<Envelope>(INCOMING_CAPACITY);
        let snapshot_pending = Arc::new(AtomicBool::new(false));
        let pending = Arc::clone(&snapshot_pending);
        // 先创建等待邮箱的写线程；第二条线程创建失败时，关闭邮箱即可确定收口。
        let writer = thread::Builder::new()
            .name("feathrise-ipc-write".into())
            .spawn(move || {
                while let Ok(message) = send_queue.recv_blocking() {
                    let snapshot = message.payload.is_some();
                    let result = write_frame(&mut write, &message, &Limits::default())
                        .map_err(|error| error.to_string());
                    if snapshot {
                        pending.store(false, Ordering::Release);
                    }
                    result?;
                }
                Ok(())
            })
            .map_err(|error| format!("创建 IPC 写线程失败：{error}"))?;
        let reader = match thread::Builder::new()
            .name("feathrise-ipc-read".into())
            .spawn(move || {
                while let Some(message) =
                    read_frame(&mut read, &Limits::default()).map_err(|error| error.to_string())?
                {
                    // 满邮箱只阻塞后台读线程；反压自然传播到管道，绝不扩容或丢可靠回执。
                    if receive_queue.send_blocking(message).is_err() {
                        break;
                    }
                }
                Ok(())
            }) {
            Ok(reader) => reader,
            Err(error) => {
                outgoing.close();
                // spawn 本身只在后台槽调用，此时写线程还未收到任何帧，不存在管道等待。
                let _ = writer.join();
                return Err(format!("创建 IPC 读线程失败：{error}"));
            }
        };
        Ok(Self {
            outgoing,
            incoming,
            snapshot_pending,
            reader: Some(reader),
            writer: Some(writer),
            result: None,
        })
    }

    /// Full 返回原消息供协调器保留；快照另设单飞槽，避免四份最大正文排队吃满内存。
    pub(super) fn try_send(&self, message: Envelope) -> Result<(), TrySendError<Envelope>> {
        if self.outgoing.is_closed() {
            return Err(TrySendError::Closed(message));
        }
        let snapshot = message.payload.is_some();
        if snapshot
            && self
                .snapshot_pending
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return Err(TrySendError::Full(message));
        }
        match self.outgoing.try_send(message) {
            Ok(()) => Ok(()),
            Err(error) => {
                if snapshot {
                    self.snapshot_pending.store(false, Ordering::Release);
                }
                Err(error)
            }
        }
    }

    pub(super) fn try_recv(&self) -> Result<Option<Envelope>, String> {
        match self.incoming.try_recv() {
            Ok(message) => Ok(Some(message)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Closed) => Err("工程 IPC 接收通道已关闭".into()),
        }
    }

    pub(super) fn close_sender(&self) {
        self.outgoing.close();
    }

    /// 唤醒因邮箱满而等待的读线程；管道 I/O 的退出仍由进程协调器终止专属 Job 保证。
    pub(super) fn shutdown(&self) {
        self.close_sender();
        self.incoming.close();
    }

    pub(super) fn is_finished(&self) -> bool {
        self.reader.as_ref().is_none_or(JoinHandle::is_finished)
            && self.writer.as_ref().is_none_or(JoinHandle::is_finished)
    }

    pub(super) fn finish(&mut self) -> Result<(), String> {
        if let Some(result) = &self.result {
            return result.clone();
        }
        if !self.is_finished() {
            return Err("IPC 线程尚未确认退出".into());
        }
        let mut errors = Vec::new();
        for (name, handle) in [("读取", self.reader.take()), ("写入", self.writer.take())] {
            if let Some(handle) = handle {
                match handle.join() {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => errors.push(format!("{name}：{error}")),
                    Err(_) => errors.push(format!("{name}线程异常退出")),
                }
            }
        }
        let result = if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("；"))
        };
        self.result = Some(result.clone());
        result
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use feathrise_editor_protocol::{Message, Mode, SessionIdentity};
    use std::{
        os::windows::io::FromRawHandle,
        ptr,
        time::{Duration, Instant},
    };

    fn pipe() -> (File, File) {
        let mut read = ptr::null_mut();
        let mut write = ptr::null_mut();
        // SAFETY：两个返回句柄各转移一次所有权，只在当前测试内关闭。
        unsafe {
            assert_ne!(
                windows_sys::Win32::System::Pipes::CreatePipe(
                    &mut read,
                    &mut write,
                    ptr::null(),
                    4096
                ),
                0
            );
            (File::from_raw_handle(read), File::from_raw_handle(write))
        }
    }

    fn identity() -> SessionIdentity {
        SessionIdentity {
            project_id: "transport-test".into(),
            session: 7,
            generation: 1,
        }
    }

    fn wait_finished(transport: &Transport) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !transport.is_finished() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(2));
        }
        assert!(
            transport.is_finished(),
            "测试管道全部关闭后 IPC 线程必须退出"
        );
    }

    #[test]
    fn dedicated_pipes_roundtrip_and_confirm_thread_exit() {
        let (host_read, mut guest_write) = pipe();
        let (mut guest_read, host_write) = pipe();
        let mut transport = Transport::spawn(host_read, host_write).unwrap();
        let guest = thread::spawn(move || {
            let incoming = read_frame(&mut guest_read, &Limits::default())
                .unwrap()
                .unwrap();
            assert!(matches!(incoming.message, Message::Stop));
            write_frame(
                &mut guest_write,
                &Envelope::new(identity(), 1, Message::Stopped),
                &Limits::default(),
            )
            .unwrap();
        });
        transport
            .try_send(Envelope::new(identity(), 1, Message::Stop))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let received = loop {
            if let Some(message) = transport.try_recv().unwrap() {
                break message;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(2));
        };
        assert!(matches!(received.message, Message::Stopped));
        guest.join().unwrap();
        transport.shutdown();
        wait_finished(&transport);
        transport.finish().unwrap();
        transport.finish().unwrap();
    }

    #[test]
    fn reader_backpressure_is_cancelled_without_draining_the_mailbox() {
        let (host_read, mut guest_write) = pipe();
        let (guest_read, host_write) = pipe();
        let mut transport = Transport::spawn(host_read, host_write).unwrap();
        let guest = thread::spawn(move || {
            for sequence in 1..=32 {
                let message = Envelope::new(
                    identity(),
                    sequence,
                    Message::Status {
                        world_id: 1,
                        revision: 0,
                        frame: sequence,
                        ticks: 0,
                        mode: Mode::Editing,
                    },
                );
                if write_frame(&mut guest_write, &message, &Limits::default()).is_err() {
                    break;
                }
            }
        });
        thread::sleep(Duration::from_millis(30));
        transport.shutdown();
        drop(guest_read);
        guest.join().unwrap();
        wait_finished(&transport);
        transport.finish().unwrap();
    }
}
