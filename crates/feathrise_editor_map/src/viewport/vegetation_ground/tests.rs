//! 身份与连续移动回归无需真实GPU。
use super::*;
use crate::game_authoring::map_interaction::fixtures; // 检查生产请求合并和采用方法。
fn context() -> GroundCursorContext {
    // 合法固定地图视图身份。
    GroundCursorContext {
        map_id: MapId("ground".into()),
        target: Some(GroundCursorTarget::Vegetation("grass".into())),
        placement_snap: None,
        terrain_revision: 11,
        surface_revision: [1; 3],
        missing_pages: [UVec4::ZERO; 16],
        camera_transform: [0; 16],
        view: EditorView::Top,
        orthographic: true,
        extent: 1,
        render_size: UVec2::ONE,
        input_epoch: 1,
        held: true,
        modifiers: 0,
        authoring_revision: None,
    } // 每个字段可独立换代。
} // 不构造场景私有页结构。
fn ray(x: f32) -> Ray3d {
    Ray3d::new(Vec3::new(x, 10., 0.), Dir3::NEG_Y)
} // 拖动改变射线起点。

#[test]
fn brush_preview_identity_comes_from_the_completed_hit() {
    let captured = context();
    let mut cursor = VegetationGroundCursor::default();
    cursor.retarget(captured.clone(), ray(3.0));
    cursor.accepted = Some(GroundCompletion {
        terrain_mesh: false, request: cursor.desired.clone().unwrap(),
        result: ClipmapGroundHit::Hit(Vec3::ZERO), point: Some(Vec3::ZERO),
    });
    let (map, height, revision, pages) = cursor.accepted_preview_identity().unwrap();
    assert_eq!(map, &captured.map_id);
    assert_eq!(height, captured.terrain_revision);
    assert_eq!(revision, captured.surface_revision);
    assert_eq!(pages, captured.missing_pages);
    cursor.accepted.as_mut().unwrap().point = None;
    assert!(cursor.accepted_preview_identity().is_none(), "未命中不能给语义预览发布来源身份");
}

#[test]
fn placement_click_freezes_control_but_cancels_when_snap_preferences_change() {
    let mut clicked = context();
    clicked.target = Some(GroundCursorTarget::PlaceAsset { asset_path: "gltf/test.glb".into(), asset_target: Default::default() });
    clicked.authoring_revision = Some(7); clicked.modifiers = 2;
    clicked.placement_snap = Some(feathrise_editor_core::EditorSnapSettings::default().translation_snap(true, [0.0; 3]));
    let mut cursor = VegetationGroundCursor::default(); cursor.queue_author_click(clicked.clone(), ray(3.0));
    let mut released = clicked.clone(); released.held = false; released.modifiers = 0;
    released.placement_snap = Some(feathrise_editor_core::EditorSnapSettings::default().translation_snap(false, [0.0; 3]));
    cursor.retain_author_click_scope(&released);
    assert!(cursor.pending_author_click.is_some());
    released.placement_snap = Some(feathrise_editor_core::EditorSnapSettings { translation_meters: 0.5, ..Default::default() }.translation_snap(false, [0.0; 3]));
    cursor.retain_author_click_scope(&released);
    assert!(cursor.pending_author_click.is_none());
}

fn paint_targets() -> [GroundCursorTarget; 2] {
    [
        GroundCursorTarget::Surface {
            layer_id: "soil-a".into(),
            preset_id: Some("soil".into()),
        },
        GroundCursorTarget::Puddles,
    ]
}

fn domain_and_portal_targets() -> [GroundCursorTarget; 5] {
    [
        fixtures::ground_target("point-layer-a-preset-a", false, "test.actor"),
        fixtures::ground_target("select-ticket-5", false, "test.actor"),
        fixtures::ground_target("move-source-a-ticket-5", false, "test.actor"),
        GroundCursorTarget::Portal(MapPortalTarget::default_spawn(MapId("destination".into()))),
        GroundCursorTarget::Portal(MapPortalTarget::portal(MapId("destination".into()), "gate-a")),
    ]
}

#[test]
fn domain_click_finishes_once_after_release_and_rejects_complete_identity_changes() {
    let target = fixtures::ground_target("route-source-a-ticket-5-generation-1-environment-2", false, "test.route");
    for variant in 0..9 {
        let mut clicked = context(); clicked.target = Some(target.clone()); clicked.authoring_revision = Some(7);
        let mut cursor = VegetationGroundCursor::default(); cursor.queue_author_click(clicked.clone(), ray(3.0));
        let request = cursor.pending_author_click.take().unwrap();
        let point = Vec3::new(3.0, 12.0, 0.0);
        cursor.completed_author_click = Some(GroundCompletion { terrain_mesh: false, request, result: ClipmapGroundHit::Hit(point), point: Some(point) });
        let mut released = clicked.clone(); released.held = false; released.input_epoch += 1;
        cursor.retain_author_click_scope(&released);
        let mut expected = target.clone();
        let mut ready = true; let mut map = clicked.map_id.clone(); let mut revision = 7; let mut height = Some(11); let mut surface = [1; 3];
        match variant {
            1..=4 => if let GroundCursorTarget::Domain(target) = &mut expected { target.identity = format!("changed-{variant}"); },
            5 => revision += 1,
            6 => height = Some(12),
            7 => map = MapId("other".into()),
            8 => { ready = false; surface = [2; 3]; },
            _ => {},
        }
        let result = cursor.take_placement_click_result_for_identity(ready, surface, &[UVec4::ZERO; 16], height, &map, expected.clone(), revision);
        assert_eq!(result, (variant == 0).then_some(ClipmapGroundHit::Hit(point)), "case {variant}");
        assert!(cursor.take_placement_click_result_for_identity(true, [1; 3], &[UVec4::ZERO; 16], Some(11), &clicked.map_id, expected, 7).is_none());
    }
}

fn building_point_targets() -> [GroundCursorTarget; 5] {
    [
        GroundCursorTarget::Building {
            preset_id: "house".into(),
            controls: [1, 2, 0, 0],
            catalog_revision: 3,
        },
        GroundCursorTarget::RelocateBuilding {
            placement_id: "house-1".into(),
            duplicate: false,
            controls: [1, 2, 0, 0],
            catalog_revision: 3,
        },
        GroundCursorTarget::RelocateBuilding {
            placement_id: "house-1".into(),
            duplicate: true,
            controls: [1, 2, 0, 0],
            catalog_revision: 3,
        },
        GroundCursorTarget::Courtyard {
            mode: 2,
            preset_id: Some("yard".into()),
            selected_id: None,
            quarter_turns: 1,
            catalogs: [3, 4],
        },
        GroundCursorTarget::Courtyard {
            mode: 3,
            preset_id: None,
            selected_id: Some("house-1".into()),
            quarter_turns: 0,
            catalogs: [3, 4],
        },
    ]
}

#[test]
fn every_author_tool_cancels_only_its_own_click_in_all_waiting_stages() {
    type Cancel = fn(&mut VegetationGroundCursor);
    let owners: [(Cancel, Vec<GroundCursorTarget>); 8] = [
        (
            |cursor| cursor.cancel_domain_click("test.route"),
            vec![fixtures::ground_target("route-source-a-ticket-5-generation-1-environment-2", false, "test.route")],
        ),
        (
            VegetationGroundCursor::cancel_spline_click,
            vec![
                GroundCursorTarget::River("river-a".into()),
                GroundCursorTarget::Waterfall("fall-a".into()),
            ],
        ),
        (
            VegetationGroundCursor::cancel_placement_click,
            vec![
                GroundCursorTarget::PlaceAsset {
                    asset_path: "gltf/tree.glb".into(),
                    asset_target: MapPlacementAssetTarget::Scene0,
                },
                GroundCursorTarget::RelocatePlacement("tree-a".into()),
            ],
        ),
        (
            VegetationGroundCursor::cancel_portal_click,
            domain_and_portal_targets()
                .into_iter()
                .filter(|target| matches!(target, GroundCursorTarget::Portal(_)))
                .collect(),
        ),
        (
            |cursor| cursor.cancel_domain_click("test.actor"),
            domain_and_portal_targets()
                .into_iter()
                .filter(|target| !matches!(target, GroundCursorTarget::Portal(_)))
                .collect(),
        ),
        (
            VegetationGroundCursor::cancel_boundary_click,
            vec![
                GroundCursorTarget::BoundaryPoint {
                    path_id: "wall-a".into(),
                    insert: false,
                },
                GroundCursorTarget::BoundaryPoint {
                    path_id: "wall-a".into(),
                    insert: true,
                },
            ],
        ),
        (
            VegetationGroundCursor::cancel_building_click,
            building_point_targets()
                .into_iter()
                .filter(|target| !matches!(target, GroundCursorTarget::Courtyard { .. }))
                .collect(),
        ),
        (
            VegetationGroundCursor::cancel_courtyard_click,
            building_point_targets()
                .into_iter()
                .filter(|target| matches!(target, GroundCursorTarget::Courtyard { .. }))
                .collect(),
        ),
    ];
    for (request_owner, (_, targets)) in owners.iter().enumerate() {
        for target in targets {
            for (cancelling_owner, (cancel, _)) in owners.iter().enumerate() {
                for stage in 0..3 {
                    let mut clicked = context();
                    clicked.target = Some(target.clone());
                    clicked.authoring_revision = Some(7);
                    let mut cursor = VegetationGroundCursor::default();
                    cursor.queue_author_click(clicked, ray(3.));
                    let request = cursor.pending_author_click.clone().unwrap();
                    let completion = GroundCompletion {
                        terrain_mesh: false,
                        request: request.clone(),
                        result: ClipmapGroundHit::Hit(Vec3::new(3., 12.5, 0.)),
                        point: Some(Vec3::new(3., 12.5, 0.)),
                    };
                    if stage == 1 {
                        cursor.pending_author_surface = Some(completion);
                    } else if stage == 2 {
                        cursor.pending_author_click = None;
                        cursor.completed_author_click = Some(completion);
                    }
                    cancel(&mut cursor);
                    let retained = match stage {
                        0 => cursor.pending_author_click.as_ref(),
                        1 => cursor
                            .pending_author_surface
                            .as_ref()
                            .map(|completion| &completion.request),
                        _ => cursor
                            .completed_author_click
                            .as_ref()
                            .map(|completion| &completion.request),
                    };
                    assert_eq!(
                        retained,
                        (request_owner != cancelling_owner).then_some(&request),
                        "错误取消了{target:?}的阶段{stage}，取消域{cancelling_owner}"
                    );
                }
            }
        }
    }
}

#[test]
fn other_author_tools_cannot_consume_completed_building_or_courtyard_clicks() {
    for target in building_point_targets() {
        let mut clicked = context();
        clicked.target = Some(target.clone());
        clicked.authoring_revision = Some(7);
        let mut ground = VegetationGroundCursor::default();
        ground.queue_author_click(clicked.clone(), ray(3.));
        let point = Vec3::new(3., 12., 0.);
        ground.completed_author_click = Some(GroundCompletion {
            terrain_mesh: false,
            request: ground.pending_author_click.take().unwrap(),
            result: ClipmapGroundHit::Hit(point),
            point: Some(point),
        });
        for other in domain_and_portal_targets()
            .into_iter()
            .chain(building_point_targets())
        {
            if std::mem::discriminant(&target) == std::mem::discriminant(&other) {
                continue; // 同域预设或目标换代继续服从既有消费后拒收门。
            }
            assert!(
                ground
                    .take_placement_click_result_for_identity(
                        true,
                        clicked.surface_revision,
                        &clicked.missing_pages,
                        Some(clicked.terrain_revision),
                        &clicked.map_id,
                        other,
                        7,
                    )
                    .is_none()
            );
            assert!(ground.completed_author_click.is_some());
        }
        assert!(
            ground
                .take_spline_click_result_for_identity(
                    true,
                    clicked.surface_revision,
                    &clicked.missing_pages,
                    Some(clicked.terrain_revision),
                    &clicked.map_id,
                    "river-a",
                    false,
                    7,
                )
                .is_none()
        );
        for expected in [Some(ClipmapGroundHit::Hit(point)), None] {
            assert_eq!(
                ground.take_placement_click_result_for_identity(
                    true,
                    clicked.surface_revision,
                    &clicked.missing_pages,
                    Some(clicked.terrain_revision),
                    &clicked.map_id,
                    target.clone(),
                    7,
                ),
                expected
            ); // 正确用途最后仍能消费一次精确结果。
        }
    }
}

#[test]
fn building_and_courtyard_clicks_survive_release_and_consume_exact_height_once() {
    for target in building_point_targets() {
        let mut clicked = context();
        clicked.target = Some(target.clone());
        clicked.authoring_revision = Some(7);
        let mut state = VegetationGroundCursor::default();
        state.queue_author_click(clicked.clone(), ray(3.));
        let mut completion = GroundCompletion {
            terrain_mesh: false,
            request: state.pending_author_click.clone().unwrap(),
            result: ClipmapGroundHit::Hit(Vec3::new(3., 999., 0.)),
            point: Some(Vec3::new(3., 999., 0.)),
        };
        assert!(!VegetationGroundCursor::apply_authoritative_result(
            &mut completion,
            ClipmapGroundHit::WaitingForHeight,
            true
        ));
        state.pending_author_surface = Some(completion);
        let mut released = clicked.clone();
        released.held = false;
        released.input_epoch += 1;
        state.retarget(released.clone(), ray(8.));
        state.retain_author_click_scope(&released);
        let point = Vec3::new(3., 12., 0.);
        assert!(VegetationGroundCursor::apply_authoritative_result(
            state.pending_author_surface.as_mut().unwrap(),
            ClipmapGroundHit::Hit(point),
            true
        ));
        state.completed_author_click = state.pending_author_surface.take();
        assert_eq!(
            state.take_placement_click_result_for_identity(
                true,
                clicked.surface_revision,
                &clicked.missing_pages,
                Some(clicked.terrain_revision),
                &clicked.map_id,
                target.clone(),
                7
            ),
            Some(ClipmapGroundHit::Hit(point))
        );
        assert!(
            state
                .take_placement_click_result_for_identity(
                    true,
                    clicked.surface_revision,
                    &clicked.missing_pages,
                    Some(clicked.terrain_revision),
                    &clicked.map_id,
                    target,
                    7
                )
                .is_none()
        );
    }
}

#[test]
fn building_clicks_do_not_cross_presets_parameters_or_courtyard_purposes() {
    for target in building_point_targets() {
        for other in building_point_targets()
            .into_iter()
            .filter(|other| *other != target)
        {
            let mut clicked = context();
            clicked.target = Some(target.clone());
            clicked.authoring_revision = Some(7);
            let mut state = VegetationGroundCursor::default();
            state.queue_author_click(clicked.clone(), ray(3.));
            let mut changed = clicked.clone();
            changed.target = Some(other);
            state.retain_author_click_scope(&changed);
            assert!(state.pending_author_click.is_none());
        }
    }
}

#[test]
fn building_click_catalog_and_each_control_change_cancel_the_frozen_ray() {
    for target in building_point_targets() {
        for change in 0..5 {
            let mut clicked = context();
            clicked.target = Some(target.clone());
            clicked.authoring_revision = Some(7);
            let mut state = VegetationGroundCursor::default();
            state.queue_author_click(clicked.clone(), ray(3.));
            let mut changed = clicked;
            match changed.target.as_mut().unwrap() {
                GroundCursorTarget::Building {
                    controls,
                    catalog_revision,
                    ..
                }
                | GroundCursorTarget::RelocateBuilding {
                    controls,
                    catalog_revision,
                    ..
                } => {
                    if change < 4 {
                        controls[change] ^= 1;
                    } else {
                        *catalog_revision += 1;
                    }
                }
                GroundCursorTarget::Courtyard {
                    catalogs,
                    quarter_turns,
                    mode,
                    ..
                } => {
                    if change < 2 {
                        catalogs[change] += 1;
                    } else if change == 2 {
                        *quarter_turns += 1;
                    } else {
                        *mode += 1;
                    }
                }
                _ => unreachable!(),
            }
            state.retain_author_click_scope(&changed);
            assert!(state.pending_author_click.is_none());
        }
    }
}

#[test]
fn boundary_click_waits_for_base_height_freezes_insert_and_survives_mouse_release_once() {
    for insert in [false, true] {
        let mut clicked = context();
        let target = GroundCursorTarget::BoundaryPoint {
            path_id: "wall-a".into(),
            insert,
        };
        clicked.target = Some(target.clone());
        clicked.authoring_revision = Some(7);
        clicked.modifiers = u8::from(insert) << 1;
        let mut state = VegetationGroundCursor::default();
        state.queue_author_click(clicked.clone(), ray(3.));
        let mut completion = GroundCompletion {
            terrain_mesh: false,
            request: state.pending_author_click.clone().unwrap(),
            result: ClipmapGroundHit::Hit(Vec3::new(3., 40., 0.)),
            point: Some(Vec3::new(3., 40., 0.)),
        };
        assert!(!VegetationGroundCursor::apply_authoritative_result(
            &mut completion,
            ClipmapGroundHit::WaitingForHeight,
            true
        ));
        state.pending_author_surface = Some(completion);
        let mut released = clicked.clone();
        released.held = false;
        released.input_epoch += 1;
        released.camera_transform[1] += 1;
        state.retarget(released.clone(), ray(8.));
        state.retain_author_click_scope(&released);
        let point = Vec3::new(3., 12.5, 0.);
        assert!(VegetationGroundCursor::apply_authoritative_result(
            state.pending_author_surface.as_mut().unwrap(),
            ClipmapGroundHit::Hit(point),
            true
        ));
        state.completed_author_click = state.pending_author_surface.take();
        for expected in [Some(ClipmapGroundHit::Hit(point)), None] {
            assert_eq!(
                state.take_placement_click_result_for_identity(
                    true,
                    clicked.surface_revision,
                    &clicked.missing_pages,
                    Some(clicked.terrain_revision),
                    &clicked.map_id,
                    target.clone(),
                    7
                ),
                expected
            );
        }
    }
}

#[test]
fn boundary_late_click_is_cancelled_by_wall_insert_drag_and_author_identity_changes() {
    let mut clicked = context();
    clicked.target = Some(GroundCursorTarget::BoundaryPoint {
        path_id: "wall-a".into(),
        insert: false,
    });
    clicked.authoring_revision = Some(7);
    for variant in 0..8 {
        let mut state = VegetationGroundCursor::default();
        state.queue_author_click(clicked.clone(), ray(3.));
        let mut changed = clicked.clone();
        match variant {
            0 => {
                changed.target = Some(GroundCursorTarget::BoundaryPoint {
                    path_id: "wall-b".into(),
                    insert: false,
                })
            }
            1 => {
                changed.target = Some(GroundCursorTarget::BoundaryPoint {
                    path_id: "wall-a".into(),
                    insert: true,
                })
            }
            2 => {
                changed.target = Some(GroundCursorTarget::BoundaryDrag {
                    path_id: "wall-a".into(),
                    point_index: 0,
                })
            }
            3 => changed.map_id = MapId("other".into()),
            4 => changed.authoring_revision = Some(8),
            5 => changed.terrain_revision += 1,
            6 => changed.surface_revision[0] += 1,
            _ => changed.missing_pages[0] = UVec4::ONE,
        }
        state.retarget(changed.clone(), ray(4.));
        state.retain_author_click_scope(&changed);
        assert!(state.pending_author_click.is_none());
    }
}

#[test]
fn boundary_drag_requires_fresh_authoritative_sample_and_frozen_control_point() {
    let target = GroundCursorTarget::BoundaryDrag {
        path_id: "wall-a".into(),
        point_index: 0,
    };
    let (mut state, mut identity) = paint_cursor(target);
    assert_eq!(paint_hit(&state, &identity), None); // 40米粗显示高度不能提供可写XZ。
    let point = Vec3::new(0., 12.5, 0.);
    VegetationGroundCursor::apply_authoritative_result(
        state.accepted.as_mut().unwrap(),
        ClipmapGroundHit::Hit(point),
        false,
    );
    state.accepted_authority_ready = true;
    assert_eq!(paint_hit(&state, &identity), Some(point));
    state.paint_ready = false;
    assert_eq!(paint_hit(&state, &identity), None); // 显示保留点不是本帧拖点证据。
    state.paint_ready = true;
    let mut other = identity.clone();
    other.target = Some(GroundCursorTarget::BoundaryDrag {
        path_id: "wall-a".into(),
        point_index: 1,
    });
    assert_eq!(paint_hit(&state, &other), None);
    identity.modifiers = 2;
    state.retarget(identity.clone(), ray(0.));
    state.adopt(GroundCompletion {
        terrain_mesh: false,
        request: state.desired.clone().unwrap(),
        result: ClipmapGroundHit::Hit(point),
        point: Some(point),
    });
    assert_eq!(paint_hit(&state, &identity), None); // Ctrl插点不能借旧拖点输入移动原点。
}

#[test]
fn boundary_blocked_input_discards_frozen_click_instead_of_replaying_after_unlock() {
    use crate::authoring::{BoundaryEditState, EditorAuthoringSession};
    use crate::resources::{MapEditorNumericScrubMouseGuard, MapEditorState};
    use crate::viewport::modes::ViewportModeState;
    for variant in 0..5 {
        let mut clicked = context();
        clicked.target = Some(GroundCursorTarget::BoundaryPoint {
            path_id: "wall-a".into(),
            insert: false,
        });
        clicked.authoring_revision = Some(7);
        let mut state = VegetationGroundCursor::default();
        state.queue_author_click(clicked, ray(3.));
        let mut app = App::new();
        app.insert_resource(state)
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<BoundaryEditState>()
            .init_resource::<EditorAuthoringSession>()
            .init_resource::<MapEditorNumericScrubMouseGuard>()
            .init_resource::<ViewportModeState>()
            .init_resource::<feathrise_editor_ui::ActiveNumericInput>()
            .insert_resource(MapEditorState {
                open: true,
                tool: EditorTool::Boundary,
                ..default()
            })
            .add_systems(
                Update,
                crate::authoring::boundary::edit_boundary_from_viewport,
            );
        let viewport = app
            .world_mut()
            .spawn((
                crate::components::MapEditorViewport,
                bevy::ui::RelativeCursorPosition {
                    cursor_over: true,
                    normalized: Some(Vec2::ZERO),
                },
            ))
            .id();
        match variant {
            0 => {
                app.world_mut()
                    .resource_mut::<ViewportModeState>()
                    .pointer_over_controls = true
            }
            1 => app
                .world_mut()
                .resource_mut::<MapEditorNumericScrubMouseGuard>()
                .advance(true, true, false),
            2 => {
                app.world_mut()
                    .resource_mut::<MapEditorState>()
                    .save_requested = true
            }
            3 => {
                app.world_mut()
                    .resource_mut::<MapEditorState>()
                    .close_requested = true
            }
            _ => {
                app.world_mut()
                    .resource_mut::<feathrise_editor_ui::ActiveNumericInput>()
                    .entity = Some(viewport)
            }
        }
        app.update();
        let state = app.world().resource::<VegetationGroundCursor>();
        assert!(
            state.pending_author_click.is_none()
                && state.pending_author_surface.is_none()
                && state.completed_author_click.is_none()
        );
        *app.world_mut().resource_mut::<ViewportModeState>() = default();
        app.world_mut()
            .resource_mut::<MapEditorNumericScrubMouseGuard>()
            .advance(false, false, false);
        app.world_mut()
            .resource_mut::<MapEditorNumericScrubMouseGuard>()
            .advance(false, false, false);
        *app.world_mut()
            .resource_mut::<feathrise_editor_ui::ActiveNumericInput>() = default();
        {
            let mut editor = app.world_mut().resource_mut::<MapEditorState>();
            editor.save_requested = false;
            editor.close_requested = false;
        }
        app.update();
        assert!(
            app.world()
                .resource::<VegetationGroundCursor>()
                .pending_author_click
                .is_none()
        );
        assert!(
            app.world()
                .resource::<EditorAuthoringSession>()
                .undo
                .is_empty()
        );
    }
}

#[test]
fn domain_and_portal_click_wait_for_base_height_survive_release_and_consume_once() {
    for target in domain_and_portal_targets() {
        let mut state = VegetationGroundCursor::default();
        let mut clicked = context();
        clicked.target = Some(target.clone());
        clicked.authoring_revision = Some(7);
        state.queue_author_click(clicked.clone(), ray(3.));
        let mut completion = GroundCompletion {
            terrain_mesh: false,
            request: state.pending_author_click.clone().unwrap(),
            result: ClipmapGroundHit::Hit(Vec3::new(3., 40., 0.)),
            point: Some(Vec3::new(3., 40., 0.)),
        };
        assert!(!VegetationGroundCursor::apply_authoritative_result(
            &mut completion,
            ClipmapGroundHit::WaitingForHeight,
            true
        ));
        state.pending_author_surface = Some(completion);
        let mut released = clicked.clone();
        released.held = false;
        released.input_epoch += 1;
        state.retarget(released.clone(), ray(5.));
        state.retain_author_click_scope(&released);
        assert!(state.pending_author_surface.is_some());
        let point = Vec3::new(3., 12.5, 0.);
        assert!(VegetationGroundCursor::apply_authoritative_result(
            state.pending_author_surface.as_mut().unwrap(),
            ClipmapGroundHit::Hit(point),
            true
        ));
        state.completed_author_click = state.pending_author_surface.take();
        assert_eq!(
            state.take_placement_click_result_for_identity(
                true,
                clicked.surface_revision,
                &clicked.missing_pages,
                Some(clicked.terrain_revision),
                &clicked.map_id,
                target.clone(),
                7
            ),
            Some(ClipmapGroundHit::Hit(point))
        );
        assert_eq!(
            state.take_placement_click_result_for_identity(
                true,
                clicked.surface_revision,
                &clicked.missing_pages,
                Some(clicked.terrain_revision),
                &clicked.map_id,
                target,
                7
            ),
            None
        );
    }
}

#[test]
fn domain_and_portal_late_click_cannot_cross_author_or_destination_identity() {
    for target in domain_and_portal_targets() {
        for variant in 0..12 {
            let mut state = VegetationGroundCursor::default();
            let mut clicked = context(); clicked.target = Some(target.clone()); clicked.authoring_revision = Some(7);
            state.queue_author_click(clicked.clone(), ray(3.));
            let mut changed = clicked.clone();
            match variant {
                0 => changed.map_id = MapId("other".into()),
                1 => changed.terrain_revision += 1,
                2 => changed.surface_revision[1] += 1,
                3 => changed.missing_pages[0] = UVec4::ONE,
                4 => changed.authoring_revision = Some(8),
                5 => changed.modifiers = 2,
                6 | 7 | 10 | 11 => changed.target = Some(fixtures::ground_target(&format!("changed-{variant}"), false, "test.actor")),
                8 => changed.target = Some(GroundCursorTarget::Portal(MapPortalTarget::portal(MapId("other-destination".into()), "gate-b"))),
                9 => changed.target = Some(match &target {
                    GroundCursorTarget::Portal(destination) => GroundCursorTarget::Portal(MapPortalTarget {
                        map_id: destination.map_id.clone(),
                        portal_id: if destination.portal_id.is_some() { None } else { Some("gate-b".into()) },
                    }),
                    _ => fixtures::ground_target("continuous-layer-a", true, "test.actor"),
                }),
                _ => unreachable!(),
            }
            state.retarget(changed.clone(), ray(5.)); state.retain_author_click_scope(&changed);
            assert!(state.pending_author_click.is_none(), "case {variant}: {target:?}");
        }
    }
}

#[test]
fn domain_brush_requires_authority_and_rejects_old_display_or_control_mode() {
    let target = fixtures::ground_target("continuous-layer-a", true, "test.actor");
    let (mut state, mut identity) = paint_cursor(target);
    assert_eq!(paint_hit(&state, &identity), None);
    let point = Vec3::new(0., 12.5, 0.);
    VegetationGroundCursor::apply_authoritative_result(state.accepted.as_mut().unwrap(), ClipmapGroundHit::Hit(point), false);
    state.accepted_authority_ready = true;
    assert_eq!(paint_hit(&state, &identity), Some(point));
    state.paint_ready = false; assert_eq!(paint_hit(&state, &identity), None); state.paint_ready = true;
    identity.modifiers = 2; state.retarget(identity.clone(), ray(0.));
    state.adopt(GroundCompletion { terrain_mesh: false, request: state.desired.clone().unwrap(), result: ClipmapGroundHit::Hit(point), point: Some(point) });
    assert_eq!(paint_hit(&state, &identity), None);
}

fn paint_cursor(target: GroundCursorTarget) -> (VegetationGroundCursor, GroundCursorContext) {
    let mut identity = context();
    identity.target = Some(target);
    let mut state = VegetationGroundCursor::default();
    state.retarget(identity.clone(), ray(0.));
    state.adopt(GroundCompletion {
        terrain_mesh: false,
        request: state.desired.clone().unwrap(),
        result: ClipmapGroundHit::Hit(Vec3::new(0., 40., 0.)),
        point: Some(Vec3::new(0., 40., 0.)),
    }); // 可见面的粗Y不具备二维作者落笔资格。
    state.paint_ready = true;
    (state, identity)
}

fn paint_hit(state: &VegetationGroundCursor, identity: &GroundCursorContext) -> Option<Vec3> {
    state.brush_hit_for_identity(
        true,
        identity.surface_revision,
        &identity.missing_pages,
        Some(identity.terrain_revision),
        &identity.map_id,
        identity.target.as_ref().unwrap(),
        identity.terrain_revision,
    )
}

#[test]
fn surface_and_puddle_brushes_require_current_authoritative_height() {
    for target in paint_targets() {
        let (mut state, identity) = paint_cursor(target);
        assert_eq!(paint_hit(&state, &identity), None);
        let point = Vec3::new(0., 12.5, 0.);
        VegetationGroundCursor::apply_authoritative_result(
            state.accepted.as_mut().unwrap(),
            ClipmapGroundHit::Hit(point),
            false,
        );
        state.accepted_authority_ready = true;
        assert_eq!(paint_hit(&state, &identity), Some(point)); // 最终点取基础三角Y。
        for (ready, revision, missing, adopted, map_id) in [
            (
                false,
                identity.surface_revision,
                identity.missing_pages,
                Some(11),
                identity.map_id.clone(),
            ),
            (
                true,
                [2; 3],
                identity.missing_pages,
                Some(11),
                identity.map_id.clone(),
            ),
            (
                true,
                identity.surface_revision,
                [UVec4::ONE; 16],
                Some(11),
                identity.map_id.clone(),
            ),
            (
                true,
                identity.surface_revision,
                identity.missing_pages,
                Some(10),
                identity.map_id.clone(),
            ),
            (
                true,
                identity.surface_revision,
                identity.missing_pages,
                Some(11),
                MapId("other".into()),
            ),
        ] {
            assert_eq!(
                state.brush_hit_for_identity(
                    ready,
                    revision,
                    &missing,
                    adopted,
                    &map_id,
                    identity.target.as_ref().unwrap(),
                    identity.terrain_revision,
                ),
                None
            ); // 旧显示、缺页身份、旧作者修订和跨图都不能写入纹理。
        }
        state.paint_ready = false;
        assert_eq!(paint_hit(&state, &identity), None); // 显示保留点不冒充本帧完成值。
    }
}

#[test]
fn paint_brush_late_rays_cannot_cross_layer_preset_press_or_view_identity() {
    for target in paint_targets() {
        for change in 0..9 {
            let (mut state, identity) = paint_cursor(target.clone());
            let old = state.desired.clone().unwrap();
            let mut next = identity;
            match change {
                0 => {
                    next.target = Some(GroundCursorTarget::Surface {
                        layer_id: "soil-b".into(),
                        preset_id: Some("soil".into()),
                    })
                }
                1 => {
                    next.target = Some(GroundCursorTarget::Surface {
                        layer_id: "soil-a".into(),
                        preset_id: Some("rock".into()),
                    })
                }
                2 => {
                    next.held = false;
                    next.input_epoch += 1;
                }
                3 => next.map_id = MapId("other".into()),
                4 => next.terrain_revision += 1,
                5 => next.modifiers = 1,
                6 => next.view = EditorView::Perspective,
                7 => next.camera_transform[0] += 1,
                _ => next.orthographic = false, // 方向和机位完全相同时，投影开关仍改变离轴射线。
            }
            state.retarget(next, ray(1.));
            assert!(!state.adopt(GroundCompletion {
                terrain_mesh: false,
                request: old,
                result: ClipmapGroundHit::Hit(Vec3::ZERO),
                point: Some(Vec3::ZERO),
            })); // 等待期间切层/预设、松手、擦除或镜头换代不补画旧射线。
        }
        let (mut state, mut identity) = paint_cursor(target);
        identity.modifiers = 2;
        state.retarget(identity.clone(), ray(1.));
        state.adopt(GroundCompletion {
            terrain_mesh: false,
            request: state.desired.clone().unwrap(),
            result: ClipmapGroundHit::Hit(Vec3::ZERO),
            point: Some(Vec3::ZERO),
        });
        state.accepted_authority_ready = true;
        state.paint_ready = true;
        assert_eq!(paint_hit(&state, &identity), None); // 当前Ctrl选择射线也不能同时涂画。
    }
}

#[test]
fn moving_paint_brushes_consume_the_completed_original_ray_once() {
    for target in paint_targets() {
        let (mut state, identity) = paint_cursor(target);
        let point = Vec3::new(0., 12.5, 0.);
        VegetationGroundCursor::apply_authoritative_result(
            state.accepted.as_mut().unwrap(),
            ClipmapGroundHit::Hit(point),
            false,
        );
        state.accepted_authority_ready = true;
        state.retarget(identity.clone(), ray(3.));
        assert_eq!(paint_hit(&state, &identity), Some(point)); // 移动不把原完成点冒充新射线XZ。
        state.paint_ready = false;
        assert_eq!(paint_hit(&state, &identity), None); // 下一帧在途时不能重复刷旧点。
    }
}

fn sculpt_cursor(tool: EditorTool) -> (VegetationGroundCursor, GroundCursorContext) {
    let mut identity = context();
    identity.target = Some(GroundCursorTarget::TerrainSculpt(tool));
    let mut state = VegetationGroundCursor::default();
    state.retarget(identity.clone(), ray(0.));
    state.adopt(GroundCompletion {
        terrain_mesh: false,
        request: state.desired.clone().unwrap(),
        result: ClipmapGroundHit::Hit(Vec3::new(0., 40., 0.)),
        point: Some(Vec3::new(0., 40., 0.)),
    }); // 先只有粗显示面命中，尚无基础作者Y。
    state.paint_ready = true;
    (state, identity)
}

#[test]
fn sculpt_waits_for_authority_and_uses_the_base_triangle_height() {
    for tool in [
        EditorTool::Raise,
        EditorTool::Lower,
        EditorTool::Smooth,
        EditorTool::Flatten,
    ] {
        let (mut state, identity) = sculpt_cursor(tool);
        let read = |state: &VegetationGroundCursor| {
            state.sculpt_hit_for_identity(
                true,
                identity.surface_revision,
                &identity.missing_pages,
                Some(identity.terrain_revision),
                &identity.map_id,
                tool,
                identity.terrain_revision,
            )
        };
        assert_eq!(read(&state), None); // 真实可见面XZ不能直接携带粗LOD高度进入雕刻。
        let completion = state.accepted.as_mut().unwrap();
        assert!(!VegetationGroundCursor::apply_authoritative_result(
            completion,
            ClipmapGroundHit::WaitingForHeight,
            false,
        ));
        assert_eq!(read(&state), None); // 基础页等待时没有零平面落笔资格。
        let point = Vec3::new(0., 12.5, 0.);
        state.accepted_authority_ready = VegetationGroundCursor::apply_authoritative_result(
            state.accepted.as_mut().unwrap(),
            ClipmapGroundHit::Hit(point),
            false,
        );
        assert_eq!(read(&state), Some(point)); // 四种工具均使用基础三角面的最终Y。
    }
}

#[test]
fn sculpt_rejects_old_display_height_and_changed_query_identity() {
    let (mut state, identity) = sculpt_cursor(EditorTool::Raise);
    state.accepted_authority_ready = true;
    let read = |ready, revision, missing: &[UVec4; 16], adopted, map_id: &MapId, tool| {
        state.sculpt_hit_for_identity(
            ready,
            revision,
            missing,
            adopted,
            map_id,
            tool,
            identity.terrain_revision,
        )
    };
    assert!(
        read(
            true,
            identity.surface_revision,
            &identity.missing_pages,
            Some(identity.terrain_revision),
            &identity.map_id,
            EditorTool::Raise
        )
        .is_some()
    );
    assert_eq!(
        read(
            true,
            identity.surface_revision,
            &identity.missing_pages,
            Some(identity.terrain_revision - 1),
            &identity.map_id,
            EditorTool::Raise
        ),
        None
    );
    assert_eq!(
        read(
            false,
            identity.surface_revision,
            &identity.missing_pages,
            Some(identity.terrain_revision),
            &identity.map_id,
            EditorTool::Raise
        ),
        None
    );
    assert_eq!(
        read(
            true,
            [2; 3],
            &identity.missing_pages,
            Some(identity.terrain_revision),
            &identity.map_id,
            EditorTool::Raise
        ),
        None
    );
    assert_eq!(
        read(
            true,
            identity.surface_revision,
            &[UVec4::ONE; 16],
            Some(identity.terrain_revision),
            &identity.map_id,
            EditorTool::Raise
        ),
        None
    );
    assert_eq!(
        read(
            true,
            identity.surface_revision,
            &identity.missing_pages,
            Some(identity.terrain_revision),
            &MapId("other".into()),
            EditorTool::Raise
        ),
        None
    );
    assert_eq!(
        read(
            true,
            identity.surface_revision,
            &identity.missing_pages,
            Some(identity.terrain_revision),
            &identity.map_id,
            EditorTool::Lower
        ),
        None
    );
}

#[test]
fn moving_sculpt_consumes_fresh_samples_but_release_and_tool_change_cancel_them() {
    for next_tool in [EditorTool::Raise, EditorTool::Lower] {
        let (mut state, identity) = sculpt_cursor(EditorTool::Raise);
        let old_request = state.desired.clone().unwrap();
        state.accepted_authority_ready = true;
        state.retarget(identity.clone(), ray(1.)); // 同笔移动可消费本帧完成的原射线。
        assert!(
            state
                .sculpt_hit_for_identity(
                    true,
                    identity.surface_revision,
                    &identity.missing_pages,
                    Some(identity.terrain_revision),
                    &identity.map_id,
                    EditorTool::Raise,
                    identity.terrain_revision
                )
                .is_some()
        );
        state.paint_ready = false;
        assert_eq!(
            state.sculpt_hit_for_identity(
                true,
                identity.surface_revision,
                &identity.missing_pages,
                Some(identity.terrain_revision),
                &identity.map_id,
                EditorTool::Raise,
                identity.terrain_revision
            ),
            None
        ); // 查询在途时旧点不重复刷。
        let mut next = identity;
        next.target = Some(GroundCursorTarget::TerrainSculpt(next_tool));
        if next_tool == EditorTool::Raise {
            next.held = false;
            next.input_epoch += 1; // 同工具松手也撤销未完成查询。
        }
        state.retarget(next, ray(2.));
        assert!(!state.adopt(GroundCompletion {
            terrain_mesh: false,
            request: old_request,
            result: ClipmapGroundHit::Hit(Vec3::ZERO),
            point: Some(Vec3::ZERO),
        })); // 不在松键或切工具后补画上一条射线。
    }
}
#[test] // 持续每帧移动不能饿死上一帧真实完成值。
fn moving_held_rays_accept_original_samples_in_order() {
    // 生产retarget只合并一个最新请求。
    let mut state = VegetationGroundCursor::default(); // 单飞状态。
    for x in 0..20 {
        // 持续移动而非停留才采用。
        state.retarget(context(), ray(x as f32)); // 冻结本次射线。
        let request = state.desired.clone().unwrap(); // 模拟正在后台计算的请求。
        state.retarget(context(), ray(x as f32 + 0.5)); // 完成前输入继续移动。
        assert!(state.adopt(GroundCompletion {
            terrain_mesh: false,
            result: ClipmapGroundHit::Hit(Vec3::ZERO),
            request,
            point: Some(Vec3::new(x as f32, 0., 0.))
        })); // 原射线同按压完成仍可落笔。
        assert_eq!(state.accepted.as_ref().unwrap().point.unwrap().x, x as f32); // 绝不替换成新射线位置。
    } // 恒定latest槽不会累积中间请求。
} // 完成值序号始终单调。
#[test] // 松手、切层、地形/页/镜头换代均拒绝迟到值。
fn changed_context_rejects_late_ground() {
    // 同图同名层也不能跨输入代次。
    for variant in 0..11 {
        // 分别审计所有关键身份。
        let mut state = VegetationGroundCursor::default();
        state.retarget(context(), ray(0.)); // 启动旧上下文。
        let request = state.desired.clone().unwrap();
        let mut next = context(); // 模拟晚到结果。
        match variant {
            0 => next.held = false,
            1 => next.input_epoch += 1,
            2 => next.map_id = MapId("other".into()),
            3 => next.target = Some(GroundCursorTarget::StillWater(Some("other".into()))),
            4 => next.surface_revision[0] += 1,
            5 => next.missing_pages[0] = UVec4::ONE,
            6 => next.camera_transform[0] += 1,
            7 => next.view = EditorView::Perspective,
            8 => next.terrain_revision += 1,
            9 => next.orthographic = false,
            _ => next.authoring_revision = Some(2),
        } // 光标移动本身不属于换代。
        state.retarget(next, ray(1.)); // 新完整身份替代旧请求。
        assert!(!state.adopt(GroundCompletion {
            terrain_mesh: false,
            result: ClipmapGroundHit::Hit(Vec3::ZERO),
            request,
            point: Some(Vec3::ZERO)
        })); // 不向R8或光标发布旧结果。
    } // 松手后无补画资格。
} // 等待同代新完成。
#[test] // 悬停允许同代真实完成延迟，取消工具立即拒绝任何完成。
fn moving_hover_accepts_original_hit_and_cancel_rejects_it() {
    // 悬停显示同样不能要求鼠标停止。
    let mut state = VegetationGroundCursor::default();
    let mut identity = context();
    identity.held = false; // 无活动按压。
    state.retarget(identity.clone(), ray(0.));
    let request = state.desired.clone().unwrap(); // 旧悬停请求。
    state.retarget(identity, ray(1.));
    assert!(state.adopt(GroundCompletion {
        terrain_mesh: false,
        result: ClipmapGroundHit::Hit(Vec3::ZERO),
        request: request.clone(),
        point: Some(Vec3::ZERO)
    })); // 连续悬停允许原射线完成，不要求鼠标停止。
    state.clear();
    assert!(!state.adopt(GroundCompletion {
        terrain_mesh: false,
        result: ClipmapGroundHit::Hit(Vec3::ZERO),
        request,
        point: Some(Vec3::ZERO)
    })); // 换工具后的结果不可采用。
} // clear不创建额外任务。

#[test]
fn author_click_survives_release_but_feature_or_revision_change_cancels_it() {
    let mut state = VegetationGroundCursor::default();
    let mut clicked = context();
    clicked.target = Some(GroundCursorTarget::River("river-a".into()));
    clicked.authoring_revision = Some(7);
    state.queue_author_click(clicked.clone(), ray(3.0));
    let clicked_sequence = state.pending_author_click.as_ref().unwrap().sequence;

    let mut released = clicked.clone();
    released.held = false;
    released.input_epoch += 1;
    state.retarget(released.clone(), ray(4.0));
    state.retain_author_click_scope(&released);
    assert_eq!(
        state.pending_author_click.as_ref().unwrap().sequence,
        clicked_sequence
    ); // 松键和后续悬停不得丢失已经冻结的真实点击射线。

    let mut another_feature = released;
    another_feature.target = Some(GroundCursorTarget::River("river-b".into()));
    state.retarget(another_feature.clone(), ray(5.0));
    state.retain_author_click_scope(&another_feature);
    assert!(state.pending_author_click.is_none()); // 切换稳定特征后迟到结果必须失效。

    state.queue_author_click(clicked.clone(), ray(6.0));
    let mut newer_author = clicked;
    newer_author.authoring_revision = Some(8);
    state.retarget(newer_author.clone(), ray(7.0));
    state.retain_author_click_scope(&newer_author);
    assert!(state.pending_author_click.is_none()); // 同特征的新作者快照也不能接收旧点。
}

#[test]
fn author_click_waits_for_authority_after_release_and_rejects_a_late_revision() {
    let mut state = VegetationGroundCursor::default();
    let mut clicked = context();
    clicked.target = Some(GroundCursorTarget::RelocatePlacement("tree-a".into()));
    clicked.authoring_revision = Some(9);
    state.queue_author_click(clicked.clone(), ray(6.0));
    let request = state.pending_author_click.clone().unwrap();
    let visible_point = Vec3::new(6.0, 40.0, 3.0); // 粗显示环带的Y不是可交给作者业务的最终值。
    let mut completion = GroundCompletion {
        terrain_mesh: false,
        request,
        result: ClipmapGroundHit::Hit(visible_point),
        point: Some(visible_point),
    };
    assert!(!VegetationGroundCursor::apply_authoritative_result(
        &mut completion,
        ClipmapGroundHit::WaitingForHeight,
        true,
    ));
    assert_eq!(completion.point, Some(visible_point)); // 松键等待期间只私有保留XZ种子。

    let authoritative_point = Vec3::new(6.0, 12.5, 3.0);
    assert!(VegetationGroundCursor::apply_authoritative_result(
        &mut completion,
        ClipmapGroundHit::Hit(authoritative_point),
        true,
    ));
    assert_eq!(completion.point, Some(authoritative_point)); // 完成值必须替换掉粗LOD的可见Y。

    state.pending_author_surface = Some(completion);
    let mut newer_surface = clicked;
    newer_surface.surface_revision[0] += 1;
    state.retarget(newer_surface.clone(), ray(7.0));
    state.retain_author_click_scope(&newer_surface);
    assert!(state.pending_author_surface.is_none()); // 高度换代后的旧Mip 0结果不能迟到发布。
}

#[test]
fn spline_click_completion_is_consumed_once_and_checks_the_adopted_surface() {
    let mut state = VegetationGroundCursor::default();
    let mut clicked = context();
    clicked.target = Some(GroundCursorTarget::River("river-a".into()));
    clicked.authoring_revision = Some(7);
    state.retarget(clicked.clone(), ray(3.0));
    let request = state.desired.clone().unwrap();
    state.completed_author_click = Some(GroundCompletion {
        terrain_mesh: false,
        request: request.clone(),
        result: ClipmapGroundHit::Hit(Vec3::new(3.0, 2.0, 1.0)),
        point: Some(Vec3::new(3.0, 2.0, 1.0)),
    });
    let empty_missing = [UVec4::ZERO; 16];
    assert_eq!(
        state.take_spline_click_result_for_identity(
            true,
            [1; 3],
            &empty_missing,
            Some(clicked.terrain_revision),
            &clicked.map_id,
            "river-a",
            false,
            7,
        ),
        Some(ClipmapGroundHit::Hit(Vec3::new(3.0, 2.0, 1.0)))
    );
    assert_eq!(
        state.take_spline_click_result_for_identity(
            true,
            [1; 3],
            &empty_missing,
            Some(clicked.terrain_revision),
            &clicked.map_id,
            "river-a",
            false,
            7,
        ),
        None
    ); // 同一真实点击完成值不能在后续帧追加第二个控制点。

    state.completed_author_click = Some(GroundCompletion {
        terrain_mesh: false,
        request,
        result: ClipmapGroundHit::Hit(Vec3::ZERO),
        point: Some(Vec3::ZERO),
    });
    assert_eq!(
        state.take_spline_click_result_for_identity(
            true,
            [2, 1, 1],
            &empty_missing,
            Some(clicked.terrain_revision),
            &clicked.map_id,
            "river-a",
            false,
            7,
        ),
        None
    ); // 高度修订变化后，迟到结果被消费并拒绝，不能等旧修订再次出现。
    assert!(state.completed_author_click.is_none());
}

#[test]
fn placement_click_survives_release_and_is_consumed_once() {
    let mut state = VegetationGroundCursor::default();
    let mut clicked = context();
    clicked.target = Some(GroundCursorTarget::RelocatePlacement("tree-a".into()));
    clicked.authoring_revision = Some(12);
    state.queue_author_click(clicked.clone(), ray(8.0));
    let request = state.pending_author_click.clone().unwrap();

    let mut released = clicked.clone();
    released.held = false;
    released.input_epoch += 1;
    state.retarget(released.clone(), ray(9.0));
    state.retain_author_click_scope(&released);
    assert_eq!(state.pending_author_click.as_ref(), Some(&request));

    state.pending_author_click = None;
    state.completed_author_click = Some(GroundCompletion {
        terrain_mesh: false,
        request,
        result: ClipmapGroundHit::Hit(Vec3::new(8.0, 3.5, -2.0)),
        point: Some(Vec3::new(8.0, 3.5, -2.0)),
    });
    let empty_missing = [UVec4::ZERO; 16];
    let target = GroundCursorTarget::RelocatePlacement("tree-a".into());
    assert_eq!(
        state.take_placement_click_result_for_identity(
            true,
            [1; 3],
            &empty_missing,
            Some(clicked.terrain_revision),
            &clicked.map_id,
            target.clone(),
            12,
        ),
        Some(ClipmapGroundHit::Hit(Vec3::new(8.0, 3.5, -2.0)))
    );
    assert_eq!(
        state.take_placement_click_result_for_identity(
            true,
            [1; 3],
            &empty_missing,
            Some(clicked.terrain_revision),
            &clicked.map_id,
            target,
            12,
        ),
        None
    ); // 同一点击不能在后续帧再次移动同一摆放。
}

#[test]
fn placement_click_rejects_changed_asset_target_and_author_revision() {
    let mut state = VegetationGroundCursor::default();
    let mut clicked = context();
    clicked.target = Some(GroundCursorTarget::PlaceAsset {
        asset_path: "gltf/tree.glb".into(),
        asset_target: MapPlacementAssetTarget::Scene0,
    });
    clicked.authoring_revision = Some(4);
    state.retarget(clicked.clone(), ray(2.0));
    let request = state.desired.clone().unwrap();
    state.completed_author_click = Some(GroundCompletion {
        terrain_mesh: false,
        request: request.clone(),
        result: ClipmapGroundHit::Hit(Vec3::ONE),
        point: Some(Vec3::ONE),
    });
    let empty_missing = [UVec4::ZERO; 16];
    assert_eq!(
        state.take_placement_click_result_for_identity(
            true,
            [1; 3],
            &empty_missing,
            Some(clicked.terrain_revision),
            &clicked.map_id,
            GroundCursorTarget::PlaceAsset {
                asset_path: "gltf/tree.glb".into(),
                asset_target: MapPlacementAssetTarget::Primitive {
                    mesh: 0,
                    primitive: 0,
                    material: None,
                },
            },
            4,
        ),
        None
    ); // 文件相同但子资产变化时不能采用旧Scene0点击。

    state.completed_author_click = Some(GroundCompletion {
        terrain_mesh: false,
        request,
        result: ClipmapGroundHit::Hit(Vec3::ONE),
        point: Some(Vec3::ONE),
    });
    assert_eq!(
        state.take_placement_click_result_for_identity(
            true,
            [1; 3],
            &empty_missing,
            Some(clicked.terrain_revision),
            &clicked.map_id,
            clicked.target.unwrap(),
            5,
        ),
        None
    ); // 作者修订变化时即使资产身份相同也拒收迟到命中。
}

#[test]
fn placement_click_rejects_old_adopted_height_revision() {
    let mut state = VegetationGroundCursor::default();
    let mut clicked = context();
    clicked.target = Some(GroundCursorTarget::RelocatePlacement("rock-a".into()));
    clicked.authoring_revision = Some(6);
    state.retarget(clicked.clone(), ray(4.0));
    state.completed_author_click = Some(GroundCompletion {
        terrain_mesh: false,
        request: state.desired.clone().unwrap(),
        result: ClipmapGroundHit::Hit(Vec3::new(4.0, 7.0, 0.0)),
        point: Some(Vec3::new(4.0, 7.0, 0.0)),
    });

    assert_eq!(
        state.take_placement_click_result_for_identity(
            true,
            [1; 3],
            &[UVec4::ZERO; 16],
            Some(clicked.terrain_revision - 1),
            &clicked.map_id,
            clicked.target.unwrap(),
            6,
        ),
        None,
    ); // 新雕刻修订尚未正式采用时，ready的旧页也不能移动摆放物。
}
