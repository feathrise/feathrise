//! Unix同窗口作者承载：保持独立工程World，通过有界共享帧与独立输入通道复用原作者界面。

mod author;
mod capture;
mod frames;
pub(crate) mod host;
mod resources;
mod session;
mod types;
#[cfg(all(test, feature = "validation"))]
mod validation;

pub(crate) use author::EmbeddedAuthorPlugin;
pub(crate) use resources::EmbeddedAuthorStatus;
pub(crate) use session::HostSession;
pub(crate) use types::EmbedTarget;

/// macOS基础作者使用Metal；其他桌面继续Vulkan，厂商增强渲染不属于本次Unix接入。
pub(crate) fn backend() -> bevy::render::settings::Backends {
    #[cfg(target_os = "macos")]
    {
        bevy::render::settings::Backends::METAL
    }
    #[cfg(not(target_os = "macos"))]
    {
        bevy::render::settings::Backends::VULKAN
    }
}
