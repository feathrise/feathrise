//! 完整水体的作者高亮：共享正式网格和波浪材质，子块按固定预算镜像，完成后原子显现。

use bevy::asset::AssetId;
use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};
use bevy::ecs::system::{EntityCommands, SystemParam};
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::picking::Pickable;
use bevy::prelude::*;
use feathrise_scene_core::view::CameraRaycastIgnore;
use feathrise_scene::{ClipmapWaterMaterial, DynamicWaterMaterial, PaintedWaterSurface};

use super::SelectedWaterViewportState;
use crate::components::{SceneEditorOwnedEntity, SceneEditorSelectedWaterBadge, SceneEditorSelectedWaterHighlight};
use crate::viewport::brush_cursor::SCENE_EDITOR_BRUSH_RENDER_LAYER;

const HIGHLIGHT_DEPTH_BIAS: f32 = 20_000.0; // 在正式透明水面之后显示作者淡青覆盖。
const HIGHLIGHT_CHUNK_OPERATIONS_PER_FRAME: usize = 64; // 只登记共享句柄，不读顶点；回收与创建共用预算。

/// 根和首块网格共同标记完整几何代次；退旧、建新期间根保持隐藏，不能只显示首块。
pub(super) struct HighlightChunkBuild {
    source: (Entity, AssetId<Mesh>),
    child_count: usize,
    next_child: Option<usize>, // None先回收上一代子块，Some随后递增遍历当前已选根的直接子项。
}

#[derive(SystemParam)]
pub(crate) struct WaterHighlightContext<'w, 's> {
    materials: ResMut<'w, Assets<DynamicWaterMaterial>>,
    clipmap_materials: ResMut<'w, Assets<ClipmapWaterMaterial>>,
    roots: Query<'w, 's, (
        Option<&'static MeshMaterial3d<DynamicWaterMaterial>>,
        Option<&'static MeshMaterial3d<ClipmapWaterMaterial>>,
        Option<&'static Children>,
    ), (With<PaintedWaterSurface>, Without<SceneEditorSelectedWaterHighlight>)>,
    chunks: Query<'w, 's, (
        &'static Mesh3d,
        &'static Transform,
        Option<&'static MeshMaterial3d<DynamicWaterMaterial>>,
        Option<&'static MeshMaterial3d<ClipmapWaterMaterial>>,
        Option<&'static Visibility>,
    ), (Without<PaintedWaterSurface>, Without<SceneEditorSelectedWaterHighlight>, Without<SceneEditorSelectedWaterBadge>)>, // 分块只读Visibility也必须排除UI徽标，不能与同系统的徽标写查询重叠。
    highlights: Query<'w, 's, (
        &'static mut Mesh3d,
        &'static mut Transform,
        &'static mut Visibility,
        Option<&'static MeshMaterial3d<DynamicWaterMaterial>>,
        Option<&'static MeshMaterial3d<ClipmapWaterMaterial>>,
    ), (With<SceneEditorSelectedWaterHighlight>, Without<PaintedWaterSurface>)>,
}

enum HighlightMaterial {
    Dynamic(Handle<DynamicWaterMaterial>),
    Clipmap(Handle<ClipmapWaterMaterial>),
}

impl HighlightMaterial {
    fn insert(&self, entity: &mut EntityCommands) {
        match self {
            Self::Dynamic(handle) => {
                entity.remove::<MeshMaterial3d<ClipmapWaterMaterial>>()
                    .insert(MeshMaterial3d(handle.clone()));
            }
            Self::Clipmap(handle) => {
                entity.remove::<MeshMaterial3d<DynamicWaterMaterial>>()
                    .insert(MeshMaterial3d(handle.clone()));
            }
        }
    }

    fn matches(&self, dynamic: Option<&MeshMaterial3d<DynamicWaterMaterial>>,
        clipmap: Option<&MeshMaterial3d<ClipmapWaterMaterial>>) -> bool {
        match self {
            Self::Dynamic(handle) => dynamic.is_some_and(|current| current.0 == *handle) && clipmap.is_none(),
            Self::Clipmap(handle) => clipmap.is_some_and(|current| current.0 == *handle) && dynamic.is_none(),
        }
    }
}

fn tint(base: &mut StandardMaterial, alpha: f32) {
    base.base_color = Color::srgba(0.30, 0.82, 0.84, alpha);
    base.alpha_mode = AlphaMode::Blend;
    base.unlit = true;
    base.double_sided = true;
    base.cull_mode = None;
    base.depth_bias = HIGHLIGHT_DEPTH_BIAS;
}

/// 保留正式扩展的波形、时间历史、顶点透明度及高度/R8绑定，只更改编辑器强调色。
fn sync_material<T: Asset>(assets: &mut Assets<T>, slot: &mut Option<Handle<T>>, value: T) -> Handle<T> {
    if let Some(handle) = slot.as_ref() {
        if let Some(mut current) = assets.get_mut(handle) {
            *current = value;
            return handle.clone();
        }
    }
    let handle = assets.add(value);
    *slot = Some(handle.clone());
    handle
}

fn spawn_highlight(commands: &mut Commands, mesh: Mesh3d, transform: Transform,
    parent: Option<Entity>, material: &HighlightMaterial) -> Entity {
    let mut entity = commands.spawn((
        Name::new("场景编辑器选中水域完整分块覆盖"),
        SceneEditorSelectedWaterHighlight,
        SceneEditorOwnedEntity,
        mesh,
        transform,
        // 根完成前隐藏全部子块；已完成覆盖由根一次解除隐藏，玩法相机始终看不到作者层。
        if parent.is_some() { Visibility::Inherited } else { Visibility::Hidden },
        RenderLayers::layer(SCENE_EDITOR_BRUSH_RENDER_LAYER),
        NoFrustumCulling,
        CameraRaycastIgnore,
        NotShadowCaster,
        NotShadowReceiver,
        Pickable::IGNORE,
    ));
    if let Some(parent) = parent { entity.insert(ChildOf(parent)); }
    material.insert(&mut entity);
    entity.id()
}

impl WaterHighlightContext<'_, '_> {
    pub(super) fn hide(&mut self, state: &SelectedWaterViewportState) {
        let Some(entity) = state.highlight_entity else { return; };
        if let Ok((_, _, mut visibility, _, _)) = self.highlights.get_mut(entity) {
            if *visibility != Visibility::Hidden { *visibility = Visibility::Hidden; }
        }
    }

    /// 稳定帧仅核验唯一根和同步固定大小材质；几何换代才按预算访问本水体的直接子块。
    pub(super) fn sync(&mut self, commands: &mut Commands, state: &mut SelectedWaterViewportState,
        source: Entity, mesh: &Mesh3d, transform: &Transform) {
        let Ok((source_dynamic, source_clipmap, children)) = self.roots.get(source) else {
            self.hide(state);
            return;
        };
        let source_dynamic = source_dynamic.map(|material| material.0.clone());
        let source_clipmap = source_clipmap.map(|material| material.0.clone());
        let child_count = children.map_or(0, |children| children.len());
        let material = if let Some(source) = source_clipmap.as_ref() {
            let Some(mut highlighted) = self.clipmap_materials.get(source).cloned() else {
                self.hide(state);
                return; // 缺少正式材质时不能回退成不带裁切的静止整面。
            };
            tint(&mut highlighted.base.base, 0.34);
            HighlightMaterial::Clipmap(sync_material(&mut self.clipmap_materials,
                &mut state.clipmap_highlight_material, highlighted))
        } else if let Some(source) = source_dynamic.as_ref() {
            let Some(mut highlighted) = self.materials.get(source).cloned() else {
                self.hide(state);
                return;
            };
            tint(&mut highlighted.base, 0.28);
            HighlightMaterial::Dynamic(sync_material(&mut self.materials,
                &mut state.dynamic_highlight_material, highlighted))
        } else {
            self.hide(state);
            return; // 只有真实强类型水材质才能证明高亮与正式水面使用同一顶点位移。
        };

        let reused = state.highlight_entity.is_some_and(|entity| self.highlights.contains(entity));
        let root = if reused {
            state.highlight_entity.expect("已核验当前高亮根")
        } else {
            let root = spawn_highlight(commands, mesh.clone(), *transform, None, &material);
            state.highlight_entity = Some(root);
            state.highlight_mesh_source = None;
            state.highlight_chunks.clear(); // 旧根已被编辑器清理时，ChildOf已同步回收其子块。
            state.highlight_build = None;
            root
        };
        let key = (source, mesh.0.id());
        let complete = state.highlight_mesh_source == Some(key)
            && state.highlight_source_child_count == child_count;
        if !complete && state.highlight_build.as_ref().is_none_or(|build|
            build.source != key || build.child_count != child_count) {
            state.highlight_build = Some(HighlightChunkBuild { source: key, child_count, next_child: None });
            state.highlight_mesh_source = None;
        }
        if let Ok((mut current_mesh, mut current_transform, mut visibility, dynamic, clipmap)) = self.highlights.get_mut(root) {
            if current_mesh.0 != mesh.0 { *current_mesh = mesh.clone(); }
            if *current_transform != *transform { *current_transform = *transform; }
            let desired = if complete { Visibility::Inherited } else { Visibility::Hidden };
            if *visibility != desired { *visibility = desired; }
            if !material.matches(dynamic, clipmap) { material.insert(&mut commands.entity(root)); }
        }
        if complete { return; }

        let build = state.highlight_build.as_mut().expect("未完成几何必须有构建状态");
        let mut remaining = HIGHLIGHT_CHUNK_OPERATIONS_PER_FRAME;
        if build.next_child.is_none() {
            while remaining > 0 {
                let Some(entity) = state.highlight_chunks.pop() else { break; };
                if self.highlights.contains(entity) { commands.entity(entity).despawn(); }
                remaining -= 1;
            }
            if !state.highlight_chunks.is_empty() { return; }
            build.next_child = Some(0);
        }
        let mut next_child = build.next_child.unwrap_or_default();
        let children = self.roots.get(source).ok().and_then(|(_, _, children)| children);
        while remaining > 0 && next_child < child_count {
            let child = children.expect("已选根子项数量来自同一查询")[next_child];
            next_child += 1;
            remaining -= 1;
            let Ok((mesh, transform, dynamic, clipmap, visibility)) = self.chunks.get(child) else { continue; };
            if visibility.is_some_and(|visibility| *visibility == Visibility::Hidden) { continue; }
            let belongs = match &material {
                HighlightMaterial::Dynamic(_) => dynamic.is_some_and(|material| Some(&material.0) == source_dynamic.as_ref()),
                HighlightMaterial::Clipmap(_) => clipmap.is_some_and(|material| Some(&material.0) == source_clipmap.as_ref()),
            };
            if !belongs { continue; } // 同根的雨水高度代理等辅助实体不属于可见水面网格。
            state.highlight_chunks.push(spawn_highlight(commands, mesh.clone(), *transform, Some(root), &material));
        }
        build.next_child = Some(next_child);
        if next_child == child_count {
            state.highlight_mesh_source = Some(key);
            state.highlight_source_child_count = child_count;
            state.highlight_build = None;
            commands.entity(root).insert(Visibility::Inherited); // 全部子块与首块在同一次Commands提交显现。
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::world::CommandQueue;
    use feathrise_scene::{
        BuiltWaterSurface, PaintedWaterMeshData, PaintedWaterPurpose, WaterSurfaceDescriptor,
        advance_water_surface_material, chunk_water_surfaces, spawn_staged_water_surface,
        stage_water_surface_chunk,
    };
    use std::sync::Arc;

    use super::super::systems::sync_selected_water_viewport;
    use crate::authoring::{EditorAuthoringSession, WaterPreviewState};
    use crate::resources::SceneEditorState;
    use crate::types::EditorTool;

    fn app() -> App {
        let mut app = App::new();
        let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
        draft.water = Some(crate::authoring::water_asset_with_layer_for_tests(&draft.scene_id, &draft.folder));
        app.insert_resource(SceneEditorState { open: true, tool: EditorTool::Water, ..default() })
            .insert_resource(EditorAuthoringSession { draft: Some(draft), ..default() })
            .init_resource::<WaterPreviewState>()
            .init_resource::<SelectedWaterViewportState>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<DynamicWaterMaterial>>()
            .init_resource::<Assets<ClipmapWaterMaterial>>()
            .add_systems(Update, sync_selected_water_viewport);
        app
    }

    /// 走独立R8、正式上传分块和真实根/子块安装；回归不能用一个假四边形冒充多块水面。
    fn surface(app: &mut App, resolution: [u32; 2], water_level: f32)
        -> (Entity, Vec<Handle<Mesh>>, Handle<DynamicWaterMaterial>) {
        let draft = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap();
        let map = draft.scene_id.clone();
        let mut layer = draft.water.as_ref().unwrap().layers[0].clone();
        layer.resolution = resolution;
        layer.water_level = water_level;
        let mut descriptor = WaterSurfaceDescriptor::from_layer(&layer);
        descriptor.macro_waves[0].amplitude_meters = 0.137;
        descriptor.macro_waves[0].wavelength_meters = 7.5;
        descriptor.shore_wave_attenuation = 1.6;
        let mesh = PaintedWaterMeshData::from_independent_mask(
            &vec![255; (resolution[0] * resolution[1]) as usize],
            resolution, [-16.0, -16.0], [16.0, 16.0], water_level,
        ).unwrap();
        let pieces = chunk_water_surfaces(vec![BuiltWaterSurface { descriptor, mesh }]).unwrap();
        let mut meshes = app.world_mut().remove_resource::<Assets<Mesh>>().unwrap();
        let mut materials = app.world_mut().remove_resource::<Assets<DynamicWaterMaterial>>().unwrap();
        let mut staged = Vec::new();
        let mut queue = CommandQueue::default();
        for piece in pieces {
            stage_water_surface_chunk(&mut Commands::new(&mut queue, app.world()),
                &mut meshes, &mut materials, &mut staged, piece, 3.0);
        }
        assert_eq!(staged.len(), 1);
        let installed = spawn_staged_water_surface(&mut Commands::new(&mut queue, app.world()),
            map, PaintedWaterPurpose::Runtime, staged.pop().unwrap());
        queue.apply(app.world_mut());
        let handles = std::iter::once(installed.mesh).chain(installed.additional_meshes).collect();
        app.insert_resource(meshes).insert_resource(materials);
        (installed.entity, handles, installed.material)
    }

    #[test]
    fn painted_highlight_covers_production_chunks_and_keeps_wave_material_and_water_level() {
        let mut app = app();
        let (source, source_meshes, source_material) = surface(&mut app, [192, 192], 8.25);
        assert!(source_meshes.len() > 1);
        let source_mesh_count = app.world().resource::<Assets<Mesh>>().len();
        app.update();

        let selection = app.world().resource::<SelectedWaterViewportState>();
        let root = selection.highlight_entity.unwrap();
        let chunks = selection.highlight_chunks.clone();
        let material = selection.dynamic_highlight_material.clone().unwrap();
        assert_eq!(selection.source_entity, Some(source));
        assert!(selection.highlight_build.is_none());
        assert_eq!(chunks.len() + 1, source_meshes.len());
        assert_eq!(*app.world().get::<Visibility>(root).unwrap(), Visibility::Inherited);
        assert_eq!(app.world().get::<Transform>(root).unwrap().translation.y, 8.25);
        for (index, entity) in std::iter::once(root).chain(chunks.iter().copied()).enumerate() {
            assert_eq!(app.world().get::<Mesh3d>(entity).unwrap().0, source_meshes[index]);
            assert_eq!(app.world().get::<MeshMaterial3d<DynamicWaterMaterial>>(entity).unwrap().0, material);
            assert!(app.world().get::<MeshMaterial3d<StandardMaterial>>(entity).is_none());
            if entity != root {
                assert_eq!(app.world().get::<ChildOf>(entity).unwrap().parent(), root);
                assert_eq!(*app.world().get::<Transform>(entity).unwrap(), Transform::IDENTITY);
            }
        }
        assert_eq!(app.world().resource::<Assets<Mesh>>().len(), source_mesh_count); // 覆盖只共享网格，不生成第二套几何。
        {
            let assets = app.world().resource::<Assets<DynamicWaterMaterial>>();
            let highlighted = assets.get(&material).unwrap();
            let original = assets.get(&source_material).unwrap();
            // 扩展字段在scene crate内封装；Debug完整包含同一当前/历史波形和纹理绑定。
            assert_eq!(format!("{:?}", highlighted.extension), format!("{:?}", original.extension));
            assert_eq!(highlighted.base.base_color, Color::srgba(0.30, 0.82, 0.84, 0.28));
            assert_eq!(highlighted.base.depth_bias, HIGHLIGHT_DEPTH_BIAS);
        }
        {
            let mut assets = app.world_mut().resource_mut::<Assets<DynamicWaterMaterial>>();
            advance_water_surface_material(&mut assets.get_mut(&source_material).unwrap(), 9.0, Vec2::Y, 2.5);
        }
        app.world_mut().get_mut::<Transform>(source).unwrap().translation.y = 12.0;
        app.update();
        let selection = app.world().resource::<SelectedWaterViewportState>();
        assert_eq!(selection.highlight_entity, Some(root));
        assert_eq!(selection.highlight_chunks, chunks); // 时间和水位变化不重建已采用分块。
        assert_eq!(app.world().get::<Transform>(root).unwrap().translation.y, 12.0);
        let assets = app.world().resource::<Assets<DynamicWaterMaterial>>();
        assert_eq!(format!("{:?}", assets.get(&material).unwrap().extension),
            format!("{:?}", assets.get(&source_material).unwrap().extension));
        app.world_mut().resource_mut::<SceneEditorState>().open = false;
        app.update();
        assert_eq!(*app.world().get::<Visibility>(root).unwrap(), Visibility::Hidden); // 隐藏唯一根同时撤销全部子块。
    }

    #[test]
    fn highlight_chunk_budget_never_reveals_partial_surface_and_reclaims_changed_source() {
        let mut app = app();
        let (source, meshes, material) = surface(&mut app, [32, 32], 1.0);
        // 已验证的正式首块用于构造大扇出夹具，测试分帧而无需同步生成百万顶点。
        let extra_count = HIGHLIGHT_CHUNK_OPERATIONS_PER_FRAME * 2 + 1;
        for _ in 0..extra_count {
            app.world_mut().spawn((Mesh3d(meshes[0].clone()), MeshMaterial3d(material.clone()),
                Transform::IDENTITY, Visibility::Inherited, ChildOf(source)));
        }
        // 同根的其他渲染辅助对象必须排除，不能因存在Mesh3d就复制成水面。
        app.world_mut().spawn((Mesh3d(meshes[0].clone()), Transform::IDENTITY, ChildOf(source)));
        app.update();
        let selection = app.world().resource::<SelectedWaterViewportState>();
        let root = selection.highlight_entity.unwrap();
        assert_eq!(selection.highlight_chunks.len(), HIGHLIGHT_CHUNK_OPERATIONS_PER_FRAME);
        assert!(selection.highlight_mesh_source.is_none());
        assert_eq!(*app.world().get::<Visibility>(root).unwrap(), Visibility::Hidden);
        app.update();
        assert_eq!(*app.world().get::<Visibility>(root).unwrap(), Visibility::Hidden);
        app.update();
        let selection = app.world().resource::<SelectedWaterViewportState>();
        let previous_chunks = selection.highlight_chunks.clone();
        assert_eq!(previous_chunks.len(), extra_count);
        assert_eq!(selection.highlight_mesh_source, Some((source, meshes[0].id())));
        assert_eq!(*app.world().get::<Visibility>(root).unwrap(), Visibility::Inherited);

        let (next, next_meshes, _) = surface(&mut app, [128, 128], 11.0);
        *app.world_mut().get_mut::<Visibility>(source).unwrap() = Visibility::Hidden;
        app.update();
        assert_eq!(*app.world().get::<Visibility>(root).unwrap(), Visibility::Hidden);
        assert!(app.world().resource::<SelectedWaterViewportState>().highlight_mesh_source.is_none());
        app.update();
        assert_eq!(*app.world().get::<Visibility>(root).unwrap(), Visibility::Hidden);
        app.update();
        let selection = app.world().resource::<SelectedWaterViewportState>();
        assert_eq!(selection.source_entity, Some(next));
        assert_eq!(selection.highlight_entity, Some(root));
        assert_eq!(selection.highlight_chunks.len() + 1, next_meshes.len());
        assert!(previous_chunks.iter().all(|entity| app.world().get_entity(*entity).is_err()));
        assert_eq!(app.world().get::<Mesh3d>(root).unwrap().0, next_meshes[0]);
        assert_eq!(app.world().get::<Transform>(root).unwrap().translation.y, 11.0);
        assert_eq!(*app.world().get::<Visibility>(root).unwrap(), Visibility::Inherited);
    }

    #[test]
    fn changing_source_during_partial_highlight_discards_old_pending_chunks() {
        let mut app = app();
        let (source, meshes, material) = surface(&mut app, [32, 32], 1.0);
        for _ in 0..HIGHLIGHT_CHUNK_OPERATIONS_PER_FRAME + 1 {
            app.world_mut().spawn((Mesh3d(meshes[0].clone()), MeshMaterial3d(material.clone()),
                Transform::IDENTITY, Visibility::Inherited, ChildOf(source)));
        }
        app.update();
        let selection = app.world().resource::<SelectedWaterViewportState>();
        let root = selection.highlight_entity.unwrap();
        let pending_chunks = selection.highlight_chunks.clone();
        assert!(selection.highlight_build.is_some());
        assert_eq!(*app.world().get::<Visibility>(root).unwrap(), Visibility::Hidden);
        let (next, next_meshes, _) = surface(&mut app, [96, 96], 5.0);
        assert!(next_meshes.len() > 1);
        *app.world_mut().get_mut::<Visibility>(source).unwrap() = Visibility::Hidden;
        app.update();
        assert_eq!(*app.world().get::<Visibility>(root).unwrap(), Visibility::Hidden);
        assert!(pending_chunks.iter().all(|entity| app.world().get_entity(*entity).is_err()));
        app.update();
        let selection = app.world().resource::<SelectedWaterViewportState>();
        assert_eq!(selection.source_entity, Some(next));
        assert_eq!(selection.highlight_mesh_source, Some((next, next_meshes[0].id())));
        assert_eq!(selection.highlight_chunks.len() + 1, next_meshes.len());
        assert_eq!(*app.world().get::<Visibility>(root).unwrap(), Visibility::Inherited);
        assert_eq!(app.world().get::<Transform>(root).unwrap().translation.y, 5.0);
    }
}
