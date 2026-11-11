//! 当前GLB网格选择到作者水绑定的事件驱动派生缓存。

use bevy::prelude::*; // Resource与ECS系统参数。
use feathrise_scene_content::{SceneId, ScenePlacementWaterSurface}; // 缓存只拥有小型稳定身份和单项绑定值。

use crate::authoring::EditorAuthoringSession; // 作者修订与当前placements权威来源。
use crate::ui::gltf_browser::{EditorGltfBrowserState, EditorGltfMeshSelection}; // 场景树当前稳定选择。
use crate::ui::placement_water::surface_index; // 选择身份到单记录水绑定的唯一匹配规则。

/// 不含易漂移mesh_index与运行时valid位的稳定选择身份。
#[derive(Clone, Debug, PartialEq, Eq)]
struct CachedMeshIdentity {
    placement_id: String,
    node_path: Vec<String>,
    mesh_name: String,
}

impl CachedMeshIdentity {
    fn matches(&self, selection: &EditorGltfMeshSelection) -> bool {
        self.placement_id == selection.placement_id
            && self.node_path == selection.node_path
            && self.mesh_name == selection.mesh_name // 重导出索引和valid刷新不触发8192项作者查找。
    }
}

impl From<&EditorGltfMeshSelection> for CachedMeshIdentity {
    fn from(selection: &EditorGltfMeshSelection) -> Self {
        Self {
            placement_id: selection.placement_id.clone(),
            node_path: selection.node_path.clone(),
            mesh_name: selection.mesh_name.clone(),
        }
    }
}

/// 属性文字、按钮与两个NumericInput共享的一份选中水绑定快照。
#[derive(Resource, Default)]
pub(crate) struct PlacementWaterSelectionCache {
    initialized: bool,                         // 首次无选择也必须与尚未刷新区分。
    scene_id: Option<SceneId>,                     // 同名placement不能跨场景复用旧派生值。
    authoring_revision: u64,                   // 任意作者草稿变化后允许重新定位一次。
    placement_structure_revision: u64,         // 同图等修订整体载入仍确定失效。
    identity: Option<CachedMeshIdentity>,      // 稳定帧只做有界路径比较。
    surface: Option<ScenePlacementWaterSurface>, // 小型值供三个Presentation消费者O(1)读取。
}

impl PlacementWaterSelectionCache {
    pub(super) fn surface(&self) -> Option<&ScenePlacementWaterSurface> {
        self.surface.as_ref() // 普通或已清除网格明确返回None。
    }

    fn source_matches(
        &self,
        selection: Option<&EditorGltfMeshSelection>,
        scene_id: Option<&SceneId>,
        authoring_revision: u64,
        placement_structure_revision: u64,
    ) -> bool {
        self.initialized
            && self.scene_id.as_ref() == scene_id
            && self.authoring_revision == authoring_revision
            && self.placement_structure_revision == placement_structure_revision
            && match (&self.identity, selection) {
                (Some(identity), Some(selection)) => identity.matches(selection),
                (None, None) => true,
                _ => false,
            }
    }
}

/// 源身份未变化时不调用lookup；测试借此锁定稳定帧绝不触及placements Vec。
fn refresh_cached_surface(
    cache: &mut PlacementWaterSelectionCache,
    selection: Option<&EditorGltfMeshSelection>,
    scene_id: Option<&SceneId>,
    authoring_revision: u64,
    placement_structure_revision: u64,
    lookup: impl FnOnce(&EditorGltfMeshSelection) -> Option<ScenePlacementWaterSurface>,
) -> bool {
    if cache.source_matches(
        selection,
        scene_id,
        authoring_revision,
        placement_structure_revision,
    ) {
        return false; // 稳定Presentation帧为标量及至多64段路径比较，不扫描8192条记录。
    }
    cache.surface = selection.and_then(lookup); // 只有选择或作者修订变化时执行一次线性定位。
    cache.identity = selection.map(CachedMeshIdentity::from);
    cache.scene_id = scene_id.cloned();
    cache.authoring_revision = authoring_revision;
    cache.placement_structure_revision = placement_structure_revision;
    cache.initialized = true;
    true
}

/// 在Presentation消费者之前刷新一次派生绑定；稳定帧只验证轻量源身份。
pub(crate) fn refresh_placement_water_selection_cache(
    browser: Res<EditorGltfBrowserState>,
    session: Res<EditorAuthoringSession>,
    mut cache: ResMut<PlacementWaterSelectionCache>,
) {
    let selection = browser.selection.as_ref();
    let draft = session.draft.as_ref();
    let scene_id = draft.map(|draft| &draft.scene_id);
    if cache.source_matches(
        selection,
        scene_id,
        session.revision,
        session.placement_structure_revision,
    ) {
        return; // 先经共享只读借用短路；稳定帧不能触发ResMut::deref_mut或伪造资源Changed。
    }
    refresh_cached_surface(
        &mut cache,
        selection,
        scene_id,
        session.revision,
        session.placement_structure_revision,
        |selection| {
            let record_index = browser.placement_record_index(
                &selection.placement_id,
                session.placement_structure_revision,
            )?; // 完整GLB签名已提供唯一ID索引，连续拖动每档只做O(1)定位。
            let record = draft?.placements.placements.get(record_index)?;
            if record.id != selection.placement_id || !record.editor.selectable() {
                return None; // 防御作者结构与浏览签名未同批采用。
            }
            surface_index(record, selection).map(|index| record.water_surfaces[index].clone())
        },
    );
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    fn selection(id: &str) -> EditorGltfMeshSelection {
        EditorGltfMeshSelection {
            placement_id: id.to_owned(),
            node_path: vec!["河道".to_owned(), "水面".to_owned()],
            mesh_name: "WaterSurface".to_owned(),
            mesh_index: Some(7),
            valid: true,
            orphaned_water_binding: false,
        }
    }

    fn surface() -> ScenePlacementWaterSurface {
        ScenePlacementWaterSurface {
            mesh_index: 7,
            node_path: vec!["河道".to_owned(), "水面".to_owned()],
            mesh_name: "WaterSurface".to_owned(),
            kind: feathrise_scene_content::ScenePlacementWaterKind::Sloped,
            flow_speed: 2.0,
            height_offset: 0.0,
        }
    }

    #[test]
    fn stable_presentation_source_never_calls_placement_lookup_again() {
        let scene_id = SceneId::parse("MAP-selection-cache").expect("测试场景ID合法");
        let selection = selection("river-1");
        let calls = Cell::new(0usize);
        let mut cache = PlacementWaterSelectionCache::default();

        assert!(refresh_cached_surface(
            &mut cache,
            Some(&selection),
            Some(&scene_id),
            11,
            3,
            |_| {
                calls.set(calls.get() + 1);
                Some(surface())
            },
        ));
        assert!(!refresh_cached_surface(
            &mut cache,
            Some(&selection),
            Some(&scene_id),
            11,
            3,
            |_| panic!("稳定帧不得扫描placements"),
        ));

        assert_eq!(calls.get(), 1);
        assert_eq!(cache.surface().expect("缓存绑定存在").flow_speed, 2.0);
    }

    #[test]
    fn authoring_revision_or_stable_selection_change_refreshes_once() {
        let scene_id = SceneId::parse("MAP-selection-cache-change").expect("测试场景ID合法");
        let first = selection("river-1");
        let second = selection("river-2");
        let calls = Cell::new(0usize);
        let mut cache = PlacementWaterSelectionCache::default();
        for (selection, revision) in [(&first, 1), (&first, 2), (&second, 2)] {
            assert!(refresh_cached_surface(
                &mut cache,
                Some(selection),
                Some(&scene_id),
                revision,
                1,
                |_| {
                    calls.set(calls.get() + 1);
                    Some(surface())
                },
            ));
        }
        assert_eq!(calls.get(), 3); // 每个真实源变化只定位一次。
    }

    #[derive(Resource, Default)]
    struct CacheChangeProbe(Vec<bool>); // 记录Presentation消费者实际看到的Bevy资源变更位。

    fn observe_cache_change(
        cache: Res<PlacementWaterSelectionCache>,
        mut probe: ResMut<CacheChangeProbe>,
    ) {
        probe.0.push(cache.is_changed()); // 每帧运行，但不修改被测缓存。
    }

    #[test]
    fn stable_refresh_does_not_mark_selection_cache_changed() {
        let mut app = App::new();
        app.init_resource::<EditorGltfBrowserState>()
            .init_resource::<EditorAuthoringSession>()
            .init_resource::<PlacementWaterSelectionCache>()
            .init_resource::<CacheChangeProbe>()
            .add_systems(
                Update,
                (
                    refresh_placement_water_selection_cache,
                    observe_cache_change,
                )
                    .chain(),
            );

        app.update(); // 首帧把“尚未刷新”收口为空选择快照，必须有Changed。
        app.update(); // 第二帧所有源身份稳定，刷新系统只能走只读短路。

        assert_eq!(app.world().resource::<CacheChangeProbe>().0, [true, false],); // 若无变化帧触发过ResMut::deref_mut，第二项会错误保持true并唤醒昂贵消费者。
    }
}
