use super::*;

#[test]
fn split_section_keeps_backtracking_edges_distinct_from_the_model() {
    use machining::ProfilePoint;
    let points = [(0., 40.), (-10., 40.), (-10., 50.), (-10., 40.), (-20., 40.)].map(|(z_mm, diameter_mm)| ProfilePoint { z_mm, diameter_mm });
    assert_eq!(drawing::simplified(&points), points);
    let straight = [(0., 40.), (-10., 40.), (-20., 40.)].map(|(z_mm, diameter_mm)| ProfilePoint { z_mm, diameter_mm });
    assert_eq!(drawing::simplified(&straight), [straight[0], straight[2]]);
}

fn manual_plan() -> TurningPlan {
    let mut plan = TurningPlan::nl2500();
    let setup = &mut plan.setups[0];
    setup.stock = Some(Stock { outer_diameter_mm: 50.0, inner_diameter_mm: 0.0, length_mm: 60.0 });
    setup.operations.push(Operation {
        spindle: None,
        cutting: None,
        id: "finish-back".into(),
        name: "Back face finish, manual separation".into(),
        kind: OperationKind::PartingFinish,
        station: Some(8),
        manual_separation: Some(ManualSeparation { finished_bore_diameter_mm: 20.0, stop_diameter_mm: 20.0 }),
    });
    plan
}

#[test]
fn owner_turret_has_empty_stations_and_no_guessed_tool_offsets() {
    let plan = TurningPlan::nl2500();
    plan.validate().unwrap();
    let slots = &plan.setups[0].turret;
    assert_eq!(slots.iter().map(|s| s.station).collect::<Vec<_>>(), (1..=12).collect::<Vec<_>>());
    assert_eq!(slots[2].role, ToolRole::Vacant);
    assert_eq!(slots[3].role, ToolRole::Vacant);
    assert_eq!(slots[10].role, ToolRole::OdGrooveRough);
    assert_eq!(slots[11].role, ToolRole::OdRough);
    assert!(slots.iter().all(|s| s.offset_number.is_none() && s.tool_name.is_none()));
    assert_eq!(OperationKind::IdRough.suggested_station(), Some(5));
    assert_eq!(OperationKind::OdFinish.suggested_station(), Some(6));
    assert_eq!(OperationKind::PartingFinish.suggested_station(), None);
}

#[test]
fn finish_to_bore_remains_a_manual_separation_after_roundtrip() {
    let plan = manual_plan();
    plan.validate().unwrap();
    let restored: TurningPlan = serde_json::from_str(&serde_json::to_string(&plan).unwrap()).unwrap();
    assert_eq!(restored, plan);
    assert_eq!(restored.setups[0].planned_outcome(), PlannedOutcome::ManualSeparationRequired);
    let mut cut = plan.clone();
    cut.setups[0].operations[0].kind = OperationKind::PartOff;
    assert!(cut.validate().is_err(), "a manual-separation record must not silently become automatic");
    cut.setups[0].operations[0].manual_separation = None;
    cut.validate().unwrap();
    assert_eq!(cut.setups[0].planned_outcome(), PlannedOutcome::CutOff);
}

#[test]
fn manual_separation_requires_positive_finite_bore_and_stop_diameters() {
    for diameter in [0.0, -1.0, f64::NAN, f64::INFINITY, 50.0, 1_000_001.0] {
        for bore in [true, false] {
            let mut plan = manual_plan();
            let manual = plan.setups[0].operations[0].manual_separation.as_mut().unwrap();
            if bore {
                manual.finished_bore_diameter_mm = diameter;
            } else {
                manual.stop_diameter_mm = diameter;
            }
            assert!(plan.validate().is_err(), "accepted {diameter}, bore={bore}");
        }
    }
    let mut missing = manual_plan();
    missing.setups[0].operations[0].manual_separation = None;
    assert!(missing.validate().is_err());
}

#[test]
fn cannot_machine_after_separation_in_the_same_setup() {
    let mut plan = manual_plan();
    let mut later = plan.setups[0].operations[0].clone();
    later.id = "later".into();
    later.kind = OperationKind::OdFinish;
    later.manual_separation = None;
    plan.setups[0].operations.push(later);
    assert!(plan.validate().is_err());
}

#[test]
fn regrip_dependencies_are_ordered_and_turrets_are_independent_snapshots() {
    let mut plan = manual_plan();
    let mut back = plan.setups[0].clone();
    back.id = "back".into();
    back.previous_setup = Some("setup-1".into());
    back.operations.clear();
    back.turret[7].tool_name = Some("Back tool".into());
    plan.setups.push(back);
    plan.validate().unwrap();
    assert!(plan.setups[0].turret[7].tool_name.is_none());
    plan.setups[0].previous_setup = Some("back".into());
    assert!(plan.validate().is_err());
}

#[test]
fn duplicate_ids_and_missing_or_vacant_stations_are_rejected() {
    let mut plan = manual_plan();
    for station in [0, 3, 4, 13, 255] {
        plan.setups[0].operations[0].station = Some(station);
        assert!(plan.validate().is_err());
    }
    plan.setups[0].operations[0].station = Some(8);
    let duplicate = plan.setups[0].clone();
    plan.setups.push(duplicate);
    assert!(plan.validate().is_err());
    plan.setups[1].id = "back".into();
    assert!(plan.validate().is_err(), "operation id still duplicated");
    plan.setups[1].operations.clear();
    plan.validate().unwrap();
    plan.setups[1].turret[0].station = 2;
    assert!(plan.validate().is_err());
}

#[test]
fn bad_stock_and_unknown_fields_are_rejected() {
    let mut plan = manual_plan();
    plan.setups[0].stock.as_mut().unwrap().inner_diameter_mm = 50.0;
    assert!(plan.validate().is_err());
    let mut value = serde_json::to_value(TurningPlan::nl2500()).unwrap();
    value["setups"][0]["diamter"] = serde_json::json!(20);
    assert!(serde_json::from_value::<TurningPlan>(value).is_err());
}
