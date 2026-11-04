//! 单窗口工作台与工程实例的有界stdio控制通道；UI线程不执行编码、文件IO或阻塞等待。
//!
//! 协议行由版本前缀、带会话token的JSON包络及换行组成。普通stdout仍作为诊断输出，
//! 不会被当成控制消息；每方向只保留一个后台线程，Drop仅关闭队列而不join阻塞读写。

use serde::{Serialize, de::DeserializeOwned};
use std::{
    io::{self, BufRead, BufReader, Read, Write},
    sync::{
        Arc, OnceLock,
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    },
    thread,
};

const PREFIX: &[u8] = b"FEATHRISE_WORKSPACE/1 ";
const PROTOCOL_FAMILY: &[u8] = b"FEATHRISE_WORKSPACE/";
// 仅传低频工程操作和最多32条最近记录的模型快照；四槽上限保持，不承载地图文档或逐帧ECS。
// 长中文路径与JSON转义可能使模型超过64KiB，512KiB上限仍限制每方向排队帧为最多2MiB。
pub(crate) const MAX_FRAME_BYTES: usize = 512 * 1024;
const QUEUE_CAPACITY: usize = 4;

pub(crate) struct Transport<In, Out> {
    outgoing: SyncSender<Out>,
    incoming: Receiver<Result<In, String>>,
    writer_failure: Arc<OnceLock<String>>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Incoming<T> {
    token: String,
    message: T,
}

#[derive(Serialize)]
struct Outgoing<'a, T> {
    token: &'a str,
    message: T,
}

impl<In, Out> Transport<In, Out>
where
    In: DeserializeOwned + Send + 'static,
    Out: Serialize + Send + 'static,
{
    pub(crate) fn new<R, W>(reader: R, writer: W, token: String) -> Result<Self, String>
    where
        R: Read + Send + 'static,
        W: Write + Send + 'static,
    {
        if token.is_empty() || token.len() > 256 || token.chars().any(char::is_control) {
            return Err("工程通信token为空、过长或包含控制字符".into());
        }
        let (outgoing, queued) = mpsc::sync_channel(QUEUE_CAPACITY);
        let (received, incoming) = mpsc::sync_channel(QUEUE_CAPACITY);
        let writer_failure = Arc::new(OnceLock::new());
        let failure = Arc::clone(&writer_failure);
        let write_events = received.clone();
        let write_token = token.clone();

        // 先启动写线程；若随后读线程创建失败，函数返回时outgoing关闭，写线程自行释放pipe。
        thread::Builder::new()
            .name("Feathrise工程通道写入".into())
            .spawn(move || write_frames(writer, queued, write_events, failure, &write_token))
            .map_err(|error| format!("无法启动工程通信写线程：{error}"))?;
        thread::Builder::new()
            .name("Feathrise工程通道读取".into())
            .spawn(move || read_frames(reader, received, &token))
            .map_err(|error| format!("无法启动工程通信读线程：{error}"))?;
        Ok(Self {
            outgoing,
            incoming,
            writer_failure,
        })
    }

    /// 只尝试入队。编码与完整write_all由写线程承担；队列满时明确拒绝，调用方保留意图重试。
    pub(crate) fn try_send(&self, message: Out) -> Result<(), String> {
        if let Some(error) = self.writer_failure.get() {
            return Err(error.clone());
        }
        self.outgoing
            .try_send(message)
            .map_err(|error| match error {
                TrySendError::Full(_) => "工程发送队列已满，请等待已提交命令收口后重试".into(),
                TrySendError::Disconnected(_) => self
                    .writer_failure
                    .get()
                    .cloned()
                    .unwrap_or_else(|| "工程通信写线程已退出".into()),
            })
    }

    /// 非阻塞接收一项。坏帧作为错误投递；读线程排空该行后仍可继续接收后续合法帧。
    pub(crate) fn try_recv(&self) -> Result<Option<In>, String> {
        match self.incoming.try_recv() {
            Ok(Ok(message)) => Ok(Some(message)),
            Ok(Err(error)) => Err(error),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(self
                .writer_failure
                .get()
                .cloned()
                .unwrap_or_else(|| "工程通信后台通道已结束".into())),
        }
    }
}

fn write_frames<In, Out>(
    mut writer: impl Write,
    queued: Receiver<Out>,
    events: SyncSender<Result<In, String>>,
    failure: Arc<OnceLock<String>>,
    token: &str,
) where
    Out: Serialize,
{
    while let Ok(message) = queued.recv() {
        let frame = match encode(token, message) {
            Ok(frame) => frame,
            Err(error) => {
                // 单条编码失败不污染pipe，也不终止后续合法命令；后台错误队列同样只有四槽。
                if events.send(Err(error)).is_err() {
                    break;
                }
                continue;
            }
        };
        if let Err(error) = writer.write_all(&frame).and_then(|_| writer.flush()) {
            let error = format!("工程通信写入失败：{error}");
            let _ = failure.set(error.clone());
            let _ = events.send(Err(error));
            break;
        }
    }
    // 不把writer泄露到主线程：发送方Drop后recv返回，正常结束即关闭本方向pipe。
}

fn encode(token: &str, message: impl Serialize) -> Result<Vec<u8>, String> {
    let mut frame = FrameBuffer {
        bytes: Vec::with_capacity(1024),
    };
    frame.bytes.extend_from_slice(PREFIX);
    serde_json::to_writer(&mut frame, &Outgoing { token, message })
        .map_err(|error| format!("工程通信消息编码失败或超过512KiB：{error}"))?;
    frame.bytes.push(b'\n');
    Ok(frame.bytes)
}

struct FrameBuffer {
    bytes: Vec<u8>,
}

impl Write for FrameBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        // 留一个字节给最终LF；即使Serialize一次交来大字符串，也不先分配整条超限JSON。
        if bytes.len() > (MAX_FRAME_BYTES - 1).saturating_sub(self.bytes.len()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "单条工程消息超过512KiB",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn read_frames<In>(reader: impl Read, events: SyncSender<Result<In, String>>, token: &str)
where
    In: DeserializeOwned,
{
    let mut reader = BufReader::new(reader);
    loop {
        let line = match read_bounded_line(&mut reader) {
            Ok(Some(line)) => line,
            Ok(None) => {
                let _ = events.send(Err("工程通信读取结束（EOF）".into()));
                return;
            }
            Err(error) => {
                let _ = events.send(Err(format!("工程通信读取失败：{error}")));
                return;
            }
        };
        if !line.bytes.starts_with(PROTOCOL_FAMILY) {
            forward_log(&line);
            continue;
        }
        let result = if line.oversized {
            Err("工程通信帧超过512KiB，已排空该行".into())
        } else if !line.terminated {
            Err("工程通信帧在换行前结束，未采用不完整消息".into())
        } else if !line.bytes.starts_with(PREFIX) {
            Err("工程通信协议前缀或版本不受支持".into())
        } else {
            let json = line.bytes[PREFIX.len()..].trim_ascii_end();
            serde_json::from_slice::<Incoming<In>>(json)
                .map_err(|error| format!("工程通信JSON帧无效：{error}"))
                .and_then(|envelope| {
                    if envelope.token != token {
                        Err("工程通信token身份不符，已拒绝该消息".into())
                    } else {
                        Ok(envelope.message)
                    }
                })
        };
        if events.send(result).is_err() {
            return;
        }
    }
}

struct BoundedLine {
    bytes: Vec<u8>,
    terminated: bool,
    oversized: bool,
}

fn read_bounded_line(reader: &mut impl BufRead) -> io::Result<Option<BoundedLine>> {
    let mut bytes = Vec::with_capacity(1024);
    let mut total = 0usize;
    loop {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            return Ok((total != 0).then_some(BoundedLine {
                bytes,
                terminated: false,
                oversized: total > MAX_FRAME_BYTES,
            }));
        }
        let (take, terminated) = match buffer.iter().position(|byte| *byte == b'\n') {
            Some(end) => (end + 1, true),
            None => (buffer.len(), false),
        };
        let keep = take.min(MAX_FRAME_BYTES.saturating_sub(bytes.len()));
        bytes.extend_from_slice(&buffer[..keep]);
        total = total.saturating_add(take);
        reader.consume(take);
        if terminated {
            return Ok(Some(BoundedLine {
                bytes,
                terminated: true,
                oversized: total > MAX_FRAME_BYTES,
            }));
        }
        // 超限后继续consume直到LF/EOF；内存不再增长，也不会把余下半行错当下一条消息。
    }
}

fn forward_log(line: &BoundedLine) {
    let text = String::from_utf8_lossy(&line.bytes);
    let text = text.trim_end_matches(['\r', '\n']);
    if !text.is_empty() {
        eprintln!(
            "{text}{}",
            if line.oversized {
                "（工程stdout行过长，后续已截断）"
            } else {
                ""
            }
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use std::{
        io::Cursor,
        sync::Mutex,
        time::{Duration, Instant},
    };

    #[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
    struct Message {
        sequence: u32,
        text: String,
    }

    fn message(sequence: u32) -> Message {
        Message {
            sequence,
            text: format!("消息 {sequence}"),
        }
    }

    fn wait_for<In: DeserializeOwned + Send + 'static, Out: Serialize + Send + 'static>(
        transport: &Transport<In, Out>,
    ) -> Result<In, String> {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match transport.try_recv() {
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(1)),
                Ok(None) => panic!("有界时间内没有收到通信结果"),
                Ok(Some(message)) => return Ok(message),
                Err(error) => return Err(error),
            }
        }
    }

    struct ChannelReader {
        receiver: Receiver<Vec<u8>>,
        current: Cursor<Vec<u8>>,
    }
    impl Read for ChannelReader {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            if bytes.is_empty() {
                return Ok(0);
            }
            if self.current.position() == self.current.get_ref().len() as u64 {
                match self.receiver.recv() {
                    Ok(next) => self.current = Cursor::new(next),
                    Err(_) => return Ok(0),
                }
            }
            self.current.read(bytes)
        }
    }
    struct ChannelWriter(SyncSender<Vec<u8>>);
    impl Write for ChannelWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0
                .send(bytes.to_vec())
                .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "测试对端已退出"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn two_bounded_transports_round_trip_typed_messages() {
        let (a_to_b, b_reader) = mpsc::sync_channel(4);
        let (b_to_a, a_reader) = mpsc::sync_channel(4);
        let a = Transport::<Message, Message>::new(
            ChannelReader {
                receiver: a_reader,
                current: Cursor::new(Vec::new()),
            },
            ChannelWriter(a_to_b),
            "same-session".into(),
        )
        .unwrap();
        let b = Transport::<Message, Message>::new(
            ChannelReader {
                receiver: b_reader,
                current: Cursor::new(Vec::new()),
            },
            ChannelWriter(b_to_a),
            "same-session".into(),
        )
        .unwrap();
        a.try_send(message(1)).unwrap();
        assert_eq!(wait_for(&b).unwrap(), message(1));
        b.try_send(message(2)).unwrap();
        assert_eq!(wait_for(&a).unwrap(), message(2));
    }

    #[test]
    fn wrong_token_is_rejected_then_next_valid_frame_is_received() {
        let mut bytes = encode("other-session", message(1)).unwrap();
        bytes.extend(encode("expected-session", message(2)).unwrap());
        let transport = Transport::<Message, Message>::new(
            Cursor::new(bytes),
            io::sink(),
            "expected-session".into(),
        )
        .unwrap();
        assert!(wait_for(&transport).unwrap_err().contains("身份不符"));
        assert_eq!(wait_for(&transport).unwrap(), message(2));
    }

    #[test]
    fn large_recent_project_model_above_64_kib_is_received_without_truncation() {
        #[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
        struct Recent {
            name: String,
            path: String,
            last_opened_unix_seconds: u64,
        }
        #[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
        struct Model {
            default_project_directory: String,
            recent_projects: Vec<Recent>,
            current_path_input: String,
        }
        let model = Model {
            default_project_directory: "C:\\Users\\示例用户\\feathrise".into(),
            recent_projects: (0..32)
                .map(|index| Recent {
                    name: format!("示例工程 {index}"),
                    path: format!("D:\\Projects\\{}\\工程 {index}", "中文目录".repeat(500)),
                    last_opened_unix_seconds: index,
                })
                .collect(),
            current_path_input: "路径输入".repeat(1000),
        };
        let bytes = encode("session", &model).unwrap();
        assert!(bytes.len() > 64 * 1024);
        assert!(bytes.len() <= MAX_FRAME_BYTES);
        let transport =
            Transport::<Model, Message>::new(Cursor::new(bytes), io::sink(), "session".into())
                .unwrap();
        assert_eq!(wait_for(&transport).unwrap(), model);
    }

    #[test]
    fn exactly_512_kib_including_prefix_and_newline_is_a_valid_frame() {
        let overhead = encode(
            "session",
            Message {
                sequence: 9,
                text: String::new(),
            },
        )
        .unwrap()
        .len();
        let message = Message {
            sequence: 9,
            text: "x".repeat(MAX_FRAME_BYTES - overhead),
        };
        let bytes = encode("session", &message).unwrap();
        assert_eq!(bytes.len(), MAX_FRAME_BYTES);
        let transport =
            Transport::<Message, Message>::new(Cursor::new(bytes), io::sink(), "session".into())
                .unwrap();
        assert_eq!(wait_for(&transport).unwrap(), message);
    }

    #[test]
    fn oversized_line_is_drained_without_losing_the_following_frame() {
        let mut bytes = PREFIX.to_vec();
        bytes.extend(vec![b'x'; MAX_FRAME_BYTES * 3]);
        bytes.push(b'\n');
        bytes.extend(encode("session", message(3)).unwrap());
        let transport =
            Transport::<Message, Message>::new(Cursor::new(bytes), io::sink(), "session".into())
                .unwrap();
        assert!(wait_for(&transport).unwrap_err().contains("已排空"));
        assert_eq!(wait_for(&transport).unwrap(), message(3));
    }

    #[test]
    fn ordinary_stdout_is_not_protocol_and_bad_json_does_not_poison_the_stream() {
        let mut bytes = b"normal project output\nFEATHRISE_WORKSPACE/1 {broken\n".to_vec();
        bytes.extend(encode("session", message(4)).unwrap());
        let transport =
            Transport::<Message, Message>::new(Cursor::new(bytes), io::sink(), "session".into())
                .unwrap();
        assert!(wait_for(&transport).unwrap_err().contains("JSON帧无效"));
        assert_eq!(wait_for(&transport).unwrap(), message(4));
        assert!(wait_for(&transport).unwrap_err().contains("EOF"));
    }

    struct BrokenWriter;
    impl Write for BrokenWriter {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "对端退出"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn broken_pipe_is_reported_and_future_sends_fail() {
        let transport = Transport::<Message, Message>::new(
            Cursor::new(Vec::<u8>::new()),
            BrokenWriter,
            "session".into(),
        )
        .unwrap();
        transport.try_send(message(5)).unwrap();
        let first = wait_for(&transport).unwrap_err();
        let error = if first.contains("EOF") {
            wait_for(&transport).unwrap_err()
        } else {
            first
        };
        assert!(error.contains("写入失败"));
        assert!(
            transport
                .try_send(message(6))
                .unwrap_err()
                .contains("写入失败")
        );
    }

    struct BlockingWriter {
        entered: Option<SyncSender<()>>,
        release: Receiver<()>,
    }
    impl Write for BlockingWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if let Some(entered) = self.entered.take() {
                entered.send(()).unwrap();
                self.release.recv().unwrap();
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn four_slot_backpressure_rejects_without_waiting_for_the_writer() {
        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let transport = Transport::<Message, Message>::new(
            Cursor::new(Vec::<u8>::new()),
            BlockingWriter {
                entered: Some(entered_tx),
                release: release_rx,
            },
            "session".into(),
        )
        .unwrap();
        transport.try_send(message(0)).unwrap();
        entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        for number in 1..=QUEUE_CAPACITY {
            transport.try_send(message(number as u32)).unwrap();
        }
        assert!(
            transport
                .try_send(message(7))
                .unwrap_err()
                .contains("队列已满")
        );
        release_tx.send(()).unwrap();
    }

    #[test]
    fn oversized_outgoing_frame_never_reaches_the_writer() {
        #[derive(Clone)]
        struct Capture(Arc<Mutex<Vec<u8>>>);
        impl Write for Capture {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let capture = Arc::new(Mutex::new(Vec::new()));
        let transport = Transport::<Message, Message>::new(
            Cursor::new(Vec::<u8>::new()),
            Capture(capture.clone()),
            "session".into(),
        )
        .unwrap();
        transport
            .try_send(Message {
                sequence: 8,
                text: "x".repeat(MAX_FRAME_BYTES),
            })
            .unwrap();
        let first = wait_for(&transport).unwrap_err();
        let error = if first.contains("EOF") {
            wait_for(&transport).unwrap_err()
        } else {
            first
        };
        assert!(error.contains("超过512KiB"));
        assert!(capture.lock().unwrap().is_empty());
    }
}
