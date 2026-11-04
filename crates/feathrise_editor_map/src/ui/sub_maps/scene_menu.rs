//! 子地图菜单冻结引用行；名称集合在唯一CPU槽准备后进入原子图引用历史。

use super::{
    components::Row,
    resources::{RenamePending, SubMapEditorState},
    systems,
};
use crate::{
    authoring::EditorAuthoringSession,
    resources::MapEditorState,
    ui::scene_context_menu::{
        SceneContextMenuState, SceneMenuAction, SceneObjectKey, SceneObjectRow, SceneObjectTarget,
    },
};
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, futures::check_ready},
};

pub(super) fn bind_rows(
    mut commands: Commands,
    session: Res<EditorAuthoringSession>,
    state: Res<SubMapEditorState>,
    rows: Query<(Entity, &Row, Option<&SceneObjectRow>)>,
) {
    for (entity, row, previous) in &rows {
        let target = session.draft.as_ref().and_then(|draft| {
            let index = state.list_start + row.0;
            let record = draft.manifest.sub_maps.get(index)?;
            Some(SceneObjectTarget {
                map: draft.map_id.clone(),
                revision: session.revision,
                key: SceneObjectKey::SubMap {
                    index,
                    id: record.id.clone(),
                },
                name: record.label.clone(),
                hidden: false,
                locked: false,
                copyable: true,
            })
        });
        if previous.is_none_or(|previous| previous.0 != target) {
            commands.entity(entity).insert(SceneObjectRow(target));
        }
    }
}

pub(super) fn rename(
    menu: Option<Res<SceneContextMenuState>>,
    mut editor: ResMut<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    mut state: ResMut<SubMapEditorState>,
) {
    let Some(request) = menu
        .as_ref()
        .and_then(|menu| menu.ready.as_ref())
        .filter(|request| request.action == SceneMenuAction::Rename)
    else {
        return;
    };
    let SceneObjectKey::SubMap { index, .. } = &request.target.key else {
        return;
    };
    if !request.target.current(&editor, &session)
        || !systems::enabled(&editor, &session, &state)
        || state.busy()
        || request.name.is_empty()
        || request.name == request.target.name
    {
        return;
    }
    if let Err(error) = request.target.validate_name(&request.name) {
        editor.status = error.into();
        return;
    }
    let references = session.draft.as_ref().unwrap().manifest.sub_maps.clone();
    let index = *index;
    let name = request.name.clone();
    let map = request.target.map.clone();
    // 只在输入帧共享Arc；完整引用集合及字符串复制在后台，不为名称编辑增加帧规模工作。
    state.rename = Some(RenamePending {
        map: request.target.map.clone(),
        revision: session.revision,
        task: AsyncComputeTaskPool::get().spawn(async move {
            let mut references = references.to_vec();
            references[index].label = name;
            feathrise_map_content::validate_map_instances(&map, &references)?;
            Ok(references.into())
        }),
    });
    editor.status = "正在准备子地图名称修改…".into();
}

pub(super) fn poll(
    mut editor: ResMut<MapEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut state: ResMut<SubMapEditorState>,
) {
    if state
        .retirement
        .as_mut()
        .is_some_and(|task| check_ready(task).is_some())
    {
        state.retirement = None;
    }
    let Some(result) = state
        .rename
        .as_mut()
        .and_then(|pending| check_ready(&mut pending.task))
    else {
        return;
    };
    let pending = state.rename.take().unwrap();
    if !editor.open
        || editor.close_requested
        || editor.map_transition_in_progress
        || session.saving
        || editor.save_requested
        || editor.pending_map_transition.is_some()
        || session.revision != pending.revision
        || editor.selected_map_id.as_ref() != Some(&pending.map)
        || session
            .draft
            .as_ref()
            .is_none_or(|draft| draft.map_id != pending.map)
    {
        editor.status = "子地图名称编辑已失效，旧结果未应用".into();
        state.retirement = Some(AsyncComputeTaskPool::get().spawn(async move {
            drop(result);
        }));
        return;
    }
    match result {
        Ok(references) => {
            let retired = std::mem::take(&mut session.redo);
            systems::apply(&mut session, &mut editor, references);
            if !retired.is_empty() {
                state.retirement = Some(AsyncComputeTaskPool::get().spawn(async move {
                    drop(retired);
                }));
            }
            editor.status = "已修改子地图名称，可撤销".into();
        }
        Err(error) => {
            editor.status = error;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::scene_context_menu::SceneObjectRequest;
    use bevy::ecs::system::RunSystemOnce;
    use feathrise_map_content::{MapId, MapInstanceId, MapInstancePlacement};

    fn app() -> App {
        let mut draft = crate::authoring::test_heightfield_draft(vec![0; 121].into());
        draft.manifest.sub_maps = (0..3)
            .map(|index| MapInstancePlacement {
                id: MapInstanceId(format!("sub-{index}")),
                source_map_id: MapId("source-map".into()),
                label: format!("原引用{index}"),
                transform: default(),
            })
            .collect::<Vec<_>>()
            .into();
        let map = draft.map_id.clone();
        let mut menu = SceneContextMenuState::default();
        menu.ready = Some(SceneObjectRequest {
            target: SceneObjectTarget {
                map: map.clone(),
                revision: 0,
                key: SceneObjectKey::SubMap {
                    index: 2,
                    id: MapInstanceId("sub-2".into()),
                },
                name: "原引用2".into(),
                hidden: false,
                locked: false,
                copyable: true,
            },
            action: SceneMenuAction::Rename,
            name: "山门里的村庄".into(),
        });
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(EditorAuthoringSession {
                draft: Some(draft),
                ..default()
            })
            .insert_resource(MapEditorState {
                open: true,
                selected_map_id: Some(map),
                ..default()
            })
            .insert_resource(menu)
            .init_resource::<SubMapEditorState>()
            .add_systems(Update, poll);
        app.world_mut().run_system_once(rename).unwrap();
        app.world_mut()
            .resource_mut::<SceneContextMenuState>()
            .ready = None;
        assert!(app.world().resource::<SubMapEditorState>().busy());
        app
    }

    fn complete(app: &mut App) {
        let start = std::time::Instant::now();
        while app.world().resource::<SubMapEditorState>().busy() {
            app.update();
            assert!(start.elapsed().as_secs() < 5);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    #[test]
    fn scene_context_menu_sub_map_renames_only_reference_label_with_original_history() {
        let mut app = app();
        complete(&mut app);
        let session = app.world().resource::<EditorAuthoringSession>();
        let references = &session.draft.as_ref().unwrap().manifest.sub_maps;
        assert_eq!(references[0].label, "原引用0");
        assert_eq!(references[1].label, "原引用1");
        assert_eq!(references[2].label, "山门里的村庄");
        assert_eq!(references[2].source_map_id.as_str(), "source-map");
        assert_eq!(session.undo.len(), 1);
        let crate::authoring::EditorCommand::SubMapsChanged { before, after } = &session.undo[0]
        else {
            panic!("应复用引用历史");
        };
        assert_eq!(before[2].label, "原引用2");
        assert_eq!(after[2].label, "山门里的村庄");
    }

    #[test]
    fn scene_context_menu_sub_map_rejects_old_background_name_after_revision_or_map_change() {
        for change_map in [false, true] {
            let mut app = app();
            if change_map {
                app.world_mut()
                    .resource_mut::<MapEditorState>()
                    .selected_map_id = None;
            } else {
                app.world_mut()
                    .resource_mut::<EditorAuthoringSession>()
                    .revision += 1;
            }
            complete(&mut app);
            let session = app.world().resource::<EditorAuthoringSession>();
            assert_eq!(
                session.draft.as_ref().unwrap().manifest.sub_maps[2].label,
                "原引用2"
            );
            assert!(session.undo.is_empty());
        }
    }

    #[test]
    fn scene_context_menu_sub_map_background_failure_preserves_original_reference_and_history() {
        let mut app = app();
        app.world_mut()
            .resource_mut::<SubMapEditorState>()
            .rename
            .as_mut()
            .unwrap()
            .task =
            AsyncComputeTaskPool::get().spawn(async { Err("引用名称校验失败".into()) });
        complete(&mut app);
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(
            session.draft.as_ref().unwrap().manifest.sub_maps[2].label,
            "原引用2"
        );
        assert!(session.undo.is_empty());
        assert_eq!(
            app.world().resource::<MapEditorState>().status,
            "引用名称校验失败"
        );
    }
}
