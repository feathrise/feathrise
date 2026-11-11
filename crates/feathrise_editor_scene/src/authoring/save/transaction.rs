//! 编辑器与无窗口工具共用同一流式跨文件事务，避免两套发布和回滚实现。

pub(crate) use feathrise_scene_content::authoring_transaction::*;
