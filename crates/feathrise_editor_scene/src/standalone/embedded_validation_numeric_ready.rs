//! 数值验收起点屏障：原作者选择、公共控件值/文字与布局完全同步后，才允许注入鼠标。

use super::*;
use crate::{
    authoring::EditorAuthoringSession,
    resources::SceneEditorState,
    ui::{gltf_browser::EditorGltfBrowserState, placement_transform::PlacementTransformField},
};
use feathrise_editor_ui::{
    ActiveNumericInput, NumericInputConfig, NumericInputDisabled, NumericInputDisplayText,
    NumericInputEditState, NumericValue, format_numeric_value, normalize_numeric_value,
    round_to_decimals,
};

#[derive(Clone, PartialEq)]
struct Observation {
    root: Entity,
    scene_id: String,
    placement: String,
    revision: u64,
    structure: u64,
    value: f32,
    center: Vec2,
    size: Vec2,
    config: NumericInputScrubConfig,
    text: String,
}

#[derive(Default)]
pub(super) struct State {
    expected: Option<String>,
    observed: Option<Observation>,
    samples: u8,
    last_frame: u64,
}

pub(super) fn select(world: &mut World, id: &str) {
    world.resource_mut::<Driver>().ready = State {
        expected: Some(id.into()),
        ..default()
    };
}

fn read(world: &mut World, root: Entity, expected: &str) -> Result<Observation, String> {
    if world.get::<PlacementTransformField>(root) != Some(&PlacementTransformField::TranslationX)
        || world.get::<NumericInputDisabled>(root).is_some()
    {
        return Err("当前根不是可编辑的原 X 字段".into());
    }
    if world
        .get_resource::<ActiveNumericInput>()
        .is_some_and(|state| state.entity.is_some())
        || world.resource::<ActiveNumericInputScrub>().entity.is_some()
        || world
            .get::<NumericInputEditState>(root)
            .is_some_and(|state| state.editing)
    {
        return Err("原数值控件仍持有上一事务".into());
    }
    let center = visible_center(world, root)?;
    let (map, revision, structure, source) = {
        let session = world.resource::<EditorAuthoringSession>();
        let browser = world.resource::<EditorGltfBrowserState>();
        if browser.selected_placement_id.as_deref() != Some(expected) {
            return Err(format!("原场景选择尚未指向验收对象 {expected}"));
        }
        if !browser.placement_index_ready(session.placement_structure_revision) {
            return Err("原场景对象索引尚未采用当前结构".into());
        }
        let index = browser
            .placement_record_index(expected, session.placement_structure_revision)
            .ok_or("当前索引缺少验收对象")?;
        let draft = session.draft.as_ref().ok_or("原作者草稿尚未就绪")?;
        if world.resource::<SceneEditorState>().selected_scene_id.as_ref() != Some(&draft.scene_id) {
            return Err("当前场景与作者来源不符".into());
        }
        let record = draft
            .placements
            .placements
            .get(index)
            .filter(|record| record.id == expected)
            .ok_or("原场景索引不再对应验收对象")?;
        (
            draft.scene_id.as_str().to_owned(),
            session.revision,
            session.placement_structure_revision,
            record.translation[0],
        )
    };
    let config = world
        .get::<NumericInputConfig>(root)
        .ok_or("原 X 字段配置缺失")?
        .0;
    let expected_value = normalize_numeric_value(
        NumericValue::F32(round_to_decimals(source, config.decimals)),
        config,
    );
    let current = world
        .get::<NumericInputValue>(root)
        .ok_or("原 X 字段模型缺失")?
        .0;
    if current != expected_value {
        return Err(format!(
            "等待原 NumericInputSetValue 采用：作者 X={source}，字段={current:?}"
        ));
    }
    let expected_text = format_numeric_value(expected_value, config);
    let text = world
        .query::<(&NumericInputDisplayText, &Text)>()
        .iter(world)
        .find_map(|(binding, text)| (binding.root == root).then(|| text.0.clone()))
        .ok_or("原 X 字段实际显示文字缺失")?;
    if text != expected_text {
        return Err(format!(
            "等待原字段文字同步：expected={expected_text}，actual={text}"
        ));
    }
    Ok(Observation {
        root,
        scene_id: map,
        placement: expected.into(),
        revision,
        structure,
        value: current.as_f32().ok_or("原 X 字段不是浮点数")?,
        center,
        size: world.get::<ComputedNode>(root).unwrap().size(),
        config: *world
            .get::<NumericInputScrubConfig>(root)
            .ok_or("原字段拖动规格缺失")?,
        text,
    })
}

pub(super) fn poll(world: &mut World, root: Entity) -> (bool, Value) {
    let expected = world.resource::<Driver>().ready.expected.clone();
    let result = expected
        .as_deref()
        .ok_or_else(|| "验收尚未固定所选对象".to_owned())
        .and_then(|id| read(world, root, id));
    let frame = world.resource::<Driver>().frame;
    let mut driver = world.resource_mut::<Driver>();
    let state = &mut driver.ready;
    match result {
        Ok(observed) => {
            if state.observed.as_ref() != Some(&observed) {
                state.observed = Some(observed);
                state.samples = 1;
            } else if state.last_frame != frame {
                state.samples = if state.last_frame + 1 == frame {
                    (state.samples + 1).min(2)
                } else {
                    1
                };
            }
            state.last_frame = frame;
            (
                state.samples >= 2,
                json!({"ready":state.samples>=2,"stable_frames":state.samples,"expected_placement":expected,
                "reason":if state.samples<2 {Some("等待同一作者、控件值与布局跨帧稳定")}else{None}}),
            )
        }
        Err(reason) => {
            state.observed = None;
            state.samples = 0;
            state.last_frame = frame;
            (
                false,
                json!({"ready":false,"stable_frames":0,"expected_placement":expected,"reason":reason}),
            )
        }
    }
}

/// 无作者来源的纯输入助手测试不启用屏障；真实验收 select 时强制绑定来源，起笔及 Press 前再次检查。
pub(super) fn check(world: &mut World, root: Entity) -> Result<(), String> {
    let (expected, observed, samples) = {
        let state = &world.resource::<Driver>().ready;
        (
            state.expected.clone(),
            state.observed.clone(),
            state.samples,
        )
    };
    let Some(expected) = expected else {
        return Ok(());
    };
    let current = read(world, root, &expected)?;
    if samples < 2 || observed.as_ref() != Some(&current) {
        return Err("原数值起笔前作者身份、控件值或布局已换代".into());
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn expected_value(world: &World) -> Option<f32> {
    world
        .resource::<Driver>()
        .ready
        .observed
        .as_ref()
        .map(|value| value.value)
}
