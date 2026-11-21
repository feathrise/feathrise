//! 仅编辑器可见的植被贡献覆盖；保留正式输入，跨地形切换按稳定画布归还。

use bevy::{ecs::system::ParamSet, prelude::*, tasks::{AsyncComputeTaskPool, Task, futures::check_ready}};
use feathrise_scene::{DynamicBladeDraft, DynamicBladeDraftSnapshot, TerrainVegetationDrafts};
use feathrise_scene_content::{ActiveDynamicScene, SceneId, SceneVegetationAsset, TerrainId};
use feathrise_scene_core::residency::InstanceResourceTable;
use std::{collections::{BTreeMap, BTreeSet}, sync::Arc};

use super::{
    EditorAuthoringSession, EditorSceneDraft,
    live_density_dynamic::{PreparedDensityImages, prepare_density_images},
    vegetation_compile::compile_vegetation_authoring,
    vegetation_layers::hide_layers_in_preview,
};
use crate::resources::SceneEditorState;

type Canvas = (SceneId, Option<TerrainId>);
type Destinations<'w, 's> = ParamSet<'w, 's, (
    ResMut<'static, DynamicBladeDraft>,
    TerrainVegetationDrafts<'static>,
    Option<Res<'static, InstanceResourceTable<DynamicBladeDraft>>>,
)>;
type Pixels = BTreeMap<String, Arc<Vec<u8>>>;

#[derive(Clone, PartialEq, Eq)]
struct Ticket {
    canvas: Canvas,
    revision: (u64, u64, usize),
    hidden: Arc<BTreeSet<String>>,
}

struct Overlay {
    original: Option<Arc<DynamicBladeDraftSnapshot>>,
    shown: Arc<DynamicBladeDraftSnapshot>,
}

struct Prepared {
    definition: SceneVegetationAsset,
    images: PreparedDensityImages,
    handles: BTreeMap<String, Handle<Image>>,
}

#[derive(Default)]
pub(super) struct VisibilityState {
    task: Option<Task<(Ticket, Result<Option<Prepared>, String>)>>,
    upload: Option<(Ticket, Prepared)>,
    attempted: BTreeMap<Canvas, Ticket>,
    overlays: BTreeMap<Canvas, Overlay>,
    observed: (Option<SceneId>, u64, u64, u64),
    root_revision: Option<u64>,
    needs_schedule: bool,
}

struct Source<'a> {
    definition: &'a SceneVegetationAsset,
    pixels: &'a Pixels,
    min: [f32; 2],
    max: [f32; 2],
    address: usize,
}

/// 三维场保存正式输出 ID。唯一引用可通过撤下预览输出关闭真实三维样本。
/// 同一输出有多个作者层时保留输出与兄弟层，历史三维样本没有可分离的作者归属。
fn hide_single_owner_outputs(definition: &mut SceneVegetationAsset, hidden: &BTreeSet<String>) -> bool {
    let Some(authoring) = &mut definition.authoring else { return false };
    let mut counts = BTreeMap::<String, (usize, bool)>::new();
    for layer in &authoring.layers {
        let entry = counts.entry(layer.output_layer_id.clone()).or_default();
        entry.0 += 1;
        entry.1 |= hidden.contains(&layer.id);
    }
    let hidden_output = |id: &str| counts.get(id) == Some(&(1, true));
    let before = definition.layers.len();
    definition.layers.retain(|layer| !hidden_output(&layer.id));
    authoring.layers.retain(|layer| !hidden_output(&layer.output_layer_id));
    definition.layers.len() != before
}

fn source<'a>(draft: &'a EditorSceneDraft, terrain: Option<&TerrainId>) -> Option<Source<'a>> {
    let active = draft.terrain_documents.active.as_ref().map(|canvas| &canvas.definition.id);
    if active == terrain {
        if !super::vegetation_compile::density_canvas_is_active(draft) || draft.terrain_documents.loading {
            return None; // 模型根由现有模型植被发布器处理，等待切换时不借用旧二维槽。
        }
        let manifest = draft.terrain_manifest();
        Some(Source {
            definition: draft.vegetation.as_ref()?,
            pixels: &draft.vegetation_author_densities,
            min: manifest.world_min,
            max: manifest.world_max,
            address: 0,
        })
    } else {
        let canvas = draft.terrain_documents.values.get(terrain?)?;
        let manifest = &canvas.identity.as_ref()?.manifest;
        Some(Source {
            definition: canvas.vegetation.as_ref()?,
            pixels: &canvas.vegetation_author_densities,
            min: manifest.world_min,
            max: manifest.world_max,
            address: Arc::as_ptr(canvas) as usize,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn vegetation_editor_visibility_keeps_shared_output_and_restores_formal_snapshot() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<Assets<Image>>()
            .init_resource::<DynamicBladeDraft>()
            .init_resource::<EditorAuthoringSession>()
            .init_resource::<SceneEditorState>()
            .add_systems(Update, sync);
        crate::ui::scene_layers::tests::load_author_layers_fixture(&mut app);
        let (map, mut formal, shared_output, hidden_layer, unique_output, unique_layer) = {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            let draft = session.draft.as_mut().unwrap();
            draft.manifest.terrains = None;
            draft.terrain_documents.active = None;
            let definition = draft.vegetation.as_mut().unwrap();
            let shared_output = definition.layers[0].id.clone();
            let unique_output = definition.layers[2].id.clone();
            definition.layers.remove(1);
            let authors = &mut definition.authoring.as_mut().unwrap().layers;
            authors[1].output_layer_id = shared_output.clone();
            let hidden_layer = authors[0].id.clone();
            let unique_layer = authors[2].id.clone();
            for (author, pixels) in authors.iter().zip([128, 64, 255]) {
                draft.vegetation_author_densities.insert(author.id.clone(), Arc::new(vec![pixels; 4]));
            }
            let compiled = compile_vegetation_authoring(definition, &draft.vegetation_author_densities,
                draft.manifest.world_min, draft.manifest.world_max).unwrap();
            let prepared = prepare_density_images(definition, &compiled.outputs).unwrap();
            draft.vegetation_densities = compiled.outputs;
            let formal = DynamicBladeDraftSnapshot {
                source_revision: 0, definition: definition.clone(), densities: default(),
                sources: prepared.sources, nonzero_layers: prepared.nonzero_layers,
            };
            (draft.scene_id.clone(), (formal, prepared.images), shared_output, hidden_layer, unique_output, unique_layer)
        };
        assert_eq!(formal.0.sources[&shared_output].as_ref(), &[192; 4]);
        {
            let mut images = app.world_mut().resource_mut::<Assets<Image>>();
            formal.0.densities = formal.1.drain(..).map(|(id, image)| (id, images.add(image))).collect();
        }
        app.world_mut().resource_mut::<DynamicBladeDraft>().publish(formal.0);
        app.insert_resource(ActiveDynamicScene { scene_id: Some(map.clone()) });
        {
            let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
            editor.open = true;
            editor.vegetation_visibility.toggle(&map, None, &hidden_layer);
            editor.vegetation_visibility.toggle(&map, None, &unique_layer);
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            app.update();
            let display = app.world().resource::<DynamicBladeDraft>().for_scene(&map).unwrap();
            if display.sources[&shared_output].as_ref() == &[64; 4] {
                assert!(!display.densities.contains_key(&unique_output), "唯一归属的旧三维输出不该保留运行时绘制配置");
                break;
            }
            assert!(Instant::now() < deadline, "未采用完整编辑器显隐结果");
            std::thread::sleep(Duration::from_millis(1));
        }
        let draft = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap();
        assert_eq!(draft.vegetation_densities[&shared_output].as_slice(), &[192; 4]);
        assert!(draft.vegetation.as_ref().unwrap().authoring.as_ref().unwrap().layers[0].enabled);
        app.world_mut().resource_mut::<SceneEditorState>().open = false;
        app.update();
        let display = app.world().resource::<DynamicBladeDraft>().for_scene(&map).unwrap();
        assert_eq!(display.sources[&shared_output].as_ref(), &[192; 4]);
        assert!(display.densities.contains_key(&unique_output));
    }
}

fn frozen(destinations: &mut Destinations, canvas: &Canvas) -> Option<Arc<DynamicBladeDraftSnapshot>> {
    match &canvas.1 {
        Some(terrain) => destinations.p1().shared(&canvas.0, terrain),
        None => destinations.p0().frozen_for_scene(&canvas.0),
    }
}

fn publish(destinations: &mut Destinations, canvas: &Canvas, snapshot: DynamicBladeDraftSnapshot) -> bool {
    match &canvas.1 {
        Some(terrain) => destinations.p1().publish(&canvas.0, vec![(terrain.clone(), snapshot)]).is_ok(),
        None => { destinations.p0().publish(snapshot); true }
    }
}

fn restore(destinations: &mut Destinations, canvas: &Canvas, overlay: &Overlay) {
    let current = frozen(destinations, canvas);
    if !current.as_ref().is_some_and(|current| Arc::ptr_eq(current, &overlay.shown)) {
        return; // 新的正式草稿已采用，旧显示覆盖无权覆盖它。
    }
    if let Some(original) = &overlay.original {
        publish(destinations, canvas, (**original).clone());
    } else {
        match &canvas.1 {
            Some(terrain) => destinations.p1().clear(&canvas.0, std::iter::once(terrain.clone())),
            None => destinations.p0().clear(),
        }
    }
}

/// 独立于正式编译，单飞计算一个画布的临时显示。完整结果仍走运行时原有原子发布。
pub(super) fn sync(
    mut state: Local<VisibilityState>,
    session: Res<EditorAuthoringSession>,
    editor: Res<SceneEditorState>,
    active: Res<ActiveDynamicScene>,
    mut destinations: Destinations,
    mut images: ResMut<Assets<Image>>,
) {
    let draft = session.draft.as_ref().filter(|draft| {
        editor.open && !editor.close_requested && !editor.scene_transition_in_progress
            && active.scene_id.as_ref() == Some(&draft.scene_id) && draft.manifest.is_data_driven()
    });
    let masks = draft.and_then(|draft| editor.vegetation_visibility.canvases(&draft.scene_id));
    if masks.is_none() && state.overlays.is_empty() && state.attempted.is_empty()
        && state.task.is_none() && state.upload.is_none() {
        return;
    }
    let requested = |canvas: &Canvas| draft.is_some_and(|draft| draft.scene_id == canvas.0)
        && masks.is_some_and(|masks| masks.contains_key(&canvas.1));
    let revision = |source: &Source| (session.vegetation_revision, session.vegetation_structure_revision, source.address);
    let current = |ticket: &Ticket| requested(&ticket.canvas)
        && masks.and_then(|masks| masks.get(&ticket.canvas.1)) == Some(&ticket.hidden)
        && draft.and_then(|draft| source(draft, ticket.canvas.1.as_ref()))
            .is_some_and(|source| revision(&source) == ticket.revision);

    let observed = (draft.map(|draft| draft.scene_id.clone()), session.revision,
        session.vegetation_revision, editor.vegetation_visibility.revision());
    let changed = observed != state.observed;
    let root_revision = draft.and_then(|draft| destinations.p0().revision_for_scene(&draft.scene_id));
    let runtime_changed = state.root_revision != root_revision || destinations.p2().is_some_and(|table| table.is_changed());
    state.root_revision = root_revision; // 正式系统每帧借用可变资源，只按实际发布修订认定根画布换代。
    if changed || runtime_changed {
        state.observed = observed;
        state.needs_schedule = true;
        state.attempted.retain(|canvas, _| requested(canvas));
        state.overlays.retain(|canvas, overlay| {
            if !requested(canvas) {
                restore(&mut destinations, canvas, overlay);
                return false;
            }
            let original = frozen(&mut destinations, canvas);
            if !original.as_ref().is_some_and(|original| Arc::ptr_eq(original, &overlay.shown))
                && publish(&mut destinations, canvas, (*overlay.shown).clone())
                && let Some(shown) = frozen(&mut destinations, canvas)
            {
                // 原作者重新发布时记录新的正式底稿。等本次过滤完成前继续使用上一个完整显示。
                overlay.original = original;
                overlay.shown = shown;
            }
            true
        }); // 稳定帧不遍历曾经隐藏过的所有画布。
    }

    if let Some((ticket, result)) = state.task.as_mut().and_then(check_ready) {
        state.task = None;
        if current(&ticket) {
            match result {
                Ok(Some(prepared)) => state.upload = Some((ticket, prepared)),
                Ok(None) => {
                    if let Some(overlay) = state.overlays.remove(&ticket.canvas) {
                        restore(&mut destinations, &ticket.canvas, &overlay);
                    }
                }
                Err(error) => warn!("植被显示预览准备失败：{error}"),
            }
        } else {
            AsyncComputeTaskPool::get().spawn(async move { drop(result); }).detach();
        }
    }
    if let Some((ticket, mut prepared)) = state.upload.take() {
        if !current(&ticket) {
            AsyncComputeTaskPool::get().spawn(async move { drop(prepared); }).detach();
        } else if let Some((id, image)) = prepared.images.images.pop_front() {
            prepared.handles.insert(id, images.add(image));
            state.upload = Some((ticket, prepared));
            return; // 每帧最多登记一张，全部图片齐备后才可见。
        } else if ticket.canvas.1.as_ref().is_some_and(|terrain| {
            !destinations.p1().ready(&ticket.canvas.0, std::iter::once(terrain.clone()))
        }) {
            state.attempted.remove(&ticket.canvas); // 等来源重新驻留时再准备，释放其他画布的单飞槽。
            AsyncComputeTaskPool::get().spawn(async move { drop(prepared); }).detach();
        } else {
            let original = state.overlays.get(&ticket.canvas)
                .map_or_else(|| frozen(&mut destinations, &ticket.canvas), |overlay| overlay.original.clone());
            let snapshot = DynamicBladeDraftSnapshot {
                source_revision: ticket.revision.0,
                definition: prepared.definition,
                densities: prepared.handles,
                sources: prepared.images.sources,
                nonzero_layers: prepared.images.nonzero_layers,
            };
            if publish(&mut destinations, &ticket.canvas, snapshot)
                && let Some(shown) = frozen(&mut destinations, &ticket.canvas)
            {
                state.overlays.insert(ticket.canvas, Overlay { original, shown });
            }
        }
    }
    if state.task.is_some() || session.vegetation_cross_busy || !state.needs_schedule { return; }
    let (Some(draft), Some(masks)) = (draft, masks) else { return };
    state.needs_schedule = false;
    for (terrain, hidden) in masks {
        let Some(source) = source(draft, terrain.as_ref()) else { continue };
        if terrain.as_ref().is_some_and(|terrain| !destinations.p1().ready(&draft.scene_id, std::iter::once(terrain.clone()))) {
            continue;
        }
        let ticket = Ticket { canvas: (draft.scene_id.clone(), terrain.clone()), revision: revision(&source), hidden: hidden.clone() };
        if state.attempted.get(&ticket.canvas) == Some(&ticket) { continue; }
        state.attempted.insert(ticket.canvas.clone(), ticket.clone());
        let mut definition = source.definition.clone();
        let pixels = source.pixels.clone();
        let (min, max) = (source.min, source.max);
        state.task = Some(AsyncComputeTaskPool::get().spawn(async move {
            let filtered = hide_layers_in_preview(&mut definition, &ticket.hidden);
            let removed = hide_single_owner_outputs(&mut definition, &ticket.hidden);
            if !filtered && !removed { return (ticket, Ok(None)); }
            let result = compile_vegetation_authoring(&definition, &pixels, min, max)
                .and_then(|compiled| prepare_density_images(&definition, &compiled.outputs))
                .map(|images| Some(Prepared { definition, images, handles: BTreeMap::new() }));
            (ticket, result)
        }));
        state.needs_schedule = true;
        break;
    }
}
