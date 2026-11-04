//! 复用真实ScreenshotCaptured落盘助手的文件快照类型。
#[derive(Clone,Debug,PartialEq,Eq)]
pub(super) struct FileStamp {
    pub bytes:Vec<u8>,
    pub modified:std::time::SystemTime,
    pub sha256:String,
}
