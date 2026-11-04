//! 插件管理视图与单槽后台任务；回执绑定工程、全局根、操作范围和请求代次。
use super::{packages, types::*};
use bevy::{
    prelude::*,
    tasks::{IoTaskPool, Task},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Resource)]
pub(crate) struct PluginManagerState {
    pub(crate) open: bool,
    pub(crate) captured: bool,
    pub(crate) revision: u64,
    pub(crate) selected: Option<String>,
    pub(crate) rows: Vec<PackageEntry>,
    pub(crate) status: String,
    pub(crate) busy: bool,
    pub(crate) confirm_remove: Option<String>,
    pub(crate) load_states: BTreeMap<String, String>,
    pub(crate) initialization_error: Option<String>,
    pub(crate) root: PathBuf,
    pub(crate) roots: PluginRoots,
    pub(crate) scope: PluginScope,
    pub(super) task: Option<Task<ManagementResult>>,
    pub(super) sequence: u64,
    pub(super) loaded_revision: u64,
    pub(super) native_diagnostics: Option<super::super::resources::DiagnosticIdentity>,
    pub(super) refreshed: bool,
    pub(super) was_open: bool,
}
pub(super) struct ManagementResult {
    pub(super) roots: PluginRoots,
    pub(super) scope: PluginScope,
    pub(super) sequence: u64,
    pub(super) changed: bool,
    pub(super) native_diagnostics: Option<super::super::resources::DiagnosticIdentity>,
    pub(super) result: Result<Catalog, String>,
}
impl PluginManagerState {
    pub(crate) fn new(root: PathBuf) -> Self {
        match packages::roots_for_project(&root) {
            Ok(roots) => Self::with_roots(roots.project, roots.global),
            Err(error) => {
                let mut state = Self::with_roots(
                    Some(root.clone()),
                    root.join(".editor/unavailable-global-plugins"),
                );
                state.initialization_error = Some(error);
                state
            }
        }
    }
    pub(crate) fn for_global() -> Self {
        match packages::global_plugins_root() {
            Ok(global) => Self::with_roots(None, global),
            Err(error) => {
                let mut state = Self::with_roots(None, PathBuf::new());
                state.initialization_error = Some(error);
                state
            }
        }
    }
    /// 测试显式注入自有全局库，不能借测试修改真实 ~/.feathrise/plugins。
    pub(crate) fn with_roots(project: Option<PathBuf>, global: PathBuf) -> Self {
        let root = project.clone().unwrap_or_else(|| global.clone());
        let scope = if project.is_some() {
            PluginScope::Project
        } else {
            PluginScope::Global
        };
        Self {
            open: false,
            captured: false,
            revision: 1,
            selected: None,
            rows: Vec::new(),
            status: "打开插件管理器后读取插件".into(),
            busy: false,
            confirm_remove: None,
            load_states: BTreeMap::new(),
            initialization_error: None,
            root,
            roots: PluginRoots { project, global },
            scope,
            task: None,
            sequence: 0,
            loaded_revision: 0,
            native_diagnostics: None,
            refreshed: false,
            was_open: false,
        }
    }
    pub(crate) fn has_project(&self) -> bool {
        self.roots.project.is_some()
    }
    pub(crate) fn project_root(&self) -> Option<&Path> {
        self.roots.project.as_deref()
    }
    pub(crate) fn captures_input(&self) -> bool {
        self.open || self.captured
    }
    pub(crate) fn switch_scope(&mut self, scope: PluginScope) -> Result<(), String> {
        if self.busy {
            return Err("正在处理插件操作，请稍候".into());
        }
        if scope == PluginScope::Project && !self.has_project() {
            return Err("尚未打开工程，请管理全局插件".into());
        }
        if self.scope == scope {
            return Ok(());
        }
        self.scope = scope;
        self.selected = None;
        self.confirm_remove = None;
        self.rows.clear();
        self.load_states.clear();
        self.loaded_revision = 0;
        self.refreshed = false;
        self.refresh()
    }
    pub(crate) fn request(&mut self, command: ManagementCommand) -> Result<(), String> {
        self.request_at(self.scope, command)
    }
    pub(crate) fn request_at(
        &mut self,
        scope: PluginScope,
        command: ManagementCommand,
    ) -> Result<(), String> {
        self.start(scope, Some(command), None)
    }
    pub(crate) fn refresh(&mut self) -> Result<(), String> {
        self.refresh_scope(self.scope)
    }
    pub(crate) fn refresh_scope(&mut self, scope: PluginScope) -> Result<(), String> {
        self.start(scope, None, None)
    }
    pub(super) fn install_package(
        &mut self,
        source: PathBuf,
        expected: Option<String>,
    ) -> Result<(), String> {
        self.install_package_at(self.scope, source, expected)
    }
    pub(super) fn install_package_at(
        &mut self,
        scope: PluginScope,
        source: PathBuf,
        expected: Option<String>,
    ) -> Result<(), String> {
        self.start(
            scope,
            Some(ManagementCommand::Install {
                source,
                replace: expected.is_some(),
            }),
            expected,
        )
    }
    fn start(
        &mut self,
        scope: PluginScope,
        command: Option<ManagementCommand>,
        expected: Option<String>,
    ) -> Result<(), String> {
        if self.busy {
            return Err("正在处理插件操作，请稍候".into());
        }
        if scope == PluginScope::Project && !self.has_project() {
            return Err("尚未打开工程，请管理全局插件".into());
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or("插件管理请求身份已耗尽")?;
        let sequence = self.sequence;
        let roots = self.roots.clone();
        let native_diagnostics = self.native_diagnostics;
        self.busy = true;
        self.confirm_remove = None;
        self.status = if matches!(&command, Some(ManagementCommand::OpenDirectory)) {
            "正在打开插件目录…"
        } else if command.is_some() {
            "正在后台校验并记录变更…"
        } else {
            "正在读取插件…"
        }
        .into();
        self.revision = self.revision.wrapping_add(1);
        self.task = Some(IoTaskPool::get().spawn(async move {
            let changed = command.as_ref()
                .is_some_and(|command| !matches!(command, ManagementCommand::OpenDirectory));
            let cancelling = matches!(&command, Some(ManagementCommand::Cancel { .. }));
            let result = std::panic::catch_unwind(|| -> Result<Catalog, String> {
                let mut catalog = match command {
                    Some(ManagementCommand::OpenDirectory) => {
                        super::directory::open(&roots, scope)?;
                        let mut catalog = packages::catalog_at(&roots, scope)?;
                        catalog.completion = Some("已打开插件目录".into());
                        return Ok(catalog);
                    }
                    Some(command) => {
                        if let (Some(id), ManagementCommand::Install { source, .. }) =
                            (&expected, &command)
                        {
                            packages::stage_update_at(&roots, scope, source.clone(), id)?
                        } else {
                            packages::stage_at(&roots, scope, command)?
                        }
                    }
                    None => packages::catalog_at(&roots, scope)?,
                };
                if roots.project.is_none() && scope == PluginScope::Global {
                    // 首页没有本实例作者会话。真正提交不可变对象/索引后再报告生效；
                    // 刷新也恢复旧版留下的全局计划，其他进程已映射的旧对象始终保留。
                    let warnings = packages::apply_pending_with_roots(&roots);
                    catalog = packages::catalog_at(&roots, scope)?;
                    catalog.warnings.extend(warnings);
                    if changed && catalog.warnings.is_empty()
                        && catalog.entries.iter().all(|entry| entry.pending.is_none())
                    {
                        catalog.completion = Some(if cancelling {
                            "待生效变更已撤销；插件文件和作者数据保留"
                        } else {
                            "全局变更已生效；已有工程会话保留当前工具和作者数据"
                        }.into());
                    }
                }
                Ok(catalog)
            })
            .unwrap_or_else(|_| Err("插件管理后台异常；请刷新检查原文件和待生效计划".into()));
            ManagementResult {
                roots,
                scope,
                sequence,
                changed,
                native_diagnostics,
                result,
            }
        }));
        Ok(())
    }
}
pub(crate) struct PluginPackagePicker {
    pub(crate) picker: crate::folder_picker::FolderPicker,
    pub(super) expected: Option<String>,
    pub(super) scope: PluginScope,
    pub(super) roots: PluginRoots,
}
