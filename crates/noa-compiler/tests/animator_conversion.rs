use noa_compiler::{
    InspectOptions,
    convert::{EntityIds, SourceIdentity, UnitySources, plan_animators},
    prepare_ir,
    unity::{UnityFile, parse_meta_guid},
};
use noa_format::ir::{ReferenceKind, SerializedValue};
use serde_yaml_ng::Value;
use std::{collections::BTreeSet, fs, path::Path};

fn sample_sources() -> UnitySources {
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/scenario/effect/kirakira_01_rip");
    let mut sources = UnitySources::default();
    for file in [
        "kirakira_01.prefab",
        "kirakira_01.controller",
        "kirakira_01.anim",
        "Stop.anim",
    ] {
        let path = root.join(file);
        sources.files.insert(
            file.into(),
            UnityFile::parse(&fs::read_to_string(&path).unwrap()).unwrap(),
        );
        let guid = parse_meta_guid(
            &fs::read_to_string(path.with_extension(format!(
                "{}.meta",
                path.extension().unwrap().to_str().unwrap()
            )))
            .unwrap(),
        )
        .unwrap();
        sources.guid_sources.insert(guid, file.into());
    }
    sources
}

fn allocate(sources: &UnitySources, plans: &[noa_compiler::convert::AnimatorPlan]) -> EntityIds {
    EntityIds::allocate(
        sources
            .files
            .iter()
            .flat_map(|(key, file)| {
                file.objects
                    .keys()
                    .map(move |id| SourceIdentity::new(key, *id))
            })
            .chain(plans.iter().flat_map(|p| p.id_sources())),
    )
    .unwrap()
}

#[test]
fn sample_animator_preserves_states_and_resolves_activation_tracks() {
    let sources = sample_sources();
    let plans = plan_animators("kirakira_01.prefab", &sources).unwrap();
    let ids = allocate(&sources, &plans);
    let (animator, animations) = plans[0].lower(&ids).unwrap();
    assert_eq!(animator.layers.len(), 1);
    let layer = &animator.layers[0];
    assert_eq!(layer.states.len(), 2);
    let default = layer
        .states
        .iter()
        .find(|s| s.id == layer.default_state_id)
        .unwrap();
    assert_eq!(default.name, "kirakira_01");
    assert_eq!(animations.len(), 2);
    for animation in animations {
        assert_eq!(animation.duration_seconds, 0.0);
        assert_eq!(animation.tracks.len(), 9);
        let expected = animation.name != "Stop";
        assert!(animation.tracks.iter().all(|t| match &t.data {
            noa_format::wire::TrackDataT::BoolTrack(t) =>
                t.keys.len() == 1 && t.keys[0].time == 0. && t.keys[0].value == expected,
            _ => false,
        }));
        assert!(layer.states.iter().any(|s| s.animation_id == animation.id));
    }
}

#[test]
fn transitions_and_multiple_layers_are_explicitly_unsupported() {
    let mut sources = sample_sources();
    let controller = sources.files.get_mut("kirakira_01.controller").unwrap();
    let state = controller
        .objects
        .values_mut()
        .find(|o| o.type_name == "AnimatorState")
        .unwrap();
    state.data["m_Transitions"] = serde_yaml_ng::from_str("[{fileID: 123}]").unwrap();
    let error = plan_animators("kirakira_01.prefab", &sources)
        .err()
        .unwrap();
    assert_eq!(error.code, "UNSUPPORTED_ANIMATOR");
    assert!(error.message.contains("m_Transitions"));
    let mut sources = sample_sources();
    let controller = sources
        .files
        .get_mut("kirakira_01.controller")
        .unwrap()
        .objects
        .values_mut()
        .find(|o| o.type_name == "AnimatorController")
        .unwrap();
    let layers = controller.data["m_AnimatorLayers"]
        .as_sequence_mut()
        .unwrap();
    layers.push(layers[0].clone());
    assert!(plan_animators("kirakira_01.prefab", &sources).is_err());
}

#[test]
fn missing_or_ambiguous_animation_paths_fail_before_encoding() {
    let mut sources = sample_sources();
    let clip = sources
        .files
        .get_mut("kirakira_01.anim")
        .unwrap()
        .objects
        .values_mut()
        .next()
        .unwrap();
    clip.data["m_FloatCurves"][0]["path"] = Value::String("missing".into());
    assert_eq!(
        plan_animators("kirakira_01.prefab", &sources)
            .err()
            .unwrap()
            .code,
        "MISSING_ANIMATION_TARGET"
    );
    let mut sources = sample_sources();
    let prefab = sources.files.get_mut("kirakira_01.prefab").unwrap();
    let count = prefab
        .objects
        .values_mut()
        .filter(|o| o.type_name == "GameObject")
        .map(|o| {
            if o.data["m_Name"].as_str() == Some("circle_up_L") {
                o.data["m_Name"] = Value::String("circle_start".into());
                1
            } else {
                0
            }
        })
        .sum::<usize>();
    assert_eq!(count, 1);
    assert_eq!(
        plan_animators("kirakira_01.prefab", &sources)
            .err()
            .unwrap()
            .code,
        "AMBIGUOUS_ANIMATION_TARGET"
    );
}

#[test]
fn reusing_a_controller_scopes_clip_and_state_ids_to_the_animator() {
    let mut sources = sample_sources();
    let mut second = sources.files["kirakira_01.prefab"].clone();
    let animator = second
        .objects
        .values_mut()
        .find(|o| o.type_name == "Animator")
        .unwrap();
    animator.data["m_Enabled"] = Value::from(0);
    sources.files.insert("second.prefab".into(), second);
    let mut plans = plan_animators("kirakira_01.prefab", &sources).unwrap();
    plans.extend(plan_animators("second.prefab", &sources).unwrap());
    let ids = allocate(&sources, &plans);
    let (first, first_clips) = plans[0].lower(&ids).unwrap();
    let (second, second_clips) = plans[1].lower(&ids).unwrap();
    assert!(first.enabled);
    assert!(!second.enabled);
    assert_ne!(
        first.layers[0].default_state_id,
        second.layers[0].default_state_id
    );
    let first_targets: BTreeSet<_> = first_clips
        .iter()
        .flat_map(|c| c.tracks.iter().map(|t| t.binding.target_id))
        .collect();
    let second_targets: BTreeSet<_> = second_clips
        .iter()
        .flat_map(|c| c.tracks.iter().map(|t| t.binding.target_id))
        .collect();
    assert!(first_targets.is_disjoint(&second_targets));
    assert_ne!(first_clips[0].id, second_clips[0].id);
}

#[test]
fn prepared_sample_resolves_main_animator_and_lists_remaining_components() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/scenario/effect/kirakira_01_rip/kirakira_01.prefab");
    let draft = prepare_ir(&path, &InspectOptions::default()).unwrap();
    assert_eq!(draft.diagnostics.len(), 1);
    assert_eq!(
        draft.diagnostics[0].code,
        "HOST_PROJECT_COLOR_SPACE_REQUIRED"
    );
    assert_eq!(draft.pending_components.len(), 32);
    assert_eq!(draft.animators.len(), 1);
    let script = draft
        .custom_components
        .iter()
        .find(|c| c.script.as_ref().unwrap().class_name == "ScenarioEffector")
        .unwrap();
    let SerializedValue::Reference(reference) = &script.fields["mainAnimator"].value else {
        panic!("typed ref");
    };
    let target = reference.target.unwrap();
    assert_eq!(target.kind, ReferenceKind::Component);
    assert_eq!(target.id, draft.animators[0].id);
    assert_eq!(
        script.fields["timelineDirector"].value,
        SerializedValue::Null
    );
    let repeated = prepare_ir(&path, &InspectOptions::default()).unwrap();
    assert_eq!(
        serde_json::to_string(&draft).unwrap(),
        serde_json::to_string(&repeated).unwrap()
    );
}
