//! 当前Scene0网格活动根、World票据和primitive身份的固定预算解析。

use bevy::gltf::GltfMeshName; // 稳定网格身份必须匹配Blender Mesh datablock名。
use bevy::prelude::*; // Children、Name、Entity、Query与Resource。
use bevy::world_serialization::{WorldAssetRoot, WorldInstance, WorldInstanceSpawner}; // 当前完整World票据和Ready门。
use feathrise_map_content::{
    MAX_MAP_PLACEMENT_WATER_NAME_CHARS, MAX_MAP_PLACEMENT_WATER_NODE_PATH_CHARS,
}; // 场景树与整网格水共用正式节点名称硬边界。
use feathrise_scene::MapPlacementInstance; // 当前活动摆放根的稳定地图与记录身份。

use super::super::{
    LOOKUP_OPERATIONS_PER_FRAME, MAX_LOOKUP_DESCENDANTS, MAX_LOOKUP_PATH_DEPTH,
    MAX_SELECTED_PRIMITIVES, PendingPlacementCandidatePath, PlacementHierarchyFrame,
    PlacementMeshLookup, PlacementMeshResolution, SelectedPlacementMeshIdentity,
    SelectedPlacementViewportState, selected_placement_mesh_info,
}; // 解析游标、稳定身份和选择快照由功能域根统一定义。
use crate::authoring::EditorAuthoringSession; // 当前草稿和未保存精确摆放预览根。
use crate::components::MapEditorSelectedPlacementHighlight; // 辅助覆盖不属于资产后代预算。
use crate::resources::MapEditorState; // 关闭F12时撤销解析与定位请求。
use crate::types::EditorTool; // 只在选择模式应用候选悬停。
use crate::ui::gltf_browser::EditorGltfBrowserState; // 读取右栏公开稳定网格选择。
use crate::ui::placement_water::PlacementInstanceIndex;
use crate::viewport::content_selection::LayerCandidateState; // 高亮解析消费统一候选的只读身份。 // 正式根按地图和稳定ID O(1)定位小集合。

/// 优先采用当前作者会话的未保存精确预览；否则只接受唯一活动正式根。
fn resolve_active_root(
    identity: &SelectedPlacementMeshIdentity,
    session: &EditorAuthoringSession,
    root_index: &PlacementInstanceIndex,
    roots: &Query<(Entity, &MapPlacementInstance)>,
) -> Result<Option<(Entity, bool)>, String> {
    if let Some(entity) = session
        .placement_previews
        .get(identity.placement_id.as_str())
        .copied()
        && roots.get(entity).is_ok_and(|(_, instance)| {
            instance.map_id() == &identity.map_id
                && instance.id() == identity.placement_id
                && instance.is_active()
        })
    {
        return Ok(Some((entity, true))); // 未保存作者预览与当前草稿一一对应，优先于旧正式版本。
    }

    let mut found = None; // 同图同ID正常情况下只存在一个活动正式根。
    let mut ambiguous = false; // 多个活动根时绝不猜测高亮哪一个空间实例。
    root_index.for_each_active_root(
        &identity.map_id,
        identity.placement_id.as_str(),
        roots,
        |entity| {
            if found.replace(entity).is_some() {
                ambiguous = true; // 索引小集合内第二个活动根即构成运行时身份歧义。
            }
        },
    );
    if ambiguous {
        Err(format!(
            "摆放{}同时存在多个活动实例根",
            identity.placement_id
        )) // 失败关闭，不发布任何一棵树的部分高亮。
    } else {
        Ok(found.map(|entity| (entity, false))) // 无作者预览时允许当前完整正式根提供空间参照。
    }
}

/// 只有身份或阶段真实变化时撤销旧解析，稳定失败/等待帧不反复清空缓存。
fn reset_if_changed(
    state: &mut SelectedPlacementViewportState,
    identity: Option<SelectedPlacementMeshIdentity>,
    resolution: PlacementMeshResolution,
) {
    if state.resolved_identity != identity || state.resolution != resolution {
        state.reset_resolution(identity, resolution); // 同身份重置会保留尚未完成的双击定位请求。
    }
}

/// 每帧共享固定层级操作预算推进当前World实例的DFS或候选父链路径。
fn advance_lookup(
    lookup: &mut PlacementMeshLookup,
    children: &Query<&Children>,
    world_roots: &Query<(), With<WorldAssetRoot>>,
    primitives: &Query<(Option<&GltfMeshName>, &ChildOf), With<Mesh3d>>,
    nodes: &Query<(&Name, &ChildOf)>,
    editor_highlights: &Query<(), With<MapEditorSelectedPlacementHighlight>>,
) -> Result<bool, String> {
    let mut budget = LOOKUP_OPERATIONS_PER_FRAME; // 完整场景规模只影响完成帧数，不影响单帧上限。
    while budget > 0 {
        budget -= 1; // 路径一段、DFS一个子节点或一个完成帧弹出都计一次操作。
        if let Some(mut pending) = lookup.pending_path.take() {
            if pending.current == lookup.root {
                pending.reversed.reverse(); // 转成与GLB浏览器一致的Scene0根后正序路径。
                if pending.reversed == lookup.identity.node_path {
                    if lookup
                        .matched_object_node
                        .is_some_and(|entity| entity != pending.object_node)
                    {
                        return Err(format!(
                            "节点路径{}在当前实例中对应多个对象",
                            lookup.identity.node_path.join("/")
                        )); // 重名对象路径歧义时不能任选一项。
                    }
                    lookup.matched_object_node = Some(pending.object_node); // 同对象的多材质primitive共同高亮。
                    if lookup.matches.len() >= MAX_SELECTED_PRIMITIVES {
                        return Err(format!(
                            "网格{}的primitive数超过上限{}",
                            lookup.identity.mesh_name, MAX_SELECTED_PRIMITIVES
                        )); // 全量成功前失败，不发布前64项。
                    }
                    lookup.matches.push(pending.entity); // 只写私有游标，DFS完成后原子发布。
                }
                continue; // 同名但路径不同的候选按稳定身份忽略。
            }
            let (name, parent) = nodes
                .get(pending.current)
                .map_err(|_| format!("网格对象节点{:?}缺少Name或ChildOf", pending.current))?;
            let parent_entity = parent.parent();
            if parent_entity == lookup.root {
                pending.current = lookup.root; // WorldAsset顶层Scene根不进入Blender作者路径。
                lookup.pending_path = Some(pending);
                continue;
            }
            if pending.reversed.len() >= MAX_LOOKUP_PATH_DEPTH {
                return Err("实例层级超过场景网格节点路径上限".to_owned());
            }
            let segment_chars = name
                .as_str()
                .chars()
                .take(MAX_MAP_PLACEMENT_WATER_NAME_CHARS + 1)
                .count(); // 长名称最多检查硬门加一，不无界扫描异常字符串。
            if segment_chars > MAX_MAP_PLACEMENT_WATER_NAME_CHARS {
                return Err(format!("网格节点{:?}名称超过字符上限", pending.current));
            }
            pending.path_chars = pending.path_chars.saturating_add(segment_chars);
            if pending.path_chars > MAX_MAP_PLACEMENT_WATER_NODE_PATH_CHARS {
                return Err("网格节点路径超过总字符上限".to_owned());
            }
            pending.reversed.push(name.as_str().to_owned()); // 每次只复制一个有界名称段。
            pending.current = parent_entity;
            lookup.pending_path = Some(pending); // 下一预算操作继续向World根恢复路径。
            continue;
        }

        let Some(frame) = lookup.frames.last_mut() else {
            return Ok(true); // 只有完整DFS结束后调用方才原子发布全部命中项。
        };
        let Ok(items) = children.get(frame.parent) else {
            lookup.frames.pop(); // 叶节点完成也计本次层级操作。
            continue;
        };
        let Some(entity) = items.get(frame.next_child).copied() else {
            lookup.frames.pop(); // 当前父节点全部子项已完成。
            continue;
        };
        frame.next_child = frame.next_child.saturating_add(1); // 后续帧不重复读取当前子项。
        if editor_highlights.contains(entity) {
            continue; // 精确覆盖属于编辑器辅助子节点，不计入资产层级或递归解析。
        }
        if lookup.descendants_seen >= MAX_LOOKUP_DESCENDANTS {
            return Err(format!(
                "当前实例后代超过{}个的场景选择上限",
                MAX_LOOKUP_DESCENDANTS
            ));
        }
        lookup.descendants_seen += 1;
        if world_roots.contains(entity) {
            continue; // 嵌套World拥有独立身份边界，不能穿透或冒充当前placement内容。
        }
        lookup.frames.push(PlacementHierarchyFrame {
            parent: entity, // 下一操作才读取其Children，保持固定预算。
            next_child: 0,
        });
        let Ok((mesh_name, parent)) = primitives.get(entity) else {
            continue; // 非primitive节点只参与层级DFS。
        };
        if lookup.identity.whole_instance {
            if lookup.matches.len() >= MAX_SELECTED_PRIMITIVES {
                return Err(format!("实例网格超过高亮上限{}", MAX_SELECTED_PRIMITIVES)); // 不发布部分实例覆盖。
            }
            lookup.matches.push(entity); // 各名称与独立Primitive统一入私有结果。
            continue; // 整实例无需恢复对象路径。
        }
        if mesh_name.is_none_or(|name| name.0 != lookup.identity.mesh_name) {
            continue; // Mesh名快筛失败时不构造父链路径。
        }
        let object_node = parent.parent();
        lookup.pending_path = Some(PendingPlacementCandidatePath {
            entity,               // 完整路径匹配后才进入私有命中桶。
            object_node,          // 同一对象的多材质primitive允许共同采用。
            current: object_node, // 下一预算操作读取第一段对象路径。
            reversed: Vec::new(), // 深度和字符数都由硬门约束。
            path_chars: 0,        // 当前候选尚未复制任何名称。
        });
    }
    Ok(false) // 本帧预算耗尽，保留轻量游标供下一帧继续。
}

/// 选择或World票据换代后按固定预算解析全部精确primitive，稳定帧只做有限缓存核验。
#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_selected_placement_mesh(
    editor: Res<MapEditorState>,
    browser: Res<EditorGltfBrowserState>,
    candidates: Res<LayerCandidateState>, // 只读悬停身份，不修改场景树正式选择。
    session: Res<EditorAuthoringSession>,
    root_index: Res<PlacementInstanceIndex>,
    instance_spawner: Res<WorldInstanceSpawner>,
    mut state: ResMut<SelectedPlacementViewportState>,
    roots: Query<(Entity, &MapPlacementInstance)>,
    world_instances: Query<&WorldInstance>,
    children: Query<&Children>,
    world_roots: Query<(), With<WorldAssetRoot>>,
    primitives: Query<(Option<&GltfMeshName>, &ChildOf), With<Mesh3d>>,
    nodes: Query<(&Name, &ChildOf)>,
    editor_highlights: Query<(), With<MapEditorSelectedPlacementHighlight>>,
) {
    state.previewing =
        editor.open && editor.tool == EditorTool::Inspect && candidates.hover.is_some() && !state.explicit_focus_pending(); // 只有显式取景优先于临时悬停。
    let Some(info) = selected_placement_mesh_info(&editor, &browser, &session, &candidates, state.explicit_focus_pending()) else {
        if !state.previewing {
            // 跨类别悬停只是隐藏旧覆盖，并未结束正式选择。
            state.clear_click_sequence(); // 关闭F12、换草稿或清空选择后双击必须重新开始。
        }
        reset_if_changed(&mut state, None, PlacementMeshResolution::WaitingForRoot);
        return;
    };
    if !info.valid || info.orphaned {
        reset_if_changed(
            &mut state,
            Some(info.identity),
            PlacementMeshResolution::Invalid,
        ); // 无效或失效绑定只显示诊断，不解析或伪造世界目标。
        return;
    }
    if state.resolved_identity.as_ref() != Some(&info.identity) {
        state.reset_resolution(
            Some(info.identity.clone()),
            PlacementMeshResolution::WaitingForRoot,
        ); // 新稳定身份完整撤销旧根、票据、primitive与范围信用。
    }

    let (root, author_preview) =
        match resolve_active_root(&info.identity, &session, &root_index, &roots) {
            Ok(Some(root)) => root,
            Ok(None) => {
                reset_if_changed(
                    &mut state,
                    Some(info.identity),
                    PlacementMeshResolution::WaitingForRoot,
                ); // 当前活动实例尚未生成，双击请求继续等待。
                return;
            }
            Err(detail) => {
                reset_if_changed(
                    &mut state,
                    Some(info.identity),
                    PlacementMeshResolution::Failed(detail),
                ); // 重复活动根失败关闭，绝不任选空间实例。
                return;
            }
        };
    // 完整GLB必须等World票据Ready；直接Primitive仅依赖已验证的活动根。
    let instance_id = world_instances.get(root).ok().map(|instance| **instance);
    let scene_instance = roots.get(root).is_ok_and(|(_, instance)| {
        matches!(
            instance.asset_target(),
            feathrise_map_content::MapPlacementAssetTarget::Scene0
        )
    }); // 作者记录决定Ready门，不能因WorldAssetRoot尚未安装误判直接Primitive。
    if scene_instance && instance_id.is_none_or(|id| !instance_spawner.instance_is_ready(id)) {
        reset_if_changed(
            &mut state,
            Some(info.identity),
            PlacementMeshResolution::WaitingForWorld,
        );
        state.root_entity = Some(root); // 未Ready不遍历未完成层级。
        state.instance_id = instance_id; // 票据变化立即淘汰旧缓存。
        return;
    }
    let generation_changed = state.root_entity != Some(root)
        || state.instance_id != instance_id
        || state.source_is_author_preview != author_preview;
    if generation_changed {
        state.reset_resolution(
            Some(info.identity.clone()),
            PlacementMeshResolution::Resolving,
        );
        state.root_entity = Some(root);
        state.instance_id = instance_id;
        state.source_is_author_preview = author_preview;
        state.lookup = Some(PlacementMeshLookup::new(
            info.identity.clone(),
            root,
            instance_id,
            author_preview,
        )); // 新一代从根开始按预算构造私有完整结果。
    }

    if state.resolution == PlacementMeshResolution::Ready {
        let cached_valid = !state.source_entities.is_empty()
            && state.source_entities.iter().all(|&entity| {
                primitives.get(entity).is_ok_and(|(name, _)| {
                    info.identity.whole_instance
                        || name.is_some_and(|name| name.0 == info.identity.mesh_name)
                })
            }); // 稳定帧最多核验64个已发布primitive，不扫描层级或Mesh顶点。
        if cached_valid {
            return;
        }
        state.source_entities.clear(); // 当前票据内异常失效也必须撤销旧覆盖信用。
        state.frame = None;
        state.resolution = PlacementMeshResolution::Resolving;
        state.lookup = Some(PlacementMeshLookup::new(
            info.identity.clone(),
            root,
            instance_id,
            author_preview,
        ));
    }
    if matches!(
        state.resolution,
        PlacementMeshResolution::Missing
            | PlacementMeshResolution::Invalid
            | PlacementMeshResolution::Failed(_)
    ) {
        return; // 同一完整World票据的终局结论稳定保留到身份或实例换代。
    }
    if state.lookup.is_none() {
        state.resolution = PlacementMeshResolution::Resolving;
        state.lookup = Some(PlacementMeshLookup::new(
            info.identity.clone(),
            root,
            instance_id,
            author_preview,
        )); // 防御Commands边界或资源恢复导致的缺失游标。
    }

    let Some(mut lookup) = state.lookup.take() else {
        return; // 上方已经保证存在，仅作防御性失败关闭。
    };
    if lookup.identity != info.identity
        || lookup.root != root
        || lookup.instance_id != instance_id
        || lookup.author_preview != author_preview
    {
        state.reset_resolution(
            Some(info.identity),
            PlacementMeshResolution::WaitingForWorld,
        ); // 私有游标身份与当前完成票据不一致时拒收迟到结果。
        return;
    }
    match advance_lookup(
        &mut lookup,
        &children,
        &world_roots,
        &primitives,
        &nodes,
        &editor_highlights,
    ) {
        Ok(false) => {
            state.lookup = Some(lookup); // 预算耗尽，下一帧从精确游标继续。
            state.resolution = PlacementMeshResolution::Resolving;
        }
        Ok(true) if lookup.matches.is_empty() => {
            state.lookup = None; // 完整实例确实没有该稳定网格身份。
            state.source_entities.clear();
            state.frame = None;
            state.resolution = PlacementMeshResolution::Missing;
        }
        Ok(true) => {
            state.source_entities = lookup.matches; // 全量DFS成功后一次性原子发布全部primitive。
            state.lookup = None;
            state.frame = None; // 下一表现系统从当前GlobalTransform和Aabb重算联合范围。
            state.resolution = PlacementMeshResolution::Ready;
        }
        Err(detail) => {
            state.lookup = None; // 错误游标及其半成品命中全部丢弃。
            state.source_entities.clear();
            state.frame = None;
            state.resolution = PlacementMeshResolution::Failed(detail);
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::SystemState; // 真实只读查询状态验证运行时层级身份。
    use bevy::world_serialization::WorldAsset; // 取得合法不可伪造InstanceId用于解析游标。
    use feathrise_map_content::MapId; // 构造完整稳定场景选择身份。

    use super::*; // 回归直接锁定固定预算解析和完整路径门。

    #[test] // 同Mesh名的其他对象必须被路径拒绝，同一目标对象的全部primitive必须共同采用。
    fn lookup_matches_complete_object_path_and_all_target_primitives() {
        let mut world = World::new(); // 手工建立与WorldAsset实例一致的Scene根、对象和primitive层级。
        let root = world
            .spawn(WorldAssetRoot(Handle::<WorldAsset>::default()))
            .id();
        let scene = world.spawn((Name::new("Scene"), ChildOf(root))).id(); // 顶层Scene名称不属于作者node_path。
        let target_group = world.spawn((Name::new("tree"), ChildOf(scene))).id();
        let target_object = world
            .spawn((Name::new("leaves"), ChildOf(target_group)))
            .id();
        let first = world
            .spawn((
                Mesh3d::default(),
                GltfMeshName("leaf_mesh".to_owned()),
                ChildOf(target_object),
            ))
            .id();
        let second = world
            .spawn((
                Mesh3d::default(),
                GltfMeshName("leaf_mesh".to_owned()),
                ChildOf(target_object),
            ))
            .id(); // 多材质对象导出的第二个primitive也必须命中。
        let other_group = world.spawn((Name::new("rock"), ChildOf(scene))).id();
        let other_object = world
            .spawn((Name::new("leaves"), ChildOf(other_group)))
            .id();
        let same_mesh_wrong_path = world
            .spawn((
                Mesh3d::default(),
                GltfMeshName("leaf_mesh".to_owned()),
                ChildOf(other_object),
            ))
            .id(); // 只按Mesh名会误高亮的反例。

        let mut spawner = WorldInstanceSpawner::default();
        let instance_id = spawner.spawn(Handle::<WorldAsset>::default()); // 公开API生成合法轻量票据，不实际生成第二棵树。
        let identity = SelectedPlacementMeshIdentity {
            map_id: MapId::parse("MAP-placement-lookup").expect("测试地图ID合法"),
            placement_id: "tree-placement".to_owned(),
            node_path: vec!["tree".to_owned(), "leaves".to_owned()],
            mesh_name: "leaf_mesh".to_owned(),
            whole_instance: false, // 原精确路径回归。
        };
        let mut lookup = PlacementMeshLookup::new(identity, root, Some(instance_id), false);
        let mut query_state: SystemState<(
            Query<&Children>,
            Query<(), With<WorldAssetRoot>>,
            Query<(Option<&GltfMeshName>, &ChildOf), With<Mesh3d>>,
            Query<(&Name, &ChildOf)>,
            Query<(), With<MapEditorSelectedPlacementHighlight>>,
        )> = SystemState::new(&mut world);
        let (children, world_roots, primitives, nodes, editor_highlights) = query_state
            .get(&world)
            .expect("五组只读测试查询应能同时借用World");
        let complete = advance_lookup(
            &mut lookup,
            &children,
            &world_roots,
            &primitives,
            &nodes,
            &editor_highlights,
        )
        .expect("合法小型层级应完成解析");

        assert!(complete); // 小型测试树应在单帧256步预算内完成。
        assert_eq!(lookup.matches.len(), 2); // 同一对象的两个primitive必须原子共同发布。
        assert!(lookup.matches.contains(&first));
        assert!(lookup.matches.contains(&second));
        assert!(!lookup.matches.contains(&same_mesh_wrong_path)); // 完整路径隔离同名Mesh。
    }
    #[test] // 整实例跨网格名、支持无GLTF名字的直接Primitive，并排除编辑器覆盖。
    fn whole_instance_lookup_collects_all_meshes_with_bounded_resume() {
        let mut world = World::new(); // 不需要加载GLB即可验证生产DFS。
        let root = world.spawn_empty().id(); // 直接Primitive根没有World票据。
        let trunk = world
            .spawn((
                Mesh3d::default(),
                GltfMeshName("trunk".to_owned()),
                ChildOf(root),
            ))
            .id(); // 树干。
        let leaves = world
            .spawn((
                Mesh3d::default(),
                GltfMeshName("leaves".to_owned()),
                ChildOf(root),
            ))
            .id(); // 树叶。
        let primitive = world.spawn((Mesh3d::default(), ChildOf(root))).id(); // 无Scene0名称的单独子资产。
        world.spawn((
            Mesh3d::default(),
            MapEditorSelectedPlacementHighlight,
            ChildOf(root),
        )); // 旧覆盖不能递归入选。
        for _ in 0..LOOKUP_OPERATIONS_PER_FRAME {
            world.spawn(ChildOf(root)); // 足够节点证明不会单帧同步完整遍历。
        }
        let identity = SelectedPlacementMeshIdentity {
            map_id: MapId::parse("MAP-whole-placement").unwrap(), // 合法地图身份。
            placement_id: "tree".to_owned(),                      // 整实例目标。
            node_path: Vec::new(),                                // 不伪造对象路径。
            mesh_name: String::new(),                             // 不按命中网格限制。
            whole_instance: true,                                 // 整实例语义。
        };
        let mut lookup = PlacementMeshLookup::new(identity, root, None, false); // 无World票据也能有界解析。
        let mut query_state: SystemState<(
            Query<&Children>,
            Query<(), With<WorldAssetRoot>>,
            Query<(Option<&GltfMeshName>, &ChildOf), With<Mesh3d>>,
            Query<(&Name, &ChildOf)>,
            Query<(), With<MapEditorSelectedPlacementHighlight>>,
        )> = SystemState::new(&mut world); // 与生产查询一致。
        let (children, roots, primitives, nodes, highlights) = query_state.get(&world).unwrap(); // 只读World。
        assert!(
            !advance_lookup(
                &mut lookup,
                &children,
                &roots,
                &primitives,
                &nodes,
                &highlights
            )
            .unwrap()
        ); // 第一帧不得超预算完成。
        let mut completed = false; // 下一帧沿用游标而非重扫。
        for _ in 0..8 {
            if advance_lookup(
                &mut lookup,
                &children,
                &roots,
                &primitives,
                &nodes,
                &highlights,
            )
            .unwrap()
            {
                completed = true; // 只有完整结果可发布。
                break;
            }
        }
        assert!(completed); // 小型固定测试在有限帧内完成。
        assert_eq!(lookup.matches, vec![trunk, leaves, primitive]); // 同实例所有部件一起高亮且无辅助覆盖污染。
    }
}
