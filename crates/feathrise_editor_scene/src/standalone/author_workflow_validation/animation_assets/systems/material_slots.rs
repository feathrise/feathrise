//! 原生窗口实拖同文件材质；校验正在显示的四个图元、原动画草稿、磁盘引用与新路径重开。

use super::*;
use feathrise_editor_animation_model::{
    resources::BindingState,
    types::{EmbeddedNodeIndex, EmbeddedPrimitive},
};

const FILE: &str = "scene/material-slots.glb";
const REOPENED: &str = "independent-material/material-slots.glb";
const OLD: [u8; 4] = [0, 0, 0, 0];
const NEW: [u8; 4] = [1, 0, 0, 0];

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    #[default]
    Open,
    CreateClip,
    Contents,
    Slots,
    Object,
    Primitive,
    Drag,
    Preview,
    DraftImage,
    Undo,
    Redo,
    Save,
    Read,
    Reopen,
    Image,
}

#[derive(Resource, Default)]
struct Probe {
    step: Step,
    drag: Option<(Entity, bevy::math::DVec2, bevy::math::DVec2, u8)>,
    read: Option<Task<Result<Value, String>>>,
    proof: Value,
}

pub(super) fn enabled() -> bool {
    std::env::var("FEATHRISE_MATERIAL_SLOT_VALIDATION").as_deref() == Ok("1")
}

pub(super) fn start(world: &mut World) -> Result<(), String> {
    world.init_resource::<Probe>();
    reveal_asset(world, FILE)
}

pub(super) fn diagnostic(world: &mut World) -> Value {
    let stage = world
        .get_resource::<Probe>()
        .map(|p| format!("{:?}", p.step));
    let file = world
        .resource::<AssetPreviewState>()
        .selected_path
        .clone()
        .unwrap_or_default();
    let colors = colors(world, &file);
    let button = bridge::composition_button(world,"slot-redo").map(|e| {
        json!({"entity":format!("{e:?}"),"visible":crate::standalone::validation_fixture::ui_visible(world,e)})
    });
    let session = world.resource::<AnimationEditorState>().current();
    json!({"stage":stage,"visible_materials":colors,"redo_button":button,
        "source_changed":session.map(|s|s.source_changed),"disk_conflict":session.map(|s|s.external_change_confirmed),
        "dirty":session.map(|s|s.dirty()),"clips":session.map(|s|s.document.clips.len())})
}

pub(super) fn advance(
    world: &mut World,
    fixture: &mut Fixture,
    paths: &Paths,
) -> Result<bool, String> {
    world.resource_scope(|world, mut p: Mut<Probe>| step(world, &mut p, fixture, paths))
}

fn step(
    world: &mut World,
    probe: &mut Probe,
    fixture: &mut Fixture,
    paths: &Paths,
) -> Result<bool, String> {
    let author = world.resource::<AnimationEditorState>();
    let file = if probe.step == Step::Reopen || probe.step == Step::Image {
        REOPENED
    } else {
        FILE
    };
    let active = author.active.as_ref() == Some(&paths.assets.join(file));
    let ready = active && !author.busy() && author.current().is_some_and(|s| !s.source_changed);
    let cursor = author.current().map(|s| s.cursor);
    let clean = author.current().is_some_and(|s| !s.dirty());
    let count = author.active_document().map(|d| d.clips.len());
    let status = author.status.clone();
    let click = |w: &mut World, f: &mut Fixture, name: &str| {
        let b = bridge::composition_button(w, name);
        click_property_button(w, f, name, b)
    };
    let before = probe.step;
    match before {
        Step::Open
            if expanded_model_ready(world, FILE) && ready && colors(world, FILE) == Some(OLD) =>
        {
            probe.step = Step::CreateClip;
        }
        Step::CreateClip
            if click_animation_button(world, fixture, bridge::AnimationCommand::CreateClip)? =>
        {
            probe.step = Step::Contents
        }
        Step::Contents if ready && count == Some(2) && cursor == Some(1) => {
            if (fixture.click.is_none() && bridge::composition_button(world, "slots").is_some())
                || click(world, fixture, "open")?
            {
                probe.step = Step::Slots;
            }
        }
        Step::Slots if click(world, fixture, "slots")? => probe.step = Step::Object,
        Step::Object if click(world, fixture, "first")? => probe.step = Step::Primitive,
        Step::Primitive if click(world, fixture, "slot-first")? => probe.step = Step::Drag,
        Step::Drag => {
            let (Some(from), Some(to)) = (
                bridge::composition_button(world, "second"),
                bridge::composition_button(world, "slot-first"),
            ) else {
                return Ok(false);
            };
            if !crate::standalone::validation_fixture::ui_visible(world, to) {
                crate::standalone::validation_fixture::reveal_property_control(world, to)?;
                return Ok(false);
            }
            if !crate::standalone::validation_fixture::ui_visible(world, from) {
                crate::standalone::validation_fixture::reveal_property_control(world, from)?;
                return Ok(false);
            }
            let point = world
                .get::<UiGlobalTransform>(to)
                .unwrap()
                .translation
                .as_dvec2();
            let window = world
                .query_filtered::<Entity, With<PrimaryWindow>>()
                .single(world)
                .map_err(|e| e.to_string())?;
            if crate::ui::file_manager_validation::drag(
                world,
                &mut probe.drag,
                window,
                Some(from),
                point,
            ) {
                probe.step = Step::Preview;
            }
        }
        Step::Preview
            if ready
                && count == Some(2)
                && cursor == Some(2)
                && colors(world, FILE) == Some(NEW) =>
        {
            probe.proof = json!({"preview":NEW});
            capture_mode(world, "glb-material-draft.png")?;
            probe.step = Step::DraftImage;
        }
        Step::DraftImage if mode_captured(world)? => probe.step = Step::Undo,
        Step::Undo if click(world, fixture, "slot-undo")? => probe.step = Step::Redo,
        Step::Redo
            if fixture.click.is_some()
                || ready
                    && cursor == Some(1)
                    && count == Some(2)
                    && colors(world, FILE) == Some(OLD) =>
        {
            probe.proof["undo"] = json!(OLD);
            if click(world, fixture, "slot-redo")? {
                probe.step = Step::Save;
            }
        }
        Step::Save
            if fixture.click.is_some()
                || ready && cursor == Some(2) && !clean && colors(world, FILE) == Some(NEW) =>
        {
            probe.proof["redo"] = json!(NEW);
            if click(world, fixture, "slot-save")? {
                probe.step = Step::Read;
            }
        }
        Step::Read if probe.read.is_none() && ready && clean => {
            let root = paths.assets.clone();
            probe.read = Some(IoTaskPool::get().spawn(async move { read(&root) }));
        }
        Step::Read if probe.read.is_some() => {
            if let Some(done) = probe.read.as_mut().and_then(check_ready) {
                let saved = done?;
                probe.proof["saved_clips"] = saved["clips"].clone();
                probe.proof["meshes"] = saved["meshes"].clone();
                reveal_asset(world, REOPENED)?;
                probe.step = Step::Reopen;
            }
        }
        Step::Reopen
            if expanded_model_ready(world, REOPENED)
                && ready
                && clean
                && cursor == Some(0)
                && count == Some(2)
                && colors(world, REOPENED) == Some(NEW) =>
        {
            probe.proof["reopened"] = json!(NEW);
            fixture.created_clip_proof.as_mut().ok_or("缺少动画回执")?["material_slots"] =
                probe.proof.clone();
            capture_mode(world, "glb-material-slots.png")?;
            probe.step = Step::Image;
        }
        Step::Image if mode_captured(world)? => return Ok(true),
        _ => {}
    }
    if before != probe.step {
        trace(
            fixture,
            json!({"kind":"material_slots","step":format!("{before:?}"),"status":status}),
        );
    }
    Ok(false)
}

/// 颜色来自当前可见实例的真实 MeshMaterial3d，独立于文档草稿和文件元数据。
fn colors(world: &mut World, path: &str) -> Option<[u8; 4]> {
    let preview = world.resource::<AssetPreviewState>();
    let [a, b] = world
        .get_resource::<BindingState>()
        .map_or([None; 2], |r| r.weight_roots(path));
    let roots = [preview.scene_root(), a, b];
    let mut result = [u8::MAX; 4];
    let mut query = world.query::<(
        &ChildOf,
        &EmbeddedPrimitive,
        &MeshMaterial3d<StandardMaterial>,
        &InheritedVisibility,
    )>();
    for (parent, primitive, handle, visibility) in query.iter(world) {
        if !visibility.get() {
            continue;
        }
        let p = parent.parent();
        let Some(node) = world.get::<EmbeddedNodeIndex>(p) else {
            continue;
        };
        if node.0 > 1 || primitive.index > 1 {
            continue;
        }
        let mut e = Some(p);
        while let Some(entity) = e {
            if roots.contains(&Some(entity)) {
                break;
            }
            e = world.get::<ChildOf>(entity).map(ChildOf::parent);
        }
        if e.is_none() {
            continue;
        }
        let color = world
            .resource::<Assets<StandardMaterial>>()
            .get(&handle.0)?
            .base_color
            .to_linear()
            .to_f32_array();
        result[2 * node.0 + primitive.index] = if near_color(color, [0.8, 0.05, 0.04, 1.0]) {
            0
        } else if near_color(color, [0.03, 0.2, 0.85, 1.0]) {
            1
        } else {
            return None;
        };
    }
    (!result.contains(&u8::MAX)).then_some(result)
}

fn read(root: &std::path::Path) -> Result<Value, String> {
    let bytes = std::fs::read(root.join(FILE)).map_err(|e| e.to_string())?;
    let file = gltf::Gltf::from_slice(&bytes).map_err(|e| e.to_string())?;
    let slots = file
        .nodes()
        .map(|n| {
            n.mesh().map(|m| {
                m.primitives()
                    .filter_map(|p| p.material().index())
                    .collect::<Vec<_>>()
            })
        })
        .collect::<Vec<_>>();
    if slots != [Some(vec![1, 0]), Some(vec![0, 0])]
        || file.meshes().count() != 2
        || file.animations().count() != 2
    {
        return Err("目标文件的共享材质引用或先前未保存动画不符".into());
    }
    let dest = root.join(REOPENED);
    std::fs::create_dir(dest.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::copy(root.join(FILE), dest).map_err(|e| e.to_string())?;
    Ok(json!({"clips":file.animations().count(),"meshes":file.meshes().count()}))
}
