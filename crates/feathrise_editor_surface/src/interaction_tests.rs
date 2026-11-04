//! 私有字段校验回归；继续使用生产预设工厂。
use super::*;
#[test]
fn crater_response_editor_accepts_rock_and_rejects_out_of_range() {
    let registry = SurfacePresetRegistry {
        catalog: default(),
        revision: 1,
        resolved: default(),
        last_error: None,
    };
    let mut state = SurfacePresetEditorState::default();
    let mut workspace = PresetWorkspaceState::default();
    create_preset(&mut state, &mut workspace, Some(&registry)).unwrap();
    let descriptor = &mut state.tabs[0].draft.descriptor;
    commit_field(descriptor, SurfacePresetField::CraterResponse, "0.1").unwrap();
    assert_eq!(descriptor.crater_response, 0.1);
    assert!(commit_field(descriptor, SurfacePresetField::CraterResponse, "1.1").is_err());
    assert!(commit_field(descriptor, SurfacePresetField::CraterResponse, "-0.1").is_err());
    assert_eq!(
        field_value(descriptor, SurfacePresetField::CraterResponse),
        "0.1"
    );
}
