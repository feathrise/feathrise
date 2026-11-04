//! 自有工具stdin的单槽后台发送；完整字节归写线程，取消后持续轮询并确认真实退役。

use super::BuildControl;
use std::{
    fs::File,
    io::{ErrorKind, Write},
    sync::{Arc, atomic::{AtomicBool, Ordering}},
    thread::{self, JoinHandle},
};
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;
#[cfg(unix)]
use std::os::fd::AsRawFd;
#[cfg(windows)]
use windows_sys::Win32::System::IO::CancelSynchronousIo;

const WRITE_CHUNK: usize = 64 * 1024;

pub(super) struct InputWriter {
    stopping: Arc<AtomicBool>,
    handle: JoinHandle<Result<(), String>>,
}

impl InputWriter {
    pub(super) fn spawn(input: File, bytes: Vec<u8>, control: BuildControl) -> Result<Self, String> {
        let stopping = Arc::new(AtomicBool::new(false));
        let worker_stop = stopping.clone();
        let handle = thread::Builder::new().name("feathrise-tool-stdin".into()).spawn(move || {
            write_input(input, bytes, control, worker_stop)
        }).map_err(|error| format!("无法创建构建stdin写线程：{error}"))?;
        Ok(Self { stopping, handle })
    }

    pub(super) fn is_finished(&self) -> bool { self.handle.is_finished() }

    pub(super) fn cancel(&self) {
        self.stopping.store(true, Ordering::Release);
        // 此句柄只属于本次专属writer，不是线程池。无在途I/O时取消可以未命中；
        // 协调器持续调用直到is_finished，覆盖刚检查停止后才进入WriteFile的竞态。
        #[cfg(windows)]
        unsafe { CancelSynchronousIo(self.handle.as_raw_handle()); }
        // Unix写端独占且已经O_NONBLOCK，最多等待一个25ms的poll片后再次检查取消位。
    }

    pub(super) fn finish(self) -> Result<(), String> {
        debug_assert!(self.is_finished(), "只能非阻塞回收已结束的stdin线程");
        self.handle.join().map_err(|_| "构建stdin写线程异常退出".to_string())?
    }
}

fn write_input(mut input: File, bytes: Vec<u8>, control: BuildControl, stopping: Arc<AtomicBool>) -> Result<(), String> {
    #[cfg(unix)]
    prepare_pipe(&input)?;
    for chunk in bytes.chunks(WRITE_CHUNK) {
        let mut remaining = chunk;
        while !remaining.is_empty() {
            if stopping.load(Ordering::Acquire) || control.is_cancelled() {
                return Err("构建stdin发送已取消，未宣称完整输入已经发送".into());
            }
            // 不使用write_all自动重试Interrupted：取消命中后必须先重新检查停止意图。
            match input.write(remaining) {
                Ok(0) => return Err("构建stdin写入未取得进展".into()),
                Ok(written) => remaining = &remaining[written..],
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                #[cfg(unix)]
                Err(error) if error.kind() == ErrorKind::WouldBlock => wait_writable(&input)?,
                Err(error) => return Err(format!("构建stdin写入失败：{error}")),
            }
        }
    }
    // 关闭唯一写端发布EOF；没有额外FlushFileBuffers等待子进程读取的同步屏障。
    drop(input);
    Ok(())
}

#[cfg(unix)]
fn prepare_pipe(input: &File) -> Result<(), String> {
    let fd = input.as_raw_fd();
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe { libc::fstat(fd, stat.as_mut_ptr()) } < 0 {
        return Err(format!("无法检查构建stdin管道：{}", std::io::Error::last_os_error()));
    }
    if unsafe { stat.assume_init() }.st_mode & libc::S_IFMT != libc::S_IFIFO {
        return Err("构建stdin必须是本次子进程独占的管道写端".into());
    }
    // native_process把父写端唯一移交给本worker，没有File克隆或共享的writer；
    // 因而改变open-file-description标志不会影响其他会话或线程的阻塞语义。
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(format!("无法设置构建stdin为非阻塞：{}", std::io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(unix)]
fn wait_writable(input: &File) -> Result<(), String> {
    let mut fd = libc::pollfd { fd: input.as_raw_fd(), events: libc::POLLOUT, revents: 0 };
    let result = unsafe { libc::poll(&mut fd, 1, 25) };
    if result < 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() != ErrorKind::Interrupted { return Err(format!("等待构建stdin失败：{error}")); }
    } else if fd.revents & libc::POLLNVAL != 0 {
        return Err("构建stdin管道已经关闭".into());
    }
    // 超时、EINTR和HUP都返回外层先检查取消；HUP的下一次非阻塞write会准确报告EPIPE。
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{io::Read, os::fd::FromRawFd, time::{Duration, Instant}};

    fn pipe() -> (File, File) {
        let mut fds = [-1; 2];
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        // 本测试独占两个新fd，之后均由File负责关闭，绝不构造第二个相同所有者。
        unsafe { (File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1])) }
    }
    fn finished(writer: &InputWriter) {
        let started = Instant::now();
        while !writer.is_finished() {
            assert!(started.elapsed() < Duration::from_secs(2), "自有stdin写线程未在有界取消片后退出");
            thread::sleep(Duration::from_millis(1));
        }
    }
    #[test]
    fn unix_writer_delivers_all_bytes_and_closes_the_unique_write_end() {
        let (mut read, write) = pipe();
        let bytes = (0..1024 * 1024).map(|n| (n % 251) as u8).collect::<Vec<_>>();
        let expected = bytes.clone();
        let reader = thread::spawn(move || { let mut output = Vec::new(); read.read_to_end(&mut output).unwrap(); output });
        let writer = InputWriter::spawn(write, bytes, BuildControl::default()).unwrap();
        finished(&writer); writer.finish().unwrap();
        assert_eq!(reader.join().unwrap(), expected);
    }
    #[test]
    fn unix_backpressure_cancellation_retires_the_actual_writer() {
        let (read, write) = pipe();
        let writer = InputWriter::spawn(write, vec![7; 8 * 1024 * 1024], BuildControl::default()).unwrap();
        // 保持读端但不消费，强制写满管道；取消不能依赖另一线程替自己读取或关闭fd。
        thread::sleep(Duration::from_millis(50));
        assert!(!writer.is_finished()); writer.cancel(); finished(&writer);
        assert!(writer.finish().unwrap_err().contains("取消")); drop(read);
    }
    #[test]
    fn unix_closed_reader_reports_failure_and_does_not_leave_a_writer() {
        let (read, write) = pipe(); drop(read);
        let writer = InputWriter::spawn(write, vec![1; 65536], BuildControl::default()).unwrap();
        finished(&writer); assert!(writer.finish().unwrap_err().contains("写入失败"));
    }
}
