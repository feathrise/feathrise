//! 显式资产与传送点放置、目标选择，以及撤销系统入口。

use bevy::color::LinearRgba; // 传送点作者预览使用高可见度自发光。
use bevy::prelude::*; // ECS、GLTF、PBR预览和输入。
use bevy::ui::RelativeCursorPosition; // 左键只在编辑视口内生效。
use feathrise_map_content::{
    MAX_MAP_PLACEMENTS_PER_MAP, MapContentRegistry, MapId, MapManifestAsset,
    MapPlacementAssetTarget, MapPlacementHeightMode, MapPlacementRecord, MapPortalRecord,
}; // 新传送点名称从目标正式清单派生，编辑结果仍直接写作者结构。
use feathrise_portal::{
    PORTAL_EDGE_BEAM_HEIGHT, PORTAL_VISUAL_RADIUS,
    build_portal_edge_beam_mesh, portal_arrow_layout, portal_destination_label,
    portal_label_billboard,
};
use feathrise_editor_core::FONT_PATH as PORTAL_LABEL_FONT_PATH; // 作者预览选择编辑器自己的字库，不依赖原游戏字体策略。
use feathrise_scene::{
    ClipmapGroundHit, DynamicClipmapGroundSurface, DynamicWaterMeshOverrides, MapPlacementInstance,
    MapPlacementPrimitiveVisual, MapPlacementWaterEvidence,
}; // 预览与正式根共用水体接管、完整摆放身份及非空保存证据。 // 未保存预览直接复用正式十四米顶部渐隐直光幕规格。

use super::resources::{EditorAuthoringSession, EditorCommand, EditorPreviewAssets};
use crate::components::{EditorPlacedAsset, EditorPortalPreview, MapEditorViewport};
use crate::resources::MapEditorState; // 传送落点由玩法页显式双列表写入编辑器状态。
use crate::types::EditorTool;
use crate::ui::asset_preview::AssetPreviewState; // 摆放读取当前预览中明确选择的Scene0或primitive。
use crate::viewport::VegetationGroundCursor; // Clipmap普通摆放消费一次冻结真实三角点击。

mod undo; // 全部作者命令的撤销、重做和专用修订推进。
#[cfg(test)]
pub(crate) use undo::water_snapshot::apply as apply_water_snapshot_for_tests; // 入口回归调用正式原子历史采用路径。

pub(crate) use undo::apply_undo_redo; // 保持作者模块既有系统注册路径不变。
#[cfg(test)]
pub(crate) use undo::drive_undo_redo_to_completion; // 整组历史夹具跨帧保留后台准备Local。

/// 三维补丁记录使用关联地形的源坐标；容器的世界范围不能代替该源的切洞范围。
pub(crate) fn placement_record_bounds(
    draft: &super::resources::EditorMapDraft,
    record: &MapPlacementRecord,
) -> Result<Rect, String> {
    // 旧单主地形也必须证明来源合法，不能因没有显式对象表就把任意补丁当作世界坐标。
    super::placement_pose::placement_record_frame(draft, record)?;
    placement_record_bounds_by_source(
        draft.manifest.world_bounds().ok_or("地图世界范围无效")?,
        draft.manifest.terrains.as_deref(),
        record,
    )
}

pub(super) fn placement_record_bounds_by_source(
    world_bounds: Rect,
    objects: Option<&[feathrise_map_content::MapTerrainObject]>,
    record: &MapPlacementRecord,
) -> Result<Rect, String> {
    let Some(patch) = &record.terrain_patch else { return Ok(world_bounds); };
    let id = patch.terrain_id.clone().unwrap_or_else(feathrise_map_content::TerrainId::legacy);
    let Some(objects) = objects else {
        if id != feathrise_map_content::TerrainId::legacy() {
            return Err(format!("三维地形{}未关联有效的旧主Clipmap", record.id));
        }
        return Ok(world_bounds); // 纯范围入口没有清单类型；上层持有draft时仍须验证旧主Clipmap。
    };
    let source = objects.iter().find(|object| object.id == id
        && object.enabled && object.terrain.kind == feathrise_map_content::MapTerrainKind::SparseClipmap)
        .ok_or_else(|| format!("三维地形{}关联的Clipmap不存在或已禁用", record.id))?;
    Ok(Rect::from_corners(source.world_min.into(), source.world_max.into()))
}

/// 已确认补丁继承真实地形父级，保留非均匀缩放与局部旋转组合产生的完整仿射矩阵。
pub(crate) fn attach_patch_preview(
    commands: &mut Commands,
    draft: &super::resources::EditorMapDraft,
    terrain_queries: &feathrise_scene::TerrainObjectQueries,
    root: Entity,
    record: &MapPlacementRecord,
) -> Result<(), String> {
    let Some(id) = patch_preview_source(draft, record)? else { return Ok(()); };
    let parent = patch_preview_parent(draft, terrain_queries, record)?.expect("明确地形源已预检");
    commands.entity(root).insert((
        ChildOf(parent),
        feathrise_scene::TerrainVolumeAuthorPreviewRoot(
            feathrise_map_content::MapInstancePath::root(draft.map_id.clone()).terrain(id)),
    )); // 复用地形换代时的稳定路径转交；删除作者根不会创建或遗留额外父容器。
    Ok(())
}

pub(crate) fn patch_preview_parent(
    draft: &super::resources::EditorMapDraft,
    terrain_queries: &feathrise_scene::TerrainObjectQueries,
    record: &MapPlacementRecord,
) -> Result<Option<Entity>, String> {
    let Some(id) = patch_preview_source(draft, record)? else { return Ok(None); };
    let (parent, definition, frame) = terrain_queries.instance_source(&draft.map_id, &id)
        .ok_or("关联地形实例尚未就绪")?;
    let expected = super::placement_pose::placement_record_frame(draft, record)?;
    if definition.terrain.kind != feathrise_map_content::MapTerrainKind::SparseClipmap
        || definition.transform != expected {
        return Err("关联地形的类型或作者姿态尚未采用，请等待来源更新".into());
    } // 相同矩阵不能证明旧类型或旧定义已经采用，真实父必须同时匹配作者来源。
    if !frame.source_to_world.abs_diff_eq(expected.as_transform().to_matrix(), 0.00001) {
        return Err("关联地形的新姿态尚未采用，请等待源框架更新".into());
    } // 来源身份相同仍可能正在采用新姿态；不能让新局部数字在旧父框架下显示或进入历史。
    Ok(Some(parent))
}

pub(super) fn patch_preview_source(
    draft: &super::resources::EditorMapDraft,
    record: &MapPlacementRecord,
) -> Result<Option<feathrise_map_content::TerrainId>, String> {
    let Some(patch) = &record.terrain_patch else { return Ok(None); };
    placement_record_bounds(draft, record)?; // 范围入口同时复用启用来源与完整姿态硬门，避免旧主来源绕过校验。
    if draft.manifest.terrains.is_some() {
        return Ok(Some(patch.terrain_id.clone().unwrap_or_else(feathrise_map_content::TerrainId::legacy)));
    }
    Ok(None) // 旧单主地形是世界identity；不能借当前作者画布的父级重复叠加变换。
}

/// 为所有未保存传送点复用一套轻量预览资产。
pub(crate) fn setup_editor_preview_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>, // 作者预览启动时登记与正式传送点相同的中文字体强句柄。
) {
    commands.insert_resource(EditorPreviewAssets {
        portal_base_mesh: meshes.add(Mesh::from(Circle::new(PORTAL_VISUAL_RADIUS))), // 底盘半径与正式触发范围一致。
        portal_arrow_head_mesh: meshes.add(Mesh::from(Triangle2d::new(
            Vec2::new(0.0, 1.8),  // 箭头尖端沿局部正Y，铺地后对应作者方向。
            Vec2::new(-1.2, 0.0), // 左侧基点沿用正式箭头宽度。
            Vec2::new(1.2, 0.0),  // 右侧基点与左侧对称。
        ))),
        portal_arrow_shaft_mesh: meshes.add(Mesh::from(Rectangle::new(0.6, 1.6))), // 杆身沿用正式紧凑比例。
        portal_edge_beam_mesh: meshes.add(build_portal_edge_beam_mesh(
            PORTAL_VISUAL_RADIUS,    // 作者预览与正式触发边缘半径完全一致。
            PORTAL_EDGE_BEAM_HEIGHT, // 十四米光幕上段通过共享顶点色渐隐。
        )),
        portal_edge_ring_mesh: meshes.add(Mesh::from(Torus::new(
            2.15,                 // 环面内半径保留原有可见厚度。
            PORTAL_VISUAL_RADIUS, // 外半径与正式触发边缘保持一致。
        ))),
        portal_base_material: materials.add(StandardMaterial {
            base_color: Color::srgba(0.15, 0.15, 0.15, 0.45), // 深色透明底盘在综合色地表上衬托箭头。
            alpha_mode: AlphaMode::Blend, // 保留地形可见性并避免底盘成为实心黑块。
            double_sided: true,           // 俯视与斜视都能读取完整传送范围。
            unlit: true,                  // 作者识别不受当前天气和日照影响。
            ..default()                   // 底盘不需要纹理、金属度或真实灯光。
        }),
        portal_arrow_material: materials.add(StandardMaterial {
            base_color: Color::srgba(0.95, 0.55, 0.08, 1.0), // 中央箭头使用新建传送点的默认橙金色。
            emissive: LinearRgba::new(0.95, 0.55, 0.08, 1.0), // 无Bloom时仍保持稳定高亮。
            alpha_mode: AlphaMode::Blend,                    // 与正式箭头保持相同透明渲染路径。
            double_sided: true,                              // 铺地二维网格从任一侧观察都可见。
            unlit: true,                                     // 作者相机的光照变化不改变方向辨识。
            ..default()                                      // 箭头不创建纹理或逐帧动画资源。
        }),
        portal_edge_beam_material: materials.add(StandardMaterial {
            base_color: Color::srgba(0.95, 0.55, 0.08, 0.09), // 筒壁保持通透，中心仍可观察地形和放置物。
            emissive: LinearRgba::new(2.8, 1.05, 0.07, 1.0), // 高草遮住地面环时，上段直光仍可定位。
            alpha_mode: AlphaMode::Add,                      // 环形能量幕使用叠加混合避免透明暗边。
            double_sided: true,                              // 从传送点内外都可看到同一圈直光。
            cull_mode: None,                                 // 无盖筒壁的内外两面都必须可见。
            unlit: true,                                     // 离屏天气与太阳光不改变作者识别色。
            ..default()                                      // 不引入纹理、真实灯光或逐帧粒子。
        }),
        portal_edge_ring_material: materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 0.62, 0.08, 0.62), // 地面环比体积束更清晰地标出边界。
            emissive: LinearRgba::new(6.5, 2.2, 0.12, 1.0),  // 边缘提供醒目的橙金溢光。
            alpha_mode: AlphaMode::Add, // 草叶间的可见片段自然叠亮而不产生黑边。
            double_sided: true,         // 斜坡与俯视均可见。
            cull_mode: None,            // 显式关闭背面裁剪兑现双面环语义。
            unlit: true,                // 作者预览不依赖当前天气亮度。
            ..default()                 // 环面无需PBR高光。
        }),
        portal_label_font: feathrise_editor_core::editor_font(&asset_server, PORTAL_LABEL_FONT_PATH), // 中文标签与正式运行时共用唯一字体路径，避免预览和保存后表现分叉。
                                                                      // 地表由完整索引PBR候选接管，启动时不再建立纯色预览替身。
    });
}

pub(super) fn scene_asset_path(path: &str) -> bool {
    if path == super::basic_shapes::CUBOID_SELECTION {
        return true; // 仅编辑器当前工具接受固定内置选择令牌；落盘前必须转为空资产路径。
    }
    let lower = path.to_ascii_lowercase();
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains("..")
        && !path.contains('\\')
        && (lower.ends_with(".glb") || lower.ends_with(".gltf"))
}

pub(crate) fn next_author_id(session: &mut EditorAuthoringSession, prefix: &str) -> String {
    session.next_serial = session.next_serial.wrapping_add(1);
    // 地图重开时revision和next_serial都可能回到旧值；持久身份使用独立会话源，
    // 显示序号仍保留原行为，已有对象与历史中的稳定ID绝不重写。
    session.author_ids.next(prefix)
}

/// 解析新传送点目标地图的作者显示名；同图未保存草稿优先于正式目录资产。
fn authoring_target_display_name(
    map_id: &MapId,                        // 新点已经显式选择的目标地图稳定ID。
    session: &EditorAuthoringSession,      // 当前草稿可能正是目标地图并包含未保存更名。
    registry: Option<&MapContentRegistry>, // 其他地图通过目录强句柄定位清单。
    manifests: &Assets<MapManifestAsset>,  // Ready清单提供“前往{地图名}”唯一名称来源。
) -> Option<String> {
    if let Some(draft) = session
        .draft
        .as_ref()
        .filter(|draft| &draft.map_id == map_id)
    {
        let display_name = draft.manifest.display_name.trim(); // 同图目标使用当前作者草稿名称。
        return (!display_name.is_empty()).then(|| display_name.to_owned()); // 空名称不能生成无目的地标签。
    }
    let handles = registry?.handles(map_id)?; // 其他目标必须仍位于已经采用的地图目录。
    let manifest = manifests.get(&handles.manifest)?; // 异步尚未Ready时拒绝放置而不是写入错误流水名。
    if manifest.id != *map_id {
        return None; // 目录句柄与清单身份失配时不得借用另一张地图名称。
    }
    let display_name = manifest.display_name.trim(); // 正式目标地图名称是新传送点标签来源。
    (!display_name.is_empty()).then(|| display_name.to_owned()) // 返回短字符串快照以释放Assets借用。
}

pub(crate) fn spawn_placement_preview(
    commands: &mut Commands,
    asset_server: &AssetServer,
    map_id: &MapId,
    record: &MapPlacementRecord,
) -> Entity {
    let has_water_surfaces = !record.water_surfaces.is_empty(); // marker与覆盖数组必须来自同一作者记录。
    let mut root_commands = commands.spawn((
        Name::new(format!("未保存地图实例_{}", record.id)),
        EditorPlacedAsset,
        MapPlacementInstance::new(map_id, record), // 预览冻结MapId、文件内目标和原始变换，保存前不能借用同名旧根。
        Transform::from_translation(Vec3::from_array(record.translation))
            .with_rotation(Quat::from_rotation_y(record.yaw_degrees.to_radians()))
            .with_scale(Vec3::from_array(record.scale)),
        Visibility::Inherited, // Scene0与直接primitive都通过同一根响应属性栏显隐和变换。
    ));
    if let Some(collision) = feathrise_scene::building_placement_collision(record) {
        root_commands.insert(collision); // 未保存建筑立即使用与正式候选相同的真实网格静态碰撞。
    }
    // 普通静态三维地形的未保存预览只显示模型；真实碰撞、洞口及拾取由保存后的联合候选采用。
    // 自动雕刻预览具有独立的完整烘焙/授信协议，不经过这个普通摆放构造入口。
    let root = root_commands.id();
    match record.asset_target {
        MapPlacementAssetTarget::Cuboid { .. } | MapPlacementAssetTarget::EditableCuboid { .. } | MapPlacementAssetTarget::EditableMesh { .. } => {
            root_commands.insert(avian3d::prelude::RigidBody::Static); // 未保存立方体拥有独立静态刚体，删除或隐藏时可单独禁用。
            feathrise_scene::spawn_builtin_placement_visual(commands, root, record.asset_target);
        }
        MapPlacementAssetTarget::Scene0 => {
            root_commands.insert((
                DynamicWaterMeshOverrides::new(record.water_surfaces.clone()), // WorldReady按当前草稿节点引用安装唯一动态水材质。
                WorldAssetRoot(
                    asset_server
                        .load(GltfAssetLabel::Scene(0).from_asset(record.asset_path.clone())),
                ),
            ));
            if has_water_surfaces {
                root_commands.insert(MapPlacementWaterEvidence); // 保存等待Query只枚举全图最多256项非空水体预览。
            }
        }
        MapPlacementAssetTarget::Primitive {
            mesh, primitive, ..
        } => {
            let mesh_reference = record
                .asset_target
                .mesh_reference(&record.asset_path)
                .expect("primitive目标必须生成网格引用");
            let material_reference = record
                .asset_target
                .material_reference(&record.asset_path)
                .expect("primitive目标必须生成材质引用");
            root_commands.with_child((
                Name::new(format!("未保存子资产_Mesh{mesh}_Primitive{primitive}")),
                MapPlacementPrimitiveVisual, // 作者预览与正式实例使用同一直接子资产视口命中身份。
                Mesh3d(asset_server.load::<Mesh>(mesh_reference)), // 精确子网格句柄热更后仍保持同一标签身份。
                MeshMaterial3d(asset_server.load::<StandardMaterial>(material_reference)), // 原关联/std材质不退化为白模。
                Transform::IDENTITY, // 与右侧单体预览一致，子资产使用自己的局部网格坐标。
                Visibility::Inherited, // 跟随placement根和保存交接容器统一显隐。
            ));
        }
    }
    root
}

pub(crate) fn spawn_portal_preview(
    commands: &mut Commands,
    assets: &EditorPreviewAssets,
    record: &MapPortalRecord,
) -> Entity {
    let position = Vec3::from_array(record.position); // 作者记录已经保存当前地形命中的完整三维坐标。
    let (flat_rotation, shaft_offset) = portal_arrow_layout(record.arrow_degrees); // 与正式视觉共享顺时针换算，东/西不会在保存接管前后镜像。
    let mut label = portal_label_billboard(
        record.label.clone(),
        assets.portal_label_font.clone(), // 显式传递中文字体，中文名称和ASCII序号都由同一字库烘焙。
    ); // 预览复用正式Billboard字体、尺寸与可见距离。
    label.y_offset = 2.2; // 标签跟随预览根并悬浮在常见草层上方，保存交接期间不会消失。
    commands
        .spawn((
            Name::new(format!("未保存传送点_{}", record.id)),
            EditorPortalPreview,
            Transform::from_translation(position),
            Visibility::default(), // 根可见性向边缘环和直光幕传播并支持一次级联回收。
            label,                 // 根销毁后Billboard系统会自动释放文字实体与独占纹理资产。
        ))
        .with_children(|parent| {
            parent.spawn((
                Name::new(format!("未保存传送点底盘_{}", record.id)), // 深色范围底盘恢复中央视觉层次。
                Mesh3d(assets.portal_base_mesh.clone()),              // 所有预览共享固定半径圆盘。
                MeshMaterial3d(assets.portal_base_material.clone()),  // 共享轻量半透明底材质。
                Transform::from_translation(Vec3::Y * 0.02).with_rotation(flat_rotation), // 底盘高出地形两厘米避免闪烁。
            ));
            parent.spawn((
                Name::new(format!("未保存传送点箭头头_{}", record.id)), // 三角头恢复出口方向提示。
                Mesh3d(assets.portal_arrow_head_mesh.clone()),          // 复用正式比例的三角网格。
                MeshMaterial3d(assets.portal_arrow_material.clone()),   // 共享橙金自发光材质。
                Transform::from_translation(Vec3::Y * 0.05).with_rotation(flat_rotation), // 箭头置于底盘上方避免共面。
            ));
            parent.spawn((
                Name::new(format!("未保存传送点箭头杆_{}", record.id)), // 矩形杆与三角头组成完整箭头。
                Mesh3d(assets.portal_arrow_shaft_mesh.clone()),         // 复用固定杆身网格。
                MeshMaterial3d(assets.portal_arrow_material.clone()), // 与箭头头部保持完全一致的颜色。
                Transform::from_translation(Vec3::Y * 0.05 + shaft_offset)
                    .with_rotation(flat_rotation), // 杆身按作者方向偏移并铺到地面。
            ));
            parent.spawn((
                Name::new(format!("未保存传送点边缘光_{}", record.id)), // 作者视口中明确显示当前新点的触发边界。
                Mesh3d(assets.portal_edge_ring_mesh.clone()), // 所有未保存点共享固定环面网格。
                MeshMaterial3d(assets.portal_edge_ring_material.clone()), // 共享橙金叠加材质。
                Transform::from_translation(Vec3::Y * 0.08),  // 高出作者地形八厘米避免深度闪烁。
            ));
            parent.spawn((
                Name::new(format!("未保存传送点边缘直光_{}", record.id)), // 光幕沿传送范围边缘垂直向上。
                Mesh3d(assets.portal_edge_beam_mesh.clone()), // 复用十四米高且顶部渐隐的无盖圆柱壁。
                MeshMaterial3d(assets.portal_edge_beam_material.clone()), // 共享橙金低透明叠加材质。
                Transform::from_translation(Vec3::Y * (PORTAL_EDGE_BEAM_HEIGHT * 0.5)), // 筒壁底端从作者落点开始并在十四米顶端渐隐。
            ));
        })
        .id()
}

/// 若该点仍由未保存作者预览持有，则用更新后的完整记录原子替换预览根和Billboard请求。
pub(crate) fn refresh_owned_portal_preview(
    commands: &mut Commands,      // 旧根回收和新根创建在同一Deferred命令批完成。
    assets: &EditorPreviewAssets, // 新预览继续复用共享网格、材质和中文字体。
    session: &mut EditorAuthoringSession, // 预览所有权表决定是否属于尚未保存的新点。
    record: &MapPortalRecord,     // 更新后完整记录提供名称、位置、方向和颜色。
) {
    let Some(old_entity) = session.portal_previews.remove(&record.id) else {
        return; // 已保存正式点不属于作者预览，等待保存热重载更新正式Billboard。
    };
    commands.entity(old_entity).try_despawn(); // 回收旧文字纹理、箭头和边缘光幕完整根。
    let new_entity = spawn_portal_preview(commands, assets, record); // 新根立即使用更新后的作者名称。
    session
        .portal_previews
        .insert(record.id.clone(), new_entity); // 稳定ID继续指向唯一活动预览。
}

/// 显式坐标创建以世界中心为准，不读取隐藏视口或替作者补贴地偏移。
pub(super) fn create_cuboid_at(
    commands: &mut Commands, asset_server: &AssetServer, editor: &mut MapEditorState,
    session: &mut EditorAuthoringSession, target: MapPlacementAssetTarget, position: Vec3,
) -> Option<String> {
    if !target.is_builtin() || target.validate().is_err() || !position.is_finite() {
        return None;
    }
    let draft = session.draft.as_ref()?;
    if draft.placements.placements.len() >= MAX_MAP_PLACEMENTS_PER_MAP {
        editor.status = format!("当前地图已达到{MAX_MAP_PLACEMENTS_PER_MAP}个显式资产上限");
        return None;
    }
    let map = draft.map_id.clone();
    if editor.selected_map_id.as_ref() != Some(&map) { return None; }
    let id = next_author_id(session, "placement");
    let mut record = MapPlacementRecord {
        mesh_lod: None, terrain_binding: None, editor: Default::default(), terrain_patch: None,
        id: id.clone(), asset_path: String::new(), asset_target: target,
        building_preset_id: None, courtyard: None, translation: position.to_array(),
        height_mode: MapPlacementHeightMode::FixedHeight, terrain_offset: 0.0,
        yaw_degrees: 0.0, scale: [1.0; 3], water_surfaces: Vec::new(),
    };
    let ordinal = session.next_serial.saturating_sub(1);
    std::sync::Arc::make_mut(&mut record.editor).name = if ordinal == 0 { "立方体".into() }
        else { format!("立方体.{ordinal:03}") };
    let entity = spawn_placement_preview(commands, asset_server, &map, &record);
    session.draft.as_mut()?.placements.placements.push(record.clone());
    session.placement_previews.insert(id.clone(), entity);
    session.undo.push(EditorCommand::PlacementAdded(record));
    session.redo.clear();
    session.mark_placement_structure_changed(editor);
    editor.tool = EditorTool::Inspect;
    editor.selected_asset = None;
    editor.placement_transform_enabled = false;
    editor.status = "已按中心坐标创建未保存立方体；可通过撤销取消".into();
    Some(id)
}

/// 左键视口放置当前模型或传送点；保存任务期间拒绝新修改。
#[allow(clippy::too_many_arguments)]
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct PlacementViewportContext<'w> {
    mesh_snap: Option<ResMut<'w, crate::viewport::snapping::MeshSnapCursor>>,
    shapes: Option<Res<'w, super::basic_shapes::BasicShapeState>>,
    browser: Option<ResMut<'w, crate::ui::gltf_browser::EditorGltfBrowserState>>,
    sidebars: Option<ResMut<'w, crate::ui::sidebars::MapEditorSidebarState>>,
    selection: Option<ResMut<'w, crate::resources::EditorViewportSelectionState>>,
}

pub(crate) fn place_from_viewport(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    viewport: Query<&RelativeCursorPosition, With<MapEditorViewport>>,
    asset_server: Res<AssetServer>,
    preview_assets: Res<EditorPreviewAssets>,
    registry: Option<Res<MapContentRegistry>>, // 新传送点名称通过当前目录解析目标地图清单。
    manifests: Res<Assets<MapManifestAsset>>,  // 目标地图显示名必须已经异步Ready。
    mut editor: ResMut<MapEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    asset_preview: Res<AssetPreviewState>, // 当前模型子列表选择只在真正落点时转入作者记录。
    transform_state: Res<crate::ui::placement_transform::PlacementTransformInputState>, // 移动模式必须独占下一次视口左键。
    mut ground_cursor: Option<ResMut<VegetationGroundCursor>>, // 独立最小World可缺省；Clipmap点击完成后由本系统唯一消费一次。
    clipmap_surface: Option<Res<DynamicClipmapGroundSurface>>, // 采用时再次核对同图同高度页身份。
    terrain_cursor: Option<Res<super::terrain_ground::TerrainGroundCursor>>,
    mut mode: Option<ResMut<crate::viewport::modes::ViewportModeState>>, // 浮层操作不能消费背后的异步点击。
    mut context: PlacementViewportContext,
) {
    let mesh_snap = &mut context.mesh_snap;
    if !matches!(editor.tool, EditorTool::PlaceAsset | EditorTool::Portal) {
        return; // 建筑等工具独占自己的点击和诊断，异步查询期间不能被通用入口误报平面未命中。
    }
    let clicked_in_viewport = mouse.just_pressed(MouseButton::Left)
        && viewport.single().is_ok_and(|cursor| cursor.cursor_over());
    if !editor.open
        || editor.new_map_dialog_open
        || editor.close_requested
        || editor.save_requested
        || editor.suppress_world_pick_this_frame // 取消/保存同帧不得把按钮左键解释为摆放落点。
        || session.saving
        || transform_state.relocation_armed()
        || mode.as_ref().is_some_and(|mode| mode.pointer_over_controls)
    {
        if matches!(editor.tool, EditorTool::Portal | EditorTool::PlaceAsset)
            && let Some(ground) = ground_cursor.as_mut()
        {
            match editor.tool {
                EditorTool::Portal => ground.cancel_portal_click(),
                _ => ground.cancel_placement_click(),
            }
        }
        return; // 模态覆盖视口时，RelativeCursorPosition仍可报告hover，不能把弹框点击解释为GLB摆放。
    }
    if !clicked_in_viewport && session.draft.is_none() {
        return; // 没有草稿时也没有可在后续帧采用的Clipmap点击，稳定帧不得覆盖状态栏。
    }
    let selected_map = editor.selected_map_id.clone();
    let Some((draft_map, terrain_kind)) = session
        .draft
        .as_ref()
        .map(|draft| (draft.map_id.clone(), draft.manifest.terrain.kind))
    else {
        editor.status = if editor.cursor_world.is_none() {
            "视口光标没有命中地形基准面".to_owned() // 保持既有同步放置的失败优先级。
        } else {
            "地图作者数据尚未就绪".to_owned()
        };
        return;
    };
    if selected_map.as_ref() != Some(&draft_map) {
        if let Some(ground) = ground_cursor.as_mut() {
            match editor.tool {
                EditorTool::Portal => ground.cancel_portal_click(),
                _ => ground.cancel_placement_click(),
            }
        }
        editor.status = "正在切换地图作者数据，请稍候".to_owned();
        return;
    }
    let cuboid_target = if super::basic_shapes::is_cuboid_selection(&editor) {
        let Some(target) = context.shapes.as_deref().and_then(|state| state.placement_target(&session)) else {
            return; // 参数和地图修订在开始点放时冻结，迟到真实地面结果不能替新地图创建。
        };
        Some(target)
    } else { None };
    let mut clipmap_asset_identity = None; // 完成帧复用点击时冻结的文件与子资产身份。
    let object_map = session.draft.as_ref().is_some_and(|draft| draft.manifest.terrains.is_some());
    let mut object_terrain_id = None; // 作者绑定只消费本次单击冻结的对象，不能读取完成帧的悬停对象。
    let work_plane = super::basic_shapes::is_cuboid_selection(&editor)
        && super::basic_shapes::work_plane_is_available(&session);
    let position = if work_plane {
        if !clicked_in_viewport { return; }
        let Some(point) = context.shapes.as_deref().and_then(|state| state.work_plane_point(&session)) else {
            editor.status = "当前视角未命中工作平面，请调整视角后放置".into();
            return;
        };
        point // 仅零地形地图的基础形状使用显式Y=0平面，不向真实承托查询发布伪命中。
    } else if object_map {
        if editor.tool == EditorTool::PlaceAsset && mesh_snap.as_ref().is_some_and(|snap| snap.requires_query()) {
            let target = match editor.tool {
                EditorTool::PlaceAsset => {
                    let Some(path) = editor.selected_asset.as_ref() else { return; };
                    let Ok(asset_target) = asset_preview.placement_target_for(path) else { return; };
                    crate::viewport::vegetation_ground::GroundCursorTarget::PlaceAsset {
                        asset_path: path.clone(), asset_target,
                    }
                }
                EditorTool::Portal => {
                    let Some(target) = editor.portal_target.clone() else { return; };
                    crate::viewport::vegetation_ground::GroundCursorTarget::Portal(target)
                }
                _ => unreachable!("入口只接受资产与传送点"),
            };
            object_terrain_id = mesh_snap.as_ref().and_then(|snap| snap.frozen_terrain()).cloned();
            let completion = mesh_snap.as_mut().and_then(|snap| snap.take_click(&draft_map, &target, session.revision, session.terrain_revision));
            if completion.is_some() {
                editor.ground_click_world = None; // 原点只用于吸附确认；采用后不得在下一帧重复冻结或放置。
            }
            match completion {
                Some(ClipmapGroundHit::Hit(point)) => point,
                Some(_) => { editor.status = "吸附网格位置没有命中所选地形的连续承托面，请换一个落点".into(); return; }
                None => { if clicked_in_viewport { editor.status = "正在核验对象吸附位置的真实地形…".into(); } return; }
            }
        } else {
            let Some(position) = editor.ground_click_world.take() else {
                if clicked_in_viewport { editor.status = "正在后台查询真实地形表面，命中后完成放置".into(); }
                return;
            };
            object_terrain_id = terrain_cursor.as_ref().and_then(|cursor| cursor.click_terrain.as_ref()).cloned();
            position
        }
    } else if editor.tool == EditorTool::PlaceAsset
        && terrain_kind == feathrise_map_content::MapTerrainKind::SparseClipmap
    {
        let Some(asset_path) = editor.selected_asset.clone() else {
            if clicked_in_viewport {
                editor.status = "请先在右侧选择GLB或GLTF模型".to_owned();
            }
            return;
        };
        if !scene_asset_path(&asset_path) {
            if clicked_in_viewport {
                editor.status = "当前选择不是可放置的GLB/GLTF场景".to_owned();
            }
            return;
        }
        let asset_target = match asset_preview.placement_target_for(&asset_path) {
            Ok(target) => target,
            Err(error) => {
                editor.status = format!("当前子资产不能摆放：{error}");
                return;
            }
        };
        let completion = clipmap_surface
            .as_ref()
            .and_then(|surface| surface.snapshot.as_ref())
            .and_then(|snapshot| {
                ground_cursor.as_mut()?.take_place_asset_click_result(
                    snapshot,
                    &draft_map,
                    &asset_path,
                    asset_target,
                    session.revision,
                )
            }); // 无完成值时保持等待；绝不读取悬停旧点或零平面。
        let Some(completion) = completion else {
            if clicked_in_viewport {
                editor.status = "正在后台查询Clipmap真实地表；命中后自动完成摆放".to_owned();
            }
            return;
        };
        let ClipmapGroundHit::Hit(position) = completion else {
            editor.status = match completion {
                ClipmapGroundHit::WaitingForHeight => {
                    "摆放位置的Clipmap高度页尚未就绪，请稍后重新单击".to_owned()
                }
                ClipmapGroundHit::Miss => "本次单击没有命中地图内真实地表".to_owned(),
                ClipmapGroundHit::Failed => "本次Clipmap地表射线无效，请重新单击".to_owned(),
                ClipmapGroundHit::Hit(_) => unreachable!("上方模式已经提取成功命中"),
            };
            return;
        };
        clipmap_asset_identity = Some((asset_path, asset_target)); // 创建记录必须与查询目标逐项相同。
        position
    } else if editor.tool == EditorTool::Portal
        && terrain_kind == feathrise_map_content::MapTerrainKind::SparseClipmap
    {
        let Some(target) = editor.portal_target.clone() else {
            if clicked_in_viewport {
                editor.status = "请先在目标地图落点列表选择默认起始位置或具体传送点".to_owned();
            }
            if let Some(ground) = ground_cursor.as_mut() {
                ground.cancel_portal_click();
            }
            return;
        };
        let completion = clipmap_surface
            .as_ref()
            .and_then(|surface| surface.snapshot.as_ref())
            .and_then(|snapshot| {
                ground_cursor.as_mut()?.take_author_click_result(
                    snapshot,
                    &draft_map,
                    crate::viewport::vegetation_ground::GroundCursorTarget::Portal(target),
                    session.revision,
                )
            });
        match completion {
            Some(ClipmapGroundHit::Hit(position)) => position,
            Some(_) => {
                editor.status = "本次单击没有取得有效Clipmap地表，请重新单击放置传送点".to_owned();
                return;
            }
            None => {
                if clicked_in_viewport {
                    editor.status =
                        "正在查询Clipmap真实地表；松键后可完成一次传送点放置".to_owned();
                }
                return; // 无完成值时不创建预览、作者记录、稳定ID或撤销命令。
            }
        }
    } else if editor.tool == EditorTool::PlaceAsset && mesh_snap.as_ref().is_some_and(|snap| snap.requires_query()) {
        let Some(path) = editor.selected_asset.as_ref() else { return; };
        let Ok(asset_target) = asset_preview.placement_target_for(path) else { return; };
        let target = crate::viewport::vegetation_ground::GroundCursorTarget::PlaceAsset { asset_path: path.clone(), asset_target };
        match mesh_snap.as_mut().and_then(|snap| snap.take_click(&draft_map, &target, session.revision, session.terrain_revision)) {
            Some(ClipmapGroundHit::Hit(point)) => point,
            Some(_) => { editor.status = "吸附网格位置没有命中当前地形承托层，请换一个落点".into(); return; }
            None => { if clicked_in_viewport { editor.status = "正在核验吸附网格位置的真实地形…".into(); } return; }
        }
    } else {
        if !clicked_in_viewport {
            return; // GLB资产与传送点继续只消费同步点击边沿。
        }
        let Some(position) = editor.cursor_world else {
            editor.status = "视口光标没有命中地形基准面".to_owned();
            return;
        };
        position
    };
    match editor.tool {
        EditorTool::PlaceAsset => {
            let Some(asset_path) = clipmap_asset_identity
                .as_ref()
                .map(|(path, _)| path.clone())
                .or_else(|| editor.selected_asset.clone())
            else {
                editor.status = "请先在右侧选择GLB或GLTF模型".to_owned();
                return;
            };
            if !scene_asset_path(&asset_path) {
                editor.status = "当前选择不是可放置的GLB/GLTF场景".to_owned();
                return;
            }
            let asset_target = if let Some(target) = cuboid_target {
                target // 地面查询用固定选择令牌，正式记录和预览采用冻结的完整形状参数。
            } else if let Some((_, target)) = clipmap_asset_identity {
                target // 异步查询已经冻结并再次核对该持久子资产身份。
            } else {
                match asset_preview.placement_target_for(&asset_path) {
                    Ok(target) => target,
                    Err(error) => {
                        editor.status = format!("当前子资产不能摆放：{error}");
                        return; // 超出持久格式的异常编号不能被截断或猜成默认材质。
                    }
                }
            };
            if session.draft.as_ref().is_some_and(|draft| {
                draft.placements.placements.len() >= MAX_MAP_PLACEMENTS_PER_MAP
            }) {
                editor.status = format!(
                    "当前地图已达到{MAX_MAP_PLACEMENTS_PER_MAP}个显式资产上限，请改用植被系统处理海量重复内容"
                );
                return; // 新放置与复制共用正式运行时数量硬门。
            }
            let id = next_author_id(&mut session, "placement");
            // 三维命中保留完整Y；不能把洞内地板或壁面重新解释为上方的高度面。
            let mesh_ground=terrain_kind==feathrise_map_content::MapTerrainKind::SparseClipmap
                && clipmap_surface.as_deref().and_then(|surface|surface.snapshot.as_ref()).is_some_and(|snapshot| {
                    !matches!(snapshot.sample_authoritative_surface(position.xz()),feathrise_scene::ClipmapGroundHit::Hit(hit) if (hit.y-position.y).abs()<0.001)
                });
            let height_mode = if !work_plane && !mesh_ground && !(object_map && editor.ground_click_is_lower_layer)
                && (object_map || terrain_kind == feathrise_map_content::MapTerrainKind::SparseClipmap
                    || session
                        .draft
                        .as_ref()
                        .is_some_and(|draft| draft.heights.is_some()))
            {
                MapPlacementHeightMode::FollowTerrain // 两种高度路线的新资产都从真实命中以零偏移跟随。
            } else {
                MapPlacementHeightMode::FixedHeight // 旧GLTF场景没有权威高度源，只能安全保存当前世界Y。
            };
            let mut record = MapPlacementRecord {
                mesh_lod: None,
                terrain_binding: None,
                editor: Default::default(),
                terrain_patch: None,
                id: id.clone(),                   // 新实例稳定作者身份。
                asset_path: if asset_target.is_builtin() { String::new() } else { asset_path }, // 内置令牌不进入地图协议或AssetServer。
                asset_target, // Scene0或当前子列表明确选择的Mesh/Primitive/Material组合。
                building_preset_id: None, // 普通“摆资产”入口不伪装成建筑预设实例。
                courtyard: None, // 独立摆放记录不属于任何院落副本。
                translation: position.to_array(), // 初始世界原点使用本帧精确地形命中。
                height_mode,  // 有高度场时默认跟随，旧场景地形安全固定。
                terrain_offset: 0.0, // 新落点默认原点贴地。
                yaw_degrees: 0.0, // 初始不旋转，随后可在属性栏编辑。
                scale: Vec3::ONE.to_array(), // 初始单位三轴缩放。
                water_surfaces: Vec::new(), // Blender 网格只有经过右栏显式赋水后才接入动态水材质。
            };
            if let MapPlacementAssetTarget::Cuboid { size_mm, .. } = asset_target {
                let half_height = size_mm[1] as f32 * 0.0005;
                record.translation[1] += half_height;
                record.terrain_offset = if height_mode.follows_terrain() { half_height } else { 0.0 };
                // Cuboid局部原点在中心；初次放置底面贴地，后续尺寸与整体变换保持独立。
                let ordinal = session.next_serial.saturating_sub(1);
                std::sync::Arc::make_mut(&mut record.editor).name = if ordinal == 0 {
                    "立方体".to_owned()
                } else { format!("立方体.{ordinal:03}") };
                // 名称仅供显示，唯一身份使用作者ID；不为递增显示名同步扫描整张地图。
            }
            if object_map && !work_plane {
                let binding=object_terrain_id.as_ref()
                    .and_then(|id|session.draft.as_ref()?.terrain_ground.as_ref().map(|ground|(id,ground)))
                    .map(|(id,ground)|ground.bind(id,feathrise_map_content::TerrainObjectTransform {
                        translation:record.translation,yaw_degrees:record.yaw_degrees,scale:record.scale,
                        ..Default::default()
                    })).transpose();
                match binding {Ok(Some(binding))=>record.terrain_binding=Some(binding),_=> {editor.status="承托地形身份已变化，请重新单击放置".into();return;} }
            }
            let entity = spawn_placement_preview(&mut commands, &asset_server, &draft_map, &record);
            let placed_target_label = record.asset_target.label(); // 历史命令取得记录所有权前冻结短反馈。
            let Some(draft) = session.draft.as_mut() else {
                return;
            };
            draft.placements.placements.push(record.clone());
            session.placement_previews.insert(id.clone(), entity);
            session.undo.push(EditorCommand::PlacementAdded(record));
            session.redo.clear();
            session.mark_placement_structure_changed(&mut editor); // 新实例根必须换代右侧GLB浏览结构。
            if asset_target.is_builtin() {
                if let Some(browser) = context.browser.as_deref_mut() {
                    browser.select_primitive_placement_from_viewport(id);
                }
                if let Some(selection) = context.selection.as_deref_mut() { selection.clear_vegetation_selection(); }
                if let Some(sidebars) = context.sidebars.as_deref_mut() { sidebars.reveal_scene_properties(); }
                editor.tool = EditorTool::Inspect; // 单次创建完成归还选择工具，释放或后续按键不会重复新增。
                if let Some(mode) = mode.as_deref_mut() { mode.mode = crate::viewport::modes::ViewportMode::Select; }
                editor.placement_transform_enabled = false;
                editor.selected_asset = None;
            }
            editor.status = format!(
                "已放置未保存{}；可通过撤销取消",
                placed_target_label,
            );
        }
        EditorTool::Portal => {
            let Some(target) = editor.portal_target.clone() else {
                editor.status = "请先在目标地图落点列表选择默认起始位置或具体传送点".to_owned(); // 双列表显式选择替代盲目循环目标。
                return;
            };
            let Some(target_display_name) = authoring_target_display_name(
                &target.map_id,      // 只解析刚才显式选择的目标地图。
                &session,            // 同图草稿名称优先。
                registry.as_deref(), // 其他地图读取正式目录。
                &manifests,          // 目标清单必须Ready且身份一致。
            ) else {
                editor.status = "目标地图名称尚未就绪，暂不能放置传送点".to_owned(); // 不再退化生成“传送点_N”。
                return;
            };
            let id = next_author_id(&mut session, "portal");
            let arrow_degrees = 0.0; // 新点先使用中性方向；有界后台外框派生完成后自动更新。
            let mut record = MapPortalRecord {
                terrain_binding: None,
                arrival_terrain_binding: None,
                id: id.clone(),
                label: portal_destination_label(&target_display_name), // 新作者数据直接保存用户确认的“前往{目标地图显示名}”。
                label_customized: false, // 默认名继续跟随目标地图改名，作者显式编辑后才冻结区域名称。
                position: position.to_array(),
                arrival: (position + Vec3::X * 4.0).to_array(),
                target,
                color_srgba: [0.95, 0.55, 0.08, 1.0],
                arrow_degrees,
            };
            if object_map {
                let binding=object_terrain_id.as_ref()
                    .and_then(|id|session.draft.as_ref()?.terrain_ground.as_ref().map(|ground|(id,ground)))
                    .map(|(id,ground)|ground.bind(id,feathrise_map_content::TerrainObjectTransform {
                        translation:record.position,yaw_degrees:record.arrow_degrees,scale:[1.;3],
                        ..Default::default()
                    })).transpose();
                match binding {Ok(Some(binding))=>record.terrain_binding=Some(binding),_=> {editor.status="传送点承托身份已变化，请重新单击放置".into();return;} }
                // 到达点后续在有界后台任务验证并绑定；小岛之外的四米偏移不能伪装成安全地面。
            }
            let entity = spawn_portal_preview(&mut commands, &preview_assets, &record);
            let Some(draft) = session.draft.as_mut() else {
                return;
            };
            draft.portals.portals.push(record.clone());
            session.portal_previews.insert(id.clone(), entity); // 保留稳定ID供当前地图列表立即高亮新源点。
            session.undo.push(EditorCommand::PortalAdded(record));
            session.redo.clear();
            editor.selected_current_portal_id = Some(id); // 新建后当前地图列表立即高亮刚放置的源点。
            session.mark_portals_changed(&mut editor); // 只换代传送列表与统一保存修订。
            editor.status = "已放置未保存传送点；保存后正式传送系统会热更新".to_owned();
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use bevy::asset::AssetPlugin; // 最小系统回归使用真实AssetServer参数而不加载模型。
    use bevy::asset::uuid::Uuid; // 构造不依赖AssetServer的非默认字体句柄以验证真实传递链。
    use bevy::ecs::system::RunSystemOnce; // 在最小World中执行真实Commands生成路径。
    use feathrise_billboard_text::BillboardText; // 直接断言预览根没有再次漏掉文字请求。
    use feathrise_map_content::MapPortalTarget; // 构造完整且合法的跨地图目标记录。

    use super::*; // 场景资产路径校验保持在放置模块。

    #[test]
    fn unsaved_static_patch_preview_has_identity_but_no_formal_terrain_collision() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<bevy::world_serialization::WorldAsset>();
        let map = MapId::parse("MAP-unsaved-static-patch").unwrap();
        let record: MapPlacementRecord = serde_json::from_value(serde_json::json!({
            "id":"cave", "asset_path":"gltf/cave.glb", "translation":[0,3,0],
            "yaw_degrees":0.0, "scale":[1.0,1.0,1.0],
            "terrain_patch":{"cutout":{"min":[-2,-2],"max":[2,2]}}
        })).unwrap();
        let root = app.world_mut().run_system_once(move |mut commands: Commands, assets: Res<AssetServer>| {
            spawn_placement_preview(&mut commands, &assets, &map, &record)
        }).unwrap();
        assert!(app.world().get::<MapPlacementInstance>(root).unwrap().is_terrain_patch());
        assert!(app.world().get::<feathrise_scene::TerrainPatchCollisionRequest>(root).is_none());
        assert!(app.world().get::<avian3d::prelude::Collider>(root).is_none());
        assert_eq!(app.world().get::<Transform>(root).unwrap().translation, Vec3::new(0.0, 3.0, 0.0));
    }

    #[test]
    fn arbitrary_asset_selection_still_rejects_non_scene_files_for_placement() {
        assert!(scene_asset_path("gltf/树.glb"));
        assert!(!scene_asset_path("textures/树.png"));
        assert!(!scene_asset_path("../tree.glb"));
    }

    #[test]
    fn generic_asset_placement_does_not_override_other_tools_missing_cursor() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<MapManifestAsset>()
            .init_resource::<AssetPreviewState>()
            .init_resource::<crate::ui::placement_transform::PlacementTransformInputState>()
            .init_resource::<ButtonInput<MouseButton>>()
            .insert_resource(EditorPreviewAssets {
                portal_base_mesh: Handle::default(),
                portal_arrow_head_mesh: Handle::default(),
                portal_arrow_shaft_mesh: Handle::default(),
                portal_edge_beam_mesh: Handle::default(),
                portal_edge_ring_mesh: Handle::default(),
                portal_base_material: Handle::default(),
                portal_arrow_material: Handle::default(),
                portal_edge_beam_material: Handle::default(),
                portal_edge_ring_material: Handle::default(),
                portal_label_font: Handle::default(),
            })
            .init_resource::<EditorAuthoringSession>()
            .init_resource::<MapEditorState>()
            .add_systems(Update, place_from_viewport);
        app.world_mut().spawn((
            MapEditorViewport,
            RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::ZERO),
            },
        ));
        for tool in [
            EditorTool::Building,
            EditorTool::Domain("fixture.domain"),
            EditorTool::Boundary,
        ] {
            for loaded in [false, true] {
                app.world_mut()
                    .resource_mut::<EditorAuthoringSession>()
                    .draft = loaded.then(|| {
                    let mut draft =
                        crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121]));
                    draft.manifest.terrain.kind = feathrise_map_content::MapTerrainKind::SparseClipmap;
                    draft
                });
                let map_id = app
                    .world()
                    .resource::<EditorAuthoringSession>()
                    .draft
                    .as_ref()
                    .map(|draft| draft.map_id.clone());
                *app.world_mut().resource_mut::<MapEditorState>() = MapEditorState {
                    open: true,
                    tool,
                    selected_map_id: map_id,
                    cursor_world: None, // 按下换代时精确射线尚在后台，不能被通用入口误报平面未命中。
                    status: "正在等待当前工具的精确落点".into(),
                    ..default()
                };
                let mut mouse = ButtonInput::<MouseButton>::default();
                mouse.press(MouseButton::Left);
                app.insert_resource(mouse);
                app.update();
                assert_eq!(
                    app.world().resource::<MapEditorState>().status,
                    "正在等待当前工具的精确落点"
                );
                let session = app.world().resource::<EditorAuthoringSession>();
                assert!(session.undo.is_empty() && session.placement_previews.is_empty());
            }
        }
    }

    #[test]
    fn clipmap_portal_waits_without_using_generic_cursor_or_creating_history() {
        let mut draft = crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121]));
        draft.manifest.terrain.kind = feathrise_map_content::MapTerrainKind::SparseClipmap;
        let map_id = draft.map_id.clone();
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<MapManifestAsset>()
            .init_resource::<AssetPreviewState>()
            .init_resource::<crate::ui::placement_transform::PlacementTransformInputState>()
            .init_resource::<VegetationGroundCursor>()
            .init_resource::<DynamicClipmapGroundSurface>()
            .init_resource::<crate::viewport::modes::ViewportModeState>()
            .insert_resource(EditorAuthoringSession {
                draft: Some(draft),
                ..default()
            })
            .insert_resource(MapEditorState {
                open: true,
                tool: EditorTool::Portal,
                selected_map_id: Some(map_id.clone()),
                portal_target: Some(MapPortalTarget::default_spawn(map_id)),
                cursor_world: Some(Vec3::new(4., 400., 4.)),
                ..default()
            })
            .insert_resource(EditorPreviewAssets {
                portal_base_mesh: Handle::default(),
                portal_arrow_head_mesh: Handle::default(),
                portal_arrow_shaft_mesh: Handle::default(),
                portal_edge_beam_mesh: Handle::default(),
                portal_edge_ring_mesh: Handle::default(),
                portal_base_material: Handle::default(),
                portal_arrow_material: Handle::default(),
                portal_edge_beam_material: Handle::default(),
                portal_edge_ring_material: Handle::default(),
                portal_label_font: Handle::default(),
            })
            .init_resource::<ButtonInput<MouseButton>>()
            .add_systems(Update, place_from_viewport);
        app.world_mut().spawn((
            MapEditorViewport,
            RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::ZERO),
            },
        ));
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        app.update();
        let session = app.world().resource::<EditorAuthoringSession>();
        assert!(session.draft.as_ref().unwrap().portals.portals.is_empty());
        assert!(session.portal_previews.is_empty() && session.undo.is_empty());
        assert_eq!(session.revision, 0); // 旧粗点或平面不能分配作者ID、创建预览或改变修订。
    }

    #[test]
    fn new_map_dialog_click_does_not_trigger_underlying_asset_tool() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<MapManifestAsset>()
            .init_resource::<EditorAuthoringSession>()
            .init_resource::<AssetPreviewState>()
            .init_resource::<crate::ui::placement_transform::PlacementTransformInputState>()
            .insert_resource(MapEditorState {
                open: true,
                new_map_dialog_open: true,
                tool: EditorTool::PlaceAsset,
                status: "请输入地图名称和尺寸".to_owned(),
                ..default()
            })
            .insert_resource(EditorPreviewAssets {
                portal_base_mesh: Handle::default(),
                portal_arrow_head_mesh: Handle::default(),
                portal_arrow_shaft_mesh: Handle::default(),
                portal_edge_beam_mesh: Handle::default(),
                portal_edge_ring_mesh: Handle::default(),
                portal_base_material: Handle::default(),
                portal_arrow_material: Handle::default(),
                portal_edge_beam_material: Handle::default(),
                portal_edge_ring_material: Handle::default(),
                portal_label_font: Handle::default(),
            })
            .add_systems(Update, place_from_viewport);
        app.world_mut().spawn((
            MapEditorViewport,
            RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::ZERO),
            },
        )); // 弹框正好盖在视口上时，底层相对位置仍可报告hover。
        let mut mouse = ButtonInput::<MouseButton>::default();
        mouse.press(MouseButton::Left);
        app.insert_resource(mouse);

        app.update();
        assert_eq!(
            app.world().resource::<MapEditorState>().status,
            "请输入地图名称和尺寸"
        ); // 模态点击不能覆盖创建提示，更不能要求选择GLB。

        app.world_mut()
            .resource_mut::<MapEditorState>()
            .new_map_dialog_open = false;
        let mut mouse = ButtonInput::<MouseButton>::default();
        mouse.press(MouseButton::Left);
        app.insert_resource(mouse);
        app.update();
        assert_eq!(
            app.world().resource::<MapEditorState>().status,
            "视口光标没有命中地形基准面"
        ); // 同一视口落点在无模态时仍由正式放置系统处理。
    }

    #[test]
    fn portal_preview_keeps_center_arrow_and_label_during_handoff() {
        let portal_label_font = Handle::<Font>::from(Uuid::from_u128(0x504f5254414c5f464f4e54)); // 稳定非默认UUID让回归测试能识别字体是否被丢弃。
        let assets = EditorPreviewAssets {
            portal_base_mesh: Handle::default(), // 测试只验证实体结构，不需要登记真实GPU网格。
            portal_arrow_head_mesh: Handle::default(), // 默认句柄足以生成箭头头部组件。
            portal_arrow_shaft_mesh: Handle::default(), // 默认句柄足以生成箭头杆身组件。
            portal_edge_beam_mesh: Handle::default(), // 默认句柄保留边缘光幕结构。
            portal_edge_ring_mesh: Handle::default(), // 默认句柄保留地面边缘环结构。
            portal_base_material: Handle::default(), // 测试无需实际PBR材质资产。
            portal_arrow_material: Handle::default(), // 箭头头部与杆身共享占位句柄。
            portal_edge_beam_material: Handle::default(), // 光幕材质只需组件身份。
            portal_edge_ring_material: Handle::default(), // 光环材质只需组件身份。
            portal_label_font: portal_label_font.clone(), // 预览生产路径必须原样带入这个中文字体占位身份。
                                                          // 地表PBR候选由独立预览资源持有，不属于传送标记夹具。
        };
        let record = MapPortalRecord {
            terrain_binding: None,
            arrival_terrain_binding: None,
            id: "portal-preview".to_owned(), // 稳定ID用于断言全部预览子节点名称。
            label: "前往黄河古道".to_owned(), // 可辨识中文确保真实标签文本进入Billboard请求。
            label_customized: true,          // 测试固定名称必须作为作者覆盖进入预览。
            position: [12.0, 3.0, -8.0],     // 非零世界坐标验证预览根承载完整作者位置。
            arrival: [16.0, 3.0, -8.0],      // 安全到达点不影响当前视觉结构。
            target: MapPortalTarget::default_spawn(
                MapId::parse("MAP-target").expect("测试目标MapId必须合法"), // 默认出生目标仍是完整传送记录。
            ),
            color_srgba: [0.95, 0.55, 0.08, 1.0], // 使用正式新建传送点默认橙金色。
            arrow_degrees: 90.0,                  // 非零方向确保预览执行真实旋转计算路径。
        };
        let mut world = World::new(); // 最小ECS世界即可验证同步固定规模生成。
        let root = world
            .run_system_once(move |mut commands: Commands| {
                spawn_portal_preview(&mut commands, &assets, &record) // 执行生产代码而非复制测试结构。
            })
            .expect("传送点作者预览应能在最小World中生成");

        let billboard = world
            .get::<BillboardText>(root)
            .expect("未保存传送点预览不能只剩边缘光而丢失目标文字"); // 保存交接期间根必须继续驱动目标文字Billboard。
        assert_eq!(billboard.text, "前往黄河古道"); // 中文源文本不能在预览构造阶段发生编码替换。
        assert_eq!(billboard.font, portal_label_font); // 预览必须携带显式中文字体，不能回退到产生方框的默认字体。
        let children = world
            .entity(root)
            .get::<Children>()
            .expect("完整传送点预览必须拥有五个固定视觉子节点")
            .iter()
            .collect::<Vec<_>>(); // 复制固定五项身份后释放World借用。
        let child_names = children
            .iter()
            .map(|child| {
                world
                    .entity(*child)
                    .get::<Name>()
                    .expect("每个传送预览子节点都必须具备可诊断名称")
                    .as_str()
                    .to_owned() // 独立字符串让后续断言不持续借用World。
            })
            .collect::<Vec<_>>(); // 固定五项集合不会随地图内容规模增长。

        assert_eq!(children.len(), 5); // 底盘、箭头头、箭头杆、边缘环和光幕缺一不可。
        assert!(
            child_names
                .iter()
                .any(|name| name.starts_with("未保存传送点箭头头_")), // 中央三角箭头头必须存在。
            "未保存传送点预览不能丢失中央箭头头"
        );
        assert!(
            child_names
                .iter()
                .any(|name| name.starts_with("未保存传送点箭头杆_")), // 中央箭头杆必须存在。
            "未保存传送点预览不能丢失中央箭头杆"
        );
    }
}
