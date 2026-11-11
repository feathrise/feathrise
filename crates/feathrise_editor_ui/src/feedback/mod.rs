//! 面板反馈只声明来源，不依赖场景外壳或重新定义保存、加载和预览事务。

mod components;
mod types;
pub use components::{GlobalPanelStatus, PanelStatusState, PanelStatusText};
pub use types::{PanelStatusPhase, PanelStatusProjection};
