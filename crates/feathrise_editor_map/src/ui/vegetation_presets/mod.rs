//! F12植被预设的无分页三列连续列表。
//!
//! 【功能设计】目录项目全部进入左栏既有滚动区域；目录规模只影响最终实体数量，
//! 结构增删和热更重绑使用固定帧预算，稳定帧不扫描完整目录或UI层级。

mod systems; // 动态按钮按预算生成、重绑并在完整一致后恢复交互。

pub(crate) use systems::sync_vegetation_preset_list; // 插件Presentation阶段只依赖这一公开系统。
