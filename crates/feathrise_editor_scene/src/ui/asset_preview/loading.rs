//! 资产选择、异步加载和离屏视图生命周期。

use crate::ui::theme as colors;
use bevy::asset::{DependencyLoadState, LoadState, RecursiveDependencyLoadState}; // Scene0完整依赖失败诊断。
use bevy::camera::visibility::RenderLayers; // 模型、灯光与相机共享专用层。
use bevy::camera::{Projection, RenderTarget}; // 预览透视投影和独立RTT目标。
use bevy::gltf::{Gltf, GltfAssetLabel}; // GLB/GLTF同时加载Scene0和真实子资产根元数据。
use bevy::prelude::*; // ECS、UI、相机、灯光与资产类型。
use bevy::render::render_resource::{
    Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
}; // RTT创建与二维纹理格式复验。
use bevy::ui::FocusPolicy; // 图像输出让父画布统一接收鼠标导航。
use bevy::ui::widget::ViewportNode; // 模型按侧栏或中央所在画布的真实尺寸维护RTT。
use bevy::world_serialization::{WorldAsset, WorldAssetRoot}; // Scene0按需生成。
use feathrise_scene_core::view::DeferredRasterAuxiliaryCamera; // 共享Deferred材质仍可在离屏PBR预览中显示。
use feathrise_editor_core::constants::render_layers::SCENE_EDITOR_ASSET_PREVIEW_LAYER; // 全局唯一预览层。 // 离屏清屏色与UI画布一致。

use super::resources::AssetPreviewNavigation; // 换资产时清除前一个预览的缩放与视角。
use super::compatible_texture::{CompatiblePreviewTexture, retry_capacity, sample_compatible};
use super::types::*; // 预览状态、阶段和全部专用marker。
use super::view_math::MODEL_PREVIEW_FOV_RADIANS; // 初始相机与交互导航共用同一投影。
use crate::components::SceneEditorOwnedEntity; // 资源审计可识别预览相机、灯光和Scene0根。
use crate::resources::SceneEditorState; // 当前F12显隐与所选资产路径。
use crate::ui::sidebars::{EditorRightSidebarTab, SceneEditorSidebarState}; // 非资产页停止额外3D渲染。

/// Scene0本体或任一依赖失败时返回作者可读的确定原因。
fn scene_load_failure(asset_server: &AssetServer, handle: &Handle<WorldAsset>) -> Option<String> {
    let (direct, dependencies, recursive) = asset_server.get_load_states(handle.id())?;
    match direct {
        LoadState::Failed(error) => Some(format!("Scene0加载失败：{error}")),
        _ if matches!(dependencies, DependencyLoadState::Failed(_)) => {
            Some("Scene0直接依赖加载失败，请检查模型引用的纹理或缓冲文件".to_owned())
        }
        _ if matches!(recursive, RecursiveDependencyLoadState::Failed(_)) => {
            Some("Scene0递归依赖加载失败，请检查模型引用链".to_owned())
        }
        _ => None,
    }
}

/// 纯动作可以没有Scene0，但根文件或依赖失败仍须报告，不能被旧缓存元数据掩盖。
fn gltf_source_load_failure(asset_server: &AssetServer, handle: &Handle<Gltf>) -> Option<String> {
    let (direct, dependencies, recursive) = asset_server.get_load_states(handle.id())?;
    match direct {
        LoadState::Failed(error) => Some(format!("GLTF源文件加载失败：{error}")),
        _ if matches!(dependencies, DependencyLoadState::Failed(_)) => {
            Some("GLTF源文件直接依赖加载失败，请检查纹理或缓冲文件".to_owned())
        }
        _ if matches!(recursive, RecursiveDependencyLoadState::Failed(_)) => {
            Some("GLTF源文件递归依赖加载失败，请检查资源引用链".to_owned())
        }
        _ => None,
    }
}

/// BSN模型画布首次出现时建立唯一离屏纹理、Camera3d和专用方向光。
pub(super) fn setup_asset_preview_viewport(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    viewports: Query<Entity, Added<AssetPreviewModelViewport>>,
    mut state: ResMut<AssetPreviewState>,
) {
    let Ok(viewport) = viewports.single() else {
        return; // 尚未生成或意外重复时都不建立第二套预览视图。
    };
    if state.camera.is_some() {
        return; // 固定画布只初始化一次。
    }
    let size = Extent3d {
        width: 1, // ViewportNode在画布真正参与布局后按物理尺寸调整。
        height: 1,
        depth_or_array_layers: 1,
    };
    let mut render_image = Image {
        texture_descriptor: TextureDescriptor {
            label: Some("scene_editor_asset_preview_target"),
            size,
            dimension: TextureDimension::D2,
            format: TextureFormat::Bgra8UnormSrgb,
            mip_level_count: 1,
            sample_count: 1,
            usage: TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_DST
                | TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        },
        ..default()
    };
    render_image.resize(size);
    let render_target = images.add(render_image);
    let camera = commands
        .spawn((
            Name::new("资产独立预览相机"),
            AssetPreviewCamera,
            SceneEditorOwnedEntity,
            DeferredRasterAuxiliaryCamera,
            Camera3d::default(),
            Camera {
                order: -4,
                is_active: false, // 只有可见的侧栏或中央显示完整模型时才付出额外View成本。
                clear_color: ClearColorConfig::Custom(colors::MODEL_PREVIEW_BG),
                ..default()
            },
            RenderTarget::from(render_target.clone()),
            Projection::Perspective(PerspectiveProjection {
                fov: MODEL_PREVIEW_FOV_RADIANS,
                near: 0.01,
                far: 10_000.0,
                ..default()
            }),
            Transform::from_xyz(2.0, 1.2, 2.0).looking_at(Vec3::ZERO, Vec3::Y),
            RenderLayers::layer(SCENE_EDITOR_ASSET_PREVIEW_LAYER),
        ))
        .id();
    let light = commands
        .spawn((
            Name::new("资产预览主光"),
            AssetPreviewLight,
            SceneEditorOwnedEntity,
            DirectionalLight {
                illuminance: 13_000.0,
                shadow_maps_enabled: false, // 小预览以稳定可读为先，不建立额外阴影图View。
                ..default()
            },
            Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, -0.75, -0.85, 0.0)),
            Visibility::Hidden,
            RenderLayers::layer(SCENE_EDITOR_ASSET_PREVIEW_LAYER),
        ))
        .id();
    commands.entity(viewport).insert((
        ViewportNode::new(camera), // 画布缩放时只由Bevy正式节点调整RTT，不做每帧重建。
        FocusPolicy::Pass,         // 鼠标到达父预览画布，由画布独占缩放和平移。
    ));
    state.camera = Some(camera);
    state.light = Some(light);
    state._render_target = Some(render_target); // 相机、UI和资源共同明确持有RTT。
}

/// 所选路径变化或F12关闭时，只切换当前一个强句柄并精确回收旧Scene0根。
pub(super) fn sync_asset_preview_selection(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    editor: Res<SceneEditorState>,
    sidebar_layout: (
        Res<SceneEditorSidebarState>,
        Option<Res<crate::resources::SceneEditorLayoutState>>,
        Option<Res<super::AssetPreviewWorkspace>>,
    ),
    mut state: ResMut<AssetPreviewState>,
    mut navigation: ResMut<AssetPreviewNavigation>, // 导航不随UI重排清零，只随当前文件身份重置。
    mut cameras: Query<&mut Camera, With<AssetPreviewCamera>>,
    mut lights: Query<&mut Visibility, With<AssetPreviewLight>>,
    external: Res<crate::game_authoring::ExternalAssetPreviewRequest>,
    tool_hosts: Query<&crate::game_authoring::DomainToolPreviewHost>,
    workspace: Res<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>, // 活动角色标签与资源预览互斥。
    mut focus: Option<ResMut<super::AssetPreviewFocus>>, // 同时决定加载来源、画布位置和裁剪范围。
    preview_canvas: (
        Query<(&ComputedNode, &UiGlobalTransform), With<AssetPreviewCanvas>>,
        Query<(&ComputedNode, &UiGlobalTransform), With<super::components::AssetPreviewCenterHost>>,
    ), // 右栏与中央都核对真实宿主的裁剪尺寸，继续只有一个离屏View。
    sidebar_pages: Query<(
        &crate::ui::sidebars::SceneEditorSidebarPage,
        &ComputedNode,
        &UiGlobalTransform,
    )>, // 三类作者预览按右侧场景页裁剪，不再借用中央矩形。
) {
    let (sidebar, layout, media) = sidebar_layout;
    let (preview_canvas, center_hosts) = preview_canvas;
    let central = media.as_ref().is_some_and(|media| media.active(workspace.active_tab));
    let browsing = focus.as_mut().is_some_and(|focus| {
        focus.sync_workspace(editor.open, workspace.active_tab); // 关闭或切标签即结束旧浏览覆盖，后台完成不修改此意图。
        focus.browsing(workspace.active_tab) // 主动选文件时沿用普通资产加载、导航和右栏可见规则。
    });
    let preview_active = editor.open && !central && !browsing && external.active;
    let tool_page = external.host.and_then(|host| tool_hosts.get(host).ok())
        .map(|host| crate::ui::sidebars::EditorLeftSidebarTab::Domain(host.0));
    let preview_page_visible = tool_page.map_or(sidebar.right == EditorRightSidebarTab::Scene, |page| sidebar.tool_page_visible(page));
    let wanted_identity = preview_active.then_some(external.identity).flatten();
    let wanted_path = if editor.open && central {
        media.as_ref().and_then(|media| media.expanded.as_ref()).map(|entry| entry.path.clone())
    } else if preview_active {
        external.model_path.clone()
    } else if editor.open {
        editor.selected_asset.clone().filter(|path| path != crate::authoring::basic_shapes::CUBOID_SELECTION)
    } else {
        None
    };
    if state.selected_path != wanted_path
        || state.external_preview != wanted_identity
        || preview_active && external.constructed && state.scene_root() != external.root

    {
        // 装备版本也属于完整生命周期身份。
        // 同模型路径的动画、装备或缩放修改也必须换代。
        *navigation = AssetPreviewNavigation::default(); // 新文件与F12关闭都不能继承旧资产的放大或拖动。
        if let Some(root) = state.scene_root() {
            if state.external_constructed {
                commands.entity(root).try_insert((Visibility::Hidden, AssetPreviewRetiredRoot));
            } else {
                commands.entity(root).try_despawn(); // 普通GLB沿用原精确回收；完整作者根必须等Last。
            }
        }
        if let Some(root) = state.model.as_ref().and_then(|model| model.primitive_root) {
            commands.entity(root).try_despawn(); // 单体副本与Scene0根使用同一个文件生命周期回收。
        }
        state.bounds_scan = None; // 旧根的未完成工作不能污染新选择。
        state.model_bounds = None; // 旧模型取景范围不能用于新文件。
        state.model = None; // 图片、数据和新GLB都不能继承旧候选、页码或材质引用。
        state.selected_path = wanted_path.clone();
        state.external_preview = wanted_identity;
        state.external_constructed = preview_active && external.constructed;
        state.external_title = if preview_active { external.title.clone() } else { None };
        state.content = AssetPreviewContent::None;
        state.texture_preview = None;
        state.display_image = None;
        state.status.clear();
        if preview_active && external.constructed && let Some(root) = external.root {
            // 完整中性作者根由其工厂和Ready门负责；内置胶囊没有GLB路径，也不虚构加载请求。
            commands.entity(root).try_insert((AssetPreviewModelRoot, SceneEditorOwnedEntity));
            let generation = state.allocate_model_generation();
            state.content = AssetPreviewContent::Model {
                handle: Handle::default(), root, phase: AssetPreviewPhase::Loading,
            };
            state.model = Some(AssetPreviewModelSession::new(generation, Handle::default()));
            state.status = "正在本地准备完整作者模型、附件与动画".into();
        } else if let Some(path) = wanted_path {
            match preview_file_kind(&path) {
                AssetPreviewFileKind::Image => {
                    state.texture_preview = Some(asset_server.load(path.clone())); // 画面和元数据始终来自同一私有安全发布。
                    state.content = AssetPreviewContent::Image {
                        handle: Handle::default(), // 不再额外请求原Image；采用后记录同一安全label供原会话使用。
                        phase: AssetPreviewPhase::Loading,
                    };
                    state.status = format!("正在异步加载图片 assets/{path}");
                }
                AssetPreviewFileKind::Model => {
                    let generation = state.allocate_model_generation(); // Scene、目录和单体加载共享同一换代身份。
                    let gltf: Handle<Gltf> = asset_server.load(path.clone()); // 根元数据只异步加载一次并保活全部子资产。
                    let handle: Handle<WorldAsset> =
                        asset_server.load(GltfAssetLabel::Scene(0).from_asset(path.clone()));
                    let root = if let Some(root) = external.root.filter(|_| preview_active) {
                        root // 应用只提交已冻结完整来源的展示根，生命周期、扫描与唯一RTT仍由本预览接管。
                    } else {
                        commands.spawn((
                            Name::new(format!("资产预览_{path}")), AssetPreviewModelRoot,
                            SceneEditorOwnedEntity, WorldAssetRoot(handle.clone()), Visibility::Hidden,
                            RenderLayers::layer(SCENE_EDITOR_ASSET_PREVIEW_LAYER),
                        )).id()
                    };
                    state.content = AssetPreviewContent::Model {
                        handle,
                        root,
                        phase: AssetPreviewPhase::Loading,
                    };
                    state.model = Some(AssetPreviewModelSession::new(generation, gltf)); // 默认Scene0，同时后台枚举全部primitive。
                    state.status = format!("正在异步加载模型Scene0 assets/{path}");
                }
                AssetPreviewFileKind::Audio => {
                    state.content = AssetPreviewContent::Audio { phase: AssetPreviewPhase::Loading };
                    state.status = format!("正在异步加载音频 assets/{path}"); // 只发布来源；试听模块按同一身份准备完整音频。
                }
                AssetPreviewFileKind::Data => {
                    state.content = AssetPreviewContent::Data;
                    state.status =
                        "作者数据文件不提供图形画面；当前只显示所选路径与格式".to_owned();
                }
                AssetPreviewFileKind::Document => {
                    state.content = AssetPreviewContent::Document; // 独立后台IO读文档，不要求AssetServer注册文本加载器。
                    state.status.clear(); // 文档自己的工具栏回显读取、草稿和保存状态。
                }
                AssetPreviewFileKind::Unsupported => {
                    state.content = AssetPreviewContent::Unsupported;
                    state.status = "当前文件格式没有可视预览器".to_owned();
                }
            }
        }
    }

    let model_ready = matches!(
        &state.content,
        AssetPreviewContent::Model {
            phase: AssetPreviewPhase::Ready,
            ..
        }
    );
    // 作者预览按各自所在的场景页或插件工具页裁剪，中央媒体仍使用中央宿主。
    let viewport = if central { center_hosts.single().ok() } else { sidebar_pages.iter().find_map(|(page, node, transform)| {
        (page.0 == tool_page.map_or(crate::ui::sidebars::EditorSidebarTab::Right(EditorRightSidebarTab::Scene), crate::ui::sidebars::EditorSidebarTab::Left))
            .then_some((node, transform))
    }) }; // 中央与作者场景页使用自己的真实裁剪范围。
    let canvas_visible = (!preview_active && !central)
        || preview_canvas
            .single()
            .ok()
            .zip(viewport) // 使用实际宿主所在的裁剪区域。
            .is_some_and(|((canvas, at), (viewport, view))| {
                if canvas.size().min_element() <= 0.0 || viewport.size().min_element() <= 0.0 {
                    return false; // 收起/首帧零尺寸页不能被正高度画布误认为仍可见。
                }
                let delta = (at.translation.y - view.translation.y).abs();
                delta < (canvas.size().y + viewport.size().y) * 0.5 && canvas.size().y > 0.0
            }); // 滚出真正宿主裁剪区后关闭额外View。
    let render_active = editor.open
        && (central || layout.as_ref().is_none_or(|layout| layout.inspector_shown())) // 中央页不依赖右栏显隐，侧栏仍遵守原可见门。
        && (central || if preview_active { preview_page_visible } else { sidebar.right == EditorRightSidebarTab::Assets }) // 隐藏页不继续执行额外渲染。
        && model_ready
        && (!central || media.as_ref().is_none_or(|media| !media.restore_view)) // 原primitive与观察状态尚未恢复时不绘制中途Scene0。
        && canvas_visible; // 右栏作者预览与普通资产共用唯一视图，隐藏时停止渲染。
    if state.render_visible != render_active {
        state.render_visible = render_active;
    } // 展示动作读取同一可见门。
    for mut camera in &mut cameras {
        if camera.is_active != render_active {
            camera.is_active = render_active;
        }
    }
    for mut visibility in &mut lights {
        let wanted = if render_active {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
}

/// 每帧只轮询当前一个图片或Scene0句柄；加载失败不阻塞，文件热更后仍可恢复。
pub(super) fn poll_asset_preview_loading(
    asset_server: Res<AssetServer>,
    images: Res<Assets<Image>>,
    previews: Option<Res<Assets<CompatiblePreviewTexture>>>,
    animation: Option<Res<super::animation::AnimationEditorState>>,
    skeleton: Option<Res<super::animation::AnimationPreviewRuntime>>,
    gltfs: Option<Res<Assets<Gltf>>>,
    mut state: ResMut<AssetPreviewState>,
    mut model_roots: Query<&mut Visibility, With<AssetPreviewModelRoot>>,
) {
    enum PollResult {
        Waiting(String),
        Ready(String),
        Failed(String),
    }

    let previewing_scene = state.model.as_ref().is_some_and(|model| {
        model.target == AssetPreviewModelTarget::Scene0 // 单体模式由独立子资产状态机诊断，不拿Scene句柄覆盖它。
    });
    // 合法纯动画可没有 Scene0 或 Mesh。只有源文档成功、GLTF根真实就绪、完整骨架已经发布时，
    // 才让骨架接管这一种预览；带网格的依赖损坏仍沿原失败诊断，不能被动画草稿遮掩。
    let animation_only_ready = animation.as_ref().zip(skeleton.as_ref()).is_some_and(|(animation, skeleton)| {
        skeleton.document_ready(animation, &state)
            && state.model.as_ref().filter(|model| asset_server.is_loaded_with_dependencies(model.catalog.gltf.id()))
                .and_then(|model| gltfs.as_ref().and_then(|gltfs| gltfs.get(&model.catalog.gltf)))
                .is_some_and(|gltf| gltf.scenes.is_empty() || gltf.meshes.is_empty())
    });
    // Ready仍须复验；主图只采用私有label，原图热更不会把整数纹理直接塞进UI绑定。
    let image_source = match &state.content {
        AssetPreviewContent::Image { handle, phase } => Some((handle.clone(), *phase)),
        _ => None,
    };
    let result = if let Some((_source, _phase)) = image_source {
        if state.texture_preview.is_none() && let Some(path) = state.selected_path.clone() {
            state.texture_preview = Some(asset_server.load(path));
        }
        state.texture_preview.clone().map(|handle| match asset_server.load_state(handle.id()) {
            LoadState::Failed(error) => {
                let diagnostic = error.to_string();
                if retry_capacity(&asset_server, &handle, &diagnostic) {
                    PollResult::Waiting("正在等待兼容预览后台槽".to_owned())
                } else { PollResult::Failed(format!("图片兼容预览失败：{diagnostic}")) }
            }
            LoadState::Loaded => {
                if let Some(preview) = previews.as_ref().and_then(|assets| assets.get(&handle)) {
                    if let Some(failure) = &preview.failure {
                        PollResult::Failed(format!("{} · {failure}", preview.description))
                    } else if images.get(&preview.image).is_some_and(sample_compatible) {
                        let display = Some(preview.image.clone());
                        if state.display_image != display { state.display_image = display; }
                        if let AssetPreviewContent::Image { handle, .. } = &mut state.content
                            && *handle != preview.image { *handle = preview.image.clone(); }
                        PollResult::Ready(preview.description.clone())
                    } else {
                        PollResult::Waiting("图片已解码，正在采用安全预览".to_owned())
                    }
                } else { PollResult::Waiting("图片已解码，正在采用安全预览".to_owned()) }
            }
            LoadState::NotLoaded | LoadState::Loading => PollResult::Waiting("正在后台准备图片预览".to_owned()),
        })
    } else { match &state.content {
        AssetPreviewContent::Model {
            handle,
            root,
            phase,
        } if !matches!(
            phase,
            AssetPreviewPhase::Scanning | AssetPreviewPhase::RevealPending
        ) && previewing_scene =>
        {
            if animation_only_ready {
                None // 骨架已采用；不在每帧把同一个合法纯动作重新置为Scene0失败。
            } else if let Some(error) = state.model.as_ref()
                .and_then(|model| gltf_source_load_failure(&asset_server, &model.catalog.gltf))
                .or_else(|| scene_load_failure(&asset_server, handle)) {
                if let Ok(mut visibility) = model_roots.get_mut(*root) {
                    *visibility = Visibility::Hidden; // 热更失败也不留下与新文件不一致的旧画面。
                }
                Some(PollResult::Failed(error))
            } else if *phase == AssetPreviewPhase::Failed {
                Some(PollResult::Waiting(
                    "模型文件已更新，正在重新加载Scene0".to_owned(),
                ))
            } else {
                None // WorldInstanceReady是完整模型可用的唯一成功门。
            }
        }
        _ => None,
    }};

    let Some(result) = result else {
        return;
    };
    match result {
        PollResult::Waiting(status) => {
            if state.display_image.is_some() { state.display_image = None; }
            if matches!(&state.content, AssetPreviewContent::Image { phase: AssetPreviewPhase::Loading, .. }) && state.status == status { return; }
            match &mut state.content {
                AssetPreviewContent::Image { phase, .. }
                | AssetPreviewContent::Model { phase, .. } => *phase = AssetPreviewPhase::Loading,
                _ => return,
            }
            state.status = status;
        }
        PollResult::Ready(status) => {
            if matches!(&state.content, AssetPreviewContent::Image { phase: AssetPreviewPhase::Ready, .. }) && state.status == status { return; }
            if let AssetPreviewContent::Image { phase, .. } = &mut state.content {
                *phase = AssetPreviewPhase::Ready;
                state.status = status;
            }
        }
        PollResult::Failed(status) => {
            if state.display_image.is_some() { state.display_image = None; }
            let already_reported = matches!(
                &state.content,
                AssetPreviewContent::Image {
                    phase: AssetPreviewPhase::Failed,
                    ..
                } | AssetPreviewContent::Model {
                    phase: AssetPreviewPhase::Failed,
                    ..
                }
            ) && state.status == status;
            if already_reported {
                return; // 持续失败也是稳定状态，不每帧唤醒整块预览UI。
            }
            match &mut state.content {
                AssetPreviewContent::Image { phase, .. }
                | AssetPreviewContent::Model { phase, .. } => *phase = AssetPreviewPhase::Failed,
                _ => return,
            }
            state.bounds_scan = None;
            state.model_bounds = None;
            state.status = status;
        }
    }
}
