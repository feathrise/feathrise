//! 编辑器路径库只保存可复用工具参数；后台读取/编解码与原子写入不触发地形雕刻。
use super::files;
use bevy::tasks::AsyncComputeTaskPool;
use feathrise_map_terrain::volume::VolumePathParameters;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct SavedSculptPath {
    pub name: String,
    pub nodes: Vec<[f32; 3]>,
    pub parameters: VolumePathParameters,
}
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct SculptPathLibrary {
    pub schema_version: u32,
    pub map_id: String,
    pub paths: Vec<SavedSculptPath>,
}
impl SculptPathLibrary {
    pub(crate) fn empty(map_id: String) -> Self {
        Self {
            schema_version: 1,
            map_id,
            paths: Vec::new(),
        }
    }
}

fn path(root: &Path, folder: &str) -> Result<PathBuf, String> {
    files::safe_asset_path(root, &format!("{folder}/editor/sculpt_paths.json"))
}

async fn read(
    root: PathBuf,
    folder: String,
    map: String,
) -> Result<Arc<SculptPathLibrary>, String> {
    let file = path(&root, &folder)?;
    let bytes = match std::fs::read(&file) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Arc::new(SculptPathLibrary::empty(map)));
        }
        Err(e) => return Err(format!("读取路径库失败：{e}")),
    };
    AsyncComputeTaskPool::get()
        .spawn(async move {
            let library: SculptPathLibrary =
                serde_json::from_slice(&bytes).map_err(|e| format!("路径库格式无效：{e}"))?;
            if library.schema_version != 1 || library.map_id != map {
                return Err("路径库版本或所属地图无效".into());
            }
            for entry in &library.paths {
                if entry.name.trim().is_empty()
                    || entry.nodes.len() < 2
                    || entry
                        .nodes
                        .iter()
                        .flatten()
                        .any(|v| !v.is_finite() || v.abs() > 100_000.)
                    || !entry.parameters.width.is_finite()
                    || !entry.parameters.height.is_finite()
                    || !(0.5..=16.).contains(&entry.parameters.width)
                    || !(0.5..=16.).contains(&entry.parameters.height)
                {
                    return Err("路径库包含无效节点或截面".into());
                }
            }
            Ok(Arc::new(library))
        })
        .await
}

pub(crate) async fn load_sculpt_paths(
    folder: String,
    map: String,
) -> Result<Arc<SculptPathLibrary>, String> {
    read(files::workspace_asset_root(), folder, map).await
}

pub(crate) async fn save_sculpt_path(
    folder: String,
    map: String,
    nodes: Arc<[[f32; 3]]>,
    parameters: VolumePathParameters,
    active: Arc<AtomicBool>,
) -> Result<Arc<SculptPathLibrary>, String> {
    save(
        files::workspace_asset_root(),
        folder,
        map,
        nodes,
        parameters,
        active,
    )
    .await
}

async fn save(
    root: PathBuf,
    folder: String,
    map: String,
    nodes: Arc<[[f32; 3]]>,
    parameters: VolumePathParameters,
    active: Arc<AtomicBool>,
) -> Result<Arc<SculptPathLibrary>, String> {
    // 与地图作者写入共用资产根锁；在后台重新读取旧库，不能用过期UI快照覆盖外部新条目。
    let _lock = feathrise_map_content::authoring_io::ContentAuthorLock::acquire(&root)?;
    let current = read(root.clone(), folder.clone(), map.clone()).await?;
    let (library, bytes) = AsyncComputeTaskPool::get()
        .spawn(async move {
            let mut paths = current.paths.clone();
            paths
                .try_reserve(1)
                .map_err(|e| format!("路径库分配失败：{e}"))?;
            let name = format!("路径 {}", paths.len() + 1);
            paths.push(SavedSculptPath {
                name,
                nodes: nodes.to_vec(),
                parameters,
            });
            let library = SculptPathLibrary {
                schema_version: 1,
                map_id: map,
                paths,
            };
            let mut bytes = serde_json::to_vec_pretty(&library).map_err(|e| e.to_string())?;
            bytes.push(b'\n');
            Ok::<_, String>((Arc::new(library), bytes))
        })
        .await?;
    if !active.load(Ordering::Acquire) {
        return Err("路径保存请求已取消，旧路径库保持不变".into());
    }
    files::atomic_write(&path(&root, &folder)?, &bytes)?;
    Ok(library)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sculpt_path_library_round_trips_and_cancellation_keeps_the_existing_file() {
        use bevy::tasks::{TaskPool, block_on};
        AsyncComputeTaskPool::get_or_init(TaskPool::new);
        let root = std::env::temp_dir().join(format!(
            "rpg-path-library-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let folder = "maps/MAP-PATH";
        let nodes: Arc<[[f32; 3]]> = Arc::from([[2., -1., 2.], [8., -2., 8.]]);
        let library = block_on(save(
            root.clone(),
            folder.into(),
            "MAP-PATH".into(),
            nodes.clone(),
            VolumePathParameters::default(),
            Arc::new(AtomicBool::new(true)),
        ))
        .unwrap();
        assert_eq!(library.paths.len(), 1);
        let file = path(&root, folder).unwrap();
        let before = std::fs::read(&file).unwrap();
        assert!(
            block_on(save(
                root.clone(),
                folder.into(),
                "MAP-PATH".into(),
                nodes,
                VolumePathParameters::default(),
                Arc::new(AtomicBool::new(false))
            ))
            .is_err()
        );
        assert_eq!(std::fs::read(&file).unwrap(), before);
        let reopened = block_on(read(root.clone(), folder.into(), "MAP-PATH".into())).unwrap();
        assert_eq!(reopened.paths[0].nodes[1], [8., -2., 8.]);
        assert!(path(&root, "../outside").is_err());
        assert!(
            root.canonicalize()
                .unwrap()
                .starts_with(std::env::temp_dir().canonicalize().unwrap())
        );
        std::fs::remove_dir_all(root).unwrap(); // 仅清理由本测试创建并核对位于临时目录内的唯一根。
    }
}
