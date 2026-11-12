//! 蒙皮工具标识，按钮分派与原权重会话共用。

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Point,
    Box,
    Bone,
    Set,
    Add,
    Subtract,
    Smooth,
}

impl Tool {
    pub fn paint(self) -> bool {
        matches!(self, Self::Add | Self::Subtract | Self::Smooth)
    }
}
