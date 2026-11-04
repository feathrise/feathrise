//! 普通地表作者的有限验收适配；投递真实动作、操作官方文本控件，不维护另一套草稿或保存逻辑。

pub(super) use super::types::{CatalogFact, TabFact};
use super::{Domain, Pulse};
use bevy::{
    input::keyboard::{Key, KeyboardInput},
    prelude::*,
    window::PrimaryWindow,
};
use feathrise_map_content::{SurfacePresetDescriptor, SurfacePresetRegistry};
use feathrise_editor_surface_model::{
    SurfacePresetEditorState,
    components::SurfacePresetButton,
    types::{SurfacePresetAction, SurfacePresetField},
};
use sha2::{Digest, Sha256};

/// 只对自有夹具的一个有限描述符取内容摘要；不扫描目录或克隆贴图/地图像素。
pub(super) fn descriptor_hash(descriptor: &SurfacePresetDescriptor) -> String {
    Sha256::digest(serde_json::to_vec(descriptor).expect("地表描述符必须能序列化"))
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(super) fn catalog(world: &World, _: Domain) -> Result<Option<CatalogFact>, String> {
    let Some(registry) = world.get_resource::<SurfacePresetRegistry>() else {
        return Ok(None);
    };
    if let Some(error) = &registry.last_error {
        return Err(error.clone());
    }
    if registry.revision == 0 {
        return Ok(None);
    }
    let preset = registry
        .presets()
        .first()
        .ok_or("普通地表目录为空，无法验收已有预设编辑")?;
    Ok(Some(CatalogFact {
        id: preset.id.clone(),
        name: preset.label.clone(),
        hash: descriptor_hash(preset),
    }))
}

pub(super) fn tab(world: &World, _: Domain, id: &str) -> Option<TabFact> {
    let state = world.get_resource::<SurfacePresetEditorState>()?;
    let tab = state
        .tabs
        .iter()
        .find(|tab| tab.draft.descriptor.id == id)?;
    Some(TabFact {
        id: tab.id,
        name: tab.draft.descriptor.label.clone(),
        hash: descriptor_hash(&tab.draft.baseline),
        revision: state.ui_revision,
        dirty: tab.draft.dirty(),
        writing: state.busy(),
    })
}

pub(super) fn status(world: &World, _: Domain) -> String {
    world
        .get_resource::<SurfacePresetEditorState>()
        .map(|state| state.status.clone())
        .unwrap_or_else(|| "普通地表作者未安装".into())
}

pub(super) fn input_blocked(world: &World) -> bool {
    world
        .get_resource::<feathrise_editor_core::EditorTextInputGuard>()
        .is_some_and(|guard| guard.block_shortcuts)
        || world
            .get_resource::<feathrise_editor_ui::ActiveNumericInput>()
            .is_some_and(|input| input.entity.is_some())
}

fn pulse(world: &mut World, action: SurfacePresetAction) -> Pulse {
    Pulse {
        entity: world
            .spawn((
                SurfacePresetButton(action),
                feathrise_editor_ui::EditorButtonClick::triggered(),
            ))
            .id(),
        synthetic: true,
    }
}

/// 选择只定位原作者层，现有生产入口不会隐式新增图层或改地图脏位。
pub(super) fn select(world: &mut World, _: Domain, id: &str) -> Result<Pulse, String> {
    let index = world
        .get_resource::<SurfacePresetRegistry>()
        .ok_or("地表目录未安装")?
        .presets()
        .iter()
        .position(|preset| preset.id == id)
        .ok_or("待选地表身份已变化")?;
    Ok(pulse(world, SurfacePresetAction::SelectPreset(index)))
}

pub(super) fn open(world: &mut World, _: Domain, id: &str) -> Result<Pulse, String> {
    if world
        .get_resource::<SurfacePresetRegistry>()
        .and_then(|registry| registry.resolve(id))
        .is_none()
    {
        return Err("待编辑地表预设已不存在".into());
    }
    Ok(pulse(world, SurfacePresetAction::EditSelected))
}

pub(super) fn close(world: &mut World, _: Domain, tab: u64) -> Pulse {
    pulse(world, SurfacePresetAction::CloseTab(tab))
}

fn actual_button(world: &mut World, action: SurfacePresetAction) -> Option<Pulse> {
    let entity = world
        .query::<(
            Entity,
            &SurfacePresetButton,
            Has<bevy::ui::InteractionDisabled>,
        )>()
        .iter(world)
        .find_map(|(entity, button, disabled)| {
            (button.0 == action && !disabled).then_some(entity)
        })?;
    world.entity_mut(entity).insert((
        Interaction::Pressed,
        feathrise_editor_ui::EditorButtonClick::triggered(),
    ));
    Some(Pulse {
        entity,
        synthetic: false,
    })
}

pub(super) fn save(world: &mut World, _: Domain) -> Option<Pulse> {
    actual_button(world, SurfacePresetAction::Save)
}

pub(super) fn name_active(world: &World) -> bool {
    world
        .get_resource::<SurfacePresetEditorState>()
        .is_some_and(|state| state.active_field == Some(SurfacePresetField::Label))
}

pub(super) fn focus_name(world: &mut World) -> Option<Pulse> {
    actual_button(world, SurfacePresetAction::Focus(SurfacePresetField::Label))
}

pub(super) fn edit_name(world: &mut World, _: Domain, tab: u64, value: &str) -> bool {
    if !world
        .get_resource::<SurfacePresetEditorState>()
        .is_some_and(|state| state.has_tab(tab))
    {
        return false;
    }
    let entity = world
        .query::<(Entity, &SurfacePresetButton, &bevy::text::EditableText)>()
        .iter(world)
        .find_map(|(entity, target, _)| {
            (target.0 == SurfacePresetAction::Focus(SurfacePresetField::Label)).then_some(entity)
        });
    let Some(entity) = entity else {
        return false;
    };
    let mut text = world.get_mut::<bevy::text::EditableText>(entity).unwrap();
    text.clear();
    text.editor.set_text(value);
    text.queue_edit(bevy::text::TextEdit::TextEnd(false));
    world
        .resource_mut::<bevy::input_focus::InputFocus>()
        .set(entity, bevy::input_focus::FocusCause::Navigated);
    true
}

pub(super) fn buffer_matches(world: &World, _: Domain, _: u64, value: &str) -> bool {
    world
        .get_resource::<SurfacePresetEditorState>()
        .is_some_and(|state| name_active(world) && state.input_buffer == value)
}

pub(super) fn submit_name(world: &mut World) -> Result<(), String> {
    let window = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
        .map_err(|error| error.to_string())?;
    // 原地表文本绑定要求Enter/Tab提交；仅失焦不会提交，不能直接填草稿来绕过真实合同。
    world.write_message(KeyboardInput {
        key_code: KeyCode::Enter,
        logical_key: Key::Enter,
        state: bevy::input::ButtonState::Pressed,
        text: None,
        repeat: false,
        window,
    });
    Ok(())
}

#[cfg(test)]
pub(crate) fn dirty_surface_fixture() -> SurfacePresetEditorState {
    use feathrise_editor_surface_model::types::{SurfaceDraft, SurfaceDraftMode};
    // 真实普通作者类型和基线差异；不使用临时游戏预设或人为dirty标记。
    let descriptor = SurfacePresetDescriptor {
        id: "close-surface".into(),
        aliases: Vec::new(),
        label: "未保存地表".into(),
        base_color_srgba: [0.5, 0.5, 0.5, 1.0],
        base_color_texture_path: None,
        normal_texture_path: None,
        arm_height_texture_path: None,
        tile_meters: 2.0,
        normal_strength: 1.0,
        normal_y_flip: false,
        height_blend_strength: 0.0,
        perceptual_roughness: 0.8,
        metallic: 0.0,
        crater_response: 1.0,
        projection: feathrise_map_content::SurfaceProjection::WorldXz,
    };
    let mut state = SurfacePresetEditorState::default();
    state.open_draft(SurfaceDraft::new(
        SurfaceDraftMode::Edit {
            original_id: descriptor.id.clone(),
        },
        descriptor,
    ));
    state.tabs[0].draft.descriptor.label.push_str("·修改");
    state
}
