//! 场景外层文件夹与摆放同文件保存。GLB 使用顶级 extras，旧场景使用原 JSON 可选字段。

use bevy::gltf::gltf::binary::Glb;
use feathrise_scene_content::ScenePlacementsAsset;
use serde_json::{Value, json};
use std::{borrow::Cow, collections::BTreeMap, ops::Bound::{Excluded, Unbounded}, sync::Arc};

const FOLDERS: &str = "feathrise_object_folders";
const PARENTS: &str = "feathrise_object_folder_parents";

#[derive(Clone, Debug)]
struct Folder {
    name: Arc<str>,
    parent: Option<usize>,
}

/// 场景摆放仍用旧名称定位直属文件夹。父级使用会话内编号，改名无需逐个重写子文件夹。
#[derive(Clone, Debug, Default)]
pub(crate) struct PlacementFolders {
    names: BTreeMap<Arc<str>, usize>,
    nodes: BTreeMap<usize, Folder>,
    next_id: usize,
}

impl PartialEq for PlacementFolders {
    fn eq(&self, other: &Self) -> bool {
        // 保存重读重新分配内部编号，比较的是文件夹名称和实际父级。
        self.names.len() == other.names.len() && self.names.keys().all(|name|
            other.contains(name) && self.parent(name) == other.parent(name))
    }
}

impl PlacementFolders {
    pub(crate) fn is_empty(&self) -> bool { self.names.is_empty() }

    pub(crate) fn contains(&self, name: &str) -> bool { self.names.contains_key(name) }

    /// 编号仅在当前场景草稿内有效，用于区分同名的新旧文件夹。
    pub(crate) fn id_for(&self, name: &str) -> Option<usize> { self.names.get(name).copied() }

    pub(crate) fn names_after<'a>(&'a self, cursor: Option<&str>) -> impl Iterator<Item = &'a str> + use<'a> {
        self.names.range::<str, _>((cursor.map_or(Unbounded, Excluded), Unbounded))
            .map(|(name, _)| name.as_ref())
    }

    pub(crate) fn parent(&self, name: &str) -> Option<&str> {
        let id = self.names.get(name)?;
        let parent = self.nodes[id].parent?;
        Some(&self.nodes[&parent].name)
    }

    pub(crate) fn contains_member(&self, folder: &str, member_folder: &str) -> bool {
        if folder == member_folder { return true; }
        let Some(&ancestor) = self.names.get(folder) else { return false; };
        let mut node = self.names.get(member_folder).copied();
        while let Some(id) = node {
            if id == ancestor { return true; }
            node = self.nodes[&id].parent;
        }
        false
    }

    pub(crate) fn insert(&mut self, name: String) {
        if self.contains(&name) { return; }
        let name: Arc<str> = name.into();
        let id = self.next_id;
        self.next_id += 1;
        self.names.insert(name.clone(), id);
        self.nodes.insert(id, Folder { name, parent: None });
    }

    pub(crate) fn remove(&mut self, name: &str) {
        if let Some(id) = self.names.remove(name) { self.nodes.remove(&id); }
    }

    pub(crate) fn rename(&mut self, before: &str, after: &str) -> Result<(), String> {
        if !self.contains(before) || self.contains(after) {
            return Err("文件夹名称已变化，原操作已保留".into());
        }
        let id = self.names.remove(before).expect("已确认原文件夹");
        let name: Arc<str> = after.into();
        self.nodes.get_mut(&id).expect("文件夹索引一致").name = name.clone();
        self.names.insert(name, id);
        Ok(())
    }

    pub(crate) fn can_move(&self, name: &str, parent: Option<&str>) -> bool {
        self.parent(name) != parent && !parent.is_some_and(|parent| self.contains_member(name, parent))
    }

    pub(crate) fn prepare_move(&self, name: &str, parent: Option<&str>) -> Result<Option<PlacementFolderMove>, String> {
        let before = self.parent(name);
        if before == parent { return Ok(None); }
        if !self.can_move(name, parent) { return Err("文件夹无法移入自己或自己的子文件夹".into()); }
        validate_name(name)?;
        if let Some(parent) = parent { validate_name(parent)?; }
        Ok(Some(PlacementFolderMove {
            name: name.to_owned(), before: before.map(str::to_owned), after: parent.map(str::to_owned),
            added_source: !self.contains(name), added_parent: parent.is_some_and(|name| !self.contains(name)),
        }))
    }

    /// 空目录经完整树投影确认后，用一条反向的创建记录保留原父级。
    pub(crate) fn prepare_empty_removal(&self, name: &str) -> Result<PlacementFolderMove, String> {
        if !self.contains(name) { return Err("文件夹已变化，请重新选择".into()); }
        Ok(PlacementFolderMove {
            name: name.to_owned(), before: None, after: self.parent(name).map(str::to_owned),
            added_source: true, added_parent: false,
        })
    }

    fn set_parent(&mut self, name: &str, parent: Option<&str>) {
        let parent = parent.map(|name| self.names[name]);
        let id = self.names[name];
        self.nodes.get_mut(&id).expect("文件夹索引一致").parent = parent;
    }
}

/// 整夹移动只记录一条父级边。旧式隐式文件夹首次参与层级归类时登记为可保存的文件夹。
#[derive(Clone)]
pub struct PlacementFolderMove {
    name: String,
    before: Option<String>,
    after: Option<String>,
    added_source: bool,
    added_parent: bool,
}

impl PlacementFolderMove {
    pub(crate) fn apply(&self, folders: &mut Arc<PlacementFolders>, undoing: bool) -> Result<(), String> {
        let (before, after) = if undoing { (&self.after, &self.before) } else { (&self.before, &self.after) };
        if folders.contains(&self.name) != (!self.added_source || undoing)
            || folders.parent(&self.name) != before.as_deref()
            || self.after.as_deref().is_some_and(|name| folders.contains(name) != (!self.added_parent || undoing))
            || self.before.as_deref().is_some_and(|name| !folders.contains(name))
        {
            return Err("文件夹父级已经变化，无法应用这笔历史".into());
        }
        let folders = Arc::make_mut(folders);
        if !undoing {
            if self.added_source { folders.insert(self.name.clone()); }
            if let Some(name) = self.after.as_ref().filter(|_| self.added_parent) { folders.insert(name.clone()); }
        }
        folders.set_parent(&self.name, after.as_deref());
        if undoing {
            if self.added_source { folders.remove(&self.name); }
            if let Some(name) = self.after.as_ref().filter(|_| self.added_parent) { folders.remove(name); }
        }
        Ok(())
    }
}

pub(crate) fn validate_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty()
        || name.trim() != name
        || name.chars().count() > 128
        || name.chars().any(char::is_control)
    {
        return Err("文件夹名称需为 1 至 128 个有效字符");
    }
    Ok(())
}

fn read_fields(json: &Value) -> Result<PlacementFolders, String> {
    let mut folders = PlacementFolders::default();
    if let Some(names) = json.get(FOLDERS) {
        for entry in names.as_array().ok_or("场景文件夹目录格式无效")? {
            let name = entry.as_str().ok_or("场景文件夹名称无效")?;
            validate_name(name)?;
            folders.insert(name.to_owned());
        }
    }
    if let Some(parents) = json.get(PARENTS) {
        for (folder, parent) in parents.as_object().ok_or("场景文件夹父级目录格式无效")? {
            let parent = parent.as_str().ok_or("场景文件夹父级名称无效")?;
            if !folders.contains(folder) || !folders.contains(parent) { return Err("场景文件夹引用不存在的名称".into()); }
            folders.set_parent(folder, Some(parent));
        }
        // 后台加载只遍历每条父级边一次，避免对长父链逐个重新查环。
        let mut visited = BTreeMap::new();
        for start in folders.nodes.keys().copied() {
            let mut node = Some(start);
            while let Some(id) = node {
                if let Some(previous) = visited.get(&id) {
                    if *previous == start { return Err("场景文件夹父级形成循环".into()); }
                    break;
                }
                visited.insert(id, start);
                node = folders.nodes[&id].parent;
            }
        }
    }
    Ok(folders)
}

pub(super) fn read_glb(bytes: &[u8]) -> Result<PlacementFolders, String> {
    let glb = Glb::from_slice(bytes).map_err(|error| error.to_string())?;
    let json: Value = serde_json::from_slice(&glb.json).map_err(|error| error.to_string())?;
    read_fields(&json["extras"])
}

pub(super) fn read_json(bytes: &[u8]) -> Result<PlacementFolders, String> {
    let json: Value = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    read_fields(&json)
}

fn write_fields(extras: &mut serde_json::Map<String, Value>, folders: &PlacementFolders) {
    if folders.is_empty() {
        extras.remove(FOLDERS);
    } else {
        extras.insert(FOLDERS.to_owned(), json!(folders.names_after(None).collect::<Vec<_>>()));
    }
    let parents = folders.names_after(None).filter_map(|name| folders.parent(name).map(|parent| (name, parent)))
        .collect::<BTreeMap<_, _>>();
    if parents.is_empty() { extras.remove(PARENTS); }
    else { extras.insert(PARENTS.to_owned(), json!(parents)); }
}

pub(super) fn write_glb(bytes: &[u8], folders: &PlacementFolders) -> Result<Vec<u8>, String> {
    let mut glb = Glb::from_slice(bytes).map_err(|error| error.to_string())?;
    let mut json: Value = serde_json::from_slice(&glb.json).map_err(|error| error.to_string())?;
    let extras = &mut json["extras"];
    if extras.is_null() {
        *extras = json!({});
    }
    let extras = extras
        .as_object_mut()
        .ok_or("场景 GLB 的 extras 不是可写入的对象")?;
    write_fields(extras, folders);
    // 只更新作者目录。节点、动画、几何摘要和原始 BIN 保持现有内容。
    glb.json = Cow::Owned(serde_json::to_vec(&json).map_err(|error| error.to_string())?);
    glb.to_vec().map_err(|error| error.to_string())
}

pub(super) fn write_json(
    placements: &ScenePlacementsAsset,
    folders: &PlacementFolders,
) -> Result<Value, String> {
    let mut json = serde_json::to_value(placements).map_err(|error| error.to_string())?;
    write_fields(json.as_object_mut().ok_or("场景摆放目录格式无效")?, folders);
    Ok(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_round_trip_without_changing_gltf_placement_and_legacy_identities() {
        use feathrise_scene_content::{read_scene_placements, write_scene_placements_glb};
        // 无成员的声明依然需要落盘并重开。
        let source = ScenePlacementsAsset {
            scene_id: feathrise_scene_content::SceneId("TEST-FOLDERS".to_owned()),
            ..Default::default()
        };
        let asset_path = "scenes/test/scene.glb";
        let glb = write_scene_placements_glb(&source, asset_path, None).unwrap();
        let mut names = PlacementFolders::default();
        names.insert("独立模型".into());
        names.insert("小院".into());
        names.set_parent("独立模型", Some("小院"));
        assert!(read_glb(&glb).unwrap().is_empty());
        let modified = write_glb(&glb, &names).unwrap();
        assert_eq!(read_glb(&modified).unwrap(), names);
        let adopted = read_scene_placements(&modified, asset_path).unwrap();
        assert_eq!(adopted.scene_id, source.scene_id);
        assert_eq!(adopted.placements, source.placements);
        let legacy = serde_json::to_vec(&write_json(&adopted, &names).unwrap()).unwrap();
        assert_eq!(read_json(&legacy).unwrap(), names);
        assert!(serde_json::from_slice::<ScenePlacementsAsset>(&legacy).is_ok());
    }

    #[test]
    fn removing_an_empty_subfolder_can_restore_its_parent() {
        let mut folders = Arc::new(PlacementFolders::default());
        folders.prepare_move("空文件夹", Some("父文件夹")).unwrap().unwrap().apply(&mut folders, false).unwrap();
        let removal = folders.prepare_empty_removal("空文件夹").unwrap();
        removal.apply(&mut folders, true).unwrap();
        assert!(folders.contains("父文件夹") && !folders.contains("空文件夹"));
        removal.apply(&mut folders, false).unwrap();
        assert_eq!(folders.parent("空文件夹"), Some("父文件夹"));
        removal.apply(&mut folders, true).unwrap();
        assert!(!folders.contains("空文件夹"));
    }

    #[test]
    fn whole_folder_moves_to_another_folder_and_root_and_history_restores_each_parent() {
        let mut folders = Arc::new(PlacementFolders::default());
        let move_under_parent = folders.prepare_move("旧成员文件夹", Some("目标")).unwrap().unwrap();
        move_under_parent.apply(&mut folders, false).unwrap();
        assert!(folders.contains_member("目标", "旧成员文件夹"));
        assert!(folders.prepare_move("目标", Some("旧成员文件夹")).is_err());
        assert!(folders.prepare_move("目标", Some("目标")).is_err());
        assert!(folders.prepare_move("旧成员文件夹", Some("目标")).unwrap().is_none());

        let to_root = folders.prepare_move("旧成员文件夹", None).unwrap().unwrap();
        to_root.apply(&mut folders, false).unwrap();
        assert_eq!(folders.parent("旧成员文件夹"), None);
        assert!(folders.contains("旧成员文件夹"));
        to_root.apply(&mut folders, true).unwrap();
        assert_eq!(folders.parent("旧成员文件夹"), Some("目标"));
        Arc::make_mut(&mut folders).rename("目标", "新目标").unwrap();
        assert_eq!(folders.parent("旧成员文件夹"), Some("新目标"));
        Arc::make_mut(&mut folders).rename("新目标", "目标").unwrap();

        move_under_parent.apply(&mut folders, true).unwrap();
        assert!(folders.is_empty(), "撤销应恢复原隐式文件夹声明");
        move_under_parent.apply(&mut folders, false).unwrap();
        assert_eq!(folders.parent("旧成员文件夹"), Some("目标"));
    }

    #[test]
    fn rejects_cyclic_and_dangling_folder_links_on_read() {
        let mut data = json!({ (FOLDERS): ["甲", "乙"], (PARENTS): {"甲":"乙", "乙":"甲"} });
        assert!(read_fields(&data).is_err());
        data[PARENTS] = json!({"甲":"不存在"});
        assert!(read_fields(&data).is_err());
    }
}
