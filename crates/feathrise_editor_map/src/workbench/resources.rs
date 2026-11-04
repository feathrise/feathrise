//! 独立宿主提供的轻量UI快照与真实布局回报，不保存作者数据或World指针。
use super::{EditorAction, WorkbenchMap, types::CatalogResult};
use bevy::{prelude::*, tasks::Task};
use std::{collections::HashMap, sync::Arc};

/// 无工程宿主交接偏好所有权时暂缓修改；不暴露宿主进程、文件或保存状态给共享工作台。
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkbenchSettingsAccess {
    pub editable: bool,
}

impl Default for WorkbenchSettingsAccess {
    fn default() -> Self {
        Self { editable: true }
    }
}

#[derive(Resource, Clone)]
pub struct WorkbenchModel {
    /// 工程切换代次；相同地图ID也不能接受前一工程的鼠标或后台结果。
    pub context: u64,
    pub visible: bool,
    pub modal: bool,
    pub busy: bool,
    pub active_map: Option<String>,
    pub active_map_label: Option<String>,
    pub selected_map: Option<String>,
    pub maps: Arc<Vec<WorkbenchMap>>,
    /// 宿主后台已核验的当前工程内容根；None时禁止回退到编辑器自身仓库。
    pub asset_root: Option<std::path::PathBuf>,
    /// 工程文件视图的显式根；不能从资产根的父目录推断，以兼容嵌套资产目录。
    pub project_root: Option<std::path::PathBuf>,
    pub status: String,
    /// 只有已接到真实宿主业务的动作才可点击，迁移期间不能伪装功能已接通。
    pub enabled_actions: Vec<EditorAction>,
}

impl Default for WorkbenchModel {
    fn default() -> Self {
        Self {
            context: 0,
            visible: true,
            modal: false,
            busy: false,
            active_map: None,
            active_map_label: None,
            selected_map: None,
            maps: Arc::default(),
            asset_root: None,
            project_root: None,
            status: String::new(),
            enabled_actions: Vec::new(),
        }
    }
}

#[derive(Resource, Default, Clone)]
pub struct WorkbenchProbe {
    pub asset_browser: super::types::AssetBrowserProbe,
    pub overlay: bool,
    /// 真实UI布局给出的物理像素矩形；原生游戏窗口跟随它，不猜固定边距。
    pub viewport: Option<[f32; 4]>,
    pub actions: Vec<(EditorAction, [f32; 4], bool)>,
    pub maps: Vec<(usize, [f32; 4])>,
    pub selected_map: Option<String>,
    pub selected_index: Option<usize>,
    pub context: u64,
}

/// 一份构建、一份退役：旧投影和索引的析构也留在后台，禁止在UI帧释放整张目录。
#[derive(Resource, Default)]
pub(super) struct CatalogJob {
    pub task: Option<Task<CatalogResult>>,
    pub retirement: Option<Task<()>>,
    pub adopted: Option<(u64, Arc<Vec<WorkbenchMap>>)>,
    pub indices: HashMap<String, usize>,
}
