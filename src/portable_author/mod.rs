//! Unix同窗口作者承载：保持独立工程World，通过有界共享帧与独立输入通道复用原作者界面。

// Windows正式作者使用原生子窗口；仍保留共享帧的跨平台单元与显式GPU回归。
#[cfg(any(unix, test))]
mod author;
#[cfg(any(unix, test))]
mod capture;
#[cfg(any(unix, test))]
mod frames;
#[cfg(any(unix, test))]
pub(crate) mod host;
#[cfg(any(unix, test))]
mod resources;
#[cfg(any(unix, test))]
mod session;
#[cfg(any(unix, test))]
mod types;
#[cfg(all(test, feature = "validation"))]
mod validation;

#[cfg(unix)]
pub(crate) use author::EmbeddedAuthorPlugin;
#[cfg(unix)]
pub(crate) use resources::EmbeddedAuthorStatus;
#[cfg(unix)]
pub(crate) use session::HostSession;
#[cfg(unix)]
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
