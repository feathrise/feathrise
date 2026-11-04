//! 边孔颜色专项的进程角色与复用磁盘助手所需的有界快照。
use std::time::SystemTime;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Role { Root, Author }
impl Role { pub fn name(self) -> &'static str { if self == Self::Root { "root" } else { "author" } } }
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FileStamp { pub bytes: Vec<u8>, pub modified: SystemTime, pub sha256: String }
pub(super) fn checkpoint(role: Role, stage: u8) -> Option<(&'static str,bool)> {
    match (role,stage) {
        (Role::Root,0) => Some(("home-closed",false)), (Role::Root,1) => Some(("home-settings",true)),
        (Role::Root,2) => Some(("home-restored",false)), (Role::Root,4) => Some(("home-returned",false)),
        (Role::Author,0) => Some(("author-closed",false)), (Role::Author,1) => Some(("author-settings",true)),
        (Role::Author,2) => Some(("author-restored",false)), _ => None,
    }
}
pub(super) fn target(role: Role, stage: u8) -> Option<&'static str> {
    match (role,stage) { (_,1)=>Some("设置"),(_,2)=>Some("完成"),(Role::Author,3)=>Some("工程"),(Role::Author,4)=>Some("关闭工程"),_=>None }
}
