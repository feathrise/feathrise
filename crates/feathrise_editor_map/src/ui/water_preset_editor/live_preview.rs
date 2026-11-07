//! 静水预设“编辑所选”的独立 `10m × 50m` 样品河面实时预览。
//!
//! 【功能设计】预览拥有专用相机、RTT、网格和渲染层，只把当前合法草稿投影到一张固定水面；
//! 天空、IBL、环境光、雾和太阳/月亮复用当前编辑场景最终设置，但地图实体绝不进入样品世界。
//! 画布支持右键旋转、中键平移与滚轮缩放；微法线未完整加载时保留上一份完整画面。

use crate::ui::theme as colors;
use bevy::asset::LoadState; // 两层微法线必须完整Ready后才原子替换样品材质。
use bevy::prelude::*; // Resource、Component、Handle与BSN场景类型。
use bevy::text::FontSourceTemplate; // 右栏说明复用项目中文字体。
use bevy::ui::{FocusPolicy, RelativeCursorPosition}; // 只读画布阻止点击穿透并提供真实悬停位置。
use feathrise_map_content::{
    WATER_MICRO_NORMAL_COUNT, WaterPresetDescriptor, WaterPresetRegistry, load_water_micro_normal,
}; // 草稿描述、正式句柄缓存与唯一线性Repeat加载入口。
use feathrise_scene::{
    DynamicWaterMaterial, WaterSurfaceDescriptor, WaterSurfaceKind, advance_water_surface_material,
    create_water_surface_material,
}; // 样品继续使用正式透明PBR水Shader，不复制假材质。 // 右栏沿用F12暗墨铜边视觉层级。

use super::fields::commit_field; // 当前合法数值字符直接投影到临时外观而不提交草稿。
use super::types::{WaterDraftMode, WaterPresetEditorState}; // 活动编辑标签和字段输入缓冲。
use crate::resources::{MapEditorLayoutState, MapEditorState}; // 会话与侧栏显隐共同决定预览生命周期。
use crate::ui::scene::FONT_PATH; // 预览文字使用项目统一字体资产。
use crate::ui::sidebars::{EditorRightSidebarTab, MapEditorSidebarState}; // 首次进入标签只揭示一次右侧场景页。
use crate::ui::vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState}; // 共享中央标签身份。

mod environment; // 复用当前天空、IBL、环境光、雾和太阳/月亮，不复制地图实体。
mod navigation; // 画布右键轨道、中键平移和滚轮缩放只改独立相机。
mod scene; // 固定十米乘五十米河面、河床、RTT和相机只创建一次。

pub(super) use environment::{sync_live_preview_environment_request, sync_live_preview_lighting}; // 父插件分别在天空请求阶段和最终光照阶段注册。
pub(crate) use navigation::WaterPresetLivePreviewNavigation; // 统一鼠标门与侧栏滚动读取预览所有权。
pub(super) use navigation::{control_live_preview_camera, sync_live_preview_camera}; // 父插件按输入和任务阶段注册。
pub(super) use scene::setup_live_preview_scene; // UI壳出现后创建唯一独立样品世界。

const PREVIEW_WIDTH_METERS: f32 = 10.0; // 扩大后的样品河面使用十米真实横向宽度。
const PREVIEW_LENGTH_METERS: f32 = 50.0; // 扩大后的样品河面使用五十米真实纵向长度。
const PREVIEW_FLOW_DIRECTION: Vec2 = Vec2::Y; // Vec2的Y对应世界Z，沿五十米长轴观察流动。
const PREVIEW_FLOW_SPEED: f32 = 0.8; // 固定温和流速只帮助辨识纹理，不覆盖预设四组风浪速度。

/// 当前活动编辑标签派生出的完整、可原子采用的样品外观。
#[derive(Clone, Debug, PartialEq)]
struct WaterPresetLivePreviewSnapshot {
    target_preset_id: String, // 切换到另一编辑标签即使视觉相同也形成明确身份边沿。
    descriptor: WaterPresetDescriptor, // 已去除纯元数据差异的完整视觉参数。
    image_handles: [Option<Handle<Image>>; WATER_MICRO_NORMAL_COUNT], // 两张微法线强句柄与参数同代持有。
}

/// 当前样品预览的最新完整期望值；不再参与地图级水网修订。
#[derive(Resource, Default)]
pub(super) struct WaterPresetLivePreview {
    revision: u64, // 完整视觉快照真实变化时递增，稳定帧不重复准备材质。
    snapshot: Option<WaterPresetLivePreviewSnapshot>, // None表示当前没有活动“编辑所选”标签。
}

impl WaterPresetLivePreview {
    /// 只有完整快照真实变化时推进修订，连续稳定帧保持零工作唤醒。
    fn replace(&mut self, snapshot: Option<WaterPresetLivePreviewSnapshot>) {
        if self.snapshot == snapshot {
            return;
        }
        self.snapshot = snapshot;
        self.revision = self.revision.wrapping_add(1).max(1);
    }
}

/// 独立样品场景的固定资源与采用身份。
#[derive(Resource, Default)]
pub(super) struct WaterPresetLivePreviewRuntime {
    _image: Option<Handle<Image>>, // RTT强句柄与相机目标共同保持离屏纹理存活。
    camera: Option<Entity>,        // UI壳若重建可重新挂载既有唯一相机，不生成重复场景。
    material: Option<Handle<DynamicWaterMaterial>>, // 稳定句柄原位替换，连续输入不会泄漏材质资产。
    active_tab: Option<u64>,       // 只在进入新的编辑标签边沿自动切一次右侧场景页。
    applied_revision: u64,         // 当前画面已经原子采用的完整样品修订。
    failed_revision: u64,          // 最近明确失败身份仅用于状态诊断，不阻止资源热恢复。
}

/// 右侧“场景”页底部样品预览的显隐父根。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct WaterPresetLivePreviewPanel;

/// 独立RTT的只读UI挂载点。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct WaterPresetLivePreviewViewport;

/// 专用样品相机；绝不与地图视口交换所有权。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct WaterPresetLivePreviewCamera;

/// 河床和水面共同使用的显隐标记；当前场景方向光只临时增加样品层。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct WaterPresetLivePreviewRenderable;

/// 水面需要在首份完整材质采用后才显示，避免默认白材质闪现。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct WaterPresetLivePreviewSurface;

/// 加载、采用与失败状态的固定文本节点。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct WaterPresetLivePreviewStatus;

/// 构造右侧场景页底部的独立样品预览壳。
pub(crate) fn water_live_preview_host() -> impl Scene {
    bsn! {
        WaterPresetLivePreviewPanel
        Node {
            display: Display::None, // 只有活动“编辑所选”标签占用右栏高度。
            width: percent(100),
            flex_direction: FlexDirection::Column,
            row_gap: px(4),
            flex_shrink: 0.0,
            padding: UiRect::all(px(4)),
            margin: UiRect::top(px(5)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(3)),
        }
        BackgroundColor(colors::PANEL_BG_SECTION)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            (crate::ui::context_help::section_header("水体实时预览 · 10m × 50m", "右键旋转 · 中键平移 · 滚轮缩放；复用当前天空与光照，世界内容仅河面和河床。")),
            (
                WaterPresetLivePreviewViewport
                RelativeCursorPosition::default() // 尊重祖先裁剪，读取画布内真实鼠标位置。
                Interaction::default() // 被弹框或其他前景UI遮挡时不得抢导航输入。
                template_value(FocusPolicy::Block) // 画布只读且不得把点击穿透给场景树。
                Node {
                    width: percent(100),
                    height: px(190),
                    min_height: px(170),
                    flex_shrink: 0.0,
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(2)),
                    overflow: Overflow::clip(),
                }
                BackgroundColor(colors::MODEL_PREVIEW_BG)
                BorderColor::all(colors::COPPER_BORDER)
            ),
            (
                WaterPresetLivePreviewStatus crate::ui::status_bar::PanelStatusText("水体实时预览") feathrise_editor_ui::feedback::GlobalPanelStatus
                Text("等待水体预设编辑标签") feathrise_editor_ui::localization::label_scene("等待水体预设编辑标签")
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(8.8) }
                TextColor(colors::TEXT_VALUE)
            ),
        ]
    }
}

/// 去除不影响GPU水面的身份和显示名，使文字编辑不会替换材质。
fn visual_descriptor(mut descriptor: WaterPresetDescriptor) -> WaterPresetDescriptor {
    descriptor.id.clear();
    descriptor.aliases.clear();
    descriptor.label.clear();
    descriptor
}

/// 从活动编辑标签投影当前可用草稿；合法的未提交数值只进入预览副本。
fn active_edit_candidate(
    state: &WaterPresetEditorState,
    active_tab: PresetWorkspaceTab,
) -> Option<(String, WaterPresetDescriptor)> {
    let draft = state.active_draft(active_tab)?;
    let WaterDraftMode::Edit { original_id } = &draft.mode else {
        return None; // 新建预设没有既有“编辑所选”身份，本次需求不显示样品宿主。
    };
    let mut descriptor = draft.descriptor.clone();
    if let Some(field) = state.active_field.filter(|field| !field.accepts_text()) {
        let _ = commit_field(&mut descriptor, field, state.input_buffer.trim());
        // 空串、半个指数或越界值失败时保留草稿中上一完整值。
    }
    Some((original_id.clone(), descriptor))
}

/// 为草稿两层微法线取得强句柄；相同路径优先复用上一样品或正式目录缓存。
fn preview_image_handles(
    asset_server: &AssetServer,
    target_preset_id: &str,
    descriptor: &WaterPresetDescriptor,
    formal: Option<&WaterPresetDescriptor>,
    formal_handles: Option<&[Option<Handle<Image>>; WATER_MICRO_NORMAL_COUNT]>,
    previous: Option<&WaterPresetLivePreviewSnapshot>,
) -> [Option<Handle<Image>>; WATER_MICRO_NORMAL_COUNT] {
    std::array::from_fn(|index| {
        let path = descriptor.micro_normals[index].texture_path.as_ref()?;
        if let Some(previous) = previous.filter(|value| {
            value.target_preset_id == target_preset_id
                && value.descriptor.micro_normals[index].texture_path.as_ref() == Some(path)
        }) {
            return previous.image_handles[index].clone();
        }
        if formal
            .is_some_and(|value| value.micro_normals[index].texture_path.as_ref() == Some(path))
            && let Some(handle) = formal_handles.and_then(|handles| handles[index].clone())
        {
            return Some(handle);
        }
        Some(load_water_micro_normal(asset_server, path.clone()))
    })
}

/// 在本帧全部输入之后发布最新完整样品快照；地图水网不读取此资源。
pub(super) fn sync_live_preview_draft(
    editor: Res<MapEditorState>,
    state: Res<WaterPresetEditorState>,
    workspace: Res<VegetationPresetEditorState>,
    registry: Option<Res<WaterPresetRegistry>>,
    asset_server: Res<AssetServer>,
    mut preview: ResMut<WaterPresetLivePreview>,
) {
    let next = if editor.open && !editor.close_requested && editor.pending_map_transition.is_none()
    {
        active_edit_candidate(&state, workspace.active_tab).map(|(original_id, descriptor)| {
            let formal = registry
                .as_deref()
                .and_then(|registry| registry.resolve(&original_id));
            let target_preset_id = formal.map_or(original_id.as_str(), |value| value.id.as_str());
            let descriptor = visual_descriptor(descriptor);
            let image_handles = preview_image_handles(
                &asset_server,
                target_preset_id,
                &descriptor,
                formal,
                registry
                    .as_deref()
                    .and_then(|registry| registry.image_handles(target_preset_id)),
                preview.snapshot.as_ref(),
            );
            WaterPresetLivePreviewSnapshot {
                target_preset_id: target_preset_id.to_owned(),
                descriptor,
                image_handles,
            }
        })
    } else {
        None
    };
    preview.replace(next);
}

/// 把预设视觉参数转换成固定样品水面描述，不赋予地图身份或互动能力。
fn sample_surface_descriptor(snapshot: &WaterPresetLivePreviewSnapshot) -> WaterSurfaceDescriptor {
    WaterSurfaceDescriptor {
        id: "water_preset_sample".to_owned(),
        kind: WaterSurfaceKind::Still,
        base_color_srgba: snapshot.descriptor.base_color_srgba,
        perceptual_roughness: snapshot.descriptor.perceptual_roughness,
        reflectance: snapshot.descriptor.reflectance,
        trough_darkening: snapshot.descriptor.trough_darkening,
        shore_wave_attenuation: snapshot.descriptor.shore_wave_attenuation,
        macro_waves: snapshot.descriptor.macro_waves,
        micro_normals: snapshot.descriptor.micro_normals.clone(),
        micro_normal_textures: snapshot.image_handles.clone(),
        flow_direction: PREVIEW_FLOW_DIRECTION,
        flow_speed: PREVIEW_FLOW_SPEED,
        translation_y: 0.0,
        rest_water_level: 0.0,
        interaction_enabled: false,
    }
}

/// 只在文字真实变化时更新固定状态节点。
fn set_status(texts: &mut Query<&mut Text, With<WaterPresetLivePreviewStatus>>, next: &str) {
    let Ok(mut text) = texts.single_mut() else {
        return;
    };
    if text.0 != next {
        text.0 = next.to_owned();
    }
}

/// 完整微法线Ready后原位替换稳定材质句柄，并在画布可见时推进正式水Shader时间。
pub(super) fn sync_live_preview_material(
    time: Res<Time>,
    asset_server: Res<AssetServer>,
    sidebar: Res<MapEditorSidebarState>,
    layout: Option<Res<MapEditorLayoutState>>,
    preview: Res<WaterPresetLivePreview>,
    mut runtime: ResMut<WaterPresetLivePreviewRuntime>,
    mut materials: ResMut<Assets<DynamicWaterMaterial>>,
    mut status_texts: Query<&mut Text, With<WaterPresetLivePreviewStatus>>,
) {
    let Some(snapshot) = preview.snapshot.as_ref() else {
        set_status(&mut status_texts, "等待水体预设编辑标签");
        return;
    };
    let Some(material_handle) = runtime.material.clone() else {
        set_status(&mut status_texts, "正在初始化独立样品场景…");
        return;
    };

    if runtime.applied_revision != preview.revision {
        let failed = snapshot
            .image_handles
            .iter()
            .flatten()
            .any(|handle| matches!(asset_server.load_state(handle.id()), LoadState::Failed(_)));
        if failed {
            runtime.failed_revision = preview.revision;
            set_status(
                &mut status_texts,
                "微法线加载失败，继续保留上一份完整样品画面",
            );
            return;
        }
        let loading = snapshot
            .image_handles
            .iter()
            .flatten()
            .any(|handle| !asset_server.is_loaded_with_dependencies(handle.id()));
        if loading {
            set_status(
                &mut status_texts,
                "正在加载微法线，继续保留上一份完整样品画面…",
            );
            return;
        }

        let descriptor = sample_surface_descriptor(snapshot);
        materials
            .insert(
                material_handle.id(),
                create_water_surface_material(&descriptor, time.elapsed_secs()),
            )
            .expect("F12样品水材质使用本模块创建的有效资产句柄");
        runtime.applied_revision = preview.revision;
        runtime.failed_revision = 0;
        set_status(&mut status_texts, "样品已同步 · 合法数值输入实时生效");
    }

    if runtime.applied_revision == preview.revision
        && sidebar.right == EditorRightSidebarTab::Scene
        && layout
            .as_ref()
            .is_none_or(|layout| layout.inspector_shown())
        && let Some(mut material) = materials.get_mut(&material_handle)
    {
        advance_water_surface_material(
            &mut material,
            time.elapsed_secs(),
            PREVIEW_FLOW_DIRECTION,
            PREVIEW_FLOW_SPEED,
        );
    }
}

/// 返回当前真正活动的“编辑所选”水体标签ID。
fn active_edit_tab_id(
    state: &WaterPresetEditorState,
    active_tab: PresetWorkspaceTab,
) -> Option<u64> {
    let PresetWorkspaceTab::Water(tab_id) = active_tab else {
        return None;
    };
    matches!(
        state.active_draft(active_tab).map(|draft| &draft.mode),
        Some(WaterDraftMode::Edit { .. })
    )
    .then_some(tab_id)
}

/// 显示右栏宿主并启停专用相机；进入新标签时只自动切一次“场景”页。
pub(super) fn sync_live_preview_visibility(
    editor: Res<MapEditorState>,
    layout: Option<Res<MapEditorLayoutState>>,
    state: Res<WaterPresetEditorState>,
    workspace: Res<VegetationPresetEditorState>,
    mut sidebar: ResMut<MapEditorSidebarState>,
    mut runtime: ResMut<WaterPresetLivePreviewRuntime>,
    mut panels: Query<&mut Node, With<WaterPresetLivePreviewPanel>>,
    mut cameras: Query<&mut Camera, With<WaterPresetLivePreviewCamera>>,
    mut renderables: Query<
        (&mut Visibility, Option<&WaterPresetLivePreviewSurface>),
        With<WaterPresetLivePreviewRenderable>,
    >,
) {
    let active_tab =
        (editor.open && !editor.close_requested && editor.pending_map_transition.is_none())
            .then(|| active_edit_tab_id(&state, workspace.active_tab))
            .flatten();

    if runtime.active_tab != active_tab {
        runtime.active_tab = active_tab;
        if active_tab.is_some() {
            sidebar.reveal_scene_properties();
        }
    }

    let panel_display = if active_tab.is_some() {
        Display::Flex
    } else {
        Display::None
    };
    for mut panel in &mut panels {
        if panel.display != panel_display {
            panel.display = panel_display;
        }
    }

    // 收起检查器或进入专注模式时不渲染隐藏样品，活动标签仍保留以免重开时抢回页签。
    let render = active_tab.is_some()
        && sidebar.right == EditorRightSidebarTab::Scene
        && layout
            .as_ref()
            .is_none_or(|layout| layout.inspector_shown());
    for mut camera in &mut cameras {
        if camera.is_active != render {
            camera.is_active = render;
        }
    }
    for (mut visibility, surface) in &mut renderables {
        let visible = render && (surface.is_none() || runtime.applied_revision > 0);
        let next = if visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != next {
            *visibility = next;
        }
    }
}

#[cfg(test)]
mod tests; // 数值投影、网格范围、导航和灯光层回归独立维护。
