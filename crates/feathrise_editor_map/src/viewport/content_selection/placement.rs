//! 显式Scene0网格与直接primitive摆放在中央视口中的稳定命中和选择发布。
//!
//! 【功能设计】Scene0继续恢复Blender节点路径与网格名，支持精确高亮、定位和整网格水；
//! 直接primitive没有这套层级，只通过专用可视标记恢复所属placement根，让整条记录可变换、
//! 复制和删除，同时不伪造节点路径、不触发Scene0加载，也不开放只属于完整场景的水绑定。

use bevy::gltf::GltfMeshName; // Scene0网格最终身份使用Blender导出的Mesh datablock名称。
use bevy::prelude::*; // Entity、Query、层级组件和资产句柄。
use feathrise_map_content::{
    MAX_MAP_PLACEMENT_WATER_NAME_CHARS, MAX_MAP_PLACEMENT_WATER_NODE_PATH_CHARS,
    MAX_MAP_PLACEMENT_WATER_NODE_PATH_DEPTH, MapId, MapPlacementAssetTarget,
}; // Scene0引用服从正式持久身份硬门，primitive只读取冻结数字目标。
use feathrise_scene::{MapPlacementInstance, MapPlacementPrimitiveVisual}; // 活动根与直接子资产可视标记共同恢复稳定记录。

use crate::resources::{EditorViewportSelectionState, MapEditorState}; // 选择摆放时撤销植被单选并发布底栏状态。
use crate::ui::gltf_browser::{
    EditorGltfBrowserState, EditorGltfMeshSelection, mesh_index_from_primitive_label,
    stable_mesh_reference,
}; // Scene0命中沿用右栏唯一稳定引用格式，primitive只发布整项选择。
use crate::ui::sidebars::MapEditorSidebarState; // 命中后显式发布右侧对象属性意图。
use crate::viewport::SelectedPlacementViewportState; // Scene0保留双击定位，primitive明确清除旧点击序列。

/// 中央视口命中的两种显式摆放语义。
#[derive(Clone)] // 后台候选只携带稳定身份，不持有ECS实体借用。
pub(super) enum PlacementViewportHit {
    /// 完整Scene0中的真实Blender网格，保留精确高亮和双击定位能力。
    SceneMesh(EditorGltfMeshSelection),
    /// 直接primitive只选中所属完整摆放记录，不制造不存在的场景节点身份。
    Primitive {
        placement_id: String,            // 资产变换、复制和删除使用稳定记录ID。
        target: MapPlacementAssetTarget, // 状态栏回显确切MeshN/PrimitiveN目标。
    },
}

/// 同实例的多个网格和primitive在统一列表只占一项，保留最近精确网格身份。
pub(super) fn append_placement_candidate(
    hit: PlacementViewportHit,
    distance: f32,
    candidates: &mut Vec<super::layers::LayerCandidate>,
    hits: &mut std::collections::HashMap<String, PlacementViewportHit>,
) {
    let (id, detail) = match &hit {
        PlacementViewportHit::SceneMesh(selection) => {
            (&selection.placement_id, selection.mesh_name.clone())
        } // 真实Blender网格名称便于识别。
        PlacementViewportHit::Primitive {
            placement_id,
            target,
        } => (placement_id, target.label()), // 子资产回显实际数字目标。
    };
    if hits.contains_key(id) {
        return; // 射线命中已按距离排序，首个命中就是该实例最近网格。
    }
    candidates.push(super::layers::LayerCandidate {
        author_index: None, // 实例不用作者图层数组下标。
        target: super::layers::LayerTarget::Placement(id.clone()), // 只按稳定实例身份轮选。
        label: format!("资产 · {id} · {detail}"), // 同源多个摆放仍能区分。
        rank: 0,            // 真实表面与水体、角色共同按距离排序。
        distance,           // 保留射线真实距离。
    });
    hits.insert(id.clone(), hit); // 场景树同步和悬停共享同次命中的精确身份。
}

/// 沿最多正式路径深度加两个loader壳层寻找当前地图活动摆放根。
pub(super) fn active_placement_root(
    entity: Entity, // 射线候选primitive或其后代。
    map_id: &MapId, // 当前草稿地图身份。
    parents: &Query<&ChildOf>,
    roots: &Query<&MapPlacementInstance>,
) -> Option<Entity> {
    let mut current = entity; // primitive本身通常不是根，但先允许受检直接根夹具。
    for _ in 0..=MAX_MAP_PLACEMENT_WATER_NODE_PATH_DEPTH.saturating_add(2) {
        if roots
            .get(current)
            .is_ok_and(|instance| instance.is_active() && instance.map_id() == map_id)
        {
            return Some(current); // 候选、退役或跨图同名根均不能提供选择信用。
        }
        current = parents.get(current).ok()?.parent(); // 父链中断即不是可选择placement后代。
    }
    None // 异常深层级不会让单次点击无界遍历。
}

/// 判断一个可绘制Mesh是否属于当前地图允许射线命中的显式摆放形态。
pub(super) fn is_placement_mesh_candidate(
    entity: Entity, // MeshRayCast正在过滤的可绘制实体。
    map_id: &MapId, // 只允许当前草稿地图的活动根。
    parents: &Query<&ChildOf>,
    roots: &Query<&MapPlacementInstance>,
    scene_primitives: &Query<(&GltfMeshName, &Mesh3d, &ChildOf, Option<&feathrise_scene::StaticMeshLodOriginal>)>,
    direct_primitives: &Query<(), (With<MapPlacementPrimitiveVisual>, With<Mesh3d>)>,
) -> bool {
    let Some(root) = active_placement_root(entity, map_id, parents, roots) else {
        return false; // 非placement、跨图、隐藏候选和退役实体都不能进入几何求交。
    };
    let Ok(instance) = roots.get(root) else {
        return false; // 根在同一帧失效时失败关闭本次候选。
    };
    match instance.asset_target() {
        MapPlacementAssetTarget::Scene0 => scene_primitives.get(entity).is_ok(), // Scene0仍要求真实GltfMeshName。
        MapPlacementAssetTarget::Primitive { .. } | MapPlacementAssetTarget::Cuboid { .. } | MapPlacementAssetTarget::EditableCuboid { .. } | MapPlacementAssetTarget::EditableMesh { .. } => direct_primitives.contains(entity), // 内置形状与直接子资产共用完整实例选择，不伪造GLTF节点。
    }
}

/// 把命中Scene0 primitive恢复成与右侧场景树相同的稳定网格选择。
fn scene_mesh_selection_from_hit(
    entity: Entity,             // MeshRayCast返回的最近Scene0 primitive。
    root: Entity,               // 已复核属于当前地图的活动placement根。
    asset_server: &AssetServer, // Mesh子资产标签提供诊断mesh_index。
    named_nodes: &Query<(&Name, &ChildOf)>,
    roots: &Query<&MapPlacementInstance>,
    scene_primitives: &Query<(&GltfMeshName, &Mesh3d, &ChildOf, Option<&feathrise_scene::StaticMeshLodOriginal>)>,
) -> Option<EditorGltfMeshSelection> {
    let (mesh_name, mesh, primitive_parent, original) = scene_primitives.get(entity).ok()?; // 非glTF Mesh不伪造对象引用。
    let mut current = primitive_parent.parent(); // primitive父级是携带显式Name的Blender对象节点。
    let mut reversed = Vec::new(); // 父链先得到对象到场景根的逆序路径。
    let mut path_chars = 0usize; // 完整路径总字符数服从正式作者硬门。
    let mut path_names_valid = true; // 任何缺名、超长或断链都只保留无效诊断选择。
    let mut reached_scene_root = false; // loader场景壳本身不能进入Blender对象路径。
    for _ in 0..=MAX_MAP_PLACEMENT_WATER_NODE_PATH_DEPTH {
        if current == root {
            reached_scene_root = true; // 防御直接挂根结构；空路径随后仍被稳定引用规则拒绝。
            break;
        }
        let Ok((name, parent)) = named_nodes.get(current) else {
            path_names_valid = false; // 缺少显式Name的节点不能成为可持久引用。
            break;
        };
        let parent_entity = parent.parent(); // 每次父链推进只消费一个有界节点。
        if parent_entity == root {
            reached_scene_root = true; // 当前节点是Bevy Scene0壳，不把其生成名写入作者路径。
            break;
        }
        let segment_chars = name
            .as_str()
            .chars()
            .take(MAX_MAP_PLACEMENT_WATER_NAME_CHARS + 1)
            .count(); // 异常长名称最多检查硬门加一。
        if segment_chars > MAX_MAP_PLACEMENT_WATER_NAME_CHARS
            || reversed.len() >= MAX_MAP_PLACEMENT_WATER_NODE_PATH_DEPTH
        {
            path_names_valid = false; // 不复制超长段或超深路径。
            break;
        }
        path_chars = path_chars.saturating_add(segment_chars); // 多段合计同样受硬门保护。
        if path_chars > MAX_MAP_PLACEMENT_WATER_NODE_PATH_CHARS {
            path_names_valid = false; // 已有有界前缀只供诊断，不授权作者动作。
            break;
        }
        reversed.push(name.as_str().to_owned()); // 每段已通过Unicode字符上限后才拥有字符串。
        current = parent_entity; // 下一次继续向Scene0壳恢复。
    }
    reversed.reverse(); // 与浏览器Scene0 DFS发布的根到对象顺序一致。
    path_names_valid &= reached_scene_root; // 深度耗尽但未到根时明确无效。
    let mesh_name_chars = mesh_name
        .0
        .chars()
        .take(MAX_MAP_PLACEMENT_WATER_NAME_CHARS + 1)
        .count(); // GltfMeshName本身同样不能无界复制。
    let mesh_name = if mesh_name_chars <= MAX_MAP_PLACEMENT_WATER_NAME_CHARS {
        mesh_name.0.clone() // 合法显式名称进入稳定选择。
    } else {
        path_names_valid = false; // 超长网格名只保留空诊断值。
        String::new()
    };
    let mesh_index = asset_server
        .get_path(original.map_or(&mesh.0, |source| &source.mesh).id())
        .and_then(|path| path.label().and_then(mesh_index_from_primitive_label)); // 只解析Bevy正式MeshN/PrimitiveM标签。
    let valid = stable_mesh_reference(&reversed, &mesh_name, mesh_index, path_names_valid); // 与右栏行发布使用唯一校验口径。
    let placement_id = roots.get(root).ok()?.id().to_owned(); // 活动根私有稳定ID是整实例选择权威。
    Some(EditorGltfMeshSelection {
        placement_id,                  // 资产属性始终作用于整条MapPlacementRecord。
        node_path: reversed,           // 精确覆盖和整网格水使用完整Blender对象路径。
        mesh_name,                     // 同对象多primitive共享该稳定Mesh名。
        mesh_index,                    // 仅供当前导出诊断，不参与最终运行时匹配。
        valid,                         // 无效引用仍允许选中整实例并显示明确诊断。
        orphaned_water_binding: false, // 世界真实命中绝不是旧作者孤儿项。
    })
}

/// 把最近几何命中映射成Scene0精确网格或直接primitive整项选择。
pub(super) fn placement_hit_from_entity(
    entity: Entity,             // MeshRayCast返回的最近可选实体。
    map_id: &MapId,             // 当前草稿地图身份。
    asset_server: &AssetServer, // Scene0网格标签提供诊断索引，直接primitive不读取它。
    parents: &Query<&ChildOf>,
    named_nodes: &Query<(&Name, &ChildOf)>,
    roots: &Query<&MapPlacementInstance>,
    scene_primitives: &Query<(&GltfMeshName, &Mesh3d, &ChildOf, Option<&feathrise_scene::StaticMeshLodOriginal>)>,
    direct_primitives: &Query<(), (With<MapPlacementPrimitiveVisual>, With<Mesh3d>)>,
) -> Option<PlacementViewportHit> {
    let root = active_placement_root(entity, map_id, parents, roots)?; // 点击后再次复核当前活动根身份。
    let instance = roots.get(root).ok()?; // 同一帧根失效时不发布过期记录ID。
    match instance.asset_target() {
        MapPlacementAssetTarget::Scene0 => scene_mesh_selection_from_hit(
            entity,
            root,
            asset_server,
            named_nodes,
            roots,
            scene_primitives,
        )
        .map(PlacementViewportHit::SceneMesh), // Scene0保留完整节点/网格稳定引用。
        target @ (MapPlacementAssetTarget::Primitive { .. } | MapPlacementAssetTarget::Cuboid { .. } | MapPlacementAssetTarget::EditableCuboid { .. } | MapPlacementAssetTarget::EditableMesh { .. })
            if direct_primitives.contains(entity) =>
        {
            Some(PlacementViewportHit::Primitive {
                placement_id: instance.id().to_owned(), // 只复制受作者记录硬门约束的稳定ID。
                target, // 数字目标为Copy小值，仅用于明确回显和分支证明。
            })
        }
        MapPlacementAssetTarget::Primitive { .. } | MapPlacementAssetTarget::Cuboid { .. } | MapPlacementAssetTarget::EditableCuboid { .. } | MapPlacementAssetTarget::EditableMesh { .. } => None, // 标记与根目标不一致时拒绝伪造命中。
    }
}

/// 选中视口命中的显式摆放；Scene0保留网格能力，primitive只发布完整记录身份。
pub(super) fn select_placement_hit(
    hit: PlacementViewportHit, // 已通过活动根和目标形态双重校验的命中。
    map_id: &MapId,            // 当前草稿地图身份。
    now_seconds: f64,          // Scene0真实时间双击时钟。
    editor: &mut MapEditorState,
    browser: &mut EditorGltfBrowserState,
    selected_viewport: &mut SelectedPlacementViewportState,
    viewport_selection: &mut EditorViewportSelectionState,
    sidebars: &mut MapEditorSidebarState,
) {
    viewport_selection.clear_vegetation_selection(); // 场景对象选择立即撤销“查看”模式整层密度覆盖。
    sidebars.reveal_scene_properties(); // 作者无需再手动切回右侧场景页核对选择。
    match hit {
        PlacementViewportHit::Primitive {
            placement_id,
            target,
        } => {
            selected_viewport.clear_click_sequence(); // 子资产没有Scene0范围身份，不能与旧网格点击组成双击。
            let valid = browser.select_primitive_placement_from_viewport(placement_id.clone()); // 同帧发布整条记录并清除旧网格水选择。
            editor.status = if valid {
                format!(
                    "已从视口选中子资产：{} / {}；可变换、复制或删除",
                    placement_id,
                    target.label(),
                ) // 明确说明本次只选择完整placement根。
            } else {
                format!("子资产摆放“{placement_id}”的ID重复，不能稳定编辑") // 重复ID绝不任选一条作者记录。
            };
        }
        PlacementViewportHit::SceneMesh(selection) => {
            let valid = browser.select_mesh_from_viewport(selection); // 同帧发布整实例和精确网格身份，并按需展开Scene0。
            let Some(selection) = browser.selection.as_ref() else {
                return; // 上方方法保证存在，防御后续重构中的提前清理。
            };
            if !valid {
                selected_viewport.clear_click_sequence(); // 无效稳定引用不能与旧有效点击组成双击。
                editor.status = format!(
                    "已从视口选中摆放“{}”，但命中网格缺少可持久稳定身份",
                    selection.placement_id,
                ); // 整实例属性仍可用，精确覆盖和网格水失败关闭。
                return;
            }
            let double_clicked =
                selected_viewport.register_mesh_click(map_id, selection, now_seconds); // 第二次同身份点击才登记相机定位。
            editor.status = if double_clicked {
                format!(
                    "正在定位视口对象：{} / {} / {}",
                    selection.placement_id,
                    selection.node_path.join("/"),
                    selection.mesh_name,
                ) // 既有解析系统取得完整联合Aabb后执行真实定位。
            } else {
                format!(
                    "已从视口选中对象：{} / {} / {}；再次点击同一对象可定位",
                    selection.placement_id,
                    selection.node_path.join("/"),
                    selection.mesh_name,
                ) // 普通单击不移动当前CAD视角。
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::asset::AssetPlugin; // 测试只需真实AssetServer资源，不执行文件IO。
    use bevy::ecs::system::SystemState; // 直接以生产Query组合验证父链和专用标记。
    use feathrise_map_content::{MapPlacementHeightMode, MapPlacementRecord}; // 构造完整primitive作者记录。

    use super::*; // 回归锁定直接primitive不会依赖GltfMeshName或Scene0节点路径。

    #[test]
    fn overlapping_instances_keep_nearest_mesh_per_stable_id() {
        let scene_hit = |id: &str, name: &str| {
            PlacementViewportHit::SceneMesh(EditorGltfMeshSelection {
                placement_id: id.to_owned(),      // 同源资产的不同摆放使用不同稳定ID。
                node_path: vec![name.to_owned()], // 两个真实网格提供可区别的命中身份。
                mesh_name: name.to_owned(),       // 最靠近相机的网格必须留给场景树。
                mesh_index: Some(0),              // 测试不依赖运行时资产加载。
                valid: true,                      // 明确合法稳定引用。
                orphaned_water_binding: false,    // 真实命中不是失效绑定。
            })
        };
        let mut candidates = Vec::new(); // 与生产点击收集使用相同容器。
        let mut hits = std::collections::HashMap::new(); // 精确网格按实例索引。
        append_placement_candidate(
            scene_hit("tree-1", "leaves"),
            1.0,
            &mut candidates,
            &mut hits,
        ); // 最近primitive先到。
        append_placement_candidate(
            scene_hit("tree-1", "trunk"),
            2.0,
            &mut candidates,
            &mut hits,
        ); // 同根第二网格不能新增一项。
        append_placement_candidate(
            scene_hit("tree-2", "leaves"),
            3.0,
            &mut candidates,
            &mut hits,
        ); // 后方不同实例也必须保留。
        assert_eq!(candidates.len(), 2); // 重叠两实例不退化为只选最近对象。
        assert_eq!(
            candidates[0].target,
            super::super::layers::LayerTarget::Placement("tree-1".to_owned())
        ); // 列表以实例身份选择。
        assert_eq!(candidates[1].distance, 3.0); // 统一排序保留第二实例真实距离。
        let mut state = super::super::layers::LayerCandidateState::default(); // 验证显示侧借用同份网格引用。
        state.placement_hits = hits; // 与完整结果一同发布。
        assert_eq!(
            state.placement_mesh_selection("tree-1").unwrap().mesh_name,
            "leaves"
        ); // 去重必须保留最近网格。
        state.placement_hits.insert(
            "part".to_owned(),
            PlacementViewportHit::Primitive {
                placement_id: "part".to_owned(), // 独立子资产仍是可选实例。
                target: MapPlacementAssetTarget::Primitive {
                    mesh: 0,
                    primitive: 0,
                    material: None,
                }, // 不具备Scene0稳定节点。
            },
        );
        assert!(state.placement_mesh_selection("part").is_none()); // 子资产不得伪造精确网格水身份。
    }

    #[test]
    fn direct_primitive_hit_selects_complete_placement_root_without_scene_identity() {
        let mut app = App::new(); // 独立ECS世界足以复现作者预览与正式根形态。
        app.add_plugins((MinimalPlugins, AssetPlugin::default())); // 提供AssetServer和基础任务池。
        let map_id = MapId::parse("MAP-direct-primitive-hit").expect("测试地图ID合法"); // 稳定地图身份。
        let record = MapPlacementRecord {
            mesh_lod: None,
            terrain_binding: None,
            editor: Default::default(),
            terrain_patch: None,
            id: "plant-part-1".to_owned(), // 视口最终必须恢复该完整记录ID。
            asset_path: "gltf/plant.glb".to_owned(), // 来源路径不参与直接命中判定。
            asset_target: MapPlacementAssetTarget::Primitive {
                mesh: 6,           // 与截图中的独立Mesh目标一致。
                primitive: 0,      // 当前GLB每个Mesh只有第零primitive。
                material: Some(2), // 原材质身份由记录冻结，但选择不冒充水绑定。
            },
            building_preset_id: None, // 直接 primitive 选择夹具是普通资产。
            courtyard: None,          // 独立摆放记录不属于任何院落副本。
            translation: [0.0, 0.0, 0.0], // 本测试不检查空间变换。
            height_mode: MapPlacementHeightMode::FixedHeight, // 保持完整合法作者记录。
            terrain_offset: 0.0,      // 固定高度忽略离地偏移。
            yaw_degrees: 0.0,         // 本测试不检查旋转。
            scale: [1.0, 1.0, 1.0],   // 使用单位缩放。
            water_surfaces: Vec::new(), // primitive依法不携带Scene0整网格水绑定。
        };
        let root = app
            .world_mut()
            .spawn(MapPlacementInstance::new(&map_id, &record))
            .id(); // 作者预览根立即处于可选择活动态。
        let child = app
            .world_mut()
            .spawn((
                MapPlacementPrimitiveVisual, // 生产侧唯一直接子资产命中身份。
                Mesh3d::default(),           // MeshRayCast候选形态必须真实携带Mesh3d。
                ChildOf(root),               // 稳定记录身份只从祖先根读取。
            ))
            .id();
        let mut query_state: SystemState<(
            Query<&ChildOf>,
            Query<(&Name, &ChildOf)>,
            Query<&MapPlacementInstance>,
            Query<(&GltfMeshName, &Mesh3d, &ChildOf, Option<&feathrise_scene::StaticMeshLodOriginal>)>,
            Query<(), (With<MapPlacementPrimitiveVisual>, With<Mesh3d>)>,
        )> = SystemState::new(app.world_mut()); // 使用与生产函数相同的五组只读查询。
        let world = app.world(); // 资产资源和查询都只读同一完整世界快照。
        let asset_server = world.resource::<AssetServer>(); // 直接分支不会查询任何Scene0标签。
        let (parents, named_nodes, roots, scene_primitives, direct_primitives) = query_state
            .get(world)
            .expect("五组只读查询应能同时借用World");

        assert!(is_placement_mesh_candidate(
            child,
            &map_id,
            &parents,
            &roots,
            &scene_primitives,
            &direct_primitives,
        )); // 专用标记让直接Mesh进入点击边沿射线。
        let hit = placement_hit_from_entity(
            child,
            &map_id,
            asset_server,
            &parents,
            &named_nodes,
            &roots,
            &scene_primitives,
            &direct_primitives,
        )
        .expect("活动直接primitive应恢复整条摆放记录");
        assert!(matches!(
            hit,
            PlacementViewportHit::Primitive {
                placement_id,
                target: MapPlacementAssetTarget::Primitive {
                    mesh: 6,
                    primitive: 0,
                    material: Some(2),
                },
            } if placement_id == "plant-part-1"
        )); // 命中只携带根ID和冻结数字目标，没有伪造node_path或mesh_name。
    }
}
