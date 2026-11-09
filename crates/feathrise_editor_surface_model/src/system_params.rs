//! 地表实现的固定 ECS 借用合同；消息游标、Local 与 Commands 仍属于原调度系统。
use feathrise_editor_ui::UiInteraction;
use super::{
    components::*,
    resources::*,
    types::{SurfaceListCache, SurfaceStyleCache},
};
use bevy::{
    ecs::system::SystemParam, input::keyboard::KeyboardInput, input_focus::InputFocus, prelude::*,
    text::EditableText, window::PrimaryWindow,
};
use feathrise_editor_core::EditorTextInputGuard;
use feathrise_map_content::SurfacePresetRegistry;

#[derive(SystemParam)]
pub struct ButtonsParams<'w, 's> {
    pub buttons: Query<
        'w,
        's,
        (Entity, &'static feathrise_editor_ui::EditorButtonClick, &'static SurfacePresetButton),
        Changed<feathrise_editor_ui::EditorButtonClick>,
    >,
    pub fields: Query<'w, 's, &'static EditableText, With<SurfacePresetButton>>,
    pub guard: Res<'w, EditorTextInputGuard>,
    pub input_focus: ResMut<'w, InputFocus>,
    pub state: ResMut<'w, SurfacePresetEditorState>,
    pub registry: Option<Res<'w, SurfacePresetRegistry>>,
    pub windows: Query<'w, 's, &'static mut Window, With<PrimaryWindow>>,
}
#[derive(SystemParam)]
pub struct TextParams<'w, 's> {
    pub keyboard_inputs: MessageReader<'w, 's, KeyboardInput>,
    pub fields: Query<'w, 's, (Entity, &'static EditableText), With<SurfacePresetButton>>,
    pub numeric: Option<Res<'w, feathrise_editor_ui::ActiveNumericInput>>,
    pub official_inputs: Query<'w, 's, (), With<EditableText>>,
    pub guard: Res<'w, EditorTextInputGuard>,
    pub input_focus: ResMut<'w, InputFocus>,
    pub state: ResMut<'w, SurfacePresetEditorState>,
    pub windows: Query<'w, 's, &'static mut Window, With<PrimaryWindow>>,
}
#[derive(SystemParam)]
pub struct PersistenceParams<'w> {
    pub state: ResMut<'w, SurfacePresetEditorState>,
    pub registry: Option<Res<'w, SurfacePresetRegistry>>,
    pub asset_server: Res<'w, AssetServer>,
}
pub type SurfaceButtonStyle = (
    &'static UiInteraction,
    &'static SurfacePresetButton,
    Option<&'static SurfaceButtonKind>,
    &'static mut BackgroundColor,
    &'static mut BorderColor,
);
#[derive(SystemParam)]
pub struct PanelParams<'w, 's> {
    pub commands: Commands<'w, 's>,
    pub asset_server: Res<'w, AssetServer>,
    pub registry: Option<Res<'w, SurfacePresetRegistry>>,
    pub state: Res<'w, SurfacePresetEditorState>,
    pub cache: Local<'s, SurfaceListCache>,
    pub hosts: Query<'w, 's, Entity, With<SurfacePresetList>>,
    pub roots: Query<'w, 's, Entity, With<SurfacePresetDynamicListRoot>>,
    pub texts: ParamSet<
        'w,
        's,
        (
            Query<'w, 's, &'static mut Text, With<SurfacePresetListTitle>>,
            Query<'w, 's, &'static mut Text, With<SurfacePresetSummaryText>>,
            Query<'w, 's, &'static mut Text, With<SurfacePresetStatusText>>,
        ),
    >,
}
#[derive(SystemParam)]
pub struct BadgeParams<'w, 's> {
    pub state: Res<'w, SurfacePresetEditorState>,
    pub seen: Local<'s, Option<(u64, u64, bool)>>,
    pub badges:
        Query<'w, 's, (&'static mut Text, &'static mut TextColor), With<SurfaceDocumentBadge>>,
}
#[derive(SystemParam)]
pub struct StylesParams<'w, 's> {
    pub state: Res<'w, SurfacePresetEditorState>,
    pub registry: Option<Res<'w, SurfacePresetRegistry>>,
    pub cache: Local<'s, SurfaceStyleCache>,
    pub buttons: ParamSet<
        'w,
        's,
        (
            Query<'w, 's, SurfaceButtonStyle, (Changed<UiInteraction>, Without<feathrise_editor_ui::workspace_tabs::WorkspaceTabButton>)>,
            Query<'w, 's, SurfaceButtonStyle, Without<feathrise_editor_ui::workspace_tabs::WorkspaceTabButton>>,
        ),
    >,
}
