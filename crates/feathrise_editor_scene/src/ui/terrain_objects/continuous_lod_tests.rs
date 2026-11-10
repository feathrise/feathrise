//! 通过真实属性滚动、原生鼠标和公共数值控件验证连续LOD列表，不以行池容量限制作者配置。
use super::*;

fn continuous_page(kind: SceneTerrainKind) -> (App, Entity, Entity) {
    let (mut app, window) = native_page(kind, 6);
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let object = &mut std::sync::Arc::make_mut(draft.manifest.terrains.as_mut().unwrap())[0];
        // 相同倍率可拥有任意多个递增作者范围，正式编译会合并几何；128层超过整个可见行池。
        let distance_lods = (0..128)
            .map(|index| SceneCollisionLod {
                // 128个真实作者环带共用16m范围，保持正式512碰撞块预算；层数与覆盖成本分开验证。
                max_distance_meters: (index + 1) as f32 * 0.125,
                spacing_multiplier: 16,
            })
            .collect::<Vec<_>>();
        if let Some(voxel) = &mut object.voxel {
            voxel.display_lods = distance_lods.clone();
            voxel.collision_lods = distance_lods;
        } else {
            object.terrain.collision_lods = distance_lods;
            object.terrain.display_lods = Some(
                (0..128)
                    .map(|index| SceneTerrainDisplayLod {
                        outer_half_extent_meters: (index < 127).then_some((index + 1) as f32 * 8.0),
                        spacing_multiplier: 32,
                    })
                    .collect(),
            );
            let spec = SparseHeightSpec::new(object.world_min, object.world_max, 0.25, -8.0, 16.0)
                .unwrap();
            compile_display_lod_plan(&spec, object.terrain.display_lods.as_ref().unwrap()).unwrap();
            feathrise_scene::validate_layered_collision_settings(&spec, &object.terrain.collision_lods)
                .unwrap();
        }
        validate_terrain_objects(&draft.manifest).unwrap();
        session.revision += 1;
    }
    let (host, properties) = {
        let world = app.world_mut();
        (
            world
                .query_filtered::<Entity, With<ListHost>>()
                .single(world)
                .unwrap(),
            world
                .query_filtered::<Entity, With<crate::ui::scene_inspector::InspectorSection>>()
                .iter(world)
                .find(|entity| {
                    world.get::<crate::ui::scene_inspector::InspectorSection>(*entity)
                        == Some(&crate::ui::scene_inspector::InspectorSection::Terrain)
                })
                .unwrap(),
        )
    };
    let parent = app.world().get::<ChildOf>(host).unwrap().parent();
    app.world_mut().get_mut::<Node>(host).unwrap().display = Display::None;
    let body = app.world().get::<ChildOf>(properties).unwrap().parent();
    app.world_mut().entity_mut(body).insert((
            Node {
                position_type: PositionType::Absolute,
                left: px(300),
                top: px(0),
                width: px(280),
                height: px(400),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::scroll_y(),
                ..default()
            },
            ChildOf(parent),
        ));
    for _ in 0..5 {
        app.update();
    }
    (app, window, body)
}

fn list_origin(app: &mut App, body: Entity, display: bool) -> (f32, f32) {
    let world = app.world_mut();
    let list = world
        .query::<(Entity, &LodList)>()
        .iter(world).find(|(_, list)| list.0 == display).unwrap().0;
    let body_node = world.get::<ComputedNode>(body).unwrap();
    let list_node = world.get::<ComputedNode>(list).unwrap();
    let body_top = world
        .get::<UiGlobalTransform>(body)
        .unwrap()
        .affine()
        .translation
        .y
        - body_node.size().y * 0.5;
    let list_top = world
        .get::<UiGlobalTransform>(list)
        .unwrap()
        .affine()
        .translation
        .y
        - list_node.size().y * 0.5;
    let inverse = body_node.inverse_scale_factor();
    (
        (list_top - body_top + body_node.scroll_position.y) * inverse,
        body_node.size().y * inverse,
    )
}

fn scroll_to_layer(app: &mut App, body: Entity, display: bool, index: usize) -> Entity {
    let (origin, height) = list_origin(app, body, display);
    app.world_mut().get_mut::<ScrollPosition>(body).unwrap().y =
        (origin + index as f32 * lod_list::row_pitch(height) - height * 0.5).max(0.0);
    for _ in 0..4 {
        app.update();
    }
    let world = app.world_mut();
    world
        .query::<(Entity, &LodRowBinding, &Node)>()
        .iter(world)
        .find(|(_, binding, node)| {
            node.display != Display::None
                && binding
                    .0
                    .as_ref()
                    .is_some_and(|target| target.index == index && target.display == display)
        })
        .expect("任意作者层必须能通过既有属性滚动访问")
        .0
}

pub(super) fn reveal(app: &mut App, body: Entity, entity: Entity) {
    let world = app.world_mut();
    let item = world.get::<ComputedNode>(entity).unwrap();
    let body_node = world.get::<ComputedNode>(body).unwrap();
    let inverse = body_node.inverse_scale_factor();
    let top = (world
        .get::<UiGlobalTransform>(entity)
        .unwrap()
        .affine()
        .translation
        .y
        - item.size().y * 0.5
        - world
            .get::<UiGlobalTransform>(body)
            .unwrap()
            .affine()
            .translation
            .y
        + body_node.size().y * 0.5)
        * inverse;
    let bottom = top + item.size().y * inverse;
    let height = body_node.size().y * inverse;
    let delta = if top < 0.0 {
        top
    } else {
        (bottom - height).max(0.0)
    };
    world.get_mut::<ScrollPosition>(body).unwrap().y += delta;
    for _ in 0..4 {
        app.update();
    }
}

#[test]
fn continuous_native_lods_scroll_all_128_author_layers_without_pages_and_commit_the_real_last_index()
 {
    for kind in [SceneTerrainKind::SparseClipmap, SceneTerrainKind::Voxel] {
        let (mut app, window, body) = continuous_page(kind);
        let row_entities =
            {
                let world = app.world_mut();
                let mut entities = world
                    .query_filtered::<Entity, With<LodRow>>()
                    .iter(world)
                    .collect::<Vec<_>>();
                entities.sort();
                assert_eq!(entities.len(), lod_list::ROW_POOL_SIZE * 2);
                assert!(world.query::<&Text>().iter(world).all(
                    |text| !text.0.contains("LOD 上一页")
                        && !text.0.contains("LOD 下一页")
                        && !text.0.contains("每页 4 层")
                ));
                entities
            };
        for display in [true, false] {
            {
                let mut state = app.world_mut().resource_mut::<TerrainObjectsEditorState>();
                state.display_lods = display;
            }
            for _ in 0..4 {
                app.update();
            }
            let before = current_object(&app);
            let last = scroll_to_layer(&mut app, body, display, 127);
            click(&mut app, window, last, MouseButton::Left);
            assert_eq!(
                app.world()
                    .resource::<TerrainObjectsEditorState>()
                    .lod(display).selected,
                127
            );
            let value = selected_lod_field(&mut app, display, usize::from(display));
            let binding = app
                .world()
                .get::<NumericFieldBinding>(value)
                .unwrap()
                .current
                .as_ref()
                .unwrap();
            assert_eq!(binding.index, 8 + 127 * 2 + usize::from(display));
            assert_eq!(binding.display_lods, display);
            if kind == SceneTerrainKind::SparseClipmap && display {
                let distance = selected_lod_field(&mut app, display, 0);
                assert!(app.world().get::<NumericInputDisabled>(distance).is_some());
            }
            reveal(&mut app, body, value);
            click(&mut app, window, value, MouseButton::Middle);
            let input = if display { "64" } else { "16.125" };
            key(
                &mut app,
                window,
                KeyCode::Digit6,
                Key::Character(input.into()),
                Some(input),
            );
            key(&mut app, window, KeyCode::Enter, Key::Enter, None);
            complete(&mut app);
            let after = current_object(&app);
            assert_other_lod_usage_preserved(&before, &after, display);
            let rows = |object: &SceneTerrainObject| -> Vec<(Option<f32>, u32)> {
                (0..128)
                    .map(|index| {
                        (
                            jobs::field_value(object, 8 + index * 2, display),
                            jobs::field_value(object, 9 + index * 2, display).unwrap() as u32,
                        )
                    })
                    .collect()
            };
            assert_eq!(&rows(&after)[..127], &rows(&before)[..127]);
            assert_eq!(
                rows(&after)[127],
                if display {
                    (rows(&before)[127].0, 64)
                } else {
                    (Some(16.125), rows(&before)[127].1)
                }
            );
            let world = app.world_mut();
            let summary = world
                .query::<(&NativeLodSummary, &Text)>()
                .iter(world).find(|(summary, _)| summary.0 == display).unwrap().1;
            assert_eq!(
                summary.0,
                format!("{} · 128 层", if display { "显示" } else { "碰撞" })
            );
            let mut entities = world
                .query_filtered::<Entity, With<LodRow>>()
                .iter(world)
                .collect::<Vec<_>>();
            entities.sort();
            assert_eq!(entities, row_entities, "滚动和真实数值采用不能增删行实体");
        }
        assert_eq!(
            app.world().resource::<EditorAuthoringSession>().undo.len(),
            2
        );
    }
}

#[test]
fn continuous_native_lod_rebinding_does_not_replay_held_selection_or_accept_an_old_row_revision() {
    let (mut app, window, body) = continuous_page(SceneTerrainKind::SparseClipmap);
    let row = scroll_to_layer(&mut app, body, true, 90);
    click(&mut app, window, row, MouseButton::Left);
    point_at(&mut app, window, row);
    mouse(&mut app, window, MouseButton::Left, ButtonState::Pressed);
    assert_eq!(
        app.world()
            .resource::<TerrainObjectsEditorState>()
            .lod(true).selected,
        90
    );
    let original = app.world().get::<LodRowBinding>(row).unwrap().0.clone();
    let _ = scroll_to_layer(&mut app, body, true, 10);
    assert_ne!(app.world().get::<LodRowBinding>(row).unwrap().0, original);
    assert_eq!(
        app.world()
            .resource::<TerrainObjectsEditorState>()
            .lod(true).selected,
        90
    );
    mouse(&mut app, window, MouseButton::Left, ButtonState::Released);
    assert_eq!(
        app.world()
            .resource::<TerrainObjectsEditorState>()
            .lod(true).selected,
        90
    );
    let stale = scroll_to_layer(&mut app, body, true, 10);
    point_at(&mut app, window, stale);
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .revision += 1;
    mouse(&mut app, window, MouseButton::Left, ButtonState::Pressed);
    mouse(&mut app, window, MouseButton::Left, ButtonState::Released);
    assert_eq!(
        app.world()
            .resource::<TerrainObjectsEditorState>()
            .lod(true).selected,
        90
    );
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().next_serial,
        0
    );
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
}

#[test]
fn inline_native_lod_scroll_rebinding_cancels_the_captured_cell_before_any_late_commit() {
    let (mut app, window, body) = continuous_page(SceneTerrainKind::SparseClipmap);
    let before = current_object(&app);
    let row = scroll_to_layer(&mut app, body, true, 65);
    let root = {
        let world = app.world_mut();
        world.query::<(Entity, &Field, &LodFieldRow)>().iter(world)
            .find(|(_, field, owner)| owner.0 == row && matches!(field, Field::Lod(true, index) if *index % 2 == 0))
            .unwrap().0
    };
    let presented = app.world().get::<LodRowBinding>(row).unwrap().0.clone().unwrap();
    point_at(&mut app, window, root);
    mouse(&mut app, window, MouseButton::Left, ButtonState::Pressed);
    assert_eq!(app.world().resource::<feathrise_editor_ui::ActiveNumericInputScrub>().entity, Some(root));
    let frozen = app.world().get::<NumericFieldBinding>(root).unwrap().editing.clone().unwrap();
    assert_eq!(frozen.scene_id, presented.scene_id);
    assert_eq!(frozen.revision, presented.revision);
    assert_eq!(frozen.terrain.as_ref(), Some(&presented.terrain));
    assert_eq!(frozen.display_lods, presented.display);
    assert_eq!(frozen.index, 8 + presented.index * 2);
    let _ = scroll_to_layer(&mut app, body, true, 10);
    assert_ne!(app.world().get::<LodRowBinding>(row).unwrap().0.as_ref(), Some(&presented));
    mouse(&mut app, window, MouseButton::Left, ButtonState::Released);
    assert_eq!(app.world().resource::<feathrise_editor_ui::ActiveNumericInputScrub>().entity, None);
    for value in [529.0, 530.0] { late_commit(&mut app, root, value); }
    assert_eq!(app.world().resource::<EditorAuthoringSession>().next_serial, 0,
        "固定行复用后旧笔必须在后台任务启动前拒收，不能借新层校验失败掩盖串写");
    assert_eq!(current_object(&app), before);
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
}

#[test]
fn continuous_native_lod_structure_adoption_invalidates_late_values_on_layers_beyond_the_pool() {
    let (mut app, window, body) = continuous_page(SceneTerrainKind::SparseClipmap);
    let later = scroll_to_layer(&mut app, body, true, 65);
    click(&mut app, window, later, MouseButton::Left);
    assert_eq!(app.world().resource::<TerrainObjectsEditorState>().lod(true).selected, 65);
    let root = selected_lod_field(&mut app, true, 0);
    // 选择换绑保留旧编辑墓碑；通过可见控件的真实中键边沿重新冻结65层。
    // 单写焦点资源未进入NumericInputEditState，公共控件会先清焦点，业务freeze便仍看到旧下标。
    reveal(&mut app, body, root);
    click(&mut app, window, root, MouseButton::Middle);
    assert_eq!(app.world().resource::<ActiveNumericInput>().entity, Some(root));
    let editing = app
        .world()
        .get::<NumericFieldBinding>(root)
        .unwrap()
        .editing
        .clone()
        .unwrap();
    assert_eq!(editing.index, 8 + 65 * 2);
    let id = current_object(&app).id;
    app.world_mut()
        .resource_scope(|world, mut session: Mut<EditorAuthoringSession>| {
            world.resource_scope(|world, mut state: Mut<TerrainObjectsEditorState>| {
                let mut editor = world.resource_mut::<SceneEditorState>();
                systems::start(
                    &mut session,
                    &mut editor,
                    &mut state,
                    types::Mutation::RemoveLod(id.clone(), true, 5),
                );
            });
        });
    complete(&mut app);
    assert_eq!(
        presentation::lod_count(Some(&current_object(&app)), true),
        127
    );
    assert_eq!(
        app.world()
            .resource::<TerrainObjectsEditorState>()
            .lod(true).selected,
        5
    );
    app.world_mut().resource_mut::<ActiveNumericInput>().entity = None;
    for value in [529.0, 530.0] {
        late_commit(&mut app, root, value);
    }
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().next_serial,
        1,
        "作者列表删除导致真实下标重排，旧数字必须在启动后台任务前失效"
    );
    let add = action(&mut app, Action::AddLod(true));
    reveal(&mut app, body, add);
    click(&mut app, window, add, MouseButton::Left);
    complete(&mut app);
    assert_eq!(
        presentation::lod_count(Some(&current_object(&app)), true),
        128
    );
    assert_eq!(
        app.world()
            .resource::<TerrainObjectsEditorState>()
            .lod(true).selected,
        127
    );
    for value in [531.0, 532.0] {
        late_commit(&mut app, root, value);
    }
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().next_serial,
        2
    );
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().undo.len(),
        2
    );
    assert!(
        !app.world()
            .resource::<TerrainObjectsEditorState>()
            .preparing()
    );
}
