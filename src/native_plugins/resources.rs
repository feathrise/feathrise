//! 主线程只持有有界快照和通道；动态库、会话指针、文件与领域草稿均留在后台。
use super::loader;
pub(super) use super::types::{Event, Job, Tool};
use bevy::prelude::*;
use feathrise_plugin_api::*;
/// 诊断身份与普通业务快照分离；新来源有独立实例，避免重建后相同修订漏报。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct DiagnosticIdentity {
    pub instance: u64,
    pub revision: u64,
}
impl Default for DiagnosticIdentity {
    fn default() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self { instance: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed), revision: 1 }
    }
}
impl DiagnosticIdentity {
    /// 只有实际写入或替换诊断时调用；地图、表单等普通快照更新不得推进它。
    pub fn advance(&mut self) { self.revision = self.revision.wrapping_add(1); }
}
#[derive(Resource)]
pub(super) struct NativeTools {
    pub tools: std::collections::BTreeMap<usize, Tool>,
    pub receiver: async_channel::Receiver<Event>,
    pub discovery_pending: bool,
    pub loading: usize,
    pub errors: Vec<String>,
    pub management_errors: Vec<String>,
    pub load_states: std::collections::BTreeMap<String, String>,
    pub load_origins: std::collections::BTreeMap<String, super::management::types::PluginOrigin>,
    pub ui_revision: u64,
    pub diagnostics: DiagnosticIdentity,
    pub input_pending: bool,
    pub save_tickets: std::collections::HashMap<u64, (u64, u64)>,
}
impl NativeTools {
    /// 仅消解本实例从未装载、且已经真实归档的同一旧目录来源。调用侧还须冻结诊断实例代次。
    /// Ok(false)表示没有该旧诊断，Err表示来源或现状不够明确，需要重开重新检查。
    pub(crate) fn clear_archived_legacy(
        &mut self,
        archived: &super::management::types::ArchivedLegacy,
        catalog: &[super::management::types::PackageEntry],
    ) -> Result<bool, ()> {
        let warning = format!("{}：{}", archived.id, archived.error);
        let status = format!("加载失败：{}", archived.error);
        let matches = self.errors.iter().filter(|error| *error == &warning).count();
        if !archived.original_vacant || self.discovery_pending
            || self.tools.values().any(|tool| tool.domain == archived.id)
            || catalog.iter().any(|entry| entry.path == archived.origin.path || entry.id == archived.id)
        {
            return Err(());
        }
        if matches == 0 && self.load_states.get(&archived.id) != Some(&status) {
            return Ok(false);
        }
        if matches != 1
            || self.load_origins.get(&archived.id) != Some(&archived.origin)
            || self.load_states.get(&archived.id) != Some(&status)
        {
            return Err(());
        }
        self.errors.retain(|error| error != &warning);
        self.load_states.remove(&archived.id);
        self.load_origins.remove(&archived.id);
        self.diagnostics.advance();
        self.ui_revision = self.ui_revision.wrapping_add(1);
        Ok(true)
    }

    pub fn start(project: &crate::project::Project) -> Self {
        Self::start_inner(project, super::management::packages::roots_for_project(&project.root))
    }

    /// 自动验收注入本轮自有全局库；生产默认路径仍统一来自当前用户主目录。
    #[cfg(all(test, feature = "validation"))]
    pub(crate) fn start_with_roots(project: &crate::project::Project, roots: super::management::types::PluginRoots) -> Self {
        Self::start_inner(project, Ok(roots))
    }

    fn start_inner(project: &crate::project::Project, roots: Result<super::management::types::PluginRoots, String>) -> Self {
        let root = project.root.clone();
        let project_id = project.id.clone();
        let asset_root = project.context.asset_roots[0].clone();
        let (sender, receiver) = async_channel::bounded(32);
        let spawn = std::thread::Builder::new()
            .name("native-plugin-discovery".into())
            .spawn(move || {
                let roots = match roots {
                    Ok(roots) if roots.project.as_deref() == Some(root.as_path()) => roots,
                    Ok(_) => {
                        let _ = sender.send_blocking(Event::Discovered { count: 0, states: Default::default(), origins: Default::default(), warnings: vec!["插件来源与当前工程根不一致".into()] });
                        return;
                    }
                    Err(error) => {
                        let _ = sender.send_blocking(Event::Discovered { count: 0, states: Default::default(), origins: Default::default(), warnings: vec![error] });
                        return;
                    }
                };
                // 只在全新作者进程、任何DLL装载前采用计划；运行中的工具始终保留原库和草稿。
                let mut warnings = super::management::packages::apply_pending_with_roots(&roots);
                for warning in &warnings {
                    let _ = sender.send_blocking(Event::ManagementWarning(warning.clone()));
                }
                let result = loader::discover_with_roots(&roots);
                match result {
                    Err(error) => {
                        warnings.push(error);
                        let _ = sender.send_blocking(Event::Discovered { count: 0, states: Default::default(), origins: Default::default(), warnings });
                    }
                    Ok(discovery) => {
                        warnings.extend(discovery.warnings);
                        let _ = sender.send_blocking(Event::Discovered {
                            count: discovery.packages.len(), states: discovery.states, origins: discovery.origins, warnings,
                        });
                        for (index, package) in discovery.packages.into_iter().enumerate() {
                            let config = SessionConfig {
                                plugin_id: package.manifest.id.clone(),
                                project_id: project_id.clone(),
                                project_root: root.to_string_lossy().into_owned(),
                                asset_root: asset_root.to_string_lossy().into_owned(),
                            };
                            let output = sender.clone();
                            let id = package.manifest.id.clone();
                            let spawn = std::thread::Builder::new()
                                .name(format!("native-plugin-{index}"))
                                .spawn(move || {
                                    let run = || -> Result<(), String> {
                                        let mut loaded = loader::Loaded::open(&package, config)?;
                                        let initial = loaded.call(&Request::Open)?;
                                        if initial.tool != package.manifest.tool {
                                            return Err("插件清单与二进制工具名不一致".into());
                                        }
                                        let (requests, jobs) = async_channel::bounded::<Job>(1);
                                        let domain: &'static str =
                                            Box::leak(package.manifest.id.clone().into_boxed_str());
                                        output
                                            .send_blocking(Event::Loaded {
                                                index,
                                                domain,
                                                snapshot: initial,
                                                requests,
                                            })
                                            .map_err(|_| "作者实例已退出")?;
                                        while let Ok(job) = jobs.recv_blocking() {
                                            let snapshot = loaded.call(&job.request);
                                            let fault = snapshot.is_err();
                                            if let Some(reply) = job.reply {
                                                // 同一个DLL工作线程仍严格串行；地图IO任务直接收回执，主帧不阻塞。
                                                let _ = reply.send(snapshot);
                                                if fault { break; }
                                                continue;
                                            }
                                            if output
                                                .send_blocking(Event::Updated {
                                                    index,
                                                    sequence: job.sequence,
                                                    activate: job.activate,
                                                    request: job.request,
                                                    snapshot,
                                                })
                                                .is_err()
                                                || fault
                                            {
                                                break;
                                            }
                                        }
                                        Ok(())
                                    };
                                    if let Err(error) = run() {
                                        let _ = output.send_blocking(Event::Failed { id: package.manifest.id, error });
                                    }
                                });
                            if let Err(error) = spawn {
                                let _ = sender.send_blocking(Event::Failed { id, error: format!("插件线程创建失败：{error}") });
                            }
                        }
                    }
                }
            });
        let errors = spawn
            .as_ref()
            .err()
            .map(|e| vec![format!("插件发现线程创建失败：{e}")])
            .unwrap_or_default();
        Self {
            tools: Default::default(),
            receiver,
            discovery_pending: spawn.is_ok(),
            loading: 0,
            errors,
            management_errors: Default::default(),
            load_states: Default::default(),
            load_origins: Default::default(),
            ui_revision: 1,
            diagnostics: Default::default(),
            input_pending: false,
            save_tickets: Default::default(),
        }
    }
}
