//! JSON诊断、保真格式化和选区映射的后台结果，不含宿主任务身份。

use crate::history::types::Selection;
use std::{ops::Range, sync::Arc};

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub line: usize,
    pub column: usize, // 显示Unicode字符列，不能把serde_json的字节列当作中文字符列。
    pub selection: Range<usize>,
    pub message: String,
}

impl Diagnostic {
    pub fn summary(&self) -> String {
        format!(
            "JSON第{}行，第{}列：{}",
            self.line, self.column, self.message
        )
    }
}

#[derive(Debug)]
pub struct Formatted {
    pub source: Arc<str>,
    pub pages: Vec<Arc<str>>,
    pub selection: Selection,
}

/// 校验与可选格式化共享一次语法扫描，宿主再绑定路径、修订与操作票据。
#[derive(Debug)]
pub struct ProcessedJson {
    pub error: Option<Diagnostic>,
    pub formatted: Result<Option<Formatted>, String>,
}
