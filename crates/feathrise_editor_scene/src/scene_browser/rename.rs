//! 场景名称和备注的后台原子事务；网格、作者源及LOD不属于资料保存范围。
#[cfg(test)]
mod tests;
use crate::authoring::{EditorAuthoringSession, safe_asset_path, workspace_asset_root};
use crate::resources::{SceneEditorState, SceneFolderPersistence};
use crate::types::SceneEditTarget;
use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, futures::check_ready};
use feathrise_scene_content::authoring_transaction::{PreparedWrite, commit_prepared_writes_checked};
use feathrise_scene_content::{
    SCENE_CATALOG_ASSET_PATH, SceneCatalogAsset, SceneCatalogEntry, SceneId, SceneManifestAsset,
};
use std::path::Path;

pub(crate) type SceneEditResult = (String, Result<EditedScene, String>);
pub(crate) struct EditedScene {
    id: SceneId,
    name: String,
    notes: String,
    name_changed: bool,
    manifest_path: String,
}

fn normalize_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 128 || name.chars().any(char::is_control) {
        return Err("场景名称须为1—128个字符，且不能包含控制字符".into());
    }
    Ok(name.into())
}

pub(crate) fn begin_scene_edit(
    target: &SceneEditTarget,
    name: &str,
    notes: &str,
    session: &EditorAuthoringSession,
    persistence: &mut SceneFolderPersistence,
    catalog: Option<&SceneCatalogAsset>,
) -> Result<(), String> {
    if session.saving || persistence.busy() {
        return Err("场景正在保存，请等待完成后编辑".into());
    }
    let name = normalize_name(name)?;
    let entry = catalog
        .and_then(|catalog| catalog.scenes.iter().find(|entry| entry.id == target.id))
        .ok_or("目标场景尚未加载或已不存在")?
        .clone();
    if entry.display_name != target.original_name {
        return Err("场景名称已变化，请重新打开资料".into());
    }
    let target = target.clone();
    let notes = notes.to_owned();
    persistence.rename_task = Some(IoTaskPool::get().spawn(async move {
        (
            entry.manifest_path(),
            edit_on_disk(&workspace_asset_root(), &entry, &target, &name, &notes),
        )
    }));
    Ok(())
}

fn edit_on_disk(
    root: &Path,
    expected: &SceneCatalogEntry,
    target: &SceneEditTarget,
    name: &str,
    notes: &str,
) -> Result<EditedScene, String> {
    let name = normalize_name(name)?;
    let catalog_path = safe_asset_path(root, SCENE_CATALOG_ASSET_PATH)?;
    let catalog_bytes =
        std::fs::read(&catalog_path).map_err(|error| format!("读取场景目录失败：{error}"))?;
    let catalog: SceneCatalogAsset =
        serde_json::from_slice(&catalog_bytes).map_err(|error| format!("场景目录损坏：{error}"))?;
    if catalog.schema_version != feathrise_scene_content::SCENE_SCHEMA_VERSION
        || catalog
            .scenes
            .iter()
            .filter(|entry| entry.id == expected.id)
            .count()
            != 1
    {
        return Err("场景目录版本或场景唯一身份无效".into());
    }
    let entry = catalog
        .scenes
        .iter()
        .find(|entry| entry.id == expected.id)
        .ok_or("待编辑场景已从目录移除")?;
    if entry.folder != expected.folder || entry.display_name != expected.display_name {
        return Err("场景名称或目录已被其他操作修改，请刷新后重试".into());
    }
    let manifest_relative = entry.manifest_path();
    let manifest_path = safe_asset_path(root, &manifest_relative)?;
    let manifest_bytes =
        std::fs::read(&manifest_path).map_err(|error| format!("读取场景清单失败：{error}"))?;
    let manifest: SceneManifestAsset = serde_json::from_slice(&manifest_bytes)
        .map_err(|error| format!("场景清单损坏：{error}"))?;
    if manifest.schema_version != feathrise_scene_content::SCENE_SCHEMA_VERSION
        || manifest.id != target.id
        || manifest.display_name != target.original_name
        || manifest.notes != target.original_notes
    {
        return Err("场景资料已被其他操作修改，请重新打开".into());
    }
    // 仅覆盖资料字段，保留磁盘上的完整网格和LOD；不加载高度源或创建运行时几何候选。
    let mut catalog_value: serde_json::Value =
        serde_json::from_slice(&catalog_bytes).map_err(|error| error.to_string())?;
    let mut manifest_value: serde_json::Value =
        serde_json::from_slice(&manifest_bytes).map_err(|error| error.to_string())?;
    let position = catalog
        .scenes
        .iter()
        .position(|entry| entry.id == expected.id)
        .ok_or("目标场景已不存在")?;
    catalog_value["scenes"][position]["display_name"] = name.clone().into();
    manifest_value["display_name"] = name.clone().into();
    if notes.is_empty() {
        manifest_value
            .as_object_mut()
            .ok_or("场景清单不是JSON对象")?
            .remove("notes");
    } else {
        manifest_value["notes"] = notes.into();
    }
    let name_changed = name != expected.display_name;
    let mut writes = vec![PreparedWrite::new(
        manifest_path.clone(),
        serde_json::to_vec_pretty(&manifest_value).map_err(|error| error.to_string())?,
    )];
    if name_changed {
        writes.push(PreparedWrite::new(
            catalog_path.clone(),
            serde_json::to_vec_pretty(&catalog_value).map_err(|error| error.to_string())?,
        ));
    }
    // 同一事务保留目录与清单的精确字节基线，外部更新仍会阻止部分发布并回滚。
    commit_prepared_writes_checked(writes, |_, path| {
        if std::fs::read(&catalog_path).map_err(|error| error.to_string())? != catalog_bytes {
            return Err("场景目录已被外部修改，已停止资料编辑".to_owned());
        }
        let baseline = if path == catalog_path {
            &catalog_bytes
        } else {
            &manifest_bytes
        };
        if std::fs::read(path).map_err(|error| error.to_string())? != *baseline {
            return Err(format!("{}已被外部修改，已停止资料编辑", path.display()));
        }
        Ok(())
    })
    .map_err(|error| error.message().to_owned())?;
    Ok(EditedScene {
        id: expected.id.clone(),
        name,
        notes: notes.into(),
        name_changed,
        manifest_path: manifest_relative,
    })
}

pub(crate) fn poll_scene_edit(
    mut persistence: ResMut<SceneFolderPersistence>,
    mut session: ResMut<EditorAuthoringSession>,
    mut editor: ResMut<SceneEditorState>,
    registry: Option<Res<feathrise_scene_content::SceneContentRegistry>>,
    mut catalogs: ResMut<Assets<SceneCatalogAsset>>,
    mut manifests: ResMut<Assets<SceneManifestAsset>>,
    asset_server: Res<AssetServer>,
    numeric: Option<Res<feathrise_editor_ui::ActiveNumericInput>>,
    mut windows: Query<&mut Window, With<bevy::window::PrimaryWindow>>,
) {
    let Some(task) = persistence.rename_task.as_mut() else {
        return;
    };
    let Some((manifest_path, result)) = check_ready(task) else {
        return;
    };
    persistence.rename_task = None;
    match result {
        Ok(result) => {
            let closed = finish_edit_dialog(
                &mut editor,
                &result,
                numeric.as_ref().is_some_and(|input| input.entity.is_some()),
            );
            if closed && let Ok(mut window) = windows.single_mut() {
                window.ime_enabled = false;
            }
            if let Some(draft) = session
                .draft
                .as_mut()
                .filter(|draft| draft.scene_id == result.id)
            {
                draft.manifest.display_name.clone_from(&result.name);
                draft.manifest.notes.clone_from(&result.notes);
            }
            if let Some(registry) = registry.as_deref() {
                if result.name_changed
                    && let Some(mut catalog) = catalogs.get_mut(&registry.catalog)
                    && let Some(entry) = catalog.scenes.iter_mut().find(|entry| entry.id == result.id)
                {
                    entry.display_name.clone_from(&result.name);
                }
                if let Some(handles) = registry.handles(&result.id)
                    && let Some(mut manifest) = manifests.get_mut(&handles.manifest)
                {
                    manifest.display_name.clone_from(&result.name);
                    manifest.notes.clone_from(&result.notes);
                }
            }
            if result.name_changed {
                asset_server.reload(SCENE_CATALOG_ASSET_PATH);
            }
            asset_server.reload(result.manifest_path);
            editor.status = format!("场景“{}”名称和备注已保存", result.name);
            if editor.new_scene_dialog_open
                && editor
                    .new_scene_edit_target
                    .as_ref()
                    .is_some_and(|target| target.id == result.id)
            {
                editor.status.push_str("；表单中后续修改尚未保存");
            }
        }
        Err(error) => {
            asset_server.reload(manifest_path);
            asset_server.reload(SCENE_CATALOG_ASSET_PATH);
            report_edit_failure(&mut editor, &error);
        }
    }
}

fn finish_edit_dialog(
    editor: &mut SceneEditorState,
    saved: &EditedScene,
    numeric_editing: bool,
) -> bool {
    let unchanged = !numeric_editing
        && editor.new_scene_name.trim() == saved.name
        && editor.new_scene_notes == saved.notes;
    let Some(target) = editor
        .new_scene_edit_target
        .as_mut()
        .filter(|target| target.id == saved.id)
    else {
        return false;
    };
    target.original_name.clone_from(&saved.name);
    target.original_notes.clone_from(&saved.notes);
    if unchanged {
        editor.new_scene_dialog_open = false;
        editor.new_scene_edit_target = None;
        return true;
    }
    false
}

fn report_edit_failure(editor: &mut SceneEditorState, error: &str) {
    editor.status = format!("场景资料编辑失败：{error}");
}
