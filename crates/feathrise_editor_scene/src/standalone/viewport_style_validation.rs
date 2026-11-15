//! 自有工程的视口样式取证，使用原菜单、参考网格和作者相机的真实 GPU 目标。
use crate::{
    resources::{SceneEditorState, SceneEditorViewportState},
    ui::{
        gltf_browser::EditorGltfBrowserState,
        scene_context_menu::{SceneMenuAction, queue_terrain_action_for_validation},
        sidebars::{EditorNavigatorTab, EditorSidebarTab, SceneEditorSidebarState, SceneEditorSidebarTabButton},
    },
    viewport::{
        content_selection::LayerCandidateState,
        shading::{self, ShadingState, types::ShadingAction},
    },
};
use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    tasks::{IoTaskPool, Task, futures::check_ready},
};
use feathrise_scene::{
    ScenePlacementInstance, ScenePlacementPrimitiveVisual, TerrainObjectInstance,
    author_view::{GridView, ShadingView},
};
use feathrise_scene_content::{EditableMeshGeometry, ScenePlacementRecord, TerrainId};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Phase {
    #[default]
    OpenSceneTree,
    HideGround,
    Frame,
    SolidWithBackground,
    OpenBackgroundMenu,
    HideBackground,
    CloseMenu,
    SolidWithoutBackground,
    UseWireframe,
    OriginalWireframe,
    SubdividedWireframe,
    RestoreRendered,
    OpenRestoreMenu,
    RestoreBackground,
    CloseRestoreMenu,
    RestoreGround,
    RestoreSelection,
    RestoreNavigator,
    Done,
}

#[derive(Default)]
pub(super) struct StyleFixture {
    phase: Phase,
    stable_frames: u8,
    capture_pending: bool,
    capture_attempt: u8,
    original_navigator: Option<EditorNavigatorTab>,
    proof: serde_json::Map<String, serde_json::Value>,
}

#[derive(Resource)]
struct Capture(Task<Result<(bool, serde_json::Value), String>>);

impl StyleFixture {
    pub(super) fn stage(&self) -> String { format!("{:?}", self.phase) }

    /// 保存验证在两次截图阶段之间继续原细分、确认、撤销和重做。
    pub(super) fn before_subdivision(
        &mut self,
        world: &mut World,
        record: &ScenePlacementRecord,
        pulse: &mut Option<Entity>,
    ) -> Result<bool, String> {
        if self.phase != Phase::SubdividedWireframe {
            self.advance(world, record, pulse)?;
        }
        Ok(self.phase == Phase::SubdividedWireframe)
    }

    pub(super) fn after_subdivision(
        &mut self,
        world: &mut World,
        record: &ScenePlacementRecord,
        pulse: &mut Option<Entity>,
    ) -> Result<Option<serde_json::Value>, String> {
        self.advance(world, record, pulse)?;
        Ok((self.phase == Phase::Done).then(|| serde_json::Value::Object(self.proof.clone())))
    }

    fn advance(
        &mut self,
        world: &mut World,
        record: &ScenePlacementRecord,
        pulse: &mut Option<Entity>,
    ) -> Result<(), String> {
        use Phase::*;
        let ground = TerrainId::parse("starter-ground")?;
        let camera = world.resource::<SceneEditorViewportState>().camera;
        let Some(view) = world.get::<ShadingView>(camera).copied() else { return Ok(()); };
        let menu_open = world.resource::<ShadingState>().menu_open();
        let press_option = |world: &mut World, action: ShadingAction| {
            shading::validation_press_map_action(world, |button| {
                matches!((button, action), (ShadingAction::Menu, ShadingAction::Menu)
                    | (ShadingAction::Background, ShadingAction::Background))
            })
        };
        match self.phase {
            OpenSceneTree => {
                if view.options.x != 3.0 || view.options.w <= 0.5 || view.wire.x <= 0.5 {
                    return Err("视口样式夹具应从隔离的默认渲染、背景和穿透设置开始".into());
                }
                let navigator = world.resource::<SceneEditorSidebarState>().navigator;
                self.original_navigator = Some(navigator);
                if navigator != EditorNavigatorTab::SceneTree {
                    *pulse = Some(press_navigator(world, EditorNavigatorTab::SceneTree)?);
                }
                self.phase = HideGround;
            }
            HideGround if world.resource::<SceneEditorSidebarState>().navigator == EditorNavigatorTab::SceneTree => {
                if queue_terrain_action_for_validation(world, &ground, SceneMenuAction::Visibility)? {
                    self.phase = Frame;
                }
            }
            Frame if ground_is_hidden(world, &ground, true) => {
                if !select_fixture_object(world, record) {
                    return Ok(());
                }
                let mut viewport = world.resource_mut::<SceneEditorViewportState>();
                let center = Vec3::from_array(record.translation);
                viewport.extent = 8.0;
                viewport.yaw = 0.72;
                viewport.pitch = 0.48;
                let (_, pose) = crate::viewport::editor_camera_pose(false, &viewport);
                // 只移动取景中心，保持原轨道最小距离；四张图共用一个画幅，便于直接比较。
                viewport.center = center + pose.back().as_vec3() * (4.8 - 8.0);
                world.resource_mut::<SceneEditorState>().orthographic = false;
                *pulse = Some(shading::validation_press_mode(world, 1)?);
                self.phase = SolidWithBackground;
            }
            SolidWithBackground if view.options.x == 1.0 && view.options.w > 0.5 => {
                if self.capture(world, record, "viewport-solid-scene-background", false)? {
                    self.phase = OpenBackgroundMenu;
                }
            }
            OpenBackgroundMenu => {
                *pulse = Some(press_option(world, ShadingAction::Menu)?);
                self.phase = HideBackground;
            }
            HideBackground if menu_open => {
                *pulse = Some(press_option(world, ShadingAction::Background)?);
                self.phase = CloseMenu;
            }
            CloseMenu if view.options.w <= 0.5 => {
                *pulse = Some(press_option(world, ShadingAction::Menu)?);
                self.phase = SolidWithoutBackground;
            }
            SolidWithoutBackground if !menu_open && view.options.x == 1.0 && view.options.w <= 0.5 => {
                if self.capture(world, record, "viewport-solid-no-background", false)? {
                    self.phase = UseWireframe;
                }
            }
            UseWireframe => {
                *pulse = Some(shading::validation_press_mode(world, 0)?);
                self.phase = OriginalWireframe;
            }
            OriginalWireframe if view.options.x == 0.0 && view.wire.x > 0.5 => {
                if self.capture(world, record, "viewport-wire-original", true)? {
                    self.phase = SubdividedWireframe;
                }
            }
            SubdividedWireframe if view.options.x == 0.0 && view.wire.x > 0.5 => {
                if select_fixture_object(world, record) && self.capture(world, record, "viewport-wire-subdivided", true)? {
                    self.phase = RestoreRendered;
                }
            }
            RestoreRendered => {
                *pulse = Some(shading::validation_press_mode(world, 3)?);
                self.phase = OpenRestoreMenu;
            }
            OpenRestoreMenu if view.options.x == 3.0 => {
                *pulse = Some(press_option(world, ShadingAction::Menu)?);
                self.phase = RestoreBackground;
            }
            RestoreBackground if menu_open => {
                *pulse = Some(press_option(world, ShadingAction::Background)?);
                self.phase = CloseRestoreMenu;
            }
            CloseRestoreMenu if view.options.w > 0.5 => {
                *pulse = Some(press_option(world, ShadingAction::Menu)?);
                self.phase = RestoreGround;
                self.stable_frames = 0;
            }
            RestoreGround if !menu_open => {
                if ground_is_hidden(world, &ground, false) {
                    self.proof.insert("restored_scene_background_and_terrain".into(), true.into());
                    self.phase = RestoreSelection;
                } else if self.stable_frames == 0
                    && queue_terrain_action_for_validation(world, &ground, SceneMenuAction::Visibility)? {
                    self.stable_frames = 1;
                }
            }
            RestoreSelection => {
                // 原地形右键动作会接管检查器。复用进入网格编辑前的稳定实例选择入口及原属性联动。
                if select_fixture_object(world, record) {
                    let original = self.original_navigator.ok_or("视口样式夹具未冻结左栏页签")?;
                    if world.resource::<SceneEditorSidebarState>().navigator != original {
                        *pulse = Some(press_navigator(world, original)?);
                    }
                    self.phase = RestoreNavigator;
                }
            }
            RestoreNavigator if self.original_navigator == Some(world.resource::<SceneEditorSidebarState>().navigator) => {
                self.phase = Done;
            }
            _ => {}
        }
        Ok(())
    }

    fn capture(&mut self, world: &mut World, record: &ScenePlacementRecord, name: &'static str, wire: bool) -> Result<bool, String> {
        if self.capture_pending {
            let Some(mut capture) = world.get_resource_mut::<Capture>() else { return Ok(false); };
            let Some(result) = check_ready(&mut capture.0) else { return Ok(false); };
            world.remove_resource::<Capture>();
            self.capture_pending = false;
            self.stable_frames = 0;
            let (accepted, proof) = result?;
            if !accepted {
                if self.capture_attempt >= 8 {
                    return Err(format!("视口真实 GPU 像素未通过 {name}：{proof}"));
                }
                return Ok(false); // 候选管线尚未采用时保留现场，重取实际帧而不伪造成功。
            }
            self.capture_attempt = 0;
            self.proof.insert(name.into(), proof);
            return Ok(true);
        }
        let viewport = world.resource::<SceneEditorViewportState>();
        let camera = viewport.camera;
        if viewport.render_size.min_element() < 128 || world.get::<GridView>(camera).is_none() {
            return Ok(false);
        }
        let image = viewport._image.clone();
        let target_size = viewport.render_size;
        let geometry = record.asset_target.editable_geometry().ok_or("截图对象必须拥有完整作者几何")?;
        let triangles = geometry.triangles();
        // 保证采用的是当前记录的实物网格，不能把重做后尚未换代的旧帧作为截图。
        let source_pose = world.query_filtered::<(&Mesh3d, &GlobalTransform, &InheritedVisibility, &ChildOf), With<ScenePlacementPrimitiveVisual>>()
            .iter(world).find_map(|(mesh, pose, visible, parent)| {
                (visible.get() && world.get::<ScenePlacementInstance>(parent.parent()).is_some_and(|root| {
                    root.id() == record.id && root.is_active()
                }) && world.resource::<Assets<Mesh>>().get(&mesh.0).is_some_and(|mesh| {
                    !wire || mesh.count_vertices() == triangles.len() * 3
                })).then_some(*pose)
            });
        let Some(source_pose) = source_pose else {
            self.stable_frames = 0;
            return Ok(false);
        };
        self.stable_frames += 1;
        if self.stable_frames < 16 {
            return Ok(false); // 相机、网格和选中派生管线各自跨帧采用，等一小段稳定画面再回读。
        }
        let directory = PathBuf::from(std::env::var_os("NEW_EDITOR_NATIVE_SMOKE_DIR").ok_or("视口截图目录未隔离")?);
        let camera_pose = world.get::<GlobalTransform>(camera).ok_or("原相机没有采用变换")?;
        let camera = world.get::<Camera>(camera).ok_or("原作者相机已退出")?;
        let logical_size = camera.logical_viewport_size().ok_or("原作者相机没有视口尺寸")?;
        let points = geometry.vertices().iter().map(|point| {
            let point = Vec3::from_array(point.map(|axis| axis as f32 * 0.0005));
            camera.world_to_viewport(camera_pose, source_pose.transform_point(point))
                .map(|point| point * target_size.as_vec2() / logical_size)
                .map_err(|error| format!("截图对象未完整取景：{error:?}"))
        }).collect::<Result<Vec<_>, _>>()?;
        let probes = CaptureProbes::new(&geometry, &points, &triangles, wire);
        let work_background = self.phase != Phase::SolidWithBackground;
        let baseline_orange = self.proof.get("viewport-solid-scene-background")
            .and_then(|proof| proof["orange_pixels"].as_u64()).unwrap_or(0) as usize;
        self.capture_attempt += 1;
        let attempt = self.capture_attempt;
        world.spawn(Screenshot::image(image)).observe(move |captured: On<ScreenshotCaptured>, mut commands: Commands| {
            let image = captured.image.clone();
            let directory = directory.clone();
            let probes = probes.clone();
            commands.insert_resource(Capture(IoTaskPool::get().spawn(async move {
                let image = image.try_into_dynamic().map_err(|error| error.to_string())?.to_rgb8();
                let (accepted, mut proof) = probes.inspect(&image, work_background, wire, baseline_orange)?;
                let filename = if accepted { format!("{name}.png") } else { format!("{name}-attempt-{attempt}.png") };
                image.save(directory.join(&filename)).map_err(|error| error.to_string())?;
                proof["screenshot"] = filename.into();
                Ok((accepted, proof))
            })));
        });
        self.capture_pending = true;
        Ok(false)
    }
}

#[derive(Clone)]
struct CaptureProbes {
    bounds: Rect,
    edges: Vec<Vec2>,
    diagonals: Vec<Vec2>,
    counts: [usize; 4],
}

impl CaptureProbes {
    fn new(geometry: &EditableMeshGeometry, points: &[Vec2], triangles: &[[usize; 3]], wire: bool) -> Self {
        let edges = geometry.edges();
        let bounds = points.iter().skip(1).fold(Rect::from_corners(points[0], points[0]),
            |bounds, point| Rect { min: bounds.min.min(*point), max: bounds.max.max(*point) });
        let mut probes = Self {
            bounds, edges: Vec::new(), diagonals: Vec::new(),
            counts: [points.len(), edges.len(), geometry.faces().len(), triangles.len()],
        };
        if !wire {
            return probes;
        }
        for &[a, b] in edges {
            probes.edges.extend([0.25, 0.5, 0.75].map(|t| points[a].lerp(points[b], t)));
        }
        // 三角内部边可能与背面真实边叠在同一像素，只取远离所有投影真实边的负样本。
        let mut sampled = std::collections::BTreeSet::new();
        for &[a, b, c] in triangles {
            for (a, b) in [(a, b), (b, c), (c, a)] {
                if edges.contains(&[a, b]) || edges.contains(&[b, a]) || !sampled.insert((a.min(b), a.max(b))) {
                    continue;
                }
                probes.diagonals.extend([0.2, 0.35, 0.5, 0.65, 0.8].into_iter()
                    .map(|t| points[a].lerp(points[b], t))
                    .filter(|p| edges.iter().all(|&[a, b]| segment_distance_squared(*p, points[a], points[b]) > 25.0)));
            }
        }
        probes
    }

    fn inspect(&self, image: &image::RgbImage, work_background: bool, wire: bool, baseline_orange: usize)
        -> Result<(bool, serde_json::Value), String>
    {
        let (width, height) = image.dimensions();
        if width < 128 || height < 128 {
            return Err(format!("读取到的真实视口尺寸无效：{width}×{height}"));
        }
        let corners = [image.get_pixel(8, 8).0, image.get_pixel(width - 9, 8).0];
        let background = corners[0][0];
        let gray = |[r, g, b]: [u8; 3]| r.abs_diff(g) <= 4 && r.abs_diff(b) <= 4;
        let neutral = corners.into_iter().all(|color| gray(color) && (25..110).contains(&color[0]));
        let center = Rect::from_center_size(self.bounds.center(), self.bounds.size() * 0.30);
        let surround = Rect::from_center_size(self.bounds.center(), self.bounds.size() + Vec2::splat(10.0));
        let mut orange_pixels = 0;
        let mut cyan_interior = 0;
        let mut light_grid_pixels = 0;
        for (x, y, pixel) in image.enumerate_pixels() {
            let point = Vec2::new(x as f32, y as f32);
            let [r, g, b] = pixel.0;
            if surround.contains(point) {
                orange_pixels += usize::from(r >= 135 && g >= 66 && r.saturating_sub(g) >= 40 && g.saturating_sub(b) >= 24);
                cyan_interior += usize::from(center.contains(point) && g.saturating_sub(r) > 22 && b.saturating_sub(r) > 22);
            } else if y > height / 3 && gray(pixel.0) && (4..=32).contains(&r.saturating_sub(background)) {
                light_grid_pixels += 1;
            }
        }
        let dark_line = |point: &&Vec2| {
            let point = point.round().as_ivec2();
            (-1..=1).any(|dy| (-1..=1).any(|dx| {
                let (x, y) = (point.x + dx, point.y + dy);
                x >= 0 && y >= 0 && x < width as i32 && y < height as i32
                    && image.get_pixel(x as u32, y as u32).0.into_iter().all(|c| c < background.saturating_sub(14))
            }))
        };
        let visible_edges = self.edges.iter().filter(dark_line).count();
        let visible_diagonals = self.diagonals.iter().filter(dark_line).count();
        let work_style_ok = neutral && light_grid_pixels >= 60 && orange_pixels > 50
            && (!wire || visible_edges * 2 >= self.edges.len()
                && self.diagonals.len() >= 8 && visible_diagonals * 10 <= self.diagonals.len());
        let solid_style_ok = wire || orange_pixels > baseline_orange + 16 && cyan_interior < 20;
        let accepted = !work_background || work_style_ok && solid_style_ok;
        Ok((accepted, serde_json::json!({
            "size": [width, height], "top_corner_rgb": corners, "orange_pixels": orange_pixels,
            "cyan_interior_pixels": cyan_interior, "light_grid_pixels": light_grid_pixels,
            "visible_author_edge_samples": visible_edges, "author_edge_samples": self.edges.len(),
            "visible_internal_diagonal_samples": visible_diagonals, "internal_diagonal_samples": self.diagonals.len(),
            "author_points": self.counts[0], "author_edges": self.counts[1], "author_faces": self.counts[2],
            "render_triangles": self.counts[3],
        })))
    }
}

fn press_navigator(world: &mut World, tab: EditorNavigatorTab) -> Result<Entity, String> {
    let entity = world.query::<(Entity, &SceneEditorSidebarTabButton)>().iter(world)
        .find_map(|(entity, button)| (button.0 == EditorSidebarTab::Navigator(tab)).then_some(entity))
        .ok_or("视口样式验收缺少原导航页签")?;
    let mut nodes = world.query::<(&Node, Option<&ChildOf>)>();
    if !crate::viewport::modes::controls_visible(entity, &nodes.query(world)) {
        return Err("视口样式验收导航页签不可见".into());
    }
    world.entity_mut(entity).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    Ok(entity)
}

fn segment_distance_squared(point: Vec2, a: Vec2, b: Vec2) -> f32 {
    let delta = b - a;
    let t = ((point - a).dot(delta) / delta.length_squared().max(f32::EPSILON)).clamp(0.0, 1.0);
    point.distance_squared(a + delta * t)
}

fn select_fixture_object(world: &mut World, record: &ScenePlacementRecord) -> bool {
    if world.resource::<EditorGltfBrowserState>().selected_placement_id.as_deref() == Some(record.id.as_str()) {
        return true;
    }
    let revision = world.resource::<crate::authoring::EditorAuthoringSession>().placement_structure_revision;
    world.resource_mut::<LayerCandidateState>().clear_for_scene_tree();
    let mut browser = world.resource_mut::<EditorGltfBrowserState>();
    if browser.placement_record_index(&record.id, revision).is_none()
        || !browser.select_primitive_placement_from_viewport(record.id.clone()) {
        return false;
    }
    world.resource_mut::<SceneEditorSidebarState>().reveal_scene_properties();
    true
}

fn ground_is_hidden(world: &mut World, ground: &TerrainId, hidden: bool) -> bool {
    world.resource::<SceneEditorState>().terrain_hidden.contains(ground) == hidden
        && world.query::<(&TerrainObjectInstance, &Visibility)>().iter(world).any(|(instance, visibility)| {
            &instance.definition.id == ground && (*visibility == Visibility::Hidden) == hidden
        })
}
