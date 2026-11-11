//! 当前静水、河流或瀑布的有限实体解析、精确Mesh覆盖、铜金范围框与双击相机定位。

use crate::ui::theme as colors;
use bevy::camera::primitives::Aabb; // 水面实体直接携带后台完成的局部包围盒。
use bevy::prelude::*; // ECS查询、PBR材质、Gizmo和三维数学类型。
use feathrise_scene::{
    ClipmapWaterMaterial, ClipmapWaterSurface, PaintedWaterPurpose, PaintedWaterSurface,
}; // Clipmap选中覆盖继续使用同一遮罩/水深材质，GLB仍复用完成Mesh。 // 铜金范围框复用F12强调色。

use super::ui::sync_badge; // 世界解析状态增量回显到固定中央徽标。
use super::{
    SOURCE_LOOKUP_RETRY_FRAMES, SelectedWaterIdentity, SelectedWaterInfo,
    SelectedWaterViewportState, focus_extent, selected_water_info,
    source_lookup_signature, surface_matches, transformed_water_frame,
}; // 状态、纯身份与取景数学集中由父模块维护。
use crate::authoring::{EditorAuthoringSession, WaterPreviewState}; // 联合水域/地形修订决定当前几何是否就绪。
use crate::components::{
    EditorWaterPreview, SceneEditorSelectedWaterBadge, SceneEditorSelectedWaterBadgeText,
    SceneEditorSelectedWaterHighlight,
}; // 结构化marker支持固定UI与唯一世界覆盖实体。
use crate::resources::{SceneEditorState, SceneEditorViewportState}; // 双击只调整现有编辑相机会话态。
use crate::types::WaterAuthoringFeatureKind; // 徽标和诊断明确区分三类作者水域对象。
use crate::viewport::LayerCandidateState;
use super::source::{WaterSourceContext, WaterSurfaceScope};
use feathrise_scene_core::residency::SceneInstanceOwner;
use crate::viewport::tool_preview::AuthorToolFrame;
use feathrise_author_tools::{AuthorTool, SelectionPreview};
use super::highlight::WaterHighlightContext; // 完整分块覆盖与同源波形材质由独立上下文按预算同步。

/// 在选择或完整预览换代时扫描一次有限水面根，并优先采用作者预览。
fn find_selected_surface(
    identity: &SelectedWaterIdentity,
    scope: WaterSurfaceScope,
    author_preview_ready: bool,
    surfaces: &Query<
        (
            Entity,
            &PaintedWaterSurface,
            &Mesh3d,
            &Transform,
            Option<&Aabb>,
            &Visibility,
            Option<&ClipmapWaterSurface>,
            Option<&MeshMaterial3d<ClipmapWaterMaterial>>,
            Option<&SceneInstanceOwner>,
            Has<EditorWaterPreview>,
        ),
        Without<SceneEditorSelectedWaterHighlight>,
    >,
) -> Option<Entity> {
    let mut runtime_fallback = None; // 作者预览尚未安装时允许高亮已保存正式水面。
    if scope == WaterSurfaceScope::Unavailable && !author_preview_ready { return None; }
    for (entity, surface, _, _, _, visibility, clipmap, _, owner, author_marker) in surfaces.iter() {
        if !surface_scope_matches(scope, owner, author_marker, surface, author_preview_ready)
            || !surface_is_selectable(surface, identity, *visibility)
            || clipmap.is_some_and(|surface| !surface.is_published())
        {
            continue; // 跳过其他身份以及已被作者预览接管的隐藏旧水面。
        }
        if surface.purpose == PaintedWaterPurpose::EditorPreview {
            return Some(entity); // 未保存作者预览是当前编辑语义的最高优先级。
        }
        runtime_fallback.get_or_insert(entity); // 正式水面只作为尚无作者预览时的安全回退。
    }
    runtime_fallback // 空层或尚未完成预览时明确返回None。
}

/// GLB作者预览在父域创建；只有完整预览采用门可独立授权它，普通无owner运行面仍服从实例门。
fn surface_scope_matches(scope: WaterSurfaceScope, owner: Option<&SceneInstanceOwner>,
    author_marker: bool, surface: &PaintedWaterSurface, allow_author_preview: bool) -> bool {
    scope.accepts(owner) || (allow_author_preview && author_marker
        && surface.purpose == PaintedWaterPurpose::EditorPreview)
}

/// 只有身份完全匹配且当前可见的水面才能支撑高亮；隐藏旧代不得让空层复活。
fn surface_is_selectable(
    surface: &PaintedWaterSurface,
    identity: &SelectedWaterIdentity,
    visibility: Visibility,
) -> bool {
    surface_matches(surface, identity) // 先验证场景、稳定对象ID和三类水面类别。
        && visibility != Visibility::Hidden // 作者预览采用后隐藏的正式旧水面不得作为回退。
}

/// 生成当前水域对象在徽标和状态栏中共用的人类可读摘要。
fn selected_water_summary(info: &SelectedWaterInfo) -> String {
    match info.identity.kind {
        WaterAuthoringFeatureKind::StillWater => format!(
            "静水体 {} · Y {:.3}m",
            info.label,
            info.water_level.unwrap_or_default(), // 静水快照保证存在水位，默认值仅防御未来坏状态。
        ),
        WaterAuthoringFeatureKind::River => format!(
            "河流 {} · {}点",
            info.label,
            info.point_count.unwrap_or_default(), // 河流快照保证存在控制点计数。
        ),
        WaterAuthoringFeatureKind::Waterfall => format!(
            "瀑布 {} · {}点",
            info.label,
            info.point_count.unwrap_or_default(), // 瀑布快照保证存在控制点计数。
        ),
    }
}

/// 状态栏短类别名与左栏统一对象列表保持一致。
fn selected_water_kind_name(kind: WaterAuthoringFeatureKind) -> &'static str {
    match kind {
        WaterAuthoringFeatureKind::StillWater => "静水体",
        WaterAuthoringFeatureKind::River => "河流",
        WaterAuthoringFeatureKind::Waterfall => "瀑布",
    }
}

/// 同步精确水面Mesh覆盖层、中央徽标，并在双击请求就绪后调整现有编辑相机。
#[allow(clippy::too_many_arguments)]
pub(crate) fn sync_selected_water_viewport(
    mut commands: Commands,
    mut editor: ResMut<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    candidates: Option<Res<LayerCandidateState>>, // 独立测试无需初始化整个编辑器候选系统。
    preview: Res<WaterPreviewState>,
    source_context: WaterSourceContext,
    mut selection: ResMut<SelectedWaterViewportState>,
    viewport: Option<ResMut<SceneEditorViewportState>>,
    mut highlight: WaterHighlightContext,
    surfaces: Query<
        (
            Entity,
            &PaintedWaterSurface,
            &Mesh3d,
            &Transform,
            Option<&Aabb>,
            &Visibility,
            Option<&ClipmapWaterSurface>,
            Option<&MeshMaterial3d<ClipmapWaterMaterial>>,
            Option<&SceneInstanceOwner>,
            Has<EditorWaterPreview>,
        ),
        Without<SceneEditorSelectedWaterHighlight>,
    >,
    mut badges: Query<
        &mut Visibility,
        (
            With<SceneEditorSelectedWaterBadge>,
            Without<PaintedWaterSurface>,
            Without<SceneEditorSelectedWaterHighlight>,
        ),
    >, // UI徽标显式排除水面源和世界高亮，向Bevy证明三个Visibility查询互斥并避免B0001。
    mut badge_texts: Query<&mut Text, With<SceneEditorSelectedWaterBadgeText>>,
) {
    let previewing = !selection.explicit_focus_pending()
        && editor.tool == crate::types::EditorTool::Inspect
        && candidates
            .as_ref()
            .is_some_and(|state| state.hover.is_some()); // 悬停只允许修改表现缓存。
    let Some(info) = selected_water_info(&editor, &session, &selection, candidates.as_deref())
    else {
        highlight.hide(&selection); // 立即移除完整水体选择表现但保留可复用实体。
        sync_badge(false, None, &mut selection, &mut badges, &mut badge_texts); // 中央徽标同步隐藏。
        if !previewing {
            selection.focus_request = None; // 正式离开水域后才撤销定位；悬停不能修改请求。
            selection.clear_click_sequence(); // 正式选择变化才终止双击。
        } // 跨类型悬停仅隐藏覆盖。
        selection.resolved_identity = None; // 下一次进入时重新按当前草稿身份解析。
        selection.lookup_signature = None; // 下一次选择允许一次有限实体扫描。
        selection.lookup_retry_frames = 0; // 取消旧Commands边界重试。
        selection.source_entity = None; // 不再验证旧水面根。
        selection.frame = None; // Gizmo系统下一帧不绘制旧取景框。
        selection.center_source = None;
        return;
    };

    if selection.resolved_identity.as_ref() != Some(&info.identity) {
        selection.resolved_identity = Some(info.identity.clone()); // 记录新稳定选择。
        selection.lookup_signature = None; // 新对象必须执行一次有限实体解析。
        selection.lookup_retry_frames = SOURCE_LOOKUP_RETRY_FRAMES; // 覆盖命令可见性边界。
        selection.source_entity = None; // 旧对象实体不可继续复用。
        selection.frame = None; // 新实体解析前先清除旧世界范围。
        selection.center_source = None;
        if !previewing && selection.focus_request.as_ref() != Some(&info.identity) {
            selection.focus_request = None; // 非本对象定位请求不得穿过选择切换。
        }
    }

    let scope = source_context.scope(&session);
    let author_source = super::WaterSourceIdentity::capture(&session, &info.identity.feature_id);
    let lookup_signature = source_lookup_signature(&info.identity, &preview, scope, author_source.clone());
    // 画布采用和切换必推进water_revision，ready联合修订因此不能为兄弟画布的同名层背书。
    let author_preview_ready = author_source.is_some() && preview.ready_for(&info.identity.scene_id,
        session.water_revision, session.terrain_revision);
    let same_cached_source = lookup_signature.source.is_some()
        && selection.lookup_signature.as_ref().is_some_and(|before|
            before.scope == lookup_signature.scope && before.source == lookup_signature.source);
    // 已由Ready验证的同一个GLB预览可在水位/R8新预览等待时保持高亮，换画布或源路径立即撤销。
    let allow_cached_author_preview = author_preview_ready || same_cached_source;
    let cached_valid = selection.source_entity.is_some_and(|entity| {
        surfaces
            .get(entity)
            .is_ok_and(|(_, surface, _, _, _, visibility, clipmap, _, owner, author_marker)| {
                surface_scope_matches(scope, owner, author_marker, surface, allow_cached_author_preview)
                    && surface_is_selectable(surface, &info.identity, *visibility)
                    && clipmap.is_none_or(|surface| surface.is_published())
            }) // O(1)验证缓存身份与当前可见性。
    });
    let cached_disappeared = selection.source_entity.is_some() && !cached_valid; // 预览原子换代后旧实体会失效。
    let generation_changed = selection.lookup_signature.as_ref() != Some(&lookup_signature); // 新完整批次需要优先切到作者预览。
    if generation_changed {
        selection.lookup_retry_frames = SOURCE_LOOKUP_RETRY_FRAMES; // 允许采用命令跨帧可见后补查。
    }
    if generation_changed || cached_disappeared || selection.lookup_retry_frames > 0 {
        selection.source_entity = find_selected_surface(&info.identity, scope, author_preview_ready, &surfaces); // 沿用既有离散查找，仅当前实例或已采用GLB作者预览有资格。
        selection.lookup_signature = Some(lookup_signature); // 稳定空层也能在重试结束后短路。
        if selection.source_entity.is_some() {
            selection.lookup_retry_frames = 0; // 找到实体后后续帧只执行O(1)验证。
        } else {
            selection.lookup_retry_frames = selection.lookup_retry_frames.saturating_sub(1); // 空层最多补查固定两帧。
        }
    }

    let source_snapshot = selection.source_entity.and_then(|entity| {
        surfaces
            .get(entity)
            .ok()
            .filter(|(_, surface, _, _, _, visibility, clipmap, _, owner, author_marker)| {
                surface_scope_matches(scope, *owner, *author_marker, surface, allow_cached_author_preview)
                    && surface_is_selectable(surface, &info.identity, **visibility)
                    && clipmap.is_none_or(|surface| surface.is_published())
            })
            .map(
                |(_, surface, mesh, transform, aabb, _, clipmap, clipmap_material, _, author_marker)| {
                    let source_to_world = if author_marker && surface.purpose == PaintedWaterPurpose::EditorPreview {
                        session.draft.as_ref().and_then(|draft| draft.terrain_documents.active.as_ref())
                            .map_or(Mat4::IDENTITY, |active| active.definition.transform.as_transform().to_matrix())
                    } else { scope.matrix() }; // 父域GLB预览也必须应用当前画布完整TRS。
                    let clipmap_frame = clipmap.map(|surface| {
                        let (min, max) = surface.world_coverage_bounds();
                        let bounds = Aabb::from_min_max(
                            Vec3::new(min.x, surface.water_level, min.y),
                            Vec3::new(max.x, surface.water_level, max.y));
                        transformed_water_frame(&Transform::from_matrix(source_to_world), &bounds)
                    }); // Clipmap范围来自R8非零包围元数据，不能使用共享整环Aabb。
                    (
                        mesh.clone(),
                        Transform::from_matrix(source_to_world * transform.to_matrix()),
                        aabb.copied(),
                        clipmap_frame,
                        clipmap_material.cloned(),
                    )
                },
            ) // 脱离Query借用后再改资源与覆盖实体。
    });
    let frame = source_snapshot
        .as_ref()
        .and_then(|(_, transform, aabb, clipmap_frame, _)| {
            clipmap_frame.or_else(|| aabb.map(|aabb| transformed_water_frame(transform, &aabb)))
        }); // 没有后台Aabb就视为不可定位几何。
    if selection.frame != frame {
        selection.frame = frame; // Gizmo只在真实范围变化时读取新完成值。
    }
    // 旧完整水面仍可高亮，但不能把其范围重新盖章为新水位、新画布或新R8的手柄依据。
    selection.center_source = frame.filter(|_| preview.ready_for(&info.identity.scene_id,
        session.water_revision, session.terrain_revision)).and(author_source);

    if let Some((source_mesh, source_transform, _, _, _)) =
        source_snapshot.as_ref().filter(|_| frame.is_some())
    {
        let source = selection.source_entity.expect("当前完整水面快照已验证来源根");
        highlight.sync(&mut commands, &mut selection, source, source_mesh, source_transform);
    } else {
        highlight.hide(&selection); // 空层、失败或预览等待时不伪造水面范围。
    }

    let preview_ready = preview.ready_for(
        &info.identity.scene_id,
        session.water_revision,
        session.terrain_revision,
    ); // 只有水域与地形联合修订都采用才代表当前完整几何。
    let preview_failed = preview.failed_for(
        &info.identity.scene_id,
        session.water_revision,
        session.terrain_revision,
    ); // 失败时可能继续保留上一完整批次供诊断。
    let summary = selected_water_summary(&info); // 三类作者对象共用一份稳定名称与参数摘要。
    let badge_text = if previewing {
        format!("预览：{summary} · 点击候选确认选择") // 临时预高亮不能冒充正式选中或提示双击。
    } else if frame.is_some() && preview_ready {
        format!("已选：{summary} · 再次点击同一水面定位") // 当前完整水面已经精确高亮。
    } else if frame.is_some() && preview_failed {
        format!("已选：{summary} · 预览失败，正高亮上一完整版本") // 保留旧几何时明确避免误认为最新作者结果。
    } else if frame.is_some() {
        format!("已选：{summary} · 水面更新中，正高亮上一完整版本") // single-flight构建期间继续提供稳定空间参照。
    } else if preview_ready {
        format!("已选：{summary} · 暂无可见水面") // 合法干涸、空层或未完成样条明确不可高亮和定位。
    } else if preview_failed {
        format!("已选：{summary} · 预览失败，暂不可定位") // 无上一批实体时只能回显明确失败。
    } else {
        format!("已选：{summary} · 水面预览生成中") // 初次构建期间不伪造空层结论。
    };
    sync_badge(
        true,             // 当前工具或“查看”点选存在有效水域对象。
        Some(badge_text), // 文案包含类别、名称和实时预览状态。
        &mut selection,
        &mut badges,
        &mut badge_texts,
    );

    if previewing || selection.focus_request.as_ref() != Some(&info.identity) {
        return; // 普通单击只高亮，绝不自动移动作者相机。
    }
    if !preview_ready && !preview_failed {
        return; // 双击发生在构建中时等待联合完整结果，避免定位旧轮廓。
    }
    if let (Some(frame), Some(mut viewport)) = (frame, viewport) {
        viewport.center = frame.center; // 保留当前透视/顶/前/侧视图，只移动观察中心。
        viewport.extent = focus_extent(frame).max(crate::viewport::camera_math::framing_extent(
            frame.size * 0.5,
            viewport.yaw,
            viewport.pitch,
            viewport.render_size,
            editor.orthographic,
        )); // 窄视口也完整容纳水面。
        selection.focus_request = None; // 成功采用后不在后续帧重复抢回用户相机。
        let kind_name = selected_water_kind_name(info.identity.kind); // 状态栏保持对象类别明确。
        editor.status = if preview_ready {
            format!("已定位{kind_name}：{}", info.label) // 当前联合修订的精确完成几何。
        } else {
            format!(
                "已定位{kind_name}上一完整版本：{} · 当前水面预览失败",
                info.label
            ) // 失败回退必须明确不是最新作者轮廓。
        };
    } else if frame.is_none() {
        selection.focus_request = None; // 已就绪但无几何时结束本次请求，避免永久等待。
        let kind_name = selected_water_kind_name(info.identity.kind); // 空几何诊断仍明确具体对象类别。
        editor.status = if preview_ready {
            format!("{kind_name}{}暂无可定位水面", info.label) // 合法空层、干涸层或未完成样条诊断。
        } else {
            format!("{kind_name}{}预览失败且没有上一完整水面可定位", info.label) // 失败且无回退实体。
        };
    }
}

/// 在精确淡青填充外围绘制当前水面的铜金三维世界范围，帮助远景快速辨认位置和尺度。
pub(crate) fn draw_selected_water_highlight_frame(
    editor: Res<SceneEditorState>,
    selection: Res<SelectedWaterViewportState>,
    mut tools: ResMut<AuthorToolFrame>,
) {
    if !editor.open {
        return; // 关闭F12时不提交辅助线。
    }
    let Some(frame) = selection.frame else {
        return; // 空层和等待状态只显示文字诊断，不伪造三维范围。
    };
    let Some(identity) = &selection.resolved_identity else { return; };
    let half = frame.size.max(Vec3::splat(0.1)) * 0.5;
    tools.push(AuthorTool::Selection(SelectionPreview {
        target: format!("water:{}/{:?}/{}", identity.scene_id.as_str(), identity.kind, identity.feature_id),
        center: frame.center.to_array(),
        half_axes: [Vec3::X * half.x, Vec3::Y * half.y, Vec3::Z * half.z].map(|axis| axis.to_array()),
        color: colors::TEXT_ACCENT.to_linear().to_f32_array(),
    }));
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::{IntoSystem, System}; // 直接初始化生产系统即可触发Bevy查询访问冲突校验。
    use feathrise_scene_content::SceneId; // 构造稳定场景与静水层身份。

    use super::*; // 直接锁定隐藏旧代不能支撑当前选择的解析门。

    #[test] // 静水源、世界高亮和UI徽标必须保持Visibility访问互斥。
    fn viewport_sync_system_initializes_without_visibility_conflicts() {
        let mut world = World::new(); // 最小World足以初始化全部SystemParam查询状态。
        let mut system = IntoSystem::into_system(sync_selected_water_viewport); // 使用真实生产系统锁定完整参数组合。
        system.initialize(&mut world); // 若互斥过滤退化，Bevy会在本行以B0001终止测试。
    }

    #[test] // 作者预览采用后隐藏的正式旧水面不得让已清空静水层重新出现。
    fn hidden_runtime_surface_cannot_back_an_empty_selected_layer() {
        let identity = SelectedWaterIdentity {
            scene_id: SceneId("MAP-hidden-water".to_owned()), // 测试场景稳定身份。
            kind: WaterAuthoringFeatureKind::StillWater,  // 模拟刚清空的静水层。
            feature_id: "lake-empty".to_owned(),          // 稳定静水层身份。
        };
        let runtime_surface = PaintedWaterSurface {
            scene_id: identity.scene_id.clone(),          // marker仍属于当前场景。
            layer_id: identity.feature_id.clone(),    // marker仍保留同一稳定层ID。
            kind: feathrise_scene::WaterSurfaceKind::Still, // 几何类别也完全匹配。
            purpose: PaintedWaterPurpose::Runtime,    // 这是已保存正式旧代。
        };
        assert!(!surface_is_selectable(
            &runtime_surface,
            &identity,
            Visibility::Hidden,
        )); // 作者预览已隐藏旧代时必须回显空层而不是重绘旧Mesh。
        assert!(surface_is_selectable(
            &runtime_surface,
            &identity,
            Visibility::Inherited,
        )); // 初次预览尚未接管时仍允许可见正式水面作为临时空间参照。
    }
}
