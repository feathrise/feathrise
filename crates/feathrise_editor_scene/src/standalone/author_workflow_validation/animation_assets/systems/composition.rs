//! 两份动作从原内容行合入蒙皮角色，通过原窗口按钮撤销、保存、另存并播放磁盘目标。

use super::*;

const SOURCES: [&str; 2] = ["scene/motion-a.glb", "scene/motion-b.glb"];
const TARGET: &str = "scene/actor.glb";
const REOPENED: &str = "independent/actor.glb";

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    #[default]
    Source,
    Open,
    Pick,
    Copy,
    Drag,
    OpenTarget,
    Target,
    Paste,
    Wait,
    EnterEdit,
    Undo,
    Redo,
    Save,
    SelectExport,
    Export,
    Confirm,
    Read,
    Reopen,
    SelectPlaybackClip,
    StartPlayback,
    Play,
    Image,
}

#[derive(Resource, Default)]
struct Probe {
    step: Step,
    index: usize,
    read: Option<Task<Result<Value, String>>>,
    first_pose: Option<(f32, Vec3)>,
    drag: Option<(Entity, bevy::math::DVec2, bevy::math::DVec2, u8)>,
}

pub(super) fn start(world: &mut World) -> Result<(), String> {
    world.init_resource::<Probe>();
    reveal_asset(world, SOURCES[0])
}

pub(super) fn advance(
    world: &mut World,
    fixture: &mut Fixture,
    paths: &Paths,
) -> Result<bool, String> {
    world.resource_scope(|world, mut probe: Mut<Probe>| step(world, &mut probe, fixture, paths))
}

fn step(
    world: &mut World,
    probe: &mut Probe,
    fixture: &mut Fixture,
    paths: &Paths,
) -> Result<bool, String> {
    let author = world.resource::<AnimationEditorState>();
    let active = author
        .active
        .as_ref()
        .is_some_and(|p| *p == paths.assets.join(TARGET));
    let count = author.active_document().map(|d| d.clips.len());
    let busy = author.busy();
    let reopened = author.active.as_ref() == Some(&paths.assets.join(REOPENED));
    let clean = author.current().is_some_and(|s| !s.dirty());
    let exported = author.selection_written;
    let status = author.status.clone();
    let before = probe.step;
    let click = |world: &mut World, fixture: &mut Fixture, name: &str| {
        let button = bridge::composition_button(world, name);
        click_property_button(world, fixture, name, button)
    };
    let playback_control = |world: &mut World, fixture: &mut Fixture, action| {
        let button = bridge::original_button(world, action);
        click_property_button(world, fixture, &format!("Click{action:?}"), button)
    };
    match before {
        Step::Source if expanded_model_ready(world, SOURCES[probe.index]) => {
            probe.step = if bridge::composition_button(world, "first").is_some() {
                Step::Pick
            } else {
                Step::Open
            };
        }
        Step::Open if click(world, fixture, "open")? => probe.step = Step::Pick,
        Step::Pick if click(world, fixture, "first")? => {
            probe.step = if probe.index == 0 {
                Step::Copy
            } else {
                Step::Drag
            };
        }
        Step::Drag => {
            use feathrise_editor_animation_model::request::GlbContentDropTarget;
            let Some(source) = bridge::composition_button(world, "first") else {
                return Ok(false);
            };
            let Some(point) = world
                .query::<(&GlbContentDropTarget, &UiGlobalTransform, &ComputedNode)>()
                .iter(world)
                .find_map(|(tab, transform, layout)| {
                    (tab.0 == TARGET && !layout.is_empty())
                        .then_some(transform.translation.as_dvec2())
                })
            else {
                return Ok(false);
            };
            let window = world
                .query_filtered::<Entity, With<PrimaryWindow>>()
                .single(world)
                .map_err(|e| e.to_string())?;
            if crate::ui::file_manager_validation::drag(
                world,
                &mut probe.drag,
                window,
                Some(source),
                point,
            ) {
                probe.step = Step::Wait;
            }
        }
        Step::Copy if click(world, fixture, "copy")? => probe.step = Step::OpenTarget,
        Step::OpenTarget if status.contains("已复制所选动画") => {
            reveal_asset(world, TARGET)?;
            probe.step = Step::Target;
        }
        Step::Target if expanded_model_ready(world, TARGET) && active && !busy => {
            probe.step = Step::Paste
        }
        Step::Paste if click(world, fixture, "paste")? => probe.step = Step::Wait,
        Step::Wait if !busy && count == Some(probe.index + 1) => {
            if probe.index == 0 {
                probe.index = 1;
                reveal_asset(world, SOURCES[1])?;
                probe.step = Step::Source;
            } else {
                probe.step = Step::EnterEdit;
            }
        }
        Step::EnterEdit
            if click_animation_button(world, fixture, bridge::AnimationCommand::ToggleEditing)? =>
        {
            probe.step = Step::Undo
        }
        Step::Undo
            if !busy && click_animation_button(world, fixture, bridge::AnimationCommand::Undo)? =>
        {
            probe.step = Step::Redo
        }
        Step::Redo
            // 按下与松开分帧，重做可以先于验收完成点击改变片段数。
            if (fixture.click.is_some() || (!busy && count == Some(1)))
                && click_animation_button(world, fixture, bridge::AnimationCommand::Redo)? =>
        {
            probe.step = Step::SelectExport
        }
        Step::Save
            if (fixture.click.is_some() || (count == Some(2) && !busy && !clean && exported))
                && click_animation_button(world, fixture, bridge::AnimationCommand::Save)? =>
        {
            probe.step = Step::Read
        }
        // 先从尚未保存的草稿导出，随后验证副本没有清掉目标的脏状态，再走原保存。
        Step::SelectExport if !busy && !clean && count == Some(2) && click(world, fixture, "second")? => {
            probe.step = Step::Export
        }
        Step::Export if click(world, fixture, "export")? => probe.step = Step::Confirm,
        Step::Confirm if click(world, fixture, "confirm")? => probe.step = Step::Save,
        Step::Read if probe.read.is_none() && !busy && clean && exported =>
        {
            let assets = paths.assets.clone();
            probe.read = Some(IoTaskPool::get().spawn(async move { read_outputs(&assets) }));
        }
        Step::Read if probe.read.is_some() => {
            if let Some(result) = probe.read.as_mut().and_then(check_ready) {
                fixture
                    .created_clip_proof
                    .as_mut()
                    .ok_or("原片段证明缺失")?["composition"] = result?;
                reveal_asset(world, REOPENED)?;
                probe.step = Step::Reopen;
            }
        }
        Step::Reopen if expanded_model_ready(world, REOPENED) && reopened && !busy && count == Some(2) => {
            probe.step = Step::SelectPlaybackClip;
        }
        Step::SelectPlaybackClip if playback_control(world, fixture, bridge::AnimationCommand::NextClip)? => {
            probe.step = Step::StartPlayback;
        }
        Step::StartPlayback => {
            let editor = world.resource::<AnimationEditorState>();
            let Some(session) = editor.current().filter(|s| reopened && s.clip == 1) else { return Ok(false); };
            let ready = !busy && !session.source_changed && world.resource::<PreviewRuntime>().published_pose.as_ref()
                .is_some_and(|pose| pose.matches_session(session));
            if (editor.playing && fixture.click.is_none()) || ((fixture.click.is_some() || ready)
                && playback_control(world, fixture, bridge::AnimationCommand::TogglePlayback)?) {
                probe.step = Step::Play;
            }
        }
        Step::Play => {
            let author = world.resource::<AnimationEditorState>();
            let Some(session) = author.current().filter(|_| reopened && !busy)
            else {
                return Ok(false);
            };
            // 文件观察器可在按下与松开间换代，原按钮会拒绝旧绑定。只在新绑定已就绪后重试。
            if session.clip != 1 || !author.playing {
                probe.step = if session.clip == 1 { Step::StartPlayback } else { Step::SelectPlaybackClip };
                return Ok(false);
            }
            let Some(pose) = world
                .resource::<PreviewRuntime>()
                .published_pose
                .as_ref()
                .filter(|p| p.matches_session(session))
            else {
                return Ok(false);
            };
            let track = session.document.clips[1]
                .channels
                .iter()
                .find(|c| c.node_name == "LeftForeArm" && c.path == "translation")
                .ok_or("导入的角色片段缺少夹具左前臂平移轨")?;
            let position = pose
                .local
                .get(track.node)
                .ok_or("左前臂姿势没有发布到目标关节")?
                .translation;
            let (first_time, first_position) =
                *probe.first_pose.get_or_insert((pose.time, position));
            if first_time != pose.time && !first_position.abs_diff_eq(position, 1e-5) {
                fixture.created_clip_proof.as_mut().ok_or("缺少落盘回执")?["composition"]["published_clip_times"] =
                    json!([first_time, pose.time]);
                capture_mode(world, "glb-content-composition.png")?;
                probe.step = Step::Image;
            }
        }
        Step::Image if mode_captured(world)? => return Ok(true),
        _ => {}
    }
    if before != probe.step {
        trace(
            fixture,
            json!({"kind":"glb_composition","step":format!("{before:?}"),"status":status}),
        );
    }
    Ok(false)
}

fn read_outputs(assets: &std::path::Path) -> Result<Value, String> {
    let read = |name: &str| {
        let bytes = std::fs::read(assets.join(name)).map_err(|e| e.to_string())?;
        gltf::Gltf::from_slice(&bytes).map_err(|e| e.to_string())
    };
    let target = read(TARGET)?;
    let exported = read("scene/actor_所选.glb")?;
    let names = |g: &gltf::Gltf| {
        g.animations()
            .map(|clip| clip.name().unwrap_or_default().to_owned())
            .collect::<Vec<_>>()
    };
    if names(&target) != ["FixtureMotion", "FixtureMotion_2"]
        || names(&exported) != ["FixtureMotion_2"]
        || target.skins().count() != 1
        || target.meshes().count() != 1
        || exported.meshes().count() != 0
        || exported
            .animations()
            .next()
            .is_none_or(|clip| clip.channels().count() == 0)
    {
        return Err("两份来源、所选另存或角色蒙皮的独立磁盘回读不符".into());
    }
    // 新目录只放目标自己的落盘字节，加载器无法复用已开的文档、标签或同目录动作。
    let reopened = assets.join(REOPENED);
    std::fs::create_dir(reopened.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::copy(assets.join(TARGET), &reopened).map_err(|e| e.to_string())?;
    Ok(
        json!({"saved_clips": names(&target), "exported_clips": names(&exported), "saved_skins": 1, "source_count": 2, "reopened":REOPENED}),
    )
}
