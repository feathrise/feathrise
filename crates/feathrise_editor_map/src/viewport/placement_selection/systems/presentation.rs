//! 当前Scene0网格精确覆盖、身份徽标、三维范围框与双击相机定位。

use bevy::camera::primitives::Aabb; // glTF primitive局部包围盒用于联合世界范围。
use bevy::camera::visibility::{NoFrustumCulling, RenderLayers}; // 精确覆盖只进入F12辅助渲染层。
use bevy::gltf::GltfMeshName; // 覆盖源与徽标Visibility查询使用稳定网格marker隔离。
use bevy::light::{NotShadowCaster, NotShadowReceiver}; // 作者辅助层不参与世界光照副作用。
use bevy::picking::Pickable; // 覆盖层不得抢占世界或UI选择。
use bevy::prelude::*; // ECS、Mesh3d、Transform、UI与Gizmo类型。
use feathrise_scene_core::view::CameraRaycastIgnore; // 相机防穿墙忽略作者覆盖。
use feathrise_scene::{MapPlacementRuntimePhase, MapPlacementRuntimeStatus}; // 正式批次采用状态。
use crate::ui::theme as colors; // 铜金三维范围框沿用F12正式视觉语言。

use super::super::ui::sync_badge; // UI固定节点由独立模块增量同步。
use super::super::{
    PlacementMeshResolution, PlacementSelectionFrame, PlacementWorldBounds,
    SelectedPlacementMeshIdentity, SelectedPlacementViewportState, placement_focus_extent,
    selected_placement_mesh_info, transformed_placement_bounds,
}; // 完整解析结果、稳定身份和纯数学由功能域根统一定义。
use crate::authoring::EditorAuthoringSession; // 当前草稿决定选择所属地图。
use crate::components::{
    MapEditorOwnedEntity, MapEditorSelectedPlacementBadge, MapEditorSelectedPlacementBadgeText,
    MapEditorSelectedPlacementHighlight,
}; // 世界覆盖与中央徽标使用互斥marker。
use crate::resources::{MapEditorState, MapEditorViewportState}; // 双击只调整现有编辑相机会话态。
use crate::types::EditorTool; // 候选悬停仅在选择模式影响辅助表现。
use crate::ui::gltf_browser::EditorGltfBrowserState; // 读取右栏公开稳定网格选择。
use crate::viewport::brush_cursor::MAP_EDITOR_BRUSH_RENDER_LAYER; // 精确填充继续使用F12专属渲染层。
use feathrise_author_tools::{AuthorTool, SelectionPreview};
use crate::viewport::tool_preview::AuthorToolFrame;
use crate::viewport::content_selection::LayerCandidateState; // 预览身份来自统一候选，不回写Outliner。

const HIGHLIGHT_DEPTH_BIAS: f32 = 20_100.0; // 排在正式透明材质之后，稳定显示淡青覆盖。

/// 隐藏全部已创建覆盖槽；实体和唯一材质保留供后续选择复用。
fn hide_highlights(
    state: &SelectedPlacementViewportState,
    highlights: &mut Query<
        (&mut Mesh3d, &mut Transform, &mut Visibility, &ChildOf),
        (
            With<MapEditorSelectedPlacementHighlight>,
            Without<MapEditorSelectedPlacementBadge>,
            Without<GltfMeshName>,
        ),
    >,
) {
    for &entity in &state.highlight_entities {
        let Ok((_, _, mut visibility, _)) = highlights.get_mut(entity) else {
            continue; // 父World换代已递归删除的覆盖槽留待下次有效选择替换。
        };
        if *visibility != Visibility::Hidden {
            *visibility = Visibility::Hidden; // 只在真实变化时通知可见性系统。
        }
    }
}

/// 当前正式批次是否已经结束构建；作者预览不依赖全图正式采用状态。
fn placement_generation_settled(
    identity: &SelectedPlacementMeshIdentity,
    author_preview: bool,
    runtime: &MapPlacementRuntimeStatus,
) -> bool {
    author_preview
        || runtime.map_id.as_ref() != Some(&identity.map_id)
        || matches!(
            runtime.phase,
            MapPlacementRuntimePhase::Ready | MapPlacementRuntimePhase::Failed
        ) // 没有当前地图状态时，唯一活动根本身就是可采用的完整空间证据。
}

/// 生成紧凑但可精确核对的实例、对象路径与网格名身份前缀。
fn placement_badge_prefix(identity: &SelectedPlacementMeshIdentity, previewing: bool) -> String {
    if identity.whole_instance {
        return format!(
            "{}资产：{} · 整实例",
            if previewing { "预览" } else { "已选" },
            identity.placement_id
        ); // 不能把整实例冒充空路径网格。
    }
    let path = if identity.node_path.is_empty() {
        "[空路径]".to_owned() // 无效行也必须可见诊断，不能静默隐藏身份缺口。
    } else {
        identity.node_path.join("/")
    };
    format!(
        "{}场景：{} / {} / {}",
        if previewing { "预览" } else { "已选" },
        identity.placement_id,
        path,
        identity.mesh_name // 预览不冒充正式选择。
    )
}

/// 同步全部精确primitive覆盖、中央徽标，并在双击请求就绪后调整现有编辑相机。
#[allow(clippy::too_many_arguments)]
pub(crate) fn sync_selected_placement_viewport(
    mut commands: Commands,
    mut editor: ResMut<MapEditorState>,
    browser: Res<EditorGltfBrowserState>,
    candidates: Res<LayerCandidateState>, // 精确覆盖与解析阶段使用同一个候选身份。
    session: Res<EditorAuthoringSession>,
    runtime: Res<MapPlacementRuntimeStatus>,
    mut selection: ResMut<SelectedPlacementViewportState>,
    viewport: Option<ResMut<MapEditorViewportState>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    sources: Query<
        (
            Option<&GltfMeshName>, // 独立Primitive没有Scene0名称。
            &Mesh3d,
            &Transform,
            &GlobalTransform,
            &ChildOf,
            Option<&Aabb>,
            Option<&InheritedVisibility>,
        ),
        Without<MapEditorSelectedPlacementHighlight>,
    >,
    mut highlights: Query<
        (&mut Mesh3d, &mut Transform, &mut Visibility, &ChildOf),
        (
            With<MapEditorSelectedPlacementHighlight>,
            Without<MapEditorSelectedPlacementBadge>,
            Without<GltfMeshName>,
        ),
    >,
    mut badges: Query<
        &mut Visibility,
        (
            With<MapEditorSelectedPlacementBadge>,
            Without<MapEditorSelectedPlacementHighlight>,
            Without<GltfMeshName>,
        ),
    >, // 世界覆盖和UI徽标以互斥marker与GltfMeshName过滤证明Visibility访问不冲突。
    mut badge_texts: Query<&mut Text, With<MapEditorSelectedPlacementBadgeText>>,
) {
    let previewing =
        editor.open && editor.tool == EditorTool::Inspect && candidates.hover.is_some() && !selection.explicit_focus_pending(); // 双击仍在悬停时暂停，F取景使用正式选择。
    let Some(info) = selected_placement_mesh_info(&editor, &browser, &session, &candidates, selection.explicit_focus_pending()) else {
        hide_highlights(&selection, &mut highlights); // 立即隐藏世界选择表现但保留可复用实体池。
        sync_badge(false, None, &mut selection, &mut badges, &mut badge_texts); // 中央徽标同步隐藏。
        selection.frame = None; // Gizmo下一帧不绘制旧范围。
        if !previewing {
            // 跨类型候选悬停保留原正式定位请求，退出后继续验证身份。
            selection.focus_request = None; // 离开选择上下文后不得迟到移动相机。
        }
        return;
    };

    let mut source_valid = selection.resolved_identity.as_ref() == Some(&info.identity)
        && selection.resolution == PlacementMeshResolution::Ready
        && !selection.source_entities.is_empty(); // 只有完整DFS原子发布的非空结果可支撑覆盖。
    let mut all_sources_visible = source_valid; // 任一primitive被父层隐藏时整组失败关闭，不显示半套覆盖。
    let mut all_sources_have_aabb = source_valid; // 缺任一Aabb时仍可覆盖，但不伪造联合定位范围。
    let mut world_bounds: Option<PlacementWorldBounds> = None; // 第一项有效Aabb播种，后续固定最多64项联合。
    if source_valid {
        for &entity in &selection.source_entities {
            let Ok((name, _, _, global, _, aabb, inherited_visibility)) = sources.get(entity)
            else {
                source_valid = false; // 当前primitive缺少完整渲染组件时不发布部分覆盖。
                break;
            };
            if !info.identity.whole_instance
                && name.is_none_or(|name| name.0 != info.identity.mesh_name)
            {
                source_valid = false; // 实体复用或异常改名不能继承旧选择信用。
                break;
            }
            if inherited_visibility.is_some_and(|visibility| !visibility.get()) {
                if info.identity.whole_instance {
                    continue; // 整实例只联合当前可见部件，隐藏规划节点或LOD不遮蔽其他网格。
                }
                all_sources_visible = false; // 精确网格保留原完整可见门。
            }
            if let Some(aabb) = aabb {
                let bounds = transformed_placement_bounds(global, aabb); // 固定八角保留父级非均匀缩放和剪切。
                world_bounds = Some(world_bounds.map_or(bounds, |current| current.merge(bounds)));
            } else {
                all_sources_have_aabb = false; // 不读取Mesh顶点同步补算，明确暂不可定位。
            }
        }
    }
    let frame = (source_valid && all_sources_visible && all_sources_have_aabb)
        .then(|| world_bounds.map(|bounds| bounds.frame()))
        .flatten(); // 覆盖完整、当前可见且所有Aabb齐备才发布取景范围。
    if selection.frame != frame {
        selection.frame = frame; // Gizmo和双击相机读取同一完整联合值。
    }

    if source_valid && all_sources_visible {
        let material = selection.highlight_material.get_or_insert_with(|| {
            materials.add(StandardMaterial {
                base_color: Color::srgba(0.30, 0.82, 0.84, 0.24), // 淡青透明填充保留原材质和场景上下文。
                alpha_mode: AlphaMode::Blend, // 不用不透明色块遮挡树叶、岩石等结构。
                unlit: true,                  // 夜晚和阴影中仍稳定可辨识当前对象。
                double_sided: true,           // 前、侧、顶和透视都能看到覆盖层。
                cull_mode: None,              // 与双面作者辅助材质语义一致。
                depth_bias: HIGHLIGHT_DEPTH_BIAS, // 与源Mesh重合时稳定叠加。
                ..default()
            })
        });
        let material = material.clone(); // 结束资源字段可变借用后再更新实体池。
        let source_count = selection.source_entities.len();
        for index in 0..source_count {
            let source_entity = selection.source_entities[index]; // Copy后结束对源数组的短借用。
            let Ok((_, source_mesh, source_transform, _, parent, _, inherited_visibility)) =
                sources.get(source_entity)
            else {
                continue; // 上方已经全量验证；仅防御同系统内不可变Query异常。
            };
            if inherited_visibility.is_some_and(|visibility| !visibility.get()) {
                if let Some(&entity) = selection.highlight_entities.get(index)
                    && let Ok((_, _, mut visibility, _)) = highlights.get_mut(entity)
                {
                    *visibility = Visibility::Hidden; // 隐藏源不能借同父级辅助覆盖重新出现。
                }
                if index >= selection.highlight_entities.len() {
                    selection.highlight_entities.push(Entity::PLACEHOLDER); // 保持源与覆盖槽位一一对应。
                }
                continue; // 其他可见部件继续正常覆盖。
            }
            let source_mesh = source_mesh.clone();
            let source_transform = *source_transform;
            let source_parent = parent.parent();
            let existing = selection.highlight_entities.get(index).copied();
            let mut reused = false;
            if let Some(entity) = existing
                && let Ok((mut mesh, mut transform, mut visibility, overlay_parent)) =
                    highlights.get_mut(entity)
            {
                if mesh.0 != source_mesh.0 {
                    *mesh = source_mesh.clone(); // 只替换强类型Mesh句柄，不复制顶点。
                }
                if *transform != source_transform {
                    *transform = source_transform; // 与源primitive共享同一局部变换。
                }
                if overlay_parent.parent() != source_parent {
                    commands.entity(entity).insert(ChildOf(source_parent)); // 重绑到新对象父级以继承完整层级仿射。
                    *visibility = Visibility::Hidden; // Commands关系下一帧可见前不显示错误空间位置。
                } else if *visibility != Visibility::Inherited {
                    *visibility = Visibility::Inherited; // 有效完整选择恢复当前覆盖槽。
                }
                reused = true;
            }
            if reused {
                continue; // 当前槽已经原位更新，不创建第二个覆盖实体。
            }
            let entity = commands
                .spawn((
                    Name::new("地图编辑器选中Scene0网格精确覆盖"), // 调试层级明确不是正式场景内容。
                    MapEditorSelectedPlacementHighlight, // 实体池与解析DFS使用稳定辅助marker。
                    MapEditorOwnedEntity,                // 编辑器资源审计和关闭清理可识别所有权。
                    source_mesh, // 共享RenderWorld Mesh资产，不新建网格或扫描顶点。
                    MeshMaterial3d(material.clone()), // 全部场景网格覆盖共用唯一淡青材质。
                    source_transform, // 作为同父级兄弟精确继承源primitive局部变换。
                    Visibility::Inherited, // 首次有效选择立即显示。
                    RenderLayers::layer(MAP_EDITOR_BRUSH_RENDER_LAYER), // 玩法相机不可见。
                    NoFrustumCulling, // 共享源Mesh且父层变化时不等待辅助Aabb传播。
                    CameraRaycastIgnore, // 不影响玩法相机防穿墙。
                    NotShadowCaster, // 作者覆盖层不投影。
                    NotShadowReceiver, // 作者覆盖层不被世界阴影压暗。
                    Pickable::IGNORE, // 不参与世界Picking。
                    ChildOf(source_parent), // 与源primitive同父级，保留父缩放、旋转和潜在剪切。
                ))
                .id();
            if index < selection.highlight_entities.len() {
                selection.highlight_entities[index] = entity; // 替换已随World父级销毁的旧槽。
            } else {
                selection.highlight_entities.push(entity); // 首次扩容最多达到64个固定上限。
            }
        }
        for index in source_count..selection.highlight_entities.len() {
            let entity = selection.highlight_entities[index];
            if let Ok((_, _, mut visibility, _)) = highlights.get_mut(entity)
                && *visibility != Visibility::Hidden
            {
                *visibility = Visibility::Hidden; // 较少primitive的新选择隐藏多余复用槽。
            }
        }
    } else {
        hide_highlights(&selection, &mut highlights); // 等待、失败、不可见或残缺结果不显示半套覆盖。
    }

    let prefix = placement_badge_prefix(&info.identity, previewing); // 明确区分临时预览与正式选择。
    let badge_text = if !info.valid {
        format!("{prefix} · 引用无效，无法高亮") // 浏览器歧义、缺名或截断必须明确失败。
    } else if info.orphaned {
        format!("{prefix} · GLB中已不存在，仅可清除旧绑定") // 失效水绑定不冒充可见网格。
    } else {
        match &selection.resolution {
            PlacementMeshResolution::WaitingForRoot => {
                format!("{prefix} · 等待当前活动实例")
            }
            PlacementMeshResolution::WaitingForWorld => {
                format!("{prefix} · World实例加载中")
            }
            PlacementMeshResolution::Resolving => {
                format!("{prefix} · 正在解析精确网格")
            }
            PlacementMeshResolution::Missing => {
                format!("{prefix} · 当前完整实例中未找到")
            }
            PlacementMeshResolution::Invalid => {
                format!("{prefix} · 引用无效，无法高亮")
            }
            PlacementMeshResolution::Failed(detail) => {
                format!("{prefix} · 解析失败：{detail}")
            }
            PlacementMeshResolution::Ready if !source_valid => {
                format!("{prefix} · primitive渲染组件不完整")
            }
            PlacementMeshResolution::Ready if !all_sources_visible => {
                format!("{prefix} · 当前活动实例网格不可见")
            }
            PlacementMeshResolution::Ready if frame.is_none() => {
                format!("{prefix} · 已精确高亮，缺少Aabb暂不可定位")
            }
            PlacementMeshResolution::Ready if info.identity.whole_instance => {
                format!("{prefix} · 已高亮") // 统一候选单击不定位，不提示不存在的双击整实例操作。
            }
            PlacementMeshResolution::Ready if selection.source_is_author_preview => {
                format!("{prefix} · 已精确高亮作者预览 · 双击列表定位")
            }
            PlacementMeshResolution::Ready
                if runtime.map_id.as_ref() == Some(&info.identity.map_id)
                    && runtime.phase == MapPlacementRuntimePhase::Failed =>
            {
                format!("{prefix} · 当前更新失败，正高亮上一完整版本 · 双击定位")
            }
            PlacementMeshResolution::Ready
                if runtime.map_id.as_ref() == Some(&info.identity.map_id)
                    && matches!(
                        runtime.phase,
                        MapPlacementRuntimePhase::Waiting | MapPlacementRuntimePhase::Building
                    ) =>
            {
                format!("{prefix} · 摆放更新中，正高亮上一完整版本")
            }
            PlacementMeshResolution::Ready => {
                format!("{prefix} · 已精确高亮 · 双击列表定位")
            }
        }
    };
    sync_badge(
        true,             // 场景树存在选择时始终显示身份和明确状态。
        Some(badge_text), // 文案由稳定身份、解析阶段和正式批次状态共同决定。
        &mut selection,
        &mut badges,
        &mut badge_texts,
    );

    if previewing || selection.focus_request.as_ref() != Some(&info.identity) {
        // 悬停期间即使身份相同也不消费双击定位请求。
        return; // 普通单击只高亮和显示身份，绝不自动移动作者相机。
    }
    if !placement_generation_settled(&info.identity, selection.source_is_author_preview, &runtime) {
        return; // 双击发生在正式批次构建中时等待新活动根，不定位旧版本。
    }
    match (selection.resolution.clone(), frame, viewport) {
        (PlacementMeshResolution::Ready, Some(frame), Some(mut viewport)) => {
            viewport.center = frame.center; // 保留当前透视、顶、前或侧视图，只移动观察中心。
            viewport.extent = placement_focus_extent(frame).max(crate::viewport::camera_math::framing_extent(
                frame.size * 0.5, viewport.yaw, viewport.pitch, viewport.render_size, editor.orthographic)); // 窄视口也容纳完整范围。
            selection.focus_request = None; // 成功采用后不在后续帧重复抢回用户相机。
            editor.status = if selection.source_is_author_preview {
                format!("已定位场景网格作者预览：{}", info.identity.mesh_name)
            } else if runtime.map_id.as_ref() == Some(&info.identity.map_id)
                && runtime.phase == MapPlacementRuntimePhase::Failed
            {
                format!(
                    "已定位场景网格上一完整版本：{} · 当前摆放更新失败",
                    info.identity.mesh_name
                )
            } else {
                format!("已定位场景网格：{}", info.identity.mesh_name)
            };
        }
        (PlacementMeshResolution::Ready, None, _) => {
            selection.focus_request = None; // 终局缺范围时结束请求，避免永久等待。
            editor.status = format!(
                "场景网格{}当前不可见或缺少Aabb，暂不能定位",
                info.identity.mesh_name
            );
        }
        (PlacementMeshResolution::Missing, _, _) => {
            selection.focus_request = None;
            editor.status = format!(
                "当前完整实例中未找到场景网格{}，无法定位",
                info.identity.mesh_name
            );
        }
        (PlacementMeshResolution::Invalid, _, _) => {
            selection.focus_request = None;
            editor.status = "当前场景网格引用无效，无法定位".to_owned();
        }
        (PlacementMeshResolution::Failed(detail), _, _) => {
            selection.focus_request = None;
            editor.status = format!("场景网格解析失败，无法定位：{detail}");
        }
        _ => {} // 根、World或固定预算解析仍在推进时保留本次定位请求。
    }
}

/// 在精确淡青填充外围绘制当前网格的铜金三维世界包围框。
pub(crate) fn draw_selected_placement_highlight_frame(
    editor: Res<MapEditorState>,
    selection: Res<SelectedPlacementViewportState>,
    mut tools: ResMut<AuthorToolFrame>,
) {
    if !editor.open {
        return; // 关闭F12时不提交任何作者辅助线。
    }
    let Some(PlacementSelectionFrame { center, size }) = selection.frame else {
        return; // 等待、失败、不可见或缺Aabb时不伪造三维范围。
    };
    let Some(identity) = &selection.resolved_identity else { return; };
    let half = size.max(Vec3::splat(0.1)) * 0.5;
    // 范围复用原有完整解析缓存；只交换稳定身份和三个半轴，由所在视口生成完整框线。
    tools.push(AuthorTool::Selection(SelectionPreview {
        target: format!("placement:{}/{}/{}/{}", identity.map_id.as_str(), identity.placement_id,
            identity.node_path.join("/"), identity.mesh_name),
        center: center.to_array(),
        half_axes: [(Vec3::X * half.x).to_array(), (Vec3::Y * half.y).to_array(), (Vec3::Z * half.z).to_array()],
        color: colors::TEXT_ACCENT.to_linear().to_f32_array(),
    }));
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::{IntoSystem, System}; // 初始化生产系统即可触发Bevy查询访问冲突校验。

    use super::*; // 回归直接锁定完整系统参数组合。

    #[test] // primitive源、世界覆盖和UI徽标必须保持Visibility访问互斥。
    fn viewport_sync_system_initializes_without_visibility_conflicts() {
        let mut world = World::new(); // 最小World足以初始化全部SystemParam查询状态。
        let mut system = IntoSystem::into_system(sync_selected_placement_viewport); // 使用真实生产系统锁定过滤组合。
        system.initialize(&mut world); // 若互斥过滤退化，Bevy会在本行以B0001终止测试。
    }
}
