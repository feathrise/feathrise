//! 有界闭壳网格的纯拓扑操作；只生成完整候选记录，输入、预览和历史由编辑状态机统一采用。

mod bevel;
mod cuts;
mod inset;
mod mesh;
mod subdivide;

pub(super) use bevel::bevel_record;
pub(super) use cuts::{knife_record, loop_cut_record};
pub(super) use inset::inset_record;
pub(super) use subdivide::subdivide_record;

#[cfg(test)]
mod tests;
