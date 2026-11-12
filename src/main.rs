//! Feathrise 的进程入口，保留窗口子系统设置、Bevy DLL链接和退出状态。

// 普通 Windows Release 作为桌面应用启动；开发与显式验收仍保留控制台诊断。
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions), not(feature = "validation")),
    windows_subsystem = "windows"
)]

// 最终程序直接接收 Bevy DLL，库级 Bevy 依赖可以先发布元数据。
#[cfg(all(feature = "bevy_dynamic_linking", not(target_family = "wasm")))]
extern crate bevy_dylib as _;

fn main() {
    if let Err(error) = feathrise_host::run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
