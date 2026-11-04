//! 将校验和可选格式化收束为一份后台结果；不读取磁盘或操作作者会话。

use super::validation;
use feathrise_editor_documents_model::{
    history::types::Selection,
    json::types::{Formatted, ProcessedJson},
};

/// 对任意输入先做完整语法检查；错误不进入假定容器已配对的词元格式化器。
pub fn format(
    source: &str,
    crlf: bool,
    bom: bool,
    selection: Selection,
) -> Result<Option<Formatted>, String> {
    process(source, crlf, bom, Some(selection)).formatted
}

/// 必须在有界后台任务内调用；路径、修订与选择票据由宿主在采用时复核。
/// `selection`为空时只诊断；格式化与诊断共用一次语法扫描，保持原后台工作量。
pub fn process(source: &str, crlf: bool, bom: bool, selection: Option<Selection>) -> ProcessedJson {
    let error = validation::validate(source);
    let formatted = match (selection, &error) {
        (Some(selection), None) => super::format::pretty(source, crlf, bom, selection),
        (Some(_), Some(error)) => Err(format!("{}；无法格式化，原草稿保持", error.summary())),
        _ => Ok(None),
    };
    ProcessedJson { error, formatted }
}
