//! 一次工程会话的后台边界：共享帧环与独立输入socket，均受token、队列和生命周期限制。

use super::{frames::FrameRing, types::*};
use crate::workspace_transport::Transport;
use std::{
    fs::OpenOptions,
    net::{Shutdown, TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::JoinHandle,
    time::Duration,
};

/// 父方拥有私有会话目录；析构仅发停止信号，worker与文件回收放到后台完成。
pub(crate) struct HostSession {
    pub(super) token: String,
    pub(super) geometry: Geometry,
    pub(super) image: Option<bevy::prelude::Handle<bevy::prelude::Image>>,
    pub(super) last_frame: u64,
    pub(super) first_frame: bool,
    pub(super) input: Option<Transport<Feedback, InputBatch>>,
    pub(super) socket: Option<TcpStream>,
    pub(super) connection: Receiver<Result<(Transport<Feedback, InputBatch>, TcpStream), String>>,
    pub(super) latest: Arc<Mutex<Option<Frame>>>,
    pub(super) error: Arc<OnceLock<String>>,
    directory: PathBuf,
    stop: Arc<AtomicBool>,
    workers: Vec<JoinHandle<()>>,
}

impl HostSession {
    pub(crate) fn new(token: &str) -> Result<Self, String> {
        let directory = std::env::temp_dir().join(format!("feathrise-author-{token}"));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(&directory)
            .map_err(|error| format!("无法创建作者私有会话：{error}"))?;
        let directory = directory
            .canonicalize()
            .map_err(|error| format!("无法固定作者会话目录：{error}"))?;
        let result = Self::prepare(directory.clone(), token);
        if result.is_err() {
            // 只回收本次create独占目录中的固定文件，不能递归删除外部路径。
            for name in ["frames.bin", "endpoint.json"] {
                let _ = std::fs::remove_file(directory.join(name));
            }
            let _ = std::fs::remove_dir(directory);
        }
        result
    }

    fn prepare(directory: PathBuf, token: &str) -> Result<Self, String> {
        let ring = FrameRing::create(&directory.join("frames.bin"), token)?;
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .map_err(|error| error.to_string())?;
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let endpoint = listener.local_addr().map_err(|error| error.to_string())?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        use std::io::Write;
        options
            .open(directory.join("endpoint.json"))
            .and_then(|mut file| file.write_all(endpoint.to_string().as_bytes()))
            .map_err(|error| format!("无法登记作者输入会话：{error}"))?;
        let stop = Arc::new(AtomicBool::new(false));
        let latest = Arc::new(Mutex::new(None));
        let error = Arc::new(OnceLock::new());
        let (connected, connection) = mpsc::sync_channel(1);
        let token_owned = token.to_owned();
        let cancel = stop.clone();
        let accept = std::thread::spawn(move || {
            while !cancel.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, address)) => {
                        if !address.ip().is_loopback() {
                            continue;
                        }
                        // accept继承非阻塞标记的行为因OS而异；Transport只在后台做阻塞读写，
                        // 必须显式恢复blocking，不能把暂时WouldBlock误判成会话断线。
                        let result = stream
                            .set_nonblocking(false)
                            .and_then(|()| stream.set_nodelay(true))
                            .map_err(|error| error.to_string())
                            .and_then(|()| stream.try_clone().map_err(|error| error.to_string()))
                            .and_then(|read| {
                                Transport::new(
                                    read,
                                    stream.try_clone().map_err(|error| error.to_string())?,
                                    token_owned.clone(),
                                )
                            })
                            .map(|transport| (transport, stream));
                        let _ = connected.try_send(result);
                        break;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Err(error) => {
                        let _ = connected.try_send(Err(error.to_string()));
                        break;
                    }
                }
            }
        });
        let cancel = stop.clone();
        let frame_out = latest.clone();
        let failed = error.clone();
        let frames = std::thread::spawn(move || {
            let mut sequence = 0;
            while !cancel.load(Ordering::Acquire) {
                match ring.latest(sequence) {
                    Ok(Some(frame)) => {
                        sequence = frame.sequence;
                        if let Ok(mut output) = frame_out.lock() {
                            *output = Some(frame);
                        }
                    }
                    Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                    Err(error) => {
                        let _ = failed.set(error);
                        break;
                    }
                }
            }
        });
        Ok(Self {
            token: token.to_owned(),
            geometry: Geometry::initial(),
            image: None,
            last_frame: 0,
            first_frame: false,
            input: None,
            socket: None,
            connection,
            latest,
            error,
            directory,
            stop,
            workers: vec![accept, frames],
        })
    }

    pub(crate) fn configure(&self, command: &mut std::process::Command, parent_window: usize) {
        command.env("FEATHRISE_WORKSPACE_SESSION", &self.directory);
        if parent_window != 0 {
            command.env(
                "FEATHRISE_WORKSPACE_PARENT_WINDOW",
                format!("x11:{parent_window}"),
            );
        } else {
            command.env_remove("FEATHRISE_WORKSPACE_PARENT_WINDOW");
        }
        command.env_remove("FEATHRISE_WORKSPACE_PARENT_HWND");
    }

    pub(crate) fn matches(&self, token: &str) -> bool {
        self.token == token
    }

    #[cfg(all(test, feature = "validation"))]
    pub(super) fn owned_fixture_target(&self) -> EmbedTarget {
        EmbedTarget {
            directory: self.directory.clone(),
            token: self.token.clone(),
            #[cfg(unix)]
            pid: unsafe { libc::getppid() } as u32,
            #[cfg(windows)]
            pid: std::process::id(),
        }
    }
    pub(crate) fn ready(&self) -> bool {
        self.first_frame
    }

    pub(super) fn connect(&mut self) -> Result<(), String> {
        if self.input.is_none() {
            match self.connection.try_recv() {
                Ok(Ok((input, socket))) => {
                    self.input = Some(input);
                    self.socket = Some(socket);
                }
                Ok(Err(error)) => return Err(error),
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => return Err("作者输入通道未建立".into()),
            }
        }
        if let Some(error) = self.error.get() {
            return Err(error.clone());
        }
        Ok(())
    }
}

impl Drop for HostSession {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(socket) = &self.socket {
            let _ = socket.shutdown(Shutdown::Both);
        }
        let workers = std::mem::take(&mut self.workers);
        let directory = self.directory.clone();
        std::thread::spawn(move || {
            for worker in workers {
                let _ = worker.join();
            }
            for name in ["frames.bin", "endpoint.json"] {
                let _ = std::fs::remove_file(directory.join(name));
            }
            let _ = std::fs::remove_dir(directory);
        });
    }
}

pub(super) struct AuthorSession {
    pub input: Transport<InputBatch, Feedback>,
    pub frames: SyncSender<MappedFrame>,
    pub published: Arc<AtomicU64>,
    pub error: Arc<OnceLock<String>>,
    socket: TcpStream,
}

impl AuthorSession {
    pub fn open(target: &EmbedTarget) -> Result<Self, String> {
        if target
            .directory
            .canonicalize()
            .map_err(|error| error.to_string())?
            != target.directory
        {
            return Err("作者共享会话目录身份已变化".into());
        }
        let ring = FrameRing::open(&target.directory.join("frames.bin"), &target.token)?;
        let endpoint: std::net::SocketAddr =
            std::fs::read_to_string(target.directory.join("endpoint.json"))
                .map_err(|error| error.to_string())?
                .parse()
                .map_err(|_| "作者输入通道地址无效")?;
        if !endpoint.ip().is_loopback() {
            return Err("作者输入通道必须是本机地址".into());
        }
        let socket = TcpStream::connect_timeout(&endpoint, Duration::from_secs(5))
            .map_err(|error| format!("无法连接作者输入：{error}"))?;
        socket
            .set_nonblocking(false)
            .map_err(|error| error.to_string())?;
        socket
            .set_nodelay(true)
            .map_err(|error| error.to_string())?;
        let input = Transport::new(
            socket.try_clone().map_err(|error| error.to_string())?,
            socket.try_clone().map_err(|error| error.to_string())?,
            target.token.clone(),
        )?;
        let (frames, receiver) = mpsc::sync_channel::<MappedFrame>(1);
        let published = Arc::new(AtomicU64::new(0));
        let error = Arc::new(OnceLock::new());
        let complete = published.clone();
        let failed = error.clone();
        std::thread::spawn(move || {
            while let Ok(frame) = receiver.recv() {
                let sequence = frame.sequence;
                let slice = frame.buffer.slice(..);
                let pixels = slice.get_mapped_range();
                let result = ring.publish_mapped(frame.geometry, sequence, &pixels);
                drop(pixels);
                frame.buffer.unmap();
                match result {
                    Ok(()) => complete.store(sequence, Ordering::Release),
                    Err(error) => {
                        let _ = failed.set(error);
                        frame.completed.store(sequence, Ordering::Release);
                        break;
                    }
                }
                // 只有完整写入且GPU缓冲已unmap后归还single-flight槽；RenderApp才能安全复用该Buffer。
                frame.completed.store(sequence, Ordering::Release);
            }
        });
        Ok(Self {
            input,
            frames,
            published,
            error,
            socket,
        })
    }
}

impl Drop for AuthorSession {
    fn drop(&mut self) {
        let _ = self.socket.shutdown(Shutdown::Both);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn accepted_input_socket_uses_background_blocking_io_and_round_trips_both_directions() {
        let token = format!(
            "input-fixture-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let mut host = HostSession::new(&token).unwrap();
        let target = EmbedTarget {
            directory: host.directory.clone(),
            pid: std::process::id(),
            token,
        };
        let author = AuthorSession::open(&target).unwrap();
        let since = Instant::now();
        while host.input.is_none() {
            host.connect().unwrap();
            assert!(since.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(5));
        }
        // 刻意让连接暂时空闲，覆盖accept继承nonblocking后后台BufRead立即报WouldBlock的回归。
        std::thread::sleep(Duration::from_millis(30));
        assert!(author.input.try_recv().unwrap().is_none());
        assert!(host.input.as_ref().unwrap().try_recv().unwrap().is_none());
        host.input
            .as_ref()
            .unwrap()
            .try_send(InputBatch {
                geometry: Geometry::initial(),
                active: true,
                focused: true,
                cursor_position: Some(bevy::prelude::Vec2::new(12., 24.)),
                events: Vec::new(),
            })
            .unwrap();
        loop {
            if let Some(batch) = author.input.try_recv().unwrap() {
                assert_eq!(
                    batch.cursor_position,
                    Some(bevy::prelude::Vec2::new(12., 24.))
                );
                assert!(batch.active && batch.focused);
                break;
            }
            assert!(since.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(5));
        }
        author
            .input
            .try_send(Feedback {
                epoch: 1,
                ime_enabled: true,
                ime_position: bevy::prelude::Vec2::new(3., 4.),
                cursor: bevy::window::CursorOptions::default(),
                cursor_icon: None,
            })
            .unwrap();
        loop {
            if let Some(feedback) = host.input.as_ref().unwrap().try_recv().unwrap() {
                assert!(feedback.ime_enabled);
                assert_eq!(feedback.ime_position, bevy::prelude::Vec2::new(3., 4.));
                break;
            }
            assert!(since.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
