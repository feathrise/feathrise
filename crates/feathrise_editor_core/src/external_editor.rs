//! 编辑器选择的固定值；启动检测和外部进程仍由地图宿主负责。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalEditor {
    VsCode,
    Zed,
    RustRover,
}

impl ExternalEditor {
    pub const ALL: [Self; 3] = [Self::VsCode, Self::Zed, Self::RustRover];
    pub fn name(self) -> &'static str {
        match self {
            Self::VsCode => "VS Code",
            Self::Zed => "Zed",
            Self::RustRover => "RustRover",
        }
    }
}
