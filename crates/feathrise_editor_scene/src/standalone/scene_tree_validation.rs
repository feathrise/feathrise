//! 原生窗口验收场景树换选与来源定位，沿用原保存和截图。

use super::{open_scene_tree, ui_visible};
use crate::{
    authoring::EditorAuthoringSession,
    components::SceneEditorRightPanelScroll,
    resources::SceneEditorState,
    ui::{
        external_files::platform_path,
        file_manager::{FileManagerState, FileRow},
        file_manager_validation::right_click,
        gltf_browser::EditorGltfBrowserState,
        scene_context_menu::{SceneMenuAction, SceneObjectKey, SceneObjectRow, press_action_for_validation},
    },
    viewport::modes::{self, ViewportMode, ViewportModeState},
};
use bevy::{prelude::*, window::PrimaryWindow};
use feathrise_scene_content::SceneId;
use serde_json::{Value, json};
use std::{fs, path::{Path, PathBuf}};

const MODEL: &str = "sample-model";
const ASSET_PATH: &str = "scene/probe.glb";
const MESH: &str = "ProjectAssetQuad";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    Open,
    Move,
    SelectRoot,
    SelectMesh,
    Verify,
    OpenMenu(u8),
    ChooseLocate,
    Locate,
}

pub(super) fn enabled() -> bool {
    std::env::var("FEATHRISE_SCENE_TREE_VALIDATION").as_deref() == Ok("1")
}

pub(super) fn preflight(project: &Path, scene: &SceneId) -> Result<(), String> {
    let output = PathBuf::from(std::env::var_os("NEW_EDITOR_NATIVE_SMOKE_DIR").ok_or("场景树验收缺少输出目录")?);
    let work = output.parent().ok_or("场景树验收缺少独占工作目录")?;
    let owner: Value = serde_json::from_slice(&fs::read(work.join(".feathrise-native-validation-owner.json"))
        .map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    if owner["schema"] != 1 || owner["purpose"] != "ordinary-author-boundary-validation"
        || owner["work_root"] != json!(work) || project != work.join("独立工程 A")
        || scene.as_str() != "M1-SAMPLE"
    {
        return Err("场景树验收只接受本轮独占工程 A 的中性样例".into());
    }
    Ok(())
}

pub(super) fn advance(
    world: &mut World,
    step: &mut Step,
    cube: &str,
    revision: u64,
    pulse: &mut Option<Entity>,
) -> Result<Option<Value>, String> {
    let session = world.resource::<EditorAuthoringSession>();
    if session.revision != revision {
        return Err("切换视口工具和树行选择不应修改场景修订".into());
    }
    let selected = world.resource::<EditorGltfBrowserState>().selected_placement_id.as_deref();
    let mode = world.resource::<ViewportModeState>().mode;
    let transform_enabled = world.resource::<SceneEditorState>().placement_transform_enabled;
    match *step {
        Step::Open => {
            if open_scene_tree(world)? && let Some(row) = click_row(world, cube, false) {
                *pulse = Some(row);
                *step = Step::Move;
            }
        }
        Step::Move if selected == Some(cube) => {
            *pulse = Some(modes::validation_press_mode(world, ViewportMode::Move)?);
            *step = Step::SelectRoot;
        }
        Step::SelectRoot if mode == ViewportMode::Move && transform_enabled => {
            if let Some(row) = click_row(world, MODEL, false) {
                *pulse = Some(row);
                *step = Step::SelectMesh;
            }
        }
        Step::SelectMesh if selected == Some(MODEL) => {
            // 原根行先展开项目的 GLB，等真实属性文本及子网格绑定完成再点击。
            if mode != ViewportMode::Move || !transform_enabled {
                return Err("场景树根行选择改变了移动工具".into());
            }
            if properties_show_path(world) && let Some(row) = click_row(world, MODEL, true) {
                *pulse = Some(row);
                *step = Step::Verify;
            }
        }
        Step::Verify => {
            if mode != ViewportMode::Move || !transform_enabled {
                return Err("场景树网格选择改变了移动工具".into());
            }
            let browser = world.resource::<EditorGltfBrowserState>();
            if selected == Some(MODEL) && let Some(mesh) = &browser.selection
                && mesh.valid && mesh.placement_id == MODEL && mesh.mesh_name == MESH
            {
                *step = Step::OpenMenu(0);
            }
        }
        Step::OpenMenu(frame) => {
            let Some(row) = scene_row(world, MODEL, true) else { return Ok(None); };
            let position = world.get::<UiGlobalTransform>(row).ok_or("网格树行尚未布局")?.translation.as_dvec2();
            let window = world.query_filtered::<Entity, With<PrimaryWindow>>().single(world)
                .map_err(|_| "没有场景树验收窗口")?;
            let mut pending = Some((position, frame));
            // 经原窗口事件右击子网格，原菜单仍须确认冻结身份和实际悬停。
            *step = if right_click(world, &mut pending, window, position) {
                Step::ChooseLocate
            } else {
                Step::OpenMenu(pending.unwrap().1)
            };
        }
        Step::ChooseLocate => {
            if let Some(button) = press_action_for_validation(world, SceneMenuAction::RevealGlb) {
                *pulse = Some(button);
                *step = Step::Locate;
            }
        }
        Step::Locate => {
            if mode != ViewportMode::Move || !transform_enabled || selected != Some(MODEL) {
                return Err("定位 GLB 改变了场景选择或移动工具".into());
            }
            if file_window_shows_source(world)? {
                return Ok(Some(json!({
                    "viewport_mode": format!("{mode:?}"),
                    "previous_placement": cube,
                    "selected_placement": MODEL,
                    "selected_mesh": MESH,
                    "asset_path_shown_in_properties": ASSET_PATH,
                    "file_window_selected": ASSET_PATH,
                    "scene_revision": revision,
                })));
            }
        }
        _ => {}
    }
    Ok(None)
}

fn click_row(world: &mut World, placement: &str, child: bool) -> Option<Entity> {
    let row = scene_row(world, placement, child)?;
    world.entity_mut(row).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    Some(row)
}

fn scene_row(world: &mut World, placement: &str, child: bool) -> Option<Entity> {
    world.query::<(Entity, &SceneObjectRow)>().iter(world).find_map(|(entity, row)| {
        let target = row.0.as_ref()?;
        matches!(&target.key, SceneObjectKey::Placement { id, mesh, .. }
            if id == placement && mesh.is_some() == child && ui_visible(world, entity)).then_some(entity)
    })
}

fn file_window_shows_source(world: &mut World) -> Result<bool, String> {
    let paths = feathrise_editor_core::editor_project_paths().ok_or("场景树验收缺少当前资产根")?;
    let path = platform_path(paths.asset_root()).join(ASSET_PATH);
    let files = world.resource::<FileManagerState>();
    let Some(window) = files.windows.iter().find(|window| {
        files.focused == Some(window.id) && !window.minimized && !window.loading
            && Some(window.path.as_path()) == path.parent()
            && window.selection.len() == 1 && window.selection.contains(&path)
    }) else { return Ok(false); };
    let window = window.id;
    Ok(world.query::<(Entity, &FileRow)>().iter(world).any(|(entity, row)| {
        row.window == window && row.path.as_deref() == Some(path.as_path()) && ui_visible(world, entity)
    }))
}

fn properties_show_path(world: &mut World) -> bool {
    world.query::<(Entity, &Text)>().iter(world).any(|(entity, text)| {
        text.0 == ASSET_PATH && ui_visible(world, entity)
            && std::iter::successors(Some(entity), |entity| world.get::<ChildOf>(*entity).map(ChildOf::parent))
                .any(|ancestor| world.get::<SceneEditorRightPanelScroll>(ancestor).is_some())
    })
}
