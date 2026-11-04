//! 水体表单字段与正式描述符之间的唯一读写映射。

use feathrise_map_content::WaterPresetDescriptor;

use super::types::WaterPresetField;

pub(super) fn field_value(descriptor: &WaterPresetDescriptor, field: WaterPresetField) -> String {
    match field {
        WaterPresetField::Label => descriptor.label.clone(),
        WaterPresetField::BaseColor(component) => descriptor
            .base_color_srgba
            .get(component)
            .copied()
            .unwrap_or_default()
            .to_string(),
        WaterPresetField::Roughness => descriptor.perceptual_roughness.to_string(),
        WaterPresetField::Reflectance => descriptor.reflectance.to_string(),
        WaterPresetField::TroughDarkening => descriptor.trough_darkening.to_string(),
        WaterPresetField::ShoreWaveAttenuation => descriptor.shore_wave_attenuation.to_string(),
        WaterPresetField::WaveDirection { wave, component } => descriptor
            .macro_waves
            .get(wave)
            .and_then(|wave| wave.direction.get(component))
            .copied()
            .unwrap_or_default()
            .to_string(),
        WaterPresetField::WaveAmplitude(wave) => descriptor
            .macro_waves
            .get(wave)
            .map_or(0.0, |wave| wave.amplitude_meters)
            .to_string(),
        WaterPresetField::WaveWavelength(wave) => descriptor
            .macro_waves
            .get(wave)
            .map_or(0.0, |wave| wave.wavelength_meters)
            .to_string(),
        WaterPresetField::WaveSpeed(wave) => descriptor
            .macro_waves
            .get(wave)
            .map_or(0.0, |wave| wave.speed_meters_per_second)
            .to_string(),
        WaterPresetField::WaveSteepness(wave) => descriptor
            .macro_waves
            .get(wave)
            .map_or(0.0, |wave| wave.steepness)
            .to_string(),
        WaterPresetField::NormalTile(layer) => descriptor
            .micro_normals
            .get(layer)
            .map_or(0.0, |normal| normal.tile_meters)
            .to_string(),
        WaterPresetField::NormalVelocity { layer, component } => descriptor
            .micro_normals
            .get(layer)
            .and_then(|normal| normal.velocity_meters_per_second.get(component))
            .copied()
            .unwrap_or_default()
            .to_string(),
        WaterPresetField::NormalStrength(layer) => descriptor
            .micro_normals
            .get(layer)
            .map_or(0.0, |normal| normal.strength)
            .to_string(),
    }
}

pub(super) fn commit_field(
    descriptor: &mut WaterPresetDescriptor,
    field: WaterPresetField,
    value: &str,
) -> Result<(), String> {
    if field == WaterPresetField::Label {
        if value.trim().is_empty() {
            return Err("显示名称不能为空".to_owned());
        }
        descriptor.label = value.trim().to_owned();
        return Ok(());
    }
    let number = value
        .parse::<f32>()
        .map_err(|error| format!("数值格式无效：{error}"))?;
    if !number.is_finite() {
        return Err("水体参数必须是有限数".to_owned());
    }
    match field {
        WaterPresetField::BaseColor(component) if in_range(number, 0.0, 1.0) => {
            *descriptor
                .base_color_srgba
                .get_mut(component)
                .ok_or_else(|| "水色通道索引无效".to_owned())? = number;
        }
        WaterPresetField::Roughness if in_range(number, 0.0, 1.0) => {
            descriptor.perceptual_roughness = number;
        }
        WaterPresetField::Reflectance if in_range(number, 0.0, 1.0) => {
            descriptor.reflectance = number;
        }
        WaterPresetField::TroughDarkening if in_range(number, 0.0, 1.0) => {
            descriptor.trough_darkening = number;
        }
        WaterPresetField::ShoreWaveAttenuation if in_range(number, 0.1, 8.0) => {
            descriptor.shore_wave_attenuation = number;
        }
        WaterPresetField::WaveDirection { wave, component } => {
            *descriptor
                .macro_waves
                .get_mut(wave)
                .and_then(|wave| wave.direction.get_mut(component))
                .ok_or_else(|| "宏观波方向索引无效".to_owned())? = number;
        }
        WaterPresetField::WaveAmplitude(wave) if in_range(number, 0.0, 10.0) => {
            descriptor
                .macro_waves
                .get_mut(wave)
                .ok_or_else(|| "宏观波索引无效".to_owned())?
                .amplitude_meters = number;
        }
        WaterPresetField::WaveWavelength(wave) if in_range(number, 0.1, 100_000.0) => {
            descriptor
                .macro_waves
                .get_mut(wave)
                .ok_or_else(|| "宏观波索引无效".to_owned())?
                .wavelength_meters = number;
        }
        WaterPresetField::WaveSpeed(wave) if in_range(number, 0.0, 1_000.0) => {
            descriptor
                .macro_waves
                .get_mut(wave)
                .ok_or_else(|| "宏观波索引无效".to_owned())?
                .speed_meters_per_second = number;
        }
        WaterPresetField::WaveSteepness(wave) if in_range(number, 0.0, 1.0) => {
            descriptor
                .macro_waves
                .get_mut(wave)
                .ok_or_else(|| "宏观波索引无效".to_owned())?
                .steepness = number;
        }
        WaterPresetField::NormalTile(layer) if in_range(number, 0.01, 100_000.0) => {
            descriptor
                .micro_normals
                .get_mut(layer)
                .ok_or_else(|| "微法线索引无效".to_owned())?
                .tile_meters = number;
        }
        WaterPresetField::NormalVelocity { layer, component } => {
            *descriptor
                .micro_normals
                .get_mut(layer)
                .and_then(|normal| normal.velocity_meters_per_second.get_mut(component))
                .ok_or_else(|| "微法线速度索引无效".to_owned())? = number;
        }
        WaterPresetField::NormalStrength(layer) if in_range(number, 0.0, 4.0) => {
            descriptor
                .micro_normals
                .get_mut(layer)
                .ok_or_else(|| "微法线索引无效".to_owned())?
                .strength = number;
        }
        WaterPresetField::Label => unreachable!(),
        _ => return Err("数值超出该字段允许范围".to_owned()),
    }
    Ok(())
}

fn in_range(value: f32, min: f32, max: f32) -> bool {
    (min..=max).contains(&value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use feathrise_map_content::{WaterMacroWave, WaterMicroNormalLayer};

    fn descriptor() -> WaterPresetDescriptor {
        WaterPresetDescriptor {
            id: "lake".to_owned(),
            aliases: Vec::new(),
            label: "湖".to_owned(),
            base_color_srgba: [0.0; 4],
            perceptual_roughness: 0.1,
            reflectance: 0.5,
            trough_darkening: 0.1,
            shore_wave_attenuation: 1.0,
            macro_waves: [WaterMacroWave {
                direction: [1.0, 0.0],
                amplitude_meters: 0.1,
                wavelength_meters: 4.0,
                speed_meters_per_second: 1.0,
                steepness: 0.2,
            }; 4],
            micro_normals: std::array::from_fn(|_| WaterMicroNormalLayer {
                texture_path: None,
                tile_meters: 1.0,
                velocity_meters_per_second: [0.0, 0.0],
                strength: 0.0,
            }),
        }
    }

    #[test]
    fn edits_each_wave_without_conflating_flow_or_other_layers() {
        let mut value = descriptor();
        commit_field(&mut value, WaterPresetField::WaveAmplitude(2), "0.45").unwrap();
        assert_eq!(value.macro_waves[2].amplitude_meters, 0.45);
        assert_eq!(value.macro_waves[1].amplitude_meters, 0.1);
        assert!(commit_field(&mut value, WaterPresetField::WaveSteepness(0), "2").is_err());
    }
    #[test] // 水体名称输入同样不能在每帧打字时重建表单。
    fn official_water_name_edit_preserves_workspace_revision() {
        // 使用现有完整水体夹具检查同步边界。
        use super::super::types::{
            WaterDraft, WaterDraftMode, WaterPresetAction, WaterPresetButton,
            WaterPresetEditorState,
        }; // 复用正式会话类型。
        use crate::ui::text_input::EditorTextInputGuard;
        use crate::ui::vegetation_preset_editor::{
            PresetWorkspaceTab, VegetationPresetEditorState,
        }; // 当前共享标签身份。
        use bevy::input::keyboard::KeyboardInput; // 业务快捷键消息。
        use bevy::input_focus::{FocusCause, InputFocus}; // 官方唯一输入焦点。
        use bevy::prelude::*; // 构造最小业务 App。
        use bevy::text::EditableText; // 官方已确认文字来源。 // 官方组合事件保护。
        let mut state = WaterPresetEditorState::default(); // 构造水体编辑会话。
        let id = state.open_draft(WaterDraft::new(WaterDraftMode::Create, descriptor())); // 使用已有有效描述符夹具。
        state.active_field = Some(WaterPresetField::Label); // 模拟名称已经聚焦。
        let revision = state.ui_revision; // 打字必须保持实体树修订不变。
        let mut workspace = VegetationPresetEditorState::default(); // 设置水体为活动正文。
        workspace.active_tab = PresetWorkspaceTab::Water(id); // 使用真实稳定标签 ID。
        let mut app = App::new(); // 无渲染业务回归。
        app.insert_resource(state)
            .insert_resource(workspace)
            .insert_resource(crate::resources::MapEditorState {
                open: true,
                ..default()
            }); // 注册有效会话。
        app.init_resource::<InputFocus>()
            .init_resource::<EditorTextInputGuard>()
            .add_message::<KeyboardInput>(); // 官方焦点和业务输入依赖。
        app.add_systems(Update, super::super::interaction::handle_text_input); // 运行真实输入业务系统。
        let entity = app
            .world_mut()
            .spawn((
                WaterPresetButton(WaterPresetAction::Focus(WaterPresetField::Label)),
                EditableText::new("碧波湖水"),
            ))
            .id(); // 官方文本持有完整中文。
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(entity, FocusCause::Navigated); // 聚焦可见字段对应实体。
        app.update(); // 同步一帧。
        let state = app.world().resource::<WaterPresetEditorState>(); // 检查生产状态。
        assert_eq!(state.input_buffer, "碧波湖水"); // 中文只进入字段缓冲。
        assert_eq!(state.ui_revision, revision); // 不因输入重建官方编辑器。
        assert_eq!(state.tabs[0].draft.descriptor.label, "湖"); // 显式提交前保持原草稿。
        assert_eq!(app.world().resource::<InputFocus>().get(), Some(entity)); // 输入实体和焦点保持稳定。
        app.world_mut()
            .get_mut::<EditableText>(entity)
            .unwrap()
            .editor
            .set_text("最后确认字"); // 模拟官方本帧已应用的最后一次编辑。
        let other = app.world_mut().spawn(EditableText::new("其他输入框")).id(); // 用户鼠标已切换到另一个官方输入域。
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(other, FocusCause::Pressed); // 切换发生在业务同步之前。
        app.world_mut()
            .resource_mut::<Messages<KeyboardInput>>()
            .write(KeyboardInput {
                // 新输入域的确认键不能提交底层草稿。
                key_code: KeyCode::Enter, // 使用真实确认键。
                logical_key: bevy::input::keyboard::Key::Enter, // 与系统生产分支一致。
                state: bevy::input::ButtonState::Pressed, // 本帧按下边沿。
                text: None,               // 控制键不含文字。
                repeat: false,            // 单次按下。
                window: other,            // 最小测试只需要合法实体身份。
            }); // 事件进入真实业务Reader。
        app.update(); // 执行失焦情况下的业务同步。
        let state = app.world().resource::<WaterPresetEditorState>(); // 检查旧会话保留的最新文字。
        assert_eq!(state.input_buffer, "最后确认字"); // 最新字必须在失焦返回前读回，而非沿用上一帧缓冲。
        assert_eq!(state.active_field, Some(WaterPresetField::Label)); // 其他输入域的Enter不能提交底层名称。
        assert_eq!(state.ui_revision, revision); // 失焦同步不重建活动输入树。
    } // 水体官方输入回归结束。
}
