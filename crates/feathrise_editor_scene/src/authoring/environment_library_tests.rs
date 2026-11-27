//! 环境库作者事务、场景来源及正式 JSON 重开后的回归。

use std::sync::Arc;

use feathrise_scene_content::{DEFAULT_ENVIRONMENT_PROFILE_ID, WorldEnvironmentAsset};

use super::EnvironmentLibraryEditRequest::*;
use super::*;

struct Fixture {
    global: GlobalEnvironmentAuthoringState,
    session: EditorAuthoringSession,
    editor: SceneEditorState,
}

impl Fixture {
    fn legacy() -> Self {
        let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
        draft.manifest.environment = serde_json::from_value(serde_json::json!({
            "skybox_brightness": 730.0,
            "skybox_yaw_degrees": 35.0,
            "celestial_overrides": { "day_base_illuminance_lux": 7200.0 },
            "weather": { "region": "kaifeng", "climate": "kaifeng_plain" }
        })).unwrap();
        let mut global = GlobalEnvironmentAuthoringState::default();
        global.draft.library = global.draft.library.clone().with_legacy_default(&global.draft.celestial_lighting);
        global.saved = global.draft.clone();
        global.maintain_environment_selection();
        Self {
            global,
            session: EditorAuthoringSession { draft: Some(draft), ..default() },
            editor: SceneEditorState::default(),
        }
    }

    fn action(&mut self, request: EnvironmentLibraryEditRequest) {
        library_action(request, &mut self.global, &mut self.session, &mut self.editor, EnvironmentSimulation::default()).unwrap();
    }

    fn environment(&self) -> &SceneEnvironmentDescriptor {
        &self.session.draft.as_ref().unwrap().manifest.environment
    }

    fn reopen(&mut self) {
        // 从正式资产类型编码的 JSON 重建环境，不沿用原草稿或缓存。
        let global = serde_json::to_vec(&self.global.draft).unwrap();
        let draft = self.session.draft.as_mut().unwrap();
        draft.manifest = serde_json::from_slice(&serde_json::to_vec(&draft.manifest).unwrap()).unwrap();
        draft.environment_dirty = false;
        self.session.undo.clear();
        self.session.redo.clear();
        let loaded: WorldEnvironmentAsset = serde_json::from_slice(&global).unwrap();
        loaded.validate().unwrap();
        self.global.saved = loaded.clone();
        self.global.draft = loaded;
        self.global.dirty = false;
        self.editor.dirty = false;
    }
}

#[test]
fn named_global_default_requests_do_not_migrate_legacy_scene_until_selected() {
    let mut fixture = Fixture::legacy();
    let legacy = fixture.environment().clone();
    fixture.action(Create);
    let shared = fixture.global.selected_environment_id.clone().unwrap();
    assert_ne!(shared, DEFAULT_ENVIRONMENT_PROFILE_ID);
    fixture.action(Rename(shared.clone(), "暮色港口".into()));
    fixture.editor.selected_asset = Some("textures/dusk.ktx2".into());
    fixture.action(SetGlobalSelectedSkybox);
    fixture.action(SetGlobalSkyMode(SceneSkyMode::Skybox));
    fixture.action(SetDefault(shared.clone()));
    assert_eq!(fixture.environment(), &legacy);
    assert!(!fixture.session.draft.as_ref().unwrap().environment_dirty);
    assert!(fixture.global.dirty);
    assert!(fixture.session.undo.iter().all(EditorCommand::is_global_environment));

    fixture.reopen();
    assert_eq!(fixture.environment(), &legacy);
    assert!(serde_json::to_value(fixture.environment()).unwrap().get("source").is_none());
    assert_eq!(fixture.global.draft.library.default_profile_id, shared);
    let profile = &fixture.global.draft.library.profiles[&shared];
    assert_eq!(profile.name, "暮色港口");
    assert_eq!(profile.appearance.celestial_lighting.day_base_illuminance_lux, 7200.0);

    fixture.action(UseProjectDefault);
    fixture.reopen();
    assert_eq!(fixture.environment().source, SceneEnvironmentSource::InheritDefault);
    let asset = &fixture.global.draft;
    let resolved = resolve_scene_environment(fixture.environment(), &asset.celestial_lighting, &asset.library);
    assert_eq!(resolved.profile_id, Some(shared));
    assert_eq!(resolved.environment.sky_mode, SceneSkyMode::Skybox);
    assert_eq!(resolved.environment.skybox_path.as_deref(), Some("textures/dusk.ktx2"));
    assert_eq!(resolved.environment.weather, legacy.weather);
}

#[test]
fn local_copy_survives_reopening_while_scene_references_a_shared_profile() {
    let mut fixture = Fixture::legacy();
    let weather = fixture.environment().weather;
    fixture.action(Create);
    let shared = fixture.global.selected_environment_id.clone().unwrap();
    fixture.action(UseShared(shared.clone()));
    fixture.action(MakeLocal);
    assert_eq!(fixture.environment().skybox_brightness, 730.0);
    assert_eq!(fixture.environment().celestial_overrides.day_base_illuminance_lux, Some(7200.0));

    let before = fixture.environment().clone();
    let after = SceneEnvironmentDescriptor { skybox_yaw_degrees: 115.0, ..before.clone() };
    commit_environment_change(&mut fixture.session, &mut fixture.editor, before, after.clone(), "测试独立环境修改");
    fixture.action(UseShared(shared.clone()));
    fixture.reopen();

    // 重开时显示共享方案，暂存的本地数值留待切回独立环境。
    assert_eq!(fixture.environment().source, SceneEnvironmentSource::Shared(shared.clone()));
    assert_eq!(fixture.environment().weather, weather);
    let asset = &fixture.global.draft;
    let resolved = resolve_scene_environment(fixture.environment(), &asset.celestial_lighting, &asset.library);
    assert_eq!(resolved.environment.skybox_yaw_degrees, 35.0);
    assert_eq!(resolved.environment.weather, weather);
    fixture.action(MakeLocal);
    assert_eq!(fixture.environment(), &after);
    assert_eq!(fixture.global.draft.library.profiles[&shared].appearance.skybox_yaw_degrees, 35.0);
}
