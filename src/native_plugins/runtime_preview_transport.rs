//! 预览专用管道的唯一读写线程；固定容量反压，退出必须核对真实线程结束。
use async_channel::{Receiver, Sender, TryRecvError, TrySendError};
use feathrise_runtime::project::author_preview::wire::{self, Packet};
use std::{
    fs::File,
    thread::{self, JoinHandle},
};

pub(super) struct Transport {
    outgoing: Sender<Packet>,
    incoming: Receiver<(Packet, Vec<u8>)>,
    reader: Option<JoinHandle<Result<(), String>>>,
    writer: Option<JoinHandle<Result<(), String>>>,
}

impl Transport {
    pub(super) fn spawn(mut read: File, mut write: File) -> Result<Self, String> {
        let (outgoing, send_queue) = async_channel::bounded::<Packet>(2);
        let (receive_queue, incoming) = async_channel::bounded(2);
        let writer = thread::Builder::new()
            .name("runtime-preview-write".into())
            .spawn(move || {
                while let Ok(packet) = send_queue.recv_blocking() {
                    wire::write(&mut write, &packet, &[])?;
                }
                Ok(())
            })
            .map_err(|error| format!("创建预览写线程失败：{error}"))?;
        let reader = match thread::Builder::new()
            .name("runtime-preview-read".into())
            .spawn(move || {
                while let Some(frame) = wire::read(&mut read)? {
                    if receive_queue.send_blocking(frame).is_err() {
                        break;
                    }
                }
                Ok(())
            }) {
            Ok(reader) => reader,
            Err(error) => {
                outgoing.close();
                let _ = writer.join(); // 尚无首包，线程只等邮箱；此函数仅在后台调用。
                return Err(format!("创建预览读线程失败：{error}"));
            }
        };
        Ok(Self {
            outgoing,
            incoming,
            reader: Some(reader),
            writer: Some(writer),
        })
    }

    pub(super) fn send(&self, packet: Packet) -> Result<(), TrySendError<Packet>> {
        self.outgoing.try_send(packet)
    }
    pub(super) fn receive(&self) -> Result<Option<(Packet, Vec<u8>)>, String> {
        match self.incoming.try_recv() {
            Ok(frame) => Ok(Some(frame)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Closed) => Err("工程预览控制管道已关闭".into()),
        }
    }
    pub(super) fn close_sender(&self) {
        self.outgoing.close();
    }
    pub(super) fn shutdown(&self) {
        self.outgoing.close();
        self.incoming.close();
    }
    pub(super) fn is_finished(&self) -> bool {
        self.reader.as_ref().is_none_or(JoinHandle::is_finished)
            && self.writer.as_ref().is_none_or(JoinHandle::is_finished)
    }
    pub(super) fn finish(&mut self) -> Result<(), String> {
        if !self.is_finished() {
            return Err("预览管道线程尚未确认收口".into());
        }
        for handle in [self.reader.take(), self.writer.take()]
            .into_iter()
            .flatten()
        {
            handle
                .join()
                .map_err(|_| "预览管道线程异常退出".to_string())??;
        }
        Ok(())
    }
}
impl Drop for Transport {
    fn drop(&mut self) {
        self.shutdown();
    }
}
