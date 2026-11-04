//! 已摆放资产的“吸附地面”视口落点与撤销提交。
//!
//! 【功能设计】按钮只武装下一次中央视口左键，落点完全复用新资产首次摆放的
//! 当前射线最近真实三角命中；洞穴、倒扣、悬空层和同一X/Z多层地形不再二次采样最高包络。

use bevy::prelude::*; // Commands、鼠标输入、Query与作者资源系统参数。
use bevy::ui::RelativeCursorPosition; // 落点只接受中央编辑视口内的左键。
use feathrise_map_content::MapPlacementRecord; // 纯帮助函数只修改完整摆放记录的落点字段，并区分Clipmap查询路线。
use feathrise_scene::{ClipmapGroundHit, DynamicClipmapGroundSurface, MapPlacementInstance}; // Clipmap普通摆放消费一次正式三角点击；活动正式根与作者预览共享稳定身份。

use super::{
    PlacementRelocationIntent, PlacementTransformInputState, actions::sync_placement_record_world_with_terrain,
}; // 武装身份、延迟复制意图与定向世界同步仍由完整变换域统一所有。
use crate::authoring::buildings::{BuildingEditRequest, BuildingTerrainHit}; // 建筑移动和复制只提交后台受检候选。
use crate::authoring::{EditorAuthoringSession, EditorCommand}; // 单次落点写回草稿并进入既有有界撤销链。
use crate::components::MapEditorViewport; // 中央视口稳定命中marker。
use crate::resources::MapEditorState; // 当前真实地形光标、dirty和状态栏。
use crate::ui::building_presets::BuildingPlacementControls; // 建筑已有实例继续使用当前世界网格吸附。
use crate::ui::gltf_browser::EditorGltfBrowserState; // 当前完整实例选择。
use crate::ui::placement_water::PlacementInstanceIndex; // 稳定ID只命中少量活动、候选和预览根。
use crate::viewport::VegetationGroundCursor; // 历史类型名保留，当前资源承载全部Clipmap作者地表点击。

/// 把与首次摆放完全相同的真实地形命中写入现有摆放记录。
fn apply_ground_snap_hit(
    record: &mut MapPlacementRecord, // 当前所选完整摆放记录。
    position: Vec3,                  // 中央视口对当前地形Mesh的最近射线命中。
) {
    let half_height = record.asset_target.editable_geometry().map_or(0.0, |geometry| {
        -geometry.vertices().iter().map(|point| point[1]).min().unwrap() as f32 * 0.0005 * record.scale[1]
    }); // 变形后的内置网格按真实最低顶点落地，不能继续使用旧盒体半高。
    record.translation = (position + Vec3::Y * half_height).to_array(); // X/Z和承托Y来自同一次真实命中，不二次猜测洞穴或倒扣地形。
    if record.height_mode.follows_terrain() {
        record.terrain_offset = half_height; // 跟随型盒的中心偏移须与底面对齐一致，GLB仍从零偏移继续跟随。
    } // 固定高度资产保留模式，但当前世界Y已采用本次地形命中。
}

/// 点击来自世界空间；补丁先移动完整世界矩阵，再转入本次真实命中的Clipmap源。
fn relocation_candidate(
    draft: &crate::authoring::EditorMapDraft, before: &MapPlacementRecord, position: Vec3,
    terrain: Option<&feathrise_map_content::TerrainId>, empty_work_plane: bool,
) -> Result<(MapPlacementRecord, Rect), String> {
    let mut after = before.clone();
    if let Some(patch) = after.terrain_patch.as_mut() {
        use crate::authoring::placement_pose::{placement_record_frame, source_to_world, world_to_source};
        if empty_work_plane { return Err("三维地形必须关联真实Clipmap，不能吸附空工作平面".into()); }
        let source = placement_record_frame(draft, before)?;
        let pose = feathrise_map_content::TerrainObjectTransform { translation: before.translation,
            yaw_degrees: before.yaw_degrees, scale: before.scale, ..Default::default() };
        let world = source_to_world(pose, source)?;
        patch.terrain_id = if draft.manifest.terrains.is_some() {
            Some(terrain.cloned().ok_or("三维地形吸附没有有效的目标Clipmap身份")?)
        } else { None };
        after.terrain_binding = None;
        let target = placement_record_frame(draft, &after)?;
        let moved = Mat4::from_translation(position - world.w_axis.truncate()) * world;
        let local = world_to_source(moved, target)?;
        after.translation = local.translation;
        after.yaw_degrees = local.yaw_degrees;
        after.scale = local.scale;
        after.height_mode = feathrise_map_content::MapPlacementHeightMode::FixedHeight;
        after.terrain_offset = 0.0;
    } else {
        apply_ground_snap_hit(&mut after, position);
        if empty_work_plane {
            after.height_mode = feathrise_map_content::MapPlacementHeightMode::FixedHeight;
            after.terrain_offset = 0.0;
            after.terrain_binding = None;
        } else if draft.manifest.terrains.is_some() {
            let id = terrain.ok_or("资产吸附的承托身份已失效，请重新单击")?;
            after.terrain_binding = Some(draft.terrain_ground.as_ref()
                .ok_or("资产吸附的承托快照尚未就绪")?.bind(id,
                    feathrise_map_content::TerrainObjectTransform { translation: after.translation,
                        yaw_degrees: after.yaw_degrees, scale: after.scale, ..Default::default() })?);
        }
    }
    let bounds = crate::authoring::placement_record_bounds(draft, &after)?;
    after.validated_transform(&draft.map_id, bounds)?;
    Ok((after, bounds))
}

/// “吸附地面”模式下，下一次中央视口左键复用首次摆放的真实地形命中。
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_selected_placement_relocation(
    mut commands: Commands, // 同帧更新作者预览或活动正式根Transform。
    mouse: Res<ButtonInput<MouseButton>>, // 只消费左键按下边沿。
    viewport: Query<&RelativeCursorPosition, With<MapEditorViewport>>, // 点击必须位于中央编辑视口。
    asset_server: Res<AssetServer>, // 根尚未生成时补作者Scene0预览。
    root_index: Res<PlacementInstanceIndex>, // 稳定ID定向当前活动根。
    roots: Query<(Entity, &MapPlacementInstance)>, // 复核正式活动门或作者预览身份。
    browser: Res<EditorGltfBrowserState>, // 当前完整实例选择。
    building_controls: Res<BuildingPlacementControls>, // 建筑移动与复制共享左栏网格。
    mut transform_state: ResMut<PlacementTransformInputState>, // 成功、失败或失效后收口吸附模式。
    mut editor: ResMut<MapEditorState>, // 当前地表光标、dirty和状态栏。
    mut session: ResMut<EditorAuthoringSession>, // 修改完整草稿并登记历史。
    mut building_edits: MessageWriter<BuildingEditRequest>, // 建筑候选由专用后台索引校验。
    mut ground_cursor: ResMut<VegetationGroundCursor>, // 普通Clipmap吸附在松键后的完成帧消费冻结点击。
    clipmap_surface: Option<Res<DynamicClipmapGroundSurface>>, // 采用时复核当前正式曲面修订和缺页身份。
    building_registry: Option<Res<feathrise_map_content::BuildingPresetRegistry>>, // 待查询建筑预设换代后拒收旧射线。
    mut snap_inputs: (Option<Res<feathrise_editor_core::EditorSettings>>, Option<Res<ButtonInput<KeyCode>>>,
        Option<ResMut<crate::viewport::snapping::MeshSnapCursor>>,
        Option<Res<crate::authoring::terrain_ground::TerrainGroundCursor>>,
        Option<Res<crate::authoring::basic_shapes::BasicShapeState>>, feathrise_scene::TerrainObjectQueries),
) {
    let clicked_in_viewport = mouse.just_pressed(MouseButton::Left)
        && viewport.single().is_ok_and(|cursor| cursor.cursor_over());
    if transform_state.relocation_target.is_none() || !editor.open || editor.new_map_dialog_open
        || editor.close_requested || editor.save_requested || editor.suppress_world_pick_this_frame {
        return; // 没有武装目标或模态打开时不接纳任何迟到落点。
    }
    if session.saving {
        transform_state.relocation_target = None; // 保存抢占后旧点击意图不能跨快照继续。
        transform_state.relocation_intent = PlacementRelocationIntent::Move;
        editor.status = "地图正在保存，本次资产吸附已取消".to_owned();
        return;
    }
    let target = transform_state
        .relocation_target
        .as_ref()
        .expect("上方已确认资产吸附目标存在")
        .clone(); // 使用按钮按下或复制完成时冻结的身份，不依赖浏览树新签名发布时序。
    if browser.selected_placement_id.as_deref() != Some(target.placement_id.as_str())
        || editor.selected_map_id.as_ref() != Some(&target.map_id)
        || session.draft.as_ref().is_none_or(|draft| draft.map_id != target.map_id)
        || target.structure_revision != session.placement_structure_revision
        || target.authoring_revision != session.revision
    {
        transform_state.relocation_target = None; // 中途改选或发生其他作者动作时禁止误吸附另一项。
        transform_state.relocation_intent = PlacementRelocationIntent::Move;
        editor.status = "待吸附资产选择或作者状态已经变化，本次吸附已取消".to_owned();
        return;
    }
    let Some((is_building, is_clipmap, object_map, is_cuboid)) = session.draft.as_ref().and_then(|draft| {
        draft
            .placements
            .placements
            .get(target.record_index)
            .filter(|record| record.id == target.placement_id)
            .map(|record| {
                let is_building = record.building_preset_id.is_some();
                (
                    is_building,
                    draft.uses_legacy_clipmap_cursor(),
                    draft.manifest.terrains.is_some(),
                    record.asset_target.is_builtin(),
                )
            })
    }) else {
        transform_state.relocation_target = None;
        transform_state.relocation_intent = PlacementRelocationIntent::Move;
        editor.status = "待吸附资产记录已经失效".to_owned();
        return;
    };
    let empty_work_plane = is_cuboid && crate::authoring::basic_shapes::work_plane_is_available(&session);
    let click_snap = if is_clipmap { ground_cursor.frozen_click_snap() } else {
        let origin = session.draft.as_ref().and_then(|draft| draft.placements.placements.get(target.record_index)
            .and_then(|record| crate::authoring::placement_pose::placement_record_world_position(draft, record).ok()))
            .map_or([0.0; 3], |point| point.to_array());
        snap_inputs.0.as_ref().map(|settings| {
            let control = snap_inputs.1.as_ref().is_some_and(|keys| keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]));
            if is_building { building_controls.snap_policy(settings.editor_snapping, control, origin) } else { settings.editor_snapping.translation_snap(control, origin) }
        })
    };
    let mut snapped_terrain = snap_inputs.2.as_ref().and_then(|snap| snap.frozen_terrain()).cloned();
    let position = if empty_work_plane {
        if !clicked_in_viewport { return; }
        let Some(point) = snap_inputs.4.as_ref().and_then(|shapes| shapes.work_plane_point(&session)) else {
            editor.status = "请在空地图的工作平面内重新选择立方体落点".into();
            return;
        };
        point // 独立平面从不伪装真实地形，也不能沿用切图、修订前的点。
    } else if is_clipmap {
        let completion = clipmap_surface
            .as_ref()
            .and_then(|surface| surface.snapshot.as_ref())
            .and_then(|snapshot| {
                if is_building {
                    ground_cursor.take_author_click_result(
                        snapshot,
                        &target.map_id,
                        transform_state.relocation_ground_target(
                            is_building,
                            crate::authoring::buildings::clipmap_controls_identity(
                                &building_controls,
                            ),
                            building_registry
                                .as_deref()
                                .map_or(0, |registry| registry.revision),
                        )?,
                        target.authoring_revision,
                    )
                } else {
                    ground_cursor.take_relocation_click_result(
                        snapshot,
                        &target.map_id,
                        &target.placement_id,
                        target.authoring_revision,
                    )
                }
            }); // 只接受点击时冻结且仍与当前地图、记录和正式高度修订一致的结果。
        let Some(completion) = completion else {
            if clicked_in_viewport {
                editor.status = "正在后台查询Clipmap真实地表；命中后自动完成资产吸附".to_owned();
            }
            return; // 查询在途时保持武装状态，松键后的完成帧仍可采用一次。
        };
        match completion {
            ClipmapGroundHit::Hit(position) => position,
            ClipmapGroundHit::WaitingForHeight => {
                editor.status = "吸附位置的Clipmap高度页尚未就绪，请稍后重新单击".to_owned();
                return;
            }
            ClipmapGroundHit::Miss => {
                editor.status = "本次单击没有命中地图内真实地表，请重新选择吸附落点".to_owned();
                return;
            }
            ClipmapGroundHit::Failed => {
                editor.status = "本次Clipmap地表射线无效，请重新选择吸附落点".to_owned();
                return;
            }
        }
    } else if !is_building && snap_inputs.2.as_ref().is_some_and(|snap| snap.requires_query()) {
        let identity = crate::viewport::vegetation_ground::GroundCursorTarget::RelocatePlacement(target.placement_id.clone());
        let completion = snap_inputs.2.as_mut().and_then(|snap| snap.take_click(&target.map_id, &identity, target.authoring_revision, session.terrain_revision));
        if completion.is_some() { editor.ground_click_world = None; } // 吸附确认用过的原始点击不得再落入非吸附路径。
        match completion {
            Some(ClipmapGroundHit::Hit(point)) => point,
            Some(_) => { editor.status = "吸附网格位置没有命中当前地形承托层，请重新选择落点".into(); return; }
            None => return,
        }
    } else if object_map && !is_building {
        let Some(position) = editor.ground_click_world.take() else {
            if clicked_in_viewport { editor.status = "正在后台查询真实地形；命中后完成资产吸附".into(); }
            return; // 多地形关闭网格吸附时仍只采用正式点击，松键后完成一次，不读取上一射线hover。
        };
        snapped_terrain = snap_inputs.3.as_ref().and_then(|cursor| cursor.click_terrain.as_ref()).cloned();
        position
    } else {
        if !clicked_in_viewport {
            return; // GLB继续只在中央视口左键边沿消费同步命中。
        }
        let Some(position) = editor.cursor_world else {
            editor.status = "视口光标没有命中真实地形，请重新选择吸附落点".to_owned(); // 失败后保留模式，方便在洞壁、斜坡或悬空层上重试。
            return; // 仍保持吸附模式，允许下一次有效点击重试。
        };
        position
    };
    if is_building {
        let world_xz = click_snap.filter(|snap| snap.active()).and_then(|snap| snap.horizontal(position.to_array()))
            .map_or_else(|| building_controls.snapped_xz(position), |point| Vec2::new(point[0], point[2])); // 连接/五点校验仍由原建筑事务受理。
        let frozen = target.building_edit_target();
        let terrain_hit = editor.cursor_triangle.map(|triangle| BuildingTerrainHit {
            position, // 与三角序号来自同一帧相机命中。
            triangle, // 后台据此隔离洞顶、洞底和悬空层。
        });
        let request = match transform_state.relocation_intent {
            PlacementRelocationIntent::Move => BuildingEditRequest::Relocate {
                target: frozen,
                world_xz,
                terrain_hit,
            },
            PlacementRelocationIntent::DuplicateBuilding => BuildingEditRequest::DuplicateAt {
                target: frozen,
                world_xz,
                terrain_hit,
            },
        };
        building_edits.write(request);
        let action = match transform_state.relocation_intent {
            PlacementRelocationIntent::Move => "移动",
            PlacementRelocationIntent::DuplicateBuilding => "复制",
        };
        transform_state.relocation_target = None; // 当前点击已经移交专用single-flight任务。
        transform_state.relocation_intent = PlacementRelocationIntent::Move;
        editor.status = format!("已提交建筑{action}候选，正在后台校验占地、门口与五点地基");
        return; // 校验成功前不改草稿、不改预览、不生成撤销记录。
    }
    let (before, after, bounds) = {
        let Some(draft) = session.draft.as_ref() else {
            transform_state.relocation_target = None;
            transform_state.relocation_intent = PlacementRelocationIntent::Move;
            editor.status = "地图作者草稿尚未就绪".to_owned();
            return;
        };
        let Some(record) = draft
            .placements
            .placements
            .get(target.record_index)
            .filter(|record| record.id == target.placement_id)
        else {
            transform_state.relocation_target = None;
            transform_state.relocation_intent = PlacementRelocationIntent::Move;
            editor.status = "待吸附资产记录已经失效".to_owned();
            return;
        };
        let before = record.clone();
        if before.terrain_patch.is_some()
            && let Err(error) = crate::authoring::patch_preview_parent(draft, &snap_inputs.5, &before) {
            editor.status = format!("资产吸附失败：{error}");
            return;
        } // 点击快照冻结后旧源仍可能退役；先确认旧父仍有效，再按其作者框架计算跨源候选。
        let (after, bounds) = match relocation_candidate(draft, &before, position, snapped_terrain.as_ref(), empty_work_plane) {
            Ok(candidate) => candidate,
            Err(error) => { editor.status = format!("资产吸附失败：{error}"); return; }
        };
        (before, after, bounds) // 候选转换与验证不修改作者数组。
    };
    transform_state.relocation_target = None; // 当前点击已经形成明确结果。
    transform_state.relocation_intent = PlacementRelocationIntent::Move;
    if before == after {
        editor.status = format!("资产 {} 已位于该吸附落点", after.id);
        return; // 同位置点击不制造撤销或dirty。
    }
    if let Err(error) = sync_placement_record_world_with_terrain(
        &mut commands, // 写入当前可见根。
        &asset_server, // 缺根时补作者预览。
        &mut session,  // 维护补根所有权。
        &root_index,   // 定向正式根。
        &roots,        // 复核活动身份。
        &after,        // 当前完整草稿记录。
        bounds,        // 当前地图世界范围。
        true,          // 吸附结果保持可见。
        true,          // 根尚未生成时补预览。
        &snap_inputs.5,
    ) {
        if let Some(record) = session
            .draft
            .as_mut()
            .and_then(|draft| draft.placements.placements.get_mut(target.record_index))
            .filter(|record| record.id == before.id)
        {
            *record = before; // ECS同步失败时恢复草稿完整起点。
        }
        editor.status = format!("资产吸附预览失败：{error}");
        return;
    }
    session.draft.as_mut().expect("吸附候选已复核草稿")
        .placements.placements[target.record_index] = after.clone();
    let session_history = &mut *session; // 拆借ResMut内部两个独立历史字段。
    session_history.push_bounded_water_history(EditorCommand::PlacementChanged {
        record_index: target.record_index, // O(1)恢复同一记录槽。
        before,                            // 单击前完整记录。
        after: after.clone(),              // 单击后完整受检记录。
    }); // 纯变换不会被误计为网格水命令，但仍复用统一分支处理。
    session.mark_changed(&mut editor); // placements JSON进入保存门但不重建GLB浏览结构。
    editor.status = format!(
        "资产 {} 已吸附地面 {:.3}, {:.3}, {:.3}",
        after.id, after.translation[0], after.translation[1], after.translation[2],
    );
}

#[cfg(test)]
#[path = "relocation_interaction_tests.rs"]
mod interaction_tests;

#[cfg(test)] // 纯记录语义回归不进入正式编译产物。
mod tests {
    use super::*; // 直接测试当前模块的私有吸附帮助函数。
    use feathrise_map_content::MapPlacementHeightMode; // 测试同时覆盖跟随与固定高度模式。

    #[test]
    fn terrain_patch_relocation_rebinds_exact_target_and_rejects_incompatible_frames() {
        let mut draft = crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![2; 121]));
        let mut source = draft.manifest.terrain_objects_for_edit()[0].clone();
        source.id = feathrise_map_content::TerrainId::parse("patch-source").unwrap();
        source.terrain.kind = feathrise_map_content::MapTerrainKind::SparseClipmap;
        source.world_min = [-200.0; 2]; source.world_max = [200.0; 2];
        source.transform = feathrise_map_content::TerrainObjectTransform {
            translation: [-18.0, 5.0, 27.0], yaw_degrees: 31.0,
            pitch_degrees: 17.0, roll_degrees: -12.0, scale: [2.0, 3.0, 5.0],
        };
        let mut target = source.clone();
        target.id = feathrise_map_content::TerrainId::parse("patch-target").unwrap();
        target.transform.translation = [-14.0, 8.0, 24.0];
        draft.manifest.terrains = Some(std::sync::Arc::from([source.clone(), target.clone()]));
        draft.manifest.terrain = feathrise_map_content::MapTerrainDescriptor::empty();
        let mut before = fixed_record(); before.water_surfaces.clear();
        before.terrain_patch = Some(feathrise_map_content::MapPlacementTerrainPatch {
            terrain_id: Some(source.id.clone()), ..Default::default()
        });
        let local = |record: &MapPlacementRecord| Transform::from_translation(record.translation.into())
            .with_rotation(Quat::from_rotation_y(record.yaw_degrees.to_radians())).with_scale(record.scale.into()).to_matrix();
        let world = source.transform.as_transform().to_matrix() * local(&before);
        let hit = world.w_axis.truncate() + Vec3::new(3.0, -2.0, 5.0);
        let (after, _) = relocation_candidate(&draft, &before, hit, Some(&target.id), false).unwrap();
        assert!(after.terrain_binding.is_none());
        assert_eq!(after.terrain_patch.as_ref().unwrap().terrain_id.as_ref(), Some(&target.id));
        let actual = target.transform.as_transform().to_matrix() * local(&after);
        let expected = Mat4::from_translation(hit - world.w_axis.truncate()) * world;
        for point in [Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z] {
            assert!(actual.transform_point3(point).abs_diff_eq(expected.transform_point3(point), 0.001));
        }
        let snapshot = before.clone();
        for invalid in 0..3 {
            let object = &mut std::sync::Arc::make_mut(draft.manifest.terrains.as_mut().unwrap())[1];
            object.transform.scale = if invalid == 0 { [5.0, 3.0, 2.0] } else { target.transform.scale };
            object.enabled = invalid != 1;
            object.terrain.kind = if invalid == 2 { feathrise_map_content::MapTerrainKind::Heightfield } else { feathrise_map_content::MapTerrainKind::SparseClipmap };
            assert!(relocation_candidate(&draft, &before, hit, Some(&target.id), false).is_err());
            assert_eq!(before, snapshot);
        }
        assert!(relocation_candidate(&draft, &before, hit, None, false).is_err());
    }

    #[test]
    fn editable_relocation_uses_changed_bottom_with_object_scale() {
        let mut record = following_record();
        record.asset_path.clear();
        record.asset_target = feathrise_map_content::MapPlacementAssetTarget::DEFAULT_CUBOID.into_editable().unwrap();
        if let feathrise_map_content::MapPlacementAssetTarget::EditableCuboid { vertices_half_mm, .. } = &mut record.asset_target {
            for index in [0, 1, 4, 5] { vertices_half_mm[index][1] = -4_000; }
        }
        record.asset_target.validate().unwrap();
        let target = record.asset_target;
        apply_ground_snap_hit(&mut record, Vec3::new(12.0, -6.25, 18.0));
        assert_eq!(record.translation, [12.0, -0.25, 18.0]);
        assert_eq!(record.terrain_offset, 6.0);
        assert_eq!(record.asset_target, target);
    }

    /// 构造一条具有非零离地偏移的跟随型资产。
    fn following_record() -> MapPlacementRecord {
        MapPlacementRecord {
            mesh_lod: None,
            terrain_binding: None,
            editor: Default::default(),
            terrain_patch: None,
            id: "ground-snap-following".to_owned(), // 稳定跟随测试身份。
            asset_path: "gltf/test.glb".to_owned(), // 测试不实际加载Scene0。
            asset_target: Default::default(),       // 吸附不改变文件内目标。
            building_preset_id: None,               // 普通吸附夹具不引用建筑预设。
            courtyard: None,                        // 独立摆放记录不属于任何院落副本。
            translation: [1.0, 25.0, 3.0],          // 旧位置故意与新命中不同。
            height_mode: MapPlacementHeightMode::FollowTerrain, // 跟随模式落地后必须归零偏移。
            terrain_offset: 4.5,                    // 非零起点能证明按钮真正完成贴地。
            yaw_degrees: 37.0,                      // 吸附不得改变旋转。
            scale: [2.0, 3.0, 4.0],                 // 吸附不得改变三轴缩放。
            water_surfaces: Vec::new(),             // 吸附不触及整网格水语义。
        } // 完成跟随型测试记录。
    }

    /// 构造一条只保持世界Y的固定高度资产。
    fn fixed_record() -> MapPlacementRecord {
        MapPlacementRecord {
            mesh_lod: None,
            terrain_binding: None,
            editor: Default::default(),
            terrain_patch: None,
            id: "ground-snap-fixed".to_owned(), // 稳定固定高度测试身份。
            asset_path: "gltf/test.glb".to_owned(), // 测试不实际加载Scene0。
            asset_target: Default::default(),   // 吸附不改变文件内目标。
            building_preset_id: None,           // 固定高度夹具不引用建筑预设。
            courtyard: None,                    // 独立摆放记录不属于任何院落副本。
            translation: [7.0, 120.0, 9.0],     // 高处起点可证明命中Y真正被采用。
            height_mode: MapPlacementHeightMode::FixedHeight, // 吸附完成后仍保持固定高度语义。
            terrain_offset: 0.0,                // 固定高度按正式协议不携带偏移。
            yaw_degrees: 145.0,                 // 吸附不得改变旋转。
            scale: [0.5, 1.5, 2.5],             // 吸附不得改变三轴缩放。
            water_surfaces: Vec::new(),         // 吸附不触及整网格水语义。
        } // 完成固定高度测试记录。
    }

    #[test] // 洞穴、倒扣或多层地形的射线命中必须原样成为新资产原点。
    fn ground_snap_uses_exact_viewport_surface_hit_and_clears_follow_offset() {
        let mut record = following_record(); // 从悬空的跟随型资产开始。
        let before_yaw = record.yaw_degrees; // 冻结非位置属性用于回归。
        let before_scale = record.scale; // 冻结完整三轴缩放用于回归。
        let hit = Vec3::new(-18.25, -43.75, 91.5); // 使用负高度证明不做平面或最高包络重采样。

        apply_ground_snap_hit(&mut record, hit); // 应用与首次摆放相同的视口命中。

        assert_eq!(record.translation, hit.to_array()); // 三维命中不经二次高度猜测。
        assert_eq!(record.terrain_offset, 0.0); // 跟随型资产从命中表面开始继续跟随。
        assert_eq!(record.height_mode, MapPlacementHeightMode::FollowTerrain); // 一次吸附不改写作者选定的持久模式。
        assert_eq!(record.yaw_degrees, before_yaw); // 旋转保持不变。
        assert_eq!(record.scale, before_scale); // 缩放保持不变。
    } // 完成真实地形命中和跟随偏移回归。

    #[test] // 固定高度资产同样应使用命中Y，但不应被隐式改为跟随模式。
    fn ground_snap_moves_fixed_height_asset_to_hit_without_changing_mode() {
        let mut record = fixed_record(); // 从高处的固定高度资产开始。
        let before_yaw = record.yaw_degrees; // 冻结非位置属性用于回归。
        let before_scale = record.scale; // 冻结完整三轴缩放用于回归。
        let hit = Vec3::new(12.0, 6.25, -64.0); // 使用任意真实三维地形命中。

        apply_ground_snap_hit(&mut record, hit); // 应用与首次摆放相同的视口命中。

        assert_eq!(record.translation, hit.to_array()); // 固定高度也采用当前命中Y。
        assert_eq!(record.terrain_offset, 0.0); // 固定模式继续没有隐藏偏移。
        assert_eq!(record.height_mode, MapPlacementHeightMode::FixedHeight); // 持久高度模式保持不变。
        assert_eq!(record.yaw_degrees, before_yaw); // 旋转保持不变。
        assert_eq!(record.scale, before_scale); // 缩放保持不变。
    } // 完成固定高度资产吸附回归。

    #[test]
    fn cuboid_relocation_places_scaled_bottom_on_the_real_hit_and_retains_follow_offset() {
        let mut record = following_record();
        record.asset_path.clear();
        record.asset_target = feathrise_map_content::MapPlacementAssetTarget::DEFAULT_CUBOID;
        let hit = Vec3::new(12.0, -6.25, 18.0);
        apply_ground_snap_hit(&mut record, hit);
        assert_eq!(record.translation, [12.0, -3.25, 18.0]);
        assert_eq!(record.terrain_offset, 3.0);
        assert!(record.height_mode.follows_terrain());
    }
}
