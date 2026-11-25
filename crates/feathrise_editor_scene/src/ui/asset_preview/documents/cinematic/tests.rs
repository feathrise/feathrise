use super::model::*;
use feathrise_cinematic::{Cinematic, RepeatPolicy};
use std::sync::Arc;

fn motion(lane: Lane, target: &str, time: &str, position: &str) -> Edit {
    Edit::Set {
        lane,
        kind: Kind::Pose,
        replace: None,
        camera: None,
        form: Form {
            target: target.into(),
            at: time.into(),
            position: position.into(),
            ..Default::default()
        },
    }
}

#[test]
fn author_edits_preserve_policy_identity_interpolation_and_disk_newlines() {
    let mut original = Cinematic::new("cut_gate", "门前相遇");
    original.repeat = RepeatPolicy::OncePerSave;
    let original: Arc<str> = serde_json::to_string(&original).unwrap().into();
    let first = prepare(
        original.clone(),
        Some(motion(Lane::Object, "gate", "0", "0, 1, 0")),
        true,
    )
    .unwrap();
    let (source, pages) = first.text.unwrap();
    assert!(pages.iter().any(|page| page.contains("\r\n")));
    let next = prepare(
        source.clone(),
        Some(motion(Lane::Object, "gate", "2", "4, 1, 0")),
        true,
    )
    .unwrap();
    let f = &next.timeline.film;
    assert_eq!(f.id, "cut_gate");
    assert!(matches!(f.repeat, RepeatPolicy::OncePerSave));
    assert_eq!(next.timeline.lanes[Lane::Object.index()].len(), 2);
    assert_eq!(
        f.sample(1.).objects[0].pose.unwrap().translation,
        [2., 1., 0.]
    );
    assert!(
        prepare(
            source.clone(),
            Some(motion(Lane::Object, "gate", "0", "7, 0, 0")),
            true
        )
        .is_err()
    );
    assert_eq!(
        Cinematic::from_slice(source.as_bytes()).unwrap().objects[0]
            .keys
            .len(),
        1
    );
    assert!(
        Cinematic::from_slice(original.as_bytes())
            .unwrap()
            .objects
            .is_empty()
    );
}

#[test]
fn binding_uses_the_stable_id_preserves_existing_rules_and_prevents_duplicates() {
    let source = r#"{"version":1,"variables":{"count":{"default":0}},"rules":[]}"#;
    let (saved, _) =
        super::binding::insert(source, "location.entered", "east_gate", "cut_gate", false).unwrap();
    let result: serde_json::Value = serde_json::from_str(&saved).unwrap();
    assert_eq!(result["variables"]["count"]["default"], 0);
    assert_eq!(result["rules"][0]["actions"][0]["target"], "cut_gate");
    assert!(
        super::binding::insert(&saved, "location.entered", "east_gate", "cut_gate", false).is_err()
    );
}
