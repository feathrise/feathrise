//! 旧目录包的显式归档移除：只搬离扫描根，原字节、空目录和失败现场全部保留。
use super::*;

/// 调用侧已经持有该工程管理锁；旧格式不加载，因此不需要等待作者会话重开。
pub(super) fn archive(store: &Store, entry: &PackageEntry) -> Result<ArchivedLegacy, String> {
    if store.scope != PluginScope::Project || !entry.legacy || entry.inherited {
        return Err("只能归档当前工程自己安装的旧目录包".into());
    }
    let diagnostic_error = entry.error.clone().ok_or("旧目录缺少待消解诊断")?;
    ordinary_chain(&entry.path)?;
    let source = entry.path.canonicalize().map_err(error)?;
    let plugins = store.plugins.canonicalize().map_err(error)?;
    if source.parent() != Some(plugins.as_path()) || !ordinary(&source)?.is_dir() {
        return Err("旧包必须是工程plugins直属普通目录".into());
    }
    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("旧包目录名无效")?;
    validate_component(name)?;
    // 防止界面看到的旧包在点击前已换成另一个身份；无法确认的旧清单不被擅自搬动。
    let manifest_bytes = read_limited(&source.join("plugin.json"), MAX_JSON)?;
    // 与列表发现采用相同的BOM解析约定；身份摘要和归档仍保留清单的完整原字节。
    let manifest: serde_json::Value = serde_json::from_slice(
        manifest_bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&manifest_bytes),
    ).map_err(error)?;
    if manifest.get("id").and_then(|id| id.as_str()) != Some(entry.id.as_str()) {
        return Err("旧包身份已改变，原目录保留，请刷新后重试".into());
    }
    let before = tree_digest(&source)?;
    if read_limited(&source.join("plugin.json"), MAX_JSON)? != manifest_bytes {
        return Err("旧包清单在核对时发生变化，原目录保留".into());
    }
    let ticket = ticket();
    let relative = Path::new("backups").join(&ticket).join("legacy").join(name);
    let archived = checked_child(&store.manager, &relative, false)?;
    if archived.exists() {
        return Err("旧包归档目标已存在，拒绝覆盖".into());
    }
    ensure_parent(&store.manager, &archived)?;
    // 同一工程卷内原子改名，不递归删除或重写作者内容；Windows仍占用旧库时允许安全失败。
    fs::rename(&source, &archived).map_err(|e| format!("无法归档旧包，原目录保留：{e}"))?;
    let result = (|| -> Result<(), String> {
        if tree_digest(&archived)? != before {
            return Err("旧包归档期间内容发生变化".into());
        }
        write_json(
            &store.manager,
            &format!("receipts/{ticket}.json"),
            &serde_json::json!({
                "kind": "legacy_archive", "scope": store.scope, "id": entry.id,
                "original": source, "archived": archived, "sha256": before,
            }),
        )
    })();
    if let Err(error) = result {
        if source.exists() {
            return Err(format!(
                "旧包归档未完成：{error}；原位置已被占用，原字节保留在{}",
                crate::project::display_path(&archived)
            ));
        }
        fs::rename(&archived, &source).map_err(|rollback| {
            format!(
                "旧包归档失败：{error}；回退失败：{rollback}；原字节保留在{}",
                crate::project::display_path(&archived),
            )
        })?;
        return Err(format!("旧包归档未完成，原目录已恢复：{error}"));
    }
    Ok(ArchivedLegacy {
        id: entry.id.clone(),
        origin: PluginOrigin {
            scope: PluginScope::Project,
            path: source,
            digest: manifest_digest(&manifest_bytes),
        },
        error: diagnostic_error,
        archived,
        original_vacant: false,
    })
}

/// 仅供旧格式拒载诊断定位来源，不把目录伪装成可装载的二进制Package。
pub(super) fn diagnostic_origin(entry: &PackageEntry) -> Result<PluginOrigin, String> {
    if !entry.legacy || entry.scope != PluginScope::Project || entry.inherited {
        return Err("不是工程旧目录诊断".into());
    }
    ordinary_chain(&entry.path)?;
    let path = entry.path.canonicalize().map_err(error)?;
    if !ordinary(&path)?.is_dir() {
        return Err("旧包诊断来源不是目录".into());
    }
    let bytes = read_limited(&path.join("plugin.json"), MAX_JSON)?;
    let manifest: serde_json::Value = serde_json::from_slice(
        bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes),
    ).map_err(error)?;
    if manifest.get("id").and_then(|id| id.as_str()) != Some(entry.id.as_str()) {
        return Err("旧包诊断来源身份不匹配".into());
    }
    Ok(PluginOrigin {
        scope: PluginScope::Project,
        path,
        digest: manifest_digest(&bytes),
    })
}

fn manifest_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// 有界树摘要同时覆盖相对路径、目录、文件长度与内容，不跟随链接或junction。
fn tree_digest(root: &Path) -> Result<String, String> {
    fn visit(
        root: &Path,
        path: &Path,
        depth: usize,
        entries: &mut usize,
        bytes: &mut u64,
        digest: &mut Sha256,
    ) -> Result<(), String> {
        if depth > 16 || *entries >= MAX_ENTRIES {
            return Err("旧包目录超过256项或16层归档预算".into());
        }
        *entries += 1;
        let info = ordinary(path)?;
        let relative = path.strip_prefix(root).map_err(|_| "旧包目录越界")?;
        let identity = serde_json::to_vec(&(relative, info.is_dir())).map_err(error)?;
        digest.update((identity.len() as u64).to_le_bytes());
        digest.update(identity);
        if info.is_dir() {
            let mut children = Vec::new();
            for entry in fs::read_dir(path).map_err(error)? {
                if children.len() >= MAX_ENTRIES {
                    return Err("旧包目录超过256项归档预算".into());
                }
                children.push(entry.map_err(error)?.path());
            }
            children.sort();
            for child in children {
                visit(root, &child, depth + 1, entries, bytes, digest)?;
            }
        } else {
            let mut file = fs::File::open(path).map_err(error)?;
            let mut content = Sha256::new();
            let mut length = 0u64;
            let mut chunk = [0; 64 * 1024];
            loop {
                let count = file.read(&mut chunk).map_err(error)?;
                if count == 0 {
                    break;
                }
                *bytes += count as u64;
                length += count as u64;
                if *bytes > MAX_BYTES {
                    return Err("旧包总量超过1GiB归档预算".into());
                }
                content.update(&chunk[..count]);
            }
            if length != info.len() {
                return Err("旧包文件在核对时发生变化".into());
            }
            digest.update(length.to_le_bytes());
            digest.update(content.finalize());
        }
        Ok(())
    }
    let mut digest = Sha256::new();
    visit(root, root, 0, &mut 0, &mut 0, &mut digest)?;
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}
