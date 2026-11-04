//! 自定义 Blade 档位结构动作与正式保存校验的回归。

use super::super::{
    fields,
    types::{BladeViewGroup, PresetDraft, PresetDraftMode, PresetField},
};
use super::*;
use feathrise_map_content::{ResolvedVegetationPresetCatalog, VegetationPresetCatalogAsset};

fn state_with_blade() -> VegetationPresetEditorState {
    let catalog: VegetationPresetCatalogAsset = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/vegetation_presets.vegetation_presets.json"
    )))
    .unwrap();
    let preset = catalog
        .presets
        .into_iter()
        .find(|preset| preset.id == "hn01_ordinary_grass")
        .unwrap();
    let mut state = VegetationPresetEditorState::default();
    state.open_draft(PresetDraft::new(PresetDraftMode::Create, preset));
    state
}

fn assert_saveable(state: &VegetationPresetEditorState) {
    let catalog = VegetationPresetCatalogAsset {
        schema_version: feathrise_map_content::VEGETATION_PRESET_SCHEMA_VERSION,
        presets: vec![state.active_draft().unwrap().descriptor.clone()],
    };
    ResolvedVegetationPresetCatalog::build(&catalog).expect("结构动作后必须仍可通过正式保存校验");
}

#[test]
fn append_and_duplicate_blade_lods_remain_saveable_through_eight_levels() {
    for duplicate in [false, true] {
        let mut state = state_with_blade();
        for count in 5..=MAX_BLADE_PRESET_LODS {
            if duplicate {
                state.active_draft_mut().unwrap().selected_lod = 0;
                duplicate_blade_lod(&mut state).unwrap();
            } else {
                append_blade_lod(&mut state).unwrap();
            }
            assert_saveable(&state);
            let VegetationPresetRuntime::Blade { lods, view_lod, .. } =
                &state.active_draft().unwrap().descriptor.runtime
            else {
                unreachable!()
            };
            assert_eq!(lods.len(), count);
            assert_eq!(view_lod.pixel_thresholds.len(), count - 1);
            assert_eq!(
                lods[count - 1].spacing,
                lods[0].spacing * 2.0_f32.powi(count as i32 - 1)
            );
            assert!(
                view_lod
                    .pixel_thresholds
                    .iter()
                    .all(|pair| pair[1] > pair[0])
            );
        }
        let before = state.active_draft().unwrap().descriptor.clone();
        assert!(append_blade_lod(&mut state).is_err());
        assert!(duplicate_blade_lod(&mut state).is_err());
        assert_eq!(state.active_draft().unwrap().descriptor, before);
    }
}

#[test]
fn removing_any_blade_level_keeps_its_parameters_aligned_and_saveable() {
    for removed_level in 0..4 {
        let mut state = state_with_blade();
        // 数组声明顺序不等于档位身份；反转后仍应按真实 level 删除对应参数。
        let draft = state.active_draft_mut().unwrap();
        let VegetationPresetRuntime::Blade { lods, view_lod, .. } = &mut draft.descriptor.runtime
        else {
            unreachable!()
        };
        let mut expected_radii = view_lod.residency_radii.clone();
        expected_radii.remove(removed_level);
        let mut expected_pairs = view_lod.pixel_thresholds.clone();
        expected_pairs.remove(removed_level.min(2));
        let mut expected_widths = view_lod.residency_transition_widths.clone();
        expected_widths.remove(removed_level.min(2));
        lods.reverse();
        draft.selected_lod = lods
            .iter()
            .position(|lod| lod.level == removed_level as u32)
            .unwrap();
        remove_blade_lod(&mut state).unwrap();
        assert_saveable(&state);
        let VegetationPresetRuntime::Blade { lods, view_lod, .. } =
            &state.active_draft().unwrap().descriptor.runtime
        else {
            unreachable!()
        };
        assert_eq!(
            lods.iter().map(|lod| lod.level).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert_eq!(view_lod.residency_radii, expected_radii);
        assert_eq!(view_lod.pixel_thresholds, expected_pairs);
        assert_eq!(view_lod.residency_transition_widths, expected_widths);
        for lod in lods {
            assert_eq!(
                lod.spacing,
                lods[0].spacing * 2.0_f32.powi(lod.level as i32)
            );
            assert!((lod.grid_side - 1) as f32 * lod.spacing * 0.5 >= lod.outer_radius);
        }
        while let VegetationPresetRuntime::Blade { lods, .. } =
            &state.active_draft().unwrap().descriptor.runtime
        {
            if lods.len() == 1 {
                break;
            }
            remove_blade_lod(&mut state).unwrap();
            assert_saveable(&state);
        }
        assert!(remove_blade_lod(&mut state).is_err());
    }
}

#[test]
fn extended_blade_view_fields_read_write_and_reject_missing_levels() {
    let mut state = state_with_blade();
    for _ in 4..8 {
        append_blade_lod(&mut state).unwrap();
    }
    let descriptor = &mut state.active_draft_mut().unwrap().descriptor;
    for (group, component, input) in [
        (BladeViewGroup::PixelThresholds(6), 1, "0.75"),
        (BladeViewGroup::ResidencyRadii, 7, "20000"),
        (BladeViewGroup::ResidencyTransitionWidths, 6, "80"),
    ] {
        let field = PresetField::BladeView { group, component };
        fields::commit_field(descriptor, field, input).unwrap();
        assert_eq!(fields::field_value(descriptor, field).unwrap(), input);
    }
    assert_saveable(&state);
    let descriptor = &mut state.active_draft_mut().unwrap().descriptor;
    let before = descriptor.clone();
    let missing = PresetField::BladeView {
        group: BladeViewGroup::PixelThresholds(7),
        component: 0,
    };
    assert!(fields::commit_field(descriptor, missing, "1").is_err());
    assert!(fields::field_value(descriptor, missing).is_err());
    assert_eq!(*descriptor, before);
}
