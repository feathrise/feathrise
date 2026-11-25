//! 用主场景的真实属性控件编辑光学参数和姿态，并在重开后核对实际参与渲染的灯光组件。

use super::*;
use crate::ui::placement_transform::{LightField, PlacementTransformField as Field};
use bevy::input::{ButtonState, keyboard::{Key, KeyboardInput}};
use feathrise_editor_ui::{ActiveNumericInput, NumericInputConfig, NumericInputDisplayText, format_numeric_value};
use feathrise_scene::ScenePlacementInstance;

pub(super) const SCREENSHOT: &str = "scene-tree-local-lights.png";
pub(super) const POINT_SCREENSHOT: &str = "scene-tree-point-light-range.png";
#[path = "light_handles.rs"]
mod handles;

const POINT: &[(Field, f32)] = &[
    (Field::TranslationY, 3.5), (Field::Light(LightField::Red), 0.25),
    (Field::Light(LightField::Intensity), 650.0), (Field::Light(LightField::Range), 7.0),
];
const SPOT: &[(Field, f32)] = &[
    (Field::TranslationY, 5.5), (Field::RotationX, -45.0), (Field::Light(LightField::Blue), 0.5),
    (Field::Light(LightField::Intensity), 850.0), (Field::Light(LightField::InnerAngle), 8.0),
    (Field::Light(LightField::OuterAngle), 24.0), (Field::Light(LightField::Range), 11.0),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in super::super) enum Step { Input(usize), ClickIntensity(usize, Entity, u8), TypeIntensity(usize, Entity), ConfirmIntensity(usize), Applied(usize), Undo, Redo, Handles(usize, u8), WaitRenderer, Screenshot }

pub(super) fn advance(world: &mut World, step: &mut Step, spot: bool, revision: &mut u64, pulse: &mut Option<Entity>) -> Result<bool, String> {
    let edits = if spot { SPOT } else { POINT };
    let press = |world: &mut World, action| -> Result<Entity, String> {
        let entity = crate::standalone::scene_format_validation::action_button(world, action).ok_or("灯光验收找不到历史按钮")?;
        world.entity_mut(entity).insert(EditorButtonClick::triggered());
        Ok(entity)
    };
    match *step {
        Step::Input(index) => {
            let (field, value) = edits[index];
            let selected = world.resource::<EditorGltfBrowserState>().selected_placement_id.as_deref();
            let session = world.resource::<EditorAuthoringSession>();
            if !session.draft.as_ref().is_some_and(|draft| draft.placements.placements.iter().any(|record|
                Some(record.id.as_str()) == selected && crate::authoring::local_lights::parameters(&record.asset_target)
                    .is_some_and(|light| light.3.is_some() == spot))) {
                return Err("创建后的场景树与视口选集没有指向该灯光".into());
            }
            let entity = world.query::<(Entity, &Field, &ComputedNode)>().iter(world)
                .find_map(|(entity, candidate, node)| (*candidate == field && !node.is_empty()).then_some(entity));
            let Some(entity) = entity else { return Ok(false); };
            if !ui_visible(world, entity) {
                crate::standalone::validation_fixture::reveal_property_control(world, entity)?;
                return Ok(false);
            }
            if matches!(field, Field::Light(LightField::Intensity)) {
                *step = Step::ClickIntensity(index, entity, 0);
                return Ok(false);
            }
            world.write_message(NumericInputCommitted { entity, value: NumericValue::F32(value),
                outcome: NumericInputCommitOutcome::Accepted, source: NumericInputCommitSource::Text, changed: true, edited: true });
            *step = Step::Applied(index);
        }
        Step::ClickIntensity(index, entity, frame) => {
            *step = click_control(world, entity, frame)?.map_or(Step::TypeIntensity(index, entity), |next| Step::ClickIntensity(index, entity, next));
        }
        Step::TypeIntensity(index, entity) => {
            if world.resource::<ActiveNumericInput>().entity != Some(entity) { return Err("单击光强数值框没有取得文本焦点".into()); }
            let text = edits[index].1.to_string();
            key(world, if spot { KeyCode::Digit8 } else { KeyCode::Digit6 }, Key::Character(text.clone().into()), Some(text))?;
            *step = Step::ConfirmIntensity(index);
        }
        Step::ConfirmIntensity(index) => {
            key(world, KeyCode::Enter, Key::Enter, None)?;
            *step = Step::Applied(index);
        }
        Step::Applied(_) | Step::Undo | Step::Redo => {
            let session = world.resource::<EditorAuthoringSession>();
            if session.revision == *revision { return Ok(false); }
            if session.revision != revision.wrapping_add(1) { return Err("灯光属性操作未形成单笔历史".into()); }
            *revision = session.revision;
            if let Step::Applied(index) = *step && index + 1 < edits.len() {
                *step = Step::Input(index + 1);
                return Ok(false);
            }
            let record = session.draft.as_ref().and_then(|draft| draft.placements.placements.iter().find(|record|
                crate::authoring::local_lights::parameters(&record.asset_target).is_some_and(|p| p.3.is_some() == spot)))
                .ok_or("灯光的作者记录消失")?;
            let expected_range = if *step == Step::Undo { 12.0 } else { edits.last().unwrap().1 };
            if crate::authoring::local_lights::parameters(&record.asset_target).unwrap().2 != Some(expected_range) {
                return Err(format!("灯光范围未随编辑或历史切换到 {expected_range} 米"));
            }
            match *step {
                Step::Applied(_) => { *pulse = Some(press(world, crate::types::EditorAction::Undo)?); *step = Step::Undo; }
                Step::Undo => { *pulse = Some(press(world, crate::types::EditorAction::Redo)?); *step = Step::Redo; }
                _ => { handles::frame(world, spot)?; *step = Step::Handles(0, 0); }
            }
        }
        Step::Handles(index, phase) => {
            *step = handles::advance(world, spot, index, phase, revision, pulse)?;
        }
        Step::WaitRenderer => {
            if spot && (rendered(world).is_none() || !inspector_shows_spot(world)) { return Ok(false); }
            request_screenshot(world, if spot { SCREENSHOT } else { POINT_SCREENSHOT })?;
            *step = Step::Screenshot;
        }
        Step::Screenshot => {
            let Some(result) = world.get_resource_mut::<StartupScreenshot>().and_then(|mut capture| check_ready(&mut capture.0)) else { return Ok(false); };
            result?;
            world.remove_resource::<StartupScreenshot>();
            return Ok(true);
        }
    }
    Ok(false)
}

fn key(world: &mut World, key_code: KeyCode, logical_key: Key, text: Option<String>) -> Result<(), String> {
    let window = world.query_filtered::<Entity, With<PrimaryWindow>>().single(world).map_err(|_| "光强输入缺少原编辑窗口")?;
    for state in [ButtonState::Pressed, ButtonState::Released] {
        crate::standalone::validation_fixture::window_event(world, KeyboardInput { window, key_code, logical_key: logical_key.clone(),
            text: text.as_deref().filter(|_| state == ButtonState::Pressed).map(Into::into), state, repeat: false });
    }
    Ok(())
}

fn inspector_shows_spot(world: &mut World) -> bool {
    // 灯具已选中且光照可见时，中央的网格解析徽标应当隐藏。
    let mut mesh_badge = world.query_filtered::<&Visibility, With<crate::components::SceneEditorSelectedPlacementBadge>>();
    if !matches!(mesh_badge.single(world), Ok(Visibility::Hidden)) { return false; }
    let extra = [(Field::Light(LightField::Red), 1.0), (Field::Light(LightField::Green), 1.0),
        (Field::ScaleX, 1.0), (Field::ScaleY, 1.0), (Field::ScaleZ, 1.0)];
    // 同时核对控件内部值和实际绘制的数字文字。拍照前允许原 UI 消费撤销后的值同步消息。
    let mut fields = world.query::<(Entity, &Field, &NumericInputValue, &NumericInputConfig)>();
    let mut labels = world.query::<(&NumericInputDisplayText, &Text, &ComputedNode)>();
    SPOT.iter().chain(&extra).all(|(field, value)| {
        let Some((entity, _, current, config)) = fields.iter(world).find(|(_, candidate, ..)| *candidate == field) else { return false; };
        let expected = match field { Field::Light(LightField::Range) => 14.0, Field::Light(LightField::InnerAngle) => 15.0,
            Field::Light(LightField::OuterAngle) => 32.0, _ => *value };
        matches!(current.0, NumericValue::F32(actual) if (actual - expected).abs() < 0.08)
            && labels.iter(world).any(|(label, text, node)| label.root == entity && !node.is_empty()
            && text.as_str() == format_numeric_value(current.0, config.0))
    })
}

pub(super) fn rendered(world: &mut World) -> Option<Value> {
    if !crate::authoring::local_lights::AVAILABLE { return Some(Value::Null); }
    let mut ids = [None, None];
    for (placement, transform, point, spot) in world.query::<(&ScenePlacementInstance, &Transform, Option<&PointLight>, Option<&SpotLight>)>().iter(world) {
        if !placement.is_active() || placement.scene_id().as_str() != "M1-SAMPLE" { continue; }
        let close = |a: f32, b: f32| (a - b).abs() < 0.001;
        let dragged = |a: f32, b: f32| (a - b).abs() < 0.08;
        if let Some(point) = point && close(transform.translation.y, POINT[0].1) && point.color == Color::linear_rgb(0.25, 1.0, 1.0)
            && close(point.intensity / (4.0 * std::f32::consts::PI), 650.0) && dragged(point.range, 9.0) {
            ids[0] = Some(placement.id().to_owned());
        }
        if let Some(spot) = spot && close(transform.translation.y, SPOT[0].1) && spot.color == Color::linear_rgb(1.0, 1.0, 0.5)
            && close(spot.intensity / (4.0 * std::f32::consts::PI), 850.0) && dragged(spot.range, 14.0)
            && dragged(spot.inner_angle.to_degrees(), 15.0) && dragged(spot.outer_angle.to_degrees(), 32.0)
            && transform.rotation.dot(Quat::from_rotation_x(-45f32.to_radians())).abs() > 0.99999 {
            ids[1] = Some(placement.id().to_owned());
        }
    }
    let [point, spot] = ids;
    Some(json!({"point":point?, "spot":spot?}))
}
