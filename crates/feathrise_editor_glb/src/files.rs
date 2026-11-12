//! 人形绑骨的工程资产身份、轻量 GLB 索引、独占暂存和无覆盖发布。

pub(crate) use feathrise_editor_assets::glb_metadata::ordinary;
use feathrise_editor_assets::{
    files::safe_asset_path,
    glb_metadata::{MAX_MODEL_BYTES as MAX_GLB, open_glb, read_json},
};
use feathrise_editor_glb_model::rigging::AnimationAsset;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

const MAX_CATALOG_ENTRIES: usize = 100_000;
const MAX_JOINTS: usize = 128;
const BODY_JOINTS: [&str; 22] = [
    "Hips",
    "Spine",
    "Spine1",
    "Spine2",
    "Neck",
    "Head",
    "LeftShoulder",
    "LeftArm",
    "LeftForeArm",
    "LeftHand",
    "LeftUpLeg",
    "LeftLeg",
    "LeftFoot",
    "LeftToeBase",
    "RightShoulder",
    "RightArm",
    "RightForeArm",
    "RightHand",
    "RightUpLeg",
    "RightLeg",
    "RightFoot",
    "RightToeBase",
];
static STAGE_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct FrozenAsset {
    pub(crate) relative: String,
    path: PathBuf,
    sha256: [u8; 32],
}

pub(crate) struct Stage(PathBuf);

impl Stage {
    pub(crate) fn new(root: &Path) -> Result<Self, String> {
        let parent = checked_root(root.parent().ok_or("资产根缺少工程父目录")?)?;
        // 暂存留在资产根旁，AssetServer 不会载入临时 GLB，正式发布仍在同一文件系统。
        for _ in 0..32 {
            let id = STAGE_ID.fetch_add(1, Ordering::Relaxed);
            let path = parent.join(format!(".feathrise-rig-{}-{id}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(format!("创建绑骨暂存目录失败：{error}")),
            }
        }
        Err("无法创建独占绑骨暂存目录".into())
    }

    pub(crate) fn root(&self) -> &Path {
        &self.0
    }

    pub(crate) fn path(&self, filename: &str) -> PathBuf {
        self.0.join(filename)
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        // 仅清理本进程 create_dir 独占的目录。目录被替换为链接时保留现场。
        if ordinary(&self.0).is_ok_and(|metadata| metadata.is_dir())
            && self.0.canonicalize().ok().as_ref() == Some(&self.0)
        {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

pub(crate) fn checked_root(root: &Path) -> Result<PathBuf, String> {
    let root = root
        .canonicalize()
        .map_err(|error| format!("无法定位工程资产根：{error}"))?;
    if !ordinary(&root)?.is_dir() {
        return Err("工程资产根必须是目录".into());
    }
    Ok(root)
}

fn is_glb(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("glb"))
}

fn checked_asset(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = safe_asset_path(root, relative)?;
    if !is_glb(&path) {
        return Err("绑骨和外部动作只接受工程资产目录中的 GLB".into());
    }
    let mut cursor = root.to_path_buf();
    for part in relative.split('/') {
        cursor.push(part);
        ordinary(&cursor)?;
    }
    let resolved = path
        .canonicalize()
        .map_err(|error| format!("无法打开 {relative}：{error}"))?;
    if !resolved.starts_with(root) {
        return Err("绑骨资产已移出当前工程资产根".into());
    }
    Ok(resolved)
}

pub(crate) fn active(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) {
        return Err("已取消人形绑骨任务".into());
    }
    Ok(())
}

pub(crate) fn snapshot(
    root: &Path,
    relative: &str,
    destination: &Path,
    cancel: &AtomicBool,
) -> Result<FrozenAsset, String> {
    let path = checked_asset(root, relative)?;
    let mut copy = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| format!("创建绑骨输入快照失败：{error}"))?;
    let sha256 = hash_and_copy(&path, &mut copy, cancel)?;
    Ok(FrozenAsset {
        relative: relative.to_owned(),
        path,
        sha256,
    })
}

pub(crate) fn freeze_bytes(
    root: &Path,
    relative: &str,
    cancel: &AtomicBool,
) -> Result<(FrozenAsset, Vec<u8>), String> {
    let path = checked_asset(root, relative)?;
    let mut bytes = Vec::new();
    let sha256 = hash_and_copy(&path, &mut bytes, cancel)?;
    Ok((
        FrozenAsset {
            relative: relative.to_owned(),
            path,
            sha256,
        },
        bytes,
    ))
}

pub(crate) fn check_frozen(
    root: &Path,
    frozen: &FrozenAsset,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let path = checked_asset(root, &frozen.relative)?;
    if path != frozen.path || hash_and_copy(&path, &mut io::sink(), cancel)? != frozen.sha256 {
        return Err(format!("输入已在外部更新：{}", frozen.relative));
    }
    Ok(())
}

fn hash_and_copy(
    path: &Path,
    target: &mut impl Write,
    cancel: &AtomicBool,
) -> Result<[u8; 32], String> {
    let mut source = open_glb(path, MAX_GLB)?.take(MAX_GLB + 1);
    let mut digest = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        active(cancel)?;
        let count = source
            .read(&mut buffer)
            .map_err(|error| format!("读取 GLB 失败：{error}"))?;
        if count == 0 {
            return Ok(digest.finalize().into());
        }
        if source.limit() == 0 {
            return Err("GLB 在读取时超过 256 MiB".into());
        }
        digest.update(&buffer[..count]);
        target
            .write_all(&buffer[..count])
            .map_err(|error| format!("写入 GLB 快照失败：{error}"))?;
    }
}

pub(crate) fn glb_json(path: &Path) -> Result<Value, String> {
    read_json(path, MAX_GLB)
}

fn list<'a>(doc: &'a Value, field: &str) -> &'a [Value] {
    doc[field].as_array().map_or(&[], Vec::as_slice)
}

fn embedded(doc: &Value) -> bool {
    ["buffers", "images"].iter().all(|key| {
        list(doc, key).iter().all(|value| {
            value
                .get("uri")
                .is_none_or(|uri| uri.as_str().is_some_and(|uri| uri.starts_with("data:")))
        })
    })
}

pub(crate) fn check_input_model(path: &Path) -> Result<(), String> {
    let doc = glb_json(path)?;
    if list(&doc, "meshes").is_empty() || !list(&doc, "skins").is_empty() {
        return Err("请选择有网格且尚未蒙皮的 GLB 模型".into());
    }
    if !embedded(&doc) {
        return Err("请选择贴图和缓冲数据都已打包的 GLB 模型".into());
    }
    Ok(())
}

fn joint_set(doc: &Value, skin: &Value) -> Option<HashSet<String>> {
    let joints = list(skin, "joints");
    if joints.is_empty() || joints.len() > MAX_JOINTS {
        return None;
    }
    let nodes = list(doc, "nodes");
    let names: HashSet<_> = joints
        .iter()
        .map(|joint| {
            let index = usize::try_from(joint.as_u64()?).ok()?;
            nodes.get(index)?.get("name")?.as_str().map(str::to_owned)
        })
        .collect::<Option<_>>()?;
    (names.len() == joints.len()).then_some(names)
}

pub(crate) fn mixamo_skin(doc: &Value) -> Option<(&Value, HashSet<String>)> {
    let mut rigs = list(doc, "skins")
        .iter()
        .filter_map(|skin| Some((skin, joint_set(doc, skin)?)))
        .filter(|(_, names)| {
            BODY_JOINTS
                .iter()
                .all(|bone| names.contains(&format!("mixamorig:{bone}")))
        });
    let (skin, names) = rigs.next()?;
    // 同一骨架可被多个 primitive 引用；同一文件内互不相同的两套骨架不作为自动参照。
    (embedded(doc) && rigs.all(|(_, other)| other == names)).then_some((skin, names))
}

pub(crate) fn check_output_model(path: &Path, expected: &HashSet<String>) -> Result<(), String> {
    let doc = glb_json(path)?;
    let meshes = list(&doc, "meshes");
    let skins = list(&doc, "skins");
    let mut used_meshes = HashSet::new();
    let mut exported_joints = HashSet::new();
    for node in list(&doc, "nodes") {
        let Some(mesh) = node["mesh"]
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
        else {
            continue;
        };
        let Some(skin) = node["skin"]
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .and_then(|index| skins.get(index))
        else {
            return Err("输出 GLB 有网格未挂接蒙皮".into());
        };
        let Some(joints) = joint_set(&doc, skin) else {
            return Err("输出 GLB 的蒙皮关节无效".into());
        };
        let Some(mesh_data) = meshes.get(mesh) else {
            return Err("输出 GLB 的网格引用无效".into());
        };
        let primitives = list(mesh_data, "primitives");
        if primitives.is_empty()
            || primitives.iter().any(|primitive| {
                let attributes = &primitive["attributes"];
                !attributes["JOINTS_0"].is_u64() || !attributes["WEIGHTS_0"].is_u64()
            })
        {
            return Err("输出 GLB 缺少顶点关节或权重".into());
        }
        used_meshes.insert(mesh);
        exported_joints.extend(joints);
    }
    if used_meshes.is_empty()
        || used_meshes.len() != meshes.len()
        || &exported_joints != expected
        || !list(&doc, "animations").is_empty()
        || !embedded(&doc)
    {
        return Err("输出 GLB 未通过独立模型、骨架及蒙皮验证".into());
    }
    Ok(())
}

pub(crate) fn catalog(root: &Path, cancel: &AtomicBool) -> Result<Vec<AnimationAsset>, String> {
    let mut result = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    let mut seen = 0;
    while let Some(folder) = folders.pop() {
        active(cancel)?;
        let entries =
            fs::read_dir(&folder).map_err(|error| format!("读取动作目录失败：{error}"))?;
        for entry in entries {
            active(cancel)?;
            seen += 1;
            if seen > MAX_CATALOG_ENTRIES {
                return Err("工程资产目录超过 10 万项，无法建立动作索引".into());
            }
            let entry = entry.map_err(|error| format!("读取动作目录项失败：{error}"))?;
            let path = entry.path();
            let Ok(metadata) = ordinary(&path) else {
                continue;
            };
            if metadata.is_dir() && !entry.file_name().to_string_lossy().starts_with('.') {
                folders.push(path);
            } else if metadata.is_file() && is_glb(&path) {
                let Ok(doc) = glb_json(&path) else { continue };
                let animations = list(&doc, "animations");
                if !animations.is_empty()
                    && let Some(path) = relative_path(root, &path)
                {
                    result.push(AnimationAsset {
                        path,
                        clips: animations
                            .iter()
                            .enumerate()
                            .map(|(index, clip)| {
                                clip["name"]
                                    .as_str()
                                    .filter(|name| !name.is_empty())
                                    .map_or_else(
                                        || format!("Animation {}", index + 1),
                                        str::to_owned,
                                    )
                            })
                            .collect(),
                        can_reference: mixamo_skin(&doc).is_some(),
                    });
                }
            }
        }
    }
    result.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    Ok(result)
}

fn relative_path(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?.to_str()?.replace('\\', "/");
    safe_asset_path(root, &relative).ok()?;
    Some(relative)
}

pub(crate) fn publish(
    staged: &Path,
    root: &Path,
    input: &FrozenAsset,
    cancel: &AtomicBool,
) -> Result<String, String> {
    publish_named(staged, root, input, "已绑骨", cancel)
}

pub(crate) fn publish_named(
    staged: &Path,
    root: &Path,
    input: &FrozenAsset,
    suffix: &str,
    cancel: &AtomicBool,
) -> Result<String, String> {
    let parent = input.path.parent().ok_or("原模型没有父目录")?;
    let stem: String = input
        .path
        .file_stem()
        .and_then(|name| name.to_str())
        .ok_or("模型文件名无效")?
        .chars()
        .take(100)
        .collect();
    OpenOptions::new()
        .write(true)
        .open(staged)
        .and_then(|file| file.sync_all())
        .map_err(|error| format!("同步蒙皮 GLB 暂存文件失败：{error}"))?;
    for index in 1..=999 {
        active(cancel)?;
        let name = if index == 1 {
            format!("{stem}_{suffix}.glb")
        } else {
            format!("{stem}_{suffix}_{index}.glb")
        };
        let destination = parent.join(name);
        let relative = relative_path(root, &destination).ok_or("输出路径越过工程资产根")?;
        // 原子创建新目录项，目标被作者或另一个任务抢先写入时选择下一个名字。
        match persist(staged, &destination) {
            Ok(()) => return Ok(relative),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("发布蒙皮模型失败：{error}")),
        }
    }
    Err("已有过多同名的绑骨模型，请整理目录后重试".into())
}

#[cfg(windows)]
fn persist(source: &Path, target: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};
    let wide = |path: &Path| {
        path.as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>()
    };
    let source = wide(source);
    let target = wide(target);
    // 不使用 REPLACE_EXISTING，避免检查与改名之间覆盖并发保存的资产。
    if unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), MOVEFILE_WRITE_THROUGH) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(windows))]
fn persist(source: &Path, target: &Path) -> io::Result<()> {
    fs::hard_link(source, target)
}
