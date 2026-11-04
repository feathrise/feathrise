//! IO池逐文件受检写盘，JSON校验逐项await CPU池；单个失败不影响后续文件。

use super::{
    super::{io, json},
    types::*,
};
use bevy::tasks::AsyncComputeTaskPool;
pub(super) async fn save_sources(
    ticket: u64, roots: super::super::source::SourceRoots, inputs: Vec<SaveInput>,
    content: feathrise_editor_documents_model::DocumentContentOperations,
) -> SaveReply {
    let mut files = Vec::with_capacity(inputs.len());
    for input in inputs {
        let bytes = input
            .pages
            .iter()
            .fold(if input.disk.bom { 3_usize } else { 0 }, |total, page| {
                total.saturating_add(page.len())
            });
        let checked = if bytes > super::super::constants::MAX_FILE_BYTES {
            Err("保存内容超过256 KiB；草稿已保留，请减少内容后再保存".into())
        } else if json::supported_for_save(&input.key.path) {
            let source = input.source;
            AsyncComputeTaskPool::get()
                .spawn(async move { json::validate_for_save(&source, content) })
                .await
        } else {
            Ok(())
        };
        let result =
            checked.and_then(|_| io::save_source(&roots, &input.key.path, &input.disk, &input.pages));
        files.push(FileReceipt {
            index: input.index,
            key: input.key,
            revision: input.revision,
            result,
        });
        // 当前快照/校验临时值在后台退役，主线程回执没有完整文本，且不排队第二个校验。
    }
    SaveReply {
        ticket,
        root: roots.assets,
        files,
    }
}
