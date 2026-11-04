//! 后台插件发现和C ABI装载；不把插件句柄、指针或动态库交给Bevy帧线程。
use feathrise_plugin_api::*;
use std::{
    collections::BTreeMap,
    ffi::c_void,
    path::{Path, PathBuf},
};

pub(super) use super::management::types::PackageManifest as Manifest;
pub(super) struct Package {
    pub manifest: Manifest,
    pub library: PathBuf,
    pub origin: super::management::types::PluginOrigin,
}

pub(super) struct Discovery {
    pub packages: Vec<Package>,
    pub states: BTreeMap<String, String>,
    pub origins: BTreeMap<String, super::management::types::PluginOrigin>,
    pub warnings: Vec<String>,
}

pub(super) fn discover(root: &Path) -> Result<Discovery, String> {
    discover_with_roots(&super::management::packages::roots_for_project(root)?)
}

pub(crate) fn discover_with_roots(
    roots: &super::management::types::PluginRoots,
) -> Result<Discovery, String> {
    let catalog = super::management::packages::catalog_at(
        roots,
        super::management::types::PluginScope::Project,
    )?;
    let mut result = Discovery {
        packages: Vec::new(),
        states: BTreeMap::new(),
        origins: BTreeMap::new(),
        // 保留旧包且已有同ID单文件只是迁移说明，由管理器展示，不进入加载失败通道。
        warnings: catalog.warnings,
    };
    for entry in catalog.entries {
        if let Some(origin) = &entry.origin {
            result.origins.insert(entry.id.clone(), origin.clone());
        } else if entry.legacy && entry.error.is_some()
            && let Some(origin) = super::management::packages::legacy_diagnostic_origin(&entry)
        {
            // 只保留拒载诊断的目录/清单身份；下面的error分支仍拒绝它进入Package和DLL装载。
            result.origins.insert(entry.id.clone(), origin);
        }
        // 同一批先检查全部身份再加载：重复ID的两个包都拒绝，目录顺序不能决定谁获胜。
        let status = if let Some(error) = entry.error {
            result.warnings.push(format!("{}：{error}", entry.id));
            format!("加载失败：{error}")
        } else if !entry.enabled {
            "已停用".into()
        } else if result.packages.len() >= MAX_PLUGINS {
            let error = "超过16个启用插件，未加载此包";
            result.warnings.push(format!("{}：{error}", entry.id));
            error.into()
        } else {
            match super::management::packages::inspect_package(&entry.directory) {
                Ok((manifest, library)) => {
                    let origin = entry.origin.ok_or("插件缺少经过摘要校验的实际来源")?;
                    result.packages.push(Package {
                        manifest,
                        library,
                        origin,
                    });
                    "加载中…".into()
                }
                Err(error) => {
                    result.warnings.push(format!("{}：{error}", entry.id));
                    format!("加载失败：{error}")
                }
            }
        };
        result.states.insert(entry.id, status);
    }
    Ok(result)
}

pub(super) struct Loaded {
    api: PluginApi,
    instance: *mut c_void,
    id: String,
}
impl Loaded {
    pub fn open(package: &Package, config: SessionConfig) -> Result<Self, String> {
        if config.plugin_id != package.manifest.id {
            return Err("会话插件ID与实际二进制元数据不一致".into());
        }
        // 实际装载前再次核对完整来源，不能把刷新目录得到的同ID新版当成本实例旧库。
        if super::management::packages::binary_digest(&package.library)? != package.origin.digest
            || read_plugin_metadata(&package.library)? != package.manifest
        {
            return Err("插件在发现后被外部修改，拒绝装载".into());
        }
        // 只允许包所在目录和标准系统位置解析Windows依赖；Linux插件使用自身$ORIGIN运行库。
        #[cfg(windows)]
        let library: libloading::Library = unsafe {
            libloading::os::windows::Library::load_with_flags(
                &package.library,
                0x00000100 | 0x00001000,
            )
        }
        .map_err(|e| format!("{}：{e:?}", crate::project::display_path(&package.library)))?
        .into();
        #[cfg(not(windows))]
        let library =
            unsafe { libloading::Library::new(&package.library) }.map_err(|e| e.to_string())?;
        // 即使握手失败，原生加载器也可能执行初始化；库保持映射到进程退出，不回收代码地址。
        let library = Box::leak(Box::new(library));
        let metadata_entry = unsafe { library.get::<MetadataEntry>(METADATA_SYMBOL) }
            .map_err(|e| format!("插件没有元数据约定函数：{e}"))?;
        let mut metadata_bytes = Vec::<u8>::new();
        let code = unsafe {
            metadata_entry(
                (&mut metadata_bytes as *mut Vec<u8>).cast(),
                receive_metadata,
            )
        };
        if code != OK
            || decode_plugin_metadata(&metadata_bytes)? != package.manifest
            || super::management::packages::binary_digest(&package.library)?
                != package.origin.digest
        {
            return Err("插件静态元数据、约定函数响应或装载文件摘要不一致".into());
        }
        let entry =
            unsafe { library.get::<PluginEntry>(ENTRY_SYMBOL) }.map_err(|e| e.to_string())?;
        let api = unsafe { entry() };
        if api.is_null() || (api as usize) % std::mem::align_of::<PluginApi>() != 0 {
            return Err("插件函数表为空或未对齐".into());
        }
        // 先读取固定头，不能把旧短表预先解释成完整结构。
        let header = api.cast::<u32>();
        let major = unsafe { header.read() };
        let size = unsafe { header.add(2).read() } as usize;
        if major != ABI_MAJOR || size < std::mem::size_of::<PluginApi>() {
            return Err("插件二进制接口版本或长度不兼容".into());
        }
        let api = unsafe { api.read() };
        let input = serde_json::to_vec(&config).map_err(|e| e.to_string())?;
        let mut output = Vec::<u8>::new();
        let mut instance = std::ptr::null_mut();
        let code = unsafe {
            (api.create)(
                input.as_ptr(),
                input.len(),
                &mut instance,
                (&mut output as *mut Vec<u8>).cast(),
                receive,
            )
        };
        if code != OK || instance.is_null() {
            if !instance.is_null() {
                unsafe { (api.destroy)(instance) };
            }
            return Err(format!(
                "插件创建失败（{code}）：{}",
                String::from_utf8_lossy(&output)
            ));
        }
        Ok(Self {
            api,
            instance,
            id: config.plugin_id,
        })
    }
    pub fn call(&mut self, request: &Request) -> Result<Snapshot, String> {
        let bytes = serde_json::to_vec(request).map_err(|e| e.to_string())?;
        if bytes.len() > MAX_MESSAGE_BYTES {
            return Err("插件请求超过512KiB".into());
        }
        let mut output = Vec::<u8>::new();
        let code = unsafe {
            (self.api.invoke)(
                self.instance,
                bytes.as_ptr(),
                bytes.len(),
                (&mut output as *mut Vec<u8>).cast(),
                receive,
            )
        };
        if code != OK {
            return Err(format!(
                "原生插件调用失败（{code}）；会话已停止，保存结果未确认，请检查磁盘后重开"
            ));
        }
        let snapshot: Snapshot =
            serde_json::from_slice(&output).map_err(|e| format!("插件响应无效：{e}"))?;
        snapshot.validate(&self.id)?;
        Ok(snapshot)
    }
}
impl Drop for Loaded {
    fn drop(&mut self) {
        unsafe { (self.api.destroy)(self.instance) };
    }
}
unsafe extern "C" fn receive_metadata(context: *mut c_void, ptr: *const u8, len: usize) -> i32 {
    if context.is_null() || ptr.is_null() || len > MAX_METADATA_BYTES {
        return INVALID;
    }
    let output = unsafe { &mut *context.cast::<Vec<u8>>() };
    if !output.is_empty() {
        return INVALID;
    }
    output.extend_from_slice(unsafe { std::slice::from_raw_parts(ptr, len) });
    OK
}
unsafe extern "C" fn receive(context: *mut c_void, ptr: *const u8, len: usize) -> i32 {
    if context.is_null() || ptr.is_null() || len > MAX_MESSAGE_BYTES {
        return INVALID;
    }
    let output = unsafe { &mut *context.cast::<Vec<u8>>() };
    if !output.is_empty() {
        return INVALID;
    }
    output.extend_from_slice(unsafe { std::slice::from_raw_parts(ptr, len) });
    OK
}
