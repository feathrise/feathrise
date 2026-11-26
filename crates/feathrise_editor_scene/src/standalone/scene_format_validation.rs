//! 新场景格式实窗验收读取原作者状态，并通过新建场景表单提交原创建请求。

#[path = "cube_save_validation.rs"]
pub mod cube_save;

use bevy::{
    prelude::*,
    text::{EditableText, TextEdit},
};
use feathrise_editor_ui::{EditorButtonClick, InteractionButton, UiInteraction};
use feathrise_scene_content::{SceneCatalogAsset, SceneContentRegistry, SceneId};

use super::validation_fixture::ui_visible;
use crate::{
    authoring::{
        EditorAuthoringSession,
        basic_shapes::{BasicShapeState, CuboidCreationRequest},
    },
    components::{
        NewSceneNameText, SceneEditorActionButton, SceneEditorGltfBrowserRowGlyph, SceneEditorSceneRootRow, SceneEditorSceneRow,
    },
    resources::{SceneBrowserIndexState, SceneBrowserState, SceneEditorState, SceneEditorViewportState},
    types::{EditorAction, SceneBrowserItem},
    ui::{
        gltf_browser::{EditorGltfBrowserState, PlacementTreeExpander},
        scene_context_menu::{SceneObjectKey, SceneObjectRow},
        scene_inspector::InspectorTree,
    },
};

#[derive(Default, Resource)]
struct OwnedSceneFixture {
    name_queued: bool,
}

/// 由宿主在验证本轮工程与报告目录的归属后安装。普通作者实例没有此端口。
pub fn install(app: &mut App) {
    app.init_resource::<OwnedSceneFixture>();
}

fn require_owned(world: &World) -> Result<(), String> {
    if world.contains_resource::<OwnedSceneFixture>() {
        Ok(())
    } else {
        Err("场景格式验收端口未安装".into())
    }
}

/// 只观察目录、草稿、摆放和成功保存序号，不替代正式加载或保存回执。
#[derive(Default)]
pub struct Observation {
    pub catalog: Option<Vec<(SceneId, String)>>,
    pub current: Option<SceneId>,
    pub selected_source: Option<SceneId>,
    pub subscenes: Vec<SceneId>,
    pub placement_count: usize,
    pub save_sequence: u64,
    pub dirty: bool,
    pub dialog: bool,
    pub directory_visible: bool,
    pub directory_ready: bool,
    pub directory_menu_active: bool,
    pub tree_visible: bool,
    pub open: bool,
}

pub fn observe(world: &World) -> Result<Observation, String> {
    require_owned(world)?;
    let Some(editor) = world.get_resource::<SceneEditorState>() else {
        return Ok(Observation::default());
    };
    let sidebar = world.get_resource::<crate::ui::sidebars::SceneEditorSidebarState>();
    let directory_ready = world
        .get_resource::<SceneBrowserState>()
        .is_some_and(|browser| {
            browser.initial_load_resolved
                && world
                    .get_resource::<SceneBrowserIndexState>()
                    .is_some_and(|index| {
                        !index.rebuilding
                            && index.applied_projection.is_some()
                            && index.desired_projection == index.applied_projection
                    })
        });
    let mut observation = Observation {
        selected_source: editor.scene_list_selection_id.clone(),
        dirty: editor.dirty,
        dialog: editor.new_scene_dialog_open,
        open: editor.open,
        directory_visible: sidebar.is_some_and(|state| state.scene_browser_visible()),
        directory_ready,
        directory_menu_active: world
            .get_resource::<crate::ui::scene_sidebar::SceneDirectoryMenuState>()
            .is_some_and(|menu| menu.owns_mouse()),
        tree_visible: sidebar.is_some_and(|state| state.scene_tree_visible()),
        ..default()
    };
    if let Some(catalog) = world
        .get_resource::<SceneContentRegistry>()
        .and_then(|registry| {
            world
                .resource::<Assets<SceneCatalogAsset>>()
                .get(&registry.catalog)
        })
    {
        if catalog.scenes.len() > 2 {
            return Err("验收工程场景数超过本轮夹具范围".into());
        }
        observation.catalog = Some(
            catalog
                .scenes
                .iter()
                .map(|entry| (entry.id.clone(), entry.display_name.clone()))
                .collect(),
        );
    }
    if let Some(session) = world.get_resource::<EditorAuthoringSession>() {
        observation.save_sequence = session.successful_save_sequence;
        if let Some(draft) = &session.draft {
            if draft.manifest.subscenes.len() > 1 {
                return Err("验收工程包含非本轮创建的子场景引用".into());
            }
            observation.current = Some(draft.scene_id.clone());
            observation.subscenes = draft
                .manifest
                .subscenes
                .iter()
                .map(|reference| reference.source_scene_id.clone())
                .collect();
            observation.placement_count = draft.placements.placements.len();
        }
    }
    Ok(observation)
}

/// 子场景自有立方体使用原坐标创建入口，使运行截图能验证实际载入的引用内容。
pub fn request_cuboid(world: &mut World, scene: &SceneId) -> Result<(), String> {
    request_cuboid_at(world, scene, Vec3::new(0., 1., 0.))
}

/// 世界中心直接交给正式坐标创建入口，由同一作者事务验证并落印。
pub fn request_cuboid_at(world: &mut World, scene: &SceneId, center: Vec3) -> Result<(), String> {
    require_owned(world)?;
    if !super::preview_ready_for(world, scene.as_str()) {
        return Err("子场景作者视口尚未就绪".into());
    }
    if let Some(reason) = super::saved_play_blocker(world) {
        return Err(reason);
    }
    let revision = world.resource::<EditorAuthoringSession>().revision;
    let mut shapes = world.resource_mut::<BasicShapeState>();
    if shapes.creation_request.is_some() {
        return Err("原坐标创建请求已占用".into());
    }
    shapes.creation_identity = Some((scene.clone(), revision));
    shapes.creation_request = Some(CuboidCreationRequest {
        target: feathrise_scene_content::ScenePlacementAssetTarget::DEFAULT_CUBOID,
        position: Some(center),
    });
    Ok(())
}

/// 目录右键菜单必须从真实布局中的根行打开，菜单按钮本身没有常驻入口。
pub fn creation_root(world: &mut World) -> Result<Option<Entity>, String> {
    require_owned(world)?;
    Ok(world
        .query_filtered::<(Entity, &ComputedNode), With<SceneEditorSceneRootRow>>()
        .iter(world)
        .find_map(|(root, layout)| (layout.size().min_element() > 2.).then_some(root)))
}

pub fn creation_menu_button(world: &mut World) -> Result<Option<Entity>, String> {
    require_owned(world)?;
    Ok(action_button(world, EditorAction::NewScene))
}

pub fn submit_creation(world: &mut World, name: &str) -> Result<Option<Entity>, String> {
    require_owned(world)?;
    let editor = world.resource::<SceneEditorState>();
    if !editor.new_scene_dialog_open {
        return Ok(None);
    }
    if editor.new_scene_name == name {
        let action = press_action(world, EditorAction::ConfirmNewScene);
        if action.is_some() {
            world.resource_mut::<OwnedSceneFixture>().name_queued = false;
        }
        return Ok(action);
    }
    if !world.resource::<OwnedSceneFixture>().name_queued {
        let input = world.query_filtered::<(Entity, &ComputedNode), (With<EditableText>, With<NewSceneNameText>)>()
            .iter(world).find_map(|(input, layout)| (layout.size().min_element() > 2.).then_some(input));
        if let Some(input) = input {
            // 先让 Bevy 原文本输入读取一次提交，再由正式按钮校验冻结的名称和创建事务。
            world
                .get_mut::<EditableText>(input)
                .ok_or("正式场景名称输入已移除")?
                .queue_edit(TextEdit::Insert(name.into()));
            world.resource_mut::<OwnedSceneFixture>().name_queued = true;
        }
    }
    Ok(None)
}

/// 选择原目录行。已打开的源场景标签也显示同名文本，但点击标签会切换当前场景。
pub fn select_source(world: &mut World, scene: &SceneId) -> Result<Option<Entity>, String> {
    require_owned(world)?;
    let entity = world
        .query_filtered::<(Entity, &SceneEditorSceneRow, &ComputedNode), (
            Without<bevy::ui::InteractionDisabled>,
            With<EditorButtonClick>,
        )>()
        .iter(world)
        .find_map(|(entity, row, layout)| {
            (matches!(&row.item, Some(SceneBrowserItem::Scene(id)) if id == scene)
                && layout.size().min_element() > 2.)
                .then_some(entity)
        });
    if let Some(entity) = entity {
        world
            .entity_mut(entity)
            .insert((UiInteraction::None, EditorButtonClick::triggered()));
    }
    Ok(entity)
}

/// 用生产场景树绑定身份核对本轮截图。目录名和其他已打开标签不能替代真实引用行。
pub fn reference_row_visible(world: &mut World, parent: &SceneId, name: &str) -> bool {
    world
        .query::<(&SceneObjectRow, &ComputedNode, Option<&InheritedVisibility>)>()
        .iter(world)
        .any(|(row, layout, visibility)| {
            visibility.is_none_or(|visibility| visibility.get())
                && layout.size().min_element() > 2.
                && row.0.as_ref().is_some_and(|target| {
                    target.scene_id == *parent
                        && target.name == name
                        && matches!(&target.key, SceneObjectKey::Subscene { .. })
                })
        })
}

pub(crate) fn tree_row_glyph(world: &World, row: Entity) -> Option<Entity> {
    world.get::<Children>(row)?.iter()
        .filter_map(|child| world.get::<Children>(child))
        .flat_map(|children| children.iter())
        .find(|&child| world.get::<SceneEditorGltfBrowserRowGlyph>(child).is_some())
}

/// 按真实图标中心比较逻辑像素，验收结果不受窗口 DPI 和界面缩放影响。
pub(crate) fn tree_icon_offset(world: &World, parent: Entity, child: Entity) -> Option<f32> {
    let parent_x = world.get::<UiGlobalTransform>(parent)?.translation.x;
    let child_x = world.get::<UiGlobalTransform>(child)?.translation.x;
    Some((child_x - parent_x) * world.get::<ComputedNode>(child)?.inverse_scale_factor())
}

/// 箭头保留独立图形，图标与箭头共用开合区域，名称用于选中。
pub(crate) fn tree_branch_arrow(world: &World, row: Entity) -> Option<Entity> {
    world.get::<Children>(row)?.iter().find(|&child| {
        world.get::<PlacementTreeExpander>(child).is_some()
            && world.get::<bevy::ui::InteractionDisabled>(child).is_none()
            && ui_visible(world, child)
    })
}

/// 只定位当前自有场景的已布局 GLB 展开箭头，展开仍由宿主发送原窗口鼠标事件。
pub fn main_glb_tree_arrow(world: &mut World, scene: &SceneId) -> Result<Option<Entity>, String> {
    require_owned(world)?;
    Ok(world.query::<(Entity, &SceneObjectRow)>().iter(world).find_map(|(entity, row)| {
        let target = row.0.as_ref()?;
        if target.scene_id != *scene || !matches!(&target.key, SceneObjectKey::MainSceneGlb) || !ui_visible(world, entity) {
            return None;
        }
        tree_branch_arrow(world, entity)
    }))
}

/// 摆放行真正可见后读取同一场景树的文案，以覆盖旧搜索框会出现的条件。
pub fn empty_terrain_placement_tree_labels(
    world: &mut World,
    scene: &SceneId,
    placement: &str,
) -> Result<Option<Vec<String>>, String> {
    require_owned(world)?;
    let session = world.resource::<EditorAuthoringSession>();
    let Some(draft) = session.draft.as_ref().filter(|draft| draft.scene_id == *scene) else {
        return Ok(None);
    };
    if !draft.manifest.terrain.is_empty()
        || draft.manifest.terrains.as_ref().is_some_and(|terrains| !terrains.is_empty())
    {
        return Err("场景显示验收需要无地形的自有场景".into());
    }
    let Some((_, tree)) = visible_placement_row(world, scene, placement) else {
        return Ok(None);
    };
    let parent = world.get::<ChildOf>(tree).ok_or("场景树缺少左栏宿主")?.parent();
    // 场景配置和主 GLB 根行位于对象滚动区外，与该树共用左栏宿主。
    Ok(Some(visible_labels_in(world, parent)))
}

/// 场景显示控件搬到属性栏后，仍在无地形场景验证同一套原生按钮。
pub fn scene_preview_property_labels(world: &mut World) -> Result<Vec<String>, String> {
    require_owned(world)?;
    let panel = world.query::<(Entity, &Name)>().iter(world).find_map(|(entity, name)| {
        (name.as_str() == "场景预览属性" && ui_visible(world, entity)).then_some(entity)
    });
    Ok(panel.map_or_else(Vec::new, |panel| visible_labels_in(world, panel)))
}

fn visible_labels_in(world: &mut World, root: Entity) -> Vec<String> {
    world.query::<(Entity, &Text)>().iter(world).filter_map(|(entity, text)| {
        (ui_ancestors(world, entity).any(|ancestor| ancestor == root) && ui_visible(world, entity))
            .then(|| text.0.clone())
    }).collect()
}

pub struct PlacementTreeView {
    pub row: Entity,
    pub selected: bool,
    pub center: Vec3,
    pub extent: f32,
}

/// 把本轮立方体的实际布局行与同一场景的离屏相机对应起来，供宿主投递真实鼠标手势。
pub fn placement_tree_view(
    world: &mut World,
    scene: &SceneId,
    placement: &str,
) -> Result<Option<PlacementTreeView>, String> {
    require_owned(world)?;
    let Some((row, _)) = visible_placement_row(world, scene, placement) else {
        return Ok(None);
    };
    Ok(world.get_resource::<SceneEditorViewportState>()
        .filter(|viewport| viewport.framed_scene_id.as_ref() == Some(scene))
        .map(|viewport| PlacementTreeView {
            row,
            selected: world.resource::<EditorGltfBrowserState>().selected_placement_id.as_deref() == Some(placement),
            center: viewport.center,
            extent: viewport.extent,
        }))
}

pub(crate) fn visible_placement_row(world: &mut World, scene: &SceneId, placement: &str) -> Option<(Entity, Entity)> {
    world.query::<(Entity, &SceneObjectRow)>().iter(world).find_map(|(entity, row)| {
        let target = row.0.as_ref()?;
        if target.scene_id != *scene
            || !matches!(&target.key, SceneObjectKey::Placement { id, mesh: None, .. } if id == placement)
            || !ui_visible(world, entity)
        {
            return None;
        }
        let tree = ui_ancestors(world, entity).find(|ancestor| world.get::<InspectorTree>(*ancestor).is_some())?;
        Some((entity, tree))
    })
}

fn ui_ancestors(world: &World, entity: Entity) -> impl Iterator<Item = Entity> + '_ {
    std::iter::successors(Some(entity), move |entity| world.get::<ChildOf>(*entity).map(ChildOf::parent))
}

pub(crate) fn action_button(world: &mut World, action: EditorAction) -> Option<Entity> {
    world
        .query_filtered::<(Entity, &SceneEditorActionButton, &ComputedNode), (
            With<InteractionButton>,
            Without<bevy::ui::InteractionDisabled>,
        )>()
        .iter(world)
        .find_map(|(entity, button, layout)| {
            (button.0 == action && layout.size().min_element() > 2.).then_some(entity)
        })
}

fn press_action(world: &mut World, action: EditorAction) -> Option<Entity> {
    let entity = action_button(world, action)?;
    world
        .entity_mut(entity)
        .insert((UiInteraction::Pressed, EditorButtonClick::triggered()));
    Some(entity)
}
