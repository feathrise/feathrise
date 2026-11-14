//! 任意GLB拓扑地形笔刷、局部撤销差分和后台完整派生预览。

use std::{collections::BTreeMap, sync::Arc}; // 单笔划记录首次世界Y，完整派生值以Arc共享。

use avian3d::prelude::{Collider, TrimeshFlags}; // 预览与正式接管使用同拓扑Trimesh。
use bevy::asset::RenderAssetUsages; // 预览Mesh和内存高度Image保留主世界与渲染世界用途。
use bevy::image::ImageSampler; // 数值高度单元固定最近采样。
use bevy::mesh::Indices; // GLB显式U32 TriangleList索引保持原顺序。
use bevy::prelude::*; // ECS、PBR、输入和数学类型。
use bevy::render::render_resource::{Extent3d, PrimitiveTopology, TextureDimension, TextureFormat}; // 后台Mesh与主线程常数登记RGBA16 Image。
use bevy::tasks::{AsyncComputeTaskPool, futures::check_ready}; // 全拓扑派生只在后台单飞执行。
use bevy::ui::RelativeCursorPosition; // 笔刷只命中中央视口。
use feathrise_environment::terrain_surface::TerrainSurfaceMaterial; // 作者预览复用正式PBR地表材质。
use feathrise_scene_content::SceneTerrainKind; // 只有动态Heightfield拥有GLB可编辑契约。
use feathrise_terrain::{TerrainDerivedData, TerrainSampleSpec, derive_terrain}; // 与运行时共用三维验证、法线和最高表面高度栅格化。
use feathrise_scene::{DynamicSceneTerrainPrimitive, TerrainGridTarget}; // 作者预览显示时隐藏正式网格，并显式加入F12三角网格目标。

use super::paint::brush_overlaps_world_bounds; // 纹理与顶点笔刷共用越界圆相交语义。
use super::resources::{
    EditorAuthoringSession,
    EditorCommand,
    HeightChange,
    SurfacePreviewState,
    TerrainBrushStroke,
    TerrainLiveApplyState,
    TerrainPreviewBuild,
    TerrainPreviewIdentity,
    TerrainPreviewState,
    WaterBrushStroke,
    WaterMaskMaterializeTask, // 静水整笔历史共享地形时复用后台写侧准备。
}; // 地形作者状态、撤销命令和同任务画面/物理采用值。
use super::water_paint::{poll_terrain_materialization, start_terrain_materialization_if_shared};
use crate::components::{EditorTerrainPreview, SceneEditorViewport}; // 作者预览与中央视口身份。
use crate::resources::SceneEditorState; // F12工具、笔刷参数和用户状态。
use crate::types::EditorTool; // 抬高、降低、平滑和压平工具枚举。 // 不等待长期历史引用释放。

/// 判断当前工具是否直接修改GLB顶点Y。
fn is_terrain_tool(tool: EditorTool) -> bool {
    matches!(
        tool,
        EditorTool::Raise | EditorTool::Lower | EditorTool::Smooth | EditorTool::Flatten
    ) // 其他纹理、摆放和水网工具不得取得地形写侧。
} // 结束地形工具判断。

/// 计算本帧抬高或降低的世界米制位移。
fn raise_lower_delta_meters(
    tool: EditorTool,
    speed: f32,
    strength: f32,
    delta_seconds: f32,
    falloff: f32,
) -> f32 {
    let direction = match tool {
        EditorTool::Raise => 1.0,  // 抬高沿世界Y正方向。
        EditorTool::Lower => -1.0, // 降低沿世界Y负方向。
        _ => 0.0,                  // 其他工具不产生升降量。
    }; // 完成方向选择。
    let speed = speed.is_finite().then_some(speed.max(0.0)).unwrap_or(0.0); // 损坏设置安全退化为零位移。
    direction * speed * strength.clamp(0.01, 1.0) * delta_seconds.max(0.0) * falloff.clamp(0.0, 1.0) // 所有倍率保持有限受界的米制语义。
} // 结束升降位移计算。

/// 左键持续修改实际GLB顶点；松开时只产生一条局部f32撤销记录。
pub(crate) fn apply_terrain_brush(
    mouse: Res<ButtonInput<MouseButton>>,
    time: Res<Time<Real>>,
    viewport: Query<&RelativeCursorPosition, With<SceneEditorViewport>>,
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut stroke: ResMut<TerrainBrushStroke>,
    mut materialize: ResMut<WaterMaskMaterializeTask>, // 与水笔、撤销共享唯一GLB后台复制槽。
) {
    poll_terrain_materialization(&mut session, &mut materialize); // 关闭或换工具时也非阻塞回收完成任务。
    if editor.open && !editor.close_requested && !session.saving && is_terrain_tool(editor.tool) {
        start_terrain_materialization_if_shared(&session, &mut materialize); // 提前准备独立网格，避免静水历史使雕刻永久等待。
    }
    let hovered = viewport
        .single()
        .is_ok_and(RelativeCursorPosition::cursor_over); // 只有中央视口消费左键雕刻。
    let terrain_editable = session.draft.as_ref().is_some_and(|draft| {
        draft.manifest.is_data_driven()
            && draft.manifest.terrain.kind == SceneTerrainKind::Gltf
            && draft.terrain_mesh.is_some()
            && draft.terrain_editing_index.is_some()
    }); // 动态GLB网格和其拓扑索引必须一起存在。
    if editor.close_requested {
        finish_stroke(&mut editor, &mut session, &mut stroke); // 关闭门主动收口当前拖动。
        return; // 收口后由后台派生和保存门继续推进。
    } // 完成关闭请求分支。
    if !editor.open
        || !is_terrain_tool(editor.tool)
        || !terrain_editable
        || session.saving
        || !hovered
    {
        if mouse.just_released(MouseButton::Left) {
            finish_stroke(&mut editor, &mut session, &mut stroke); // 鼠标在视口外松开也必须结束命令。
        } // 完成外部松手处理。
        return; // 当前帧不修改顶点。
    } // 完成编辑入口门。
    let Some(world) = editor.brush_cursor_world else {
        if mouse.just_released(MouseButton::Left) {
            finish_stroke(&mut editor, &mut session, &mut stroke); // 射线离开实际Mesh后松手仍结束命令。
        } // 完成无命中松手处理。
        return; // 只有实际预览Mesh射线命中才能雕刻。
    }; // 完成世界命中门。
    let snapshot_is_freezing = session
        .draft
        .as_ref()
        .and_then(|draft| draft.terrain_mesh.as_ref())
        .is_some_and(|mesh| Arc::strong_count(mesh) > 1); // 历史或派生共享时等待后台独立写侧。
    if snapshot_is_freezing {
        if mouse.pressed(MouseButton::Left) {
            editor.status = "正在后台冻结上一修订的地形网格；完成后即可继续下一笔".to_owned();
        }
        return; // 禁止Arc::make_mut在输入帧同步复制完整顶点与索引；下一次按下只访问局部桶。
    } // 完成后台快照冻结门。
    if mouse.just_pressed(MouseButton::Left) {
        stroke.active = true; // 新拖动开始唯一命令。
        stroke.before.clear(); // 清除上一笔首次值表。
        let (minimum, maximum) = session
            .draft
            .as_ref()
            .map(|draft| {
                let minimum = draft.manifest.terrain.base_height; // GLB允许的最低世界Y。
                (minimum, minimum + draft.manifest.terrain.height_scale) // GLB允许的最高世界Y。
            })
            .unwrap_or((world.y, world.y)); // 防御性退化不产生越界目标。
        stroke.flatten_target = Some(world.y.clamp(minimum, maximum)); // 压平直接冻结实际三角命中的世界高度。
    } // 完成新笔划初始化。
    if mouse.pressed(MouseButton::Left) && stroke.active {
        let changed = modify_vertex_neighborhood(
            editor.tool,
            world,
            editor.brush_radius,
            editor.brush_strength,
            editor.terrain_sculpt_speed,
            time.delta_secs().clamp(0.0, 0.05),
            stroke.flatten_target,
            editor.brush_cursor_triangle,
            &mut stroke.before,
            session.draft.as_mut(),
        ); // X/Z空间索引只访问实际圆内顶点。
        if changed {
            session.mark_terrain_changed(&mut editor); // 每个真实帧变化推进派生目标，任务只在松手后启动。
            editor.status = "地形顶点已修改；松开后后台派生高度、法线和Trimesh".to_owned(); // 明确当前仍是拖动阶段。
        } // 完成真实变化处理。
    } // 完成按住雕刻处理。
    if mouse.just_released(MouseButton::Left) {
        finish_stroke(&mut editor, &mut session, &mut stroke); // 一次拖动合并为一条撤销记录。
    } // 完成松手收口。
} // 结束地形笔刷系统。

/// 使用空间桶与真实一环邻接修改圆内顶点世界Y。
#[allow(clippy::too_many_arguments)]
fn modify_vertex_neighborhood(
    tool: EditorTool,
    world: Vec3,
    radius: f32,
    strength: f32,
    speed: f32,
    delta_seconds: f32,
    flatten_target: Option<f32>,
    hit_triangle: Option<u32>,
    before: &mut BTreeMap<usize, f32>,
    draft: Option<&mut super::resources::EditorSceneDraft>,
) -> bool {
    let Some(draft) = draft else {
        return false; // 草稿不存在时无写侧。
    }; // 完成草稿门。
    if !radius.is_finite()
        || radius <= 0.0
        || !brush_overlaps_world_bounds(
            world,
            radius,
            draft.manifest.world_min,
            draft.manifest.world_max,
        )
    {
        return false; // 损坏或完全离图圆不取得Arc唯一写侧。
    } // 完成笔刷范围门。
    let (Some(mesh), Some(editing_index)) = (
        draft.terrain_mesh.as_mut(),
        draft.terrain_editing_index.as_ref(),
    ) else {
        return false; // GLB和拓扑索引缺一不可。
    }; // 完成地形作者源门。
    if Arc::strong_count(mesh) != 1 {
        return false; // 调用入口遗漏冻结门时也绝不在主线程触发整图写时复制。
    } // 完成唯一写侧门。
    let minimum = draft.manifest.terrain.base_height;
    let maximum = minimum + draft.manifest.terrain.height_scale;
    let kind = match tool {
        EditorTool::Raise | EditorTool::Lower => feathrise_terrain::TerrainHeightBrushKind::RaiseLower,
        EditorTool::Smooth => feathrise_terrain::TerrainHeightBrushKind::Smooth,
        EditorTool::Flatten => feathrise_terrain::TerrainHeightBrushKind::Flatten,
        _ => return false,
    };
    let brush = feathrise_terrain::TerrainHeightBrush { kind, center: world, radius, strength, delta_seconds,
        // 平滑和压平使用相同米制速度预算，方向由冻结的目标高度决定。
        delta_height: raise_lower_delta_meters(if tool == EditorTool::Lower { EditorTool::Lower } else { EditorTool::Raise }, speed, strength, delta_seconds, 1.0),
        flatten_target, minimum, maximum, hit_triangle };
    let Some(mesh) = Arc::get_mut(mesh) else { return false; };
    let Ok(changes) = feathrise_terrain::sculpt_height_vertices(mesh, editing_index, brush) else { return false; };
    let changed = !changes.is_empty();
    for change in changes { before.entry(change.index).or_insert(change.before); }
    changed
} // 结束局部顶点修改。

/// 收口当前拖动并生成保持GLB顶点顺序的f32局部命令。
fn finish_stroke(
    editor: &mut SceneEditorState,
    session: &mut EditorAuthoringSession,
    stroke: &mut TerrainBrushStroke,
) {
    if !stroke.active {
        return; // 没有活动拖动时不生成空命令。
    } // 完成活动门。
    stroke.active = false; // 从此允许后台派生启动。
    let changes = session
        .draft
        .as_ref()
        .and_then(|draft| draft.terrain_mesh.as_ref())
        .map(|mesh| {
            stroke
                .before
                .iter()
                .filter_map(|(&index, &before)| {
                    let after = mesh.positions.get(index)?[1]; // 按原GLB顶点下标读取最终世界Y。
                    (before != after).then_some(HeightChange {
                        index,
                        before,
                        after,
                    }) // 拖回原值不保留命令。
                })
                .collect::<Vec<_>>() // 只保存真实变化顶点。
        })
        .unwrap_or_default(); // 草稿意外换代时安全丢弃旧笔划。
    stroke.before.clear(); // 释放局部首次值表。
    stroke.flatten_target = None; // 下一笔重新从实际三角命中取目标。
    if !changes.is_empty() {
        session.undo.push(EditorCommand::TerrainChanged(changes)); // 一次拖动恰好一条命令。
        session.redo.clear(); // 新作者分支使旧重做历史失效。
        editor.status = "地形笔划完成；正在后台派生内存高度、法线和Trimesh".to_owned(); // 保存保持禁用直到完整结果落稳。
    } // 完成非空命令登记。
} // 结束笔划收口。

/// 采样规格属于当前作者源；多地形容器的空描述不能代替所选GLB的合法规格。
fn terrain_preview_identity(session: &EditorAuthoringSession) -> Option<TerrainPreviewIdentity> {
    let draft = session.draft.as_ref()?;
    if draft.terrain_documents.loading
        || (draft.manifest.terrains.is_some() && draft.terrain_documents.active.is_none()) {
        return None; // 作者槽尚未完整交换时，不把上一槽网格与新来源配对。
    }
    let source = draft.terrain_manifest();
    if !source.is_data_driven() || source.terrain.kind != SceneTerrainKind::Gltf {
        return None; // 原资产GLB、Clipmap和体素各用自己的运行后端，不强行生成最高高度包络。
    }
    Some(TerrainPreviewIdentity {
        scene_id: draft.scene_id.clone(),
        terrain_id: draft.terrain_documents.active.as_ref().map(|active| active.definition.id.clone()),
        revision: session.terrain_revision,
        spec: TerrainSampleSpec {
            resolution: source.terrain.resolution,
            world_min: source.world_min,
            world_max: source.world_max,
            base_height: source.terrain.base_height,
            height_scale: source.terrain.height_scale,
        },
        source_path: source.terrain.glb_path.clone(),
        source_sha256: draft.terrain_source_sha256,
        mesh_identity: Arc::as_ptr(draft.terrain_mesh.as_ref()?) as usize,
    })
}

/// 地形笔划、撤销或水域下挖提交后启动一个完整GLB派生任务。
pub(crate) fn start_terrain_preview_rebuild(
    editor: Res<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    terrain_stroke: Res<TerrainBrushStroke>,
    water_stroke: Res<WaterBrushStroke>,
    mut preview: ResMut<TerrainPreviewState>,
    mut live: ResMut<TerrainLiveApplyState>,
) {
    if preview.retirement.as_mut().and_then(check_ready).is_some() {
        preview.retirement = None;
    }
    if !editor.open
        || terrain_stroke.active
        || water_stroke.active
        || editor.undo_requested
        || editor.redo_requested
    {
        return; // 活动拖动与撤销事务中不启动会立即过期的整图任务。
    } // 完成稳定输入门。
    let current = terrain_preview_identity(&session);
    if preview.desired_identity != current {
        preview.failed_identity = None; // 只有输入真实换代才允许重新尝试，稳定坏修订不占用后台槽。
        live.failed_scene_id = None;
        live.failed_revision = None;
    }
    preview.desired_scene_id = current.as_ref().map(|identity| identity.scene_id.clone());
    preview.desired_revision = session.terrain_revision;
    preview.desired_identity = current.clone();
    let Some(identity) = current else { return; };
    if preview.task.is_some()
        || preview.retirement.is_some()
        || preview.failed_identity.as_ref() == Some(&identity)
        || (preview.applied_identity.as_ref() == Some(&identity)
            && preview.ready_for(&identity.scene_id, identity.revision))
    {
        return; // single-flight或当前修订已经完整采用。
    } // 完成任务槽与完成证据门。
    let mesh = session.draft.as_ref().and_then(|draft| draft.terrain_mesh.clone())
        .expect("完整派生身份已核对作者网格");
    preview.task = Some(AsyncComputeTaskPool::get().spawn(async move {
        let snapshot = (*mesh).clone(); // 深复制GLB数组发生在后台而非启动帧。
        drop(mesh); // 快照冻结完成即归还作者网格唯一写侧，不等待后续完整派生结束。
        let result = build_preview(snapshot, identity.spec);
        (identity, result) // 同一对象的冻结规格同时用于后台派生与主世界Image采用。
    })); // 启动当前唯一地形CPU任务。
} // 结束地形派生启动系统。

/// 后台一次构造重算法线Mesh、Trimesh、CPU高度和RGBA16高度单元。
fn build_preview(
    mesh: feathrise_terrain::TerrainMeshInput,
    spec: TerrainSampleSpec,
) -> Result<TerrainPreviewBuild, String> {
    let derived = derive_terrain(mesh, spec)?; // 共享算法允许洞穴多层，同时拒绝无地面采样列、越界和退化拓扑。
    let collider = Collider::trimesh_with_config(
        derived
            .positions
            .iter()
            .map(|position| Vec3::from_array(*position))
            .collect(), // Collider取得独立世界顶点。
        derived
            .indices
            .chunks_exact(3)
            .map(|triangle| [triangle[0], triangle[1], triangle[2]])
            .collect(), // 保持GLB原三角顺序与连接关系。
        TrimeshFlags::FIX_INTERNAL_EDGES, // 与正式动态场景使用同一内部边修正。
    ); // 完成后台Trimesh构造。
    let mesh = derived_to_mesh(&derived); // 渲染Mesh属性循环同样留在后台。
    Ok(TerrainPreviewBuild {
        mesh,
        collider,
        heights: derived.heights,
        height_cells_rgba16: derived.height_cells_rgba16,
    }) // 全部成功后才返回可原子采用批次。
} // 结束后台完整构建。

/// 把共享派生值转为带编辑期UV的Bevy Mesh。
fn derived_to_mesh(derived: &TerrainDerivedData) -> Mesh {
    let width = (derived.max[0] - derived.min[0]).max(f32::EPSILON); // UV归一化X跨度。
    let depth = (derived.max[2] - derived.min[2]).max(f32::EPSILON); // UV归一化Z跨度。
    let uvs = derived
        .positions
        .iter()
        .map(|position| {
            [
                (position[0] - derived.min[0]) / width,
                (position[2] - derived.min[2]) / depth,
            ] // UV只供编辑器综合色预览，保存GLB不写该属性。
        })
        .collect::<Vec<_>>(); // 大循环发生在后台任务。
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    ); // 完成空正式预览Mesh。
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, derived.positions.clone()); // 保持GLB原顶点顺序。
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, derived.normals.clone()); // 使用共享面积加权重算法线。
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs); // 编辑器临时UV不进入保存契约。
    mesh.insert_indices(Indices::U32(derived.indices.clone())); // 保持GLB原三角连接关系。
    mesh // 返回完整预览Mesh。
} // 结束渲染Mesh构造。

/// 非阻塞采用身份仍有效的全套派生结果，并同时发布预览、CPU高度、GPU Image和Trimesh候选。
#[allow(clippy::too_many_arguments)]
pub(crate) fn poll_terrain_preview_rebuild(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut preview: ResMut<TerrainPreviewState>,
    mut live: ResMut<TerrainLiveApplyState>,
) {
    let Some(task) = preview.task.as_mut() else {
        return; // 稳定帧没有在途任务。
    }; // 完成任务槽门。
    let Some((identity, result)) = check_ready(task) else {
        return; // 后台未完成时绝不阻塞主线程。
    }; // 完成非阻塞轮询。
    preview.task = None; // 真正完成后释放single-flight槽。
    if preview.desired_identity.as_ref() != Some(&identity)
        || terrain_preview_identity(&session).as_ref() != Some(&identity)
    {
        preview.retirement = Some(AsyncComputeTaskPool::get().spawn(async move { drop(result); }));
        return; // 切换场景、换对象、换源或新笔划后，迟到大结果在后台回收且没有主世界副作用。
    } // 完成采用身份门。
    let scene_id = identity.scene_id.clone();
    let revision = identity.revision;
    let build = match result {
        Ok(build) => build, // 画面、碰撞和两类高度全部成功才继续。
        Err(error) => {
            preview.failed_identity = Some(identity); // 启动端使用相同完整身份停止确定失败的重试。
            live.failed_scene_id = Some(scene_id.clone()); // 同一损坏修订不在稳定帧无限重试。
            live.failed_revision = Some(revision); // 新笔划自然建立新身份。
            editor.status = format!("地形GLB派生失败：{error}"); // 无地面采样列、退化面或量程错误直接呈现给作者。
            error!("场景{scene_id}地形派生失败：revision={revision} error={error}"); // 每任务只记录一次。
            return; // 保留上一完整画面、碰撞和查询高度。
        } // 完成失败分支。
    }; // 完成结果拆分。
    let resolution = identity.spec.resolution; // 成功结果已校验2..=513，不能再读取容器0×0导致下溢。
    let mut height_image = Image::new(
        Extent3d {
            width: resolution[0] - 1,
            height: resolution[1] - 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        build.height_cells_rgba16,
        TextureFormat::Rgba16Unorm,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    ); // 主线程只转移后台完成字节并登记资产。
    height_image.sampler = ImageSampler::nearest(); // 数值四角禁止颜色插值。
    let height_image = images.add(height_image); // 当前作者修订唯一GPU高度句柄。
    let mesh = meshes.add(build.mesh); // 当前作者修订唯一预览Mesh句柄。
    if let Some(entity) = preview.entity {
        commands.entity(entity).insert(Mesh3d(mesh.clone())); // 复用预览实体并原子换Mesh。
    } else {
        preview.entity = Some(
            commands
                .spawn((
                    Name::new("场景编辑器实际拓扑地形预览"),
                    EditorTerrainPreview,
                    TerrainGridTarget, // 未保存作者Mesh与正式地形共用同一显示网格开关，仍无碰撞和正式身份。
                    Mesh3d(mesh.clone()),
                    Visibility::Hidden, // 先保持隐藏；只有未保存地表综合色完整就绪后，显隐系统才允许它接管正式PBR地表。
                ))
                .id(),
        ); // 首次采用创建唯一作者预览实体。
    } // 完成预览实体创建或更新。
    if let Some(draft) = session.draft.as_mut() {
        draft.heights = Some(Arc::new(build.heights)); // CPU规则高度与同任务Mesh/Image一起成为当前修订。
    } // 完成草稿派生查询高度采用。
    preview.mesh = Some(mesh.clone()); // 保留预览Mesh强句柄。
    preview.height_image = Some(height_image); // 保留GPU高度强句柄。
    preview.applied_scene_id = Some(scene_id.clone()); // 发布完整采用场景身份。
    preview.applied_revision = revision; // 发布完整采用地形修订。
    preview.applied_identity = Some(identity);
    preview.failed_identity = None;
    live.failed_scene_id = None;
    live.failed_revision = None;
    let terrain_dirty = session
        .draft
        .as_ref()
        .is_some_and(|draft| draft.terrain_dirty && draft.manifest.terrains.is_none()); // 独立对象通过自己的候选采用，不能借旧单地形链覆盖其他对象。
    if terrain_dirty {
        live.desired_scene_id = Some(scene_id.clone()); // 联合候选属于当前作者场景。
        live.desired_revision = revision; // 联合候选属于当前顶点修订。
        live.active_scene_id = Some(scene_id.clone()); // 画面与物理同时完成后一次登记。
        live.active_revision = revision; // 同一修订证据。
        live.active_mesh = Some(mesh); // 复用后台完整预览Mesh。
        live.active_collider = Some(build.collider); // 采用实际拓扑Trimesh而非规则HeightField。
        live.ready = false; // 下一帧由正式实体维护系统确认组件实际落地。
        live.failed_scene_id = None; // 当前完整成功解除旧失败门。
        live.failed_revision = None; // 当前完整成功解除旧失败修订。
        editor.status = "地形高度、法线、内存Image和Trimesh已完成；正在原子接管正式地形".to_owned(); // 明确联合提交阶段。
    } // 完成未保存正式接管候选登记。
} // 结束地形派生采用系统。

/// GPU地表完整后接管作者网格；新笔划待就绪时保留同场景上一完整PBR画面。
pub(crate) fn sync_terrain_preview_visibility(
    mut commands: Commands, // 同一次系统提交中绑定材质与切换显隐。
    editor: Res<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    mut preview: ResMut<TerrainPreviewState>,
    surface_preview: Res<SurfacePreviewState>,
    runtime_author: Option<Res<feathrise_scene::DynamicSurfaceAuthorPreview>>, // 动态场景正式源接管后不再隐藏游戏地形。
    active: Option<Res<feathrise_scene_content::ActiveDynamicScene>>, // 浏览其他场景仍使用独立作者预览。
    mut preview_visibilities: Query<
        (
            Entity,
            &mut Visibility,
            Option<&MeshMaterial3d<TerrainSurfaceMaterial>>,
        ), // 同时核对实际材质，避免只切显隐留下纯色地表。
        (
            With<EditorTerrainPreview>,
            Without<DynamicSceneTerrainPrimitive>,
        ),
    >,
    mut runtime_terrains: Query<
        (Entity, &mut Visibility),
        (
            With<DynamicSceneTerrainPrimitive>,
            Without<EditorTerrainPreview>,
        ),
    >,
) {
    let use_runtime_source = runtime_author.is_some()
        && session.draft.as_ref().is_some_and(|draft| {
            active.as_ref().and_then(|active| active.scene_id.as_ref()) == Some(&draft.scene_id)
        }); // 只替代当前游戏场景的预览路径。
    let should_show_author_surface = !use_runtime_source
        && editor.open
        && session.draft.as_ref().is_some_and(|draft| {
            draft.surface_dirty // 干净草稿、查看和摆放必须直接显示游戏内同一套正式PBR材质。
                && draft.terrain_mesh.is_some()
                && draft.manifest.terrain.kind == SceneTerrainKind::Gltf
                && editor.selected_scene_id.as_ref() == Some(&draft.scene_id)
                && preview.applied_scene_id.as_ref() == Some(&draft.scene_id)
                && preview.applied_revision == session.terrain_revision
                && surface_preview.desired_scene_id.as_ref() == Some(&draft.scene_id)
                && surface_preview.applied_scene_id.as_ref() == Some(&draft.scene_id)
                && surface_preview.material.is_some() // applied只在GPU完整就绪时发布；下一修订准备期间继续使用上一完整材质。
        }); // 迟到的地形、综合色或预设结果都不能遮住上一套完整正式地表。
    for (entity, mut visibility, material) in &mut preview_visibilities {
        if should_show_author_surface {
            let ready_material = surface_preview
                .material
                .as_ref()
                .expect("完整预览已核对材质"); // 只采用已经完整发布的句柄。
            if material.is_none_or(|bound| bound.0 != *ready_material) {
                commands
                    .entity(entity)
                    .remove::<MeshMaterial3d<StandardMaterial>>()
                    .insert(MeshMaterial3d(ready_material.clone())); // 清除旧纯色材质，避免同时进入两条材质渲染路径。
            } // 稳定帧不重复插入材质。
        } // 完整材质绑定后才允许隐藏正式网格。
        let target = if should_show_author_surface {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        }; // 差异化写显隐。
        if *visibility != target {
            *visibility = target; // 稳定帧不反复触发变更检测。
        } // 完成预览显隐差异写。
    } // 完成预览实体遍历。
    preview
        .hidden_runtime_entities
        .retain(|(entity, _)| runtime_terrains.contains(*entity)); // 清除已经失效的正式实体恢复目标。
    if should_show_author_surface {
        for (entity, mut visibility) in &mut runtime_terrains {
            if !preview
                .hidden_runtime_entities
                .iter()
                .any(|(hidden, _)| *hidden == entity)
            {
                preview.hidden_runtime_entities.push((entity, *visibility)); // 首次接管冻结原显隐。
            } // 完成原显隐登记。
            if *visibility != Visibility::Hidden {
                *visibility = Visibility::Hidden; // 实际预览成为F12唯一可见地形。
            } // 完成正式地形隐藏。
        } // 完成正式运行网格遍历。
    } else {
        for (entity, original) in std::mem::take(&mut preview.hidden_runtime_entities) {
            if let Ok((_, mut visibility)) = runtime_terrains.get_mut(entity)
                && *visibility != original
            {
                *visibility = original; // 关闭F12或切换场景时原样归还正式显隐。
            } // 完成有效实体恢复。
        } // 完成全部恢复目标处理。
    } // 完成接管或归还分支。
} // 结束预览显隐系统。

#[cfg(test)]
#[path = "terrain_tests.rs"]
pub(crate) mod tests; // 任意拓扑笔刷、平滑、撤销和派生行为回归。
