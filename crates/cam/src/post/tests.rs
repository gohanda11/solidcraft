use super::*;
use crate::{
    Operation, Stock, TurningPlan,
    machining::{OdJob, ProfilePoint},
};

fn setup() -> Setup {
    let mut plan = TurningPlan::nl2500();
    let setup = &mut plan.setups[0];
    setup.stock = Some(Stock { outer_diameter_mm: 40., inner_diameter_mm: 0., length_mm: 60. });
    setup.od_job = Some(OdJob {
        profile: vec![ProfilePoint { z_mm: 0., diameter_mm: 30. }, ProfilePoint { z_mm: -20., diameter_mm: 30. }],
        stock_front_z_mm: 1.,
        chuck_front_z_mm: -45.,
        clearance_mm: 2.,
        radial_depth_mm: 1.,
        face_depth_mm: 1.,
        rough_feed_mm_rev: 0.1,
        finish_feed_mm_rev: 0.06,
        finish: true,
        ..OdJob::default()
    });
    setup.operations = [K::FaceRough, K::OdRough, K::FaceFinish, K::OdFinish]
        .into_iter()
        .enumerate()
        .map(|(index, kind)| Operation {
            id: format!("op-{index}"),
            name: "owner name (not NC syntax)".into(),
            kind,
            station: kind.suggested_station(),
            cutting: None,
            spindle: None,
            manual_separation: None,
        })
        .collect();
    setup.clone()
}

#[test]
fn nl_blocks_follow_reference_dialect_and_stay_disarmed_before_every_instruction() {
    let s = setup();
    let text = review_nc(&s, &NlPost::default()).unwrap();
    assert!(text.starts_with("%\nO9001 (SOLIDCRAFT NL SAMPLE REVIEW)\nM30\n"));
    assert_eq!(text.matches("G99G18\nG50S2000\nG54\nG0T").count(), 4);
    assert_eq!(text.matches("G0T1212").count(), 2);
    assert_eq!(text.matches("G0T0606").count(), 2);
    assert_eq!(text.matches("G96S150.000M3").count(), 2);
    assert_eq!(text.matches("G96S200.000M3").count(), 2);
    assert_eq!(text.lines().filter(|l| *l == "M01").count(), 4);
    assert_eq!(text.lines().filter(|l| l.starts_with('N')).collect::<Vec<_>>(), ["N1", "N2", "N3", "N4"]);
    assert!(text.ends_with("M30\n%\n"));
    assert!(!text.contains("owner name"));
    for code in ["M86", "M85", "G53X0Z0"] {
        assert!(!text.lines().any(|l| l == code), "{code} requires explicit selection");
    }
}

fn word(line: &str, letter: char) -> f64 {
    let tail = line.split_once(letter).unwrap().1;
    let end = tail.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(tail.len());
    tail[..end].parse().unwrap()
}

#[test]
fn signed_output_roundtrips_every_planned_endpoint_and_feed_without_changing_allowances() {
    let s = setup();
    let path = sequence::calculate(&s).unwrap();
    for side in [XSide::Negative, XSide::Positive] {
        let post = NlPost { x_side: side, ..Default::default() };
        let text = review_nc(&s, &post).unwrap();
        let mut output = text.lines().filter(|l| l.starts_with("G0X") || l.starts_with("G1X"));
        for op in &s.operations {
            let moves: Vec<_> = path.moves.iter().filter(|m| crate::simulation::operation(m.phase) == Some(op.kind)).collect();
            assert!(!moves.is_empty());
            let approach = output.next().unwrap();
            assert_eq!(word(approach, 'Z'), moves[0].from.z_mm);
            for m in moves {
                let line = output.next().unwrap();
                assert!((word(line, 'X') - side.coordinate(m.to.diameter_mm)).abs() < 1e-8);
                assert!((word(line, 'Z') - m.to.z_mm).abs() < 1e-8);
                assert_eq!(line.starts_with("G0"), m.motion == Motion::Rapid);
                if let Some(feed) = m.feed_mm_rev {
                    assert!((word(line, 'F') - feed).abs() < 1e-8);
                }
            }
        }
        assert!(output.next().is_none());
        assert_eq!(path.job.radial_stock_to_leave_mm, 0.1);
        assert_eq!(path.job.axial_stock_to_leave_mm, 0.1);
    }
}

#[test]
fn offsets_and_operation_spindle_overrides_are_explicit_and_bounded() {
    let mut s = setup();
    let mut post = NlPost { example_same_number_offsets: false, ..Default::default() };
    assert!(review_nc(&s, &post).is_err());
    for slot in &mut s.turret {
        slot.offset_number = Some(30 + u16::from(slot.station));
    }
    s.operations[0].spindle = Some(Spindle { mode: SpeedMode::Rpm, speed: 1800., max_rpm: 1200 });
    let nc = review_nc(&s, &post).unwrap();
    assert!(nc.contains("G0T1242\n") && nc.contains("G0T0636\n"));
    assert!(nc.contains("G50S1200\n") && nc.contains("G97S1200M3\n"));
    assert!(!nc.contains("OFFSET USES EXAMPLE"));
    post.machine_zero_return = true;
    post.example_m86_m85 = true;
    let nc = review_nc(&s, &post).unwrap();
    assert!(nc.find("M30").unwrap() < nc.find("\nM86\n").unwrap());
    assert_eq!(nc.lines().filter(|l| *l == "G53X0Z0").count(), 4);
    assert!(nc.contains("\nM85\nM30\n"));
    s.turret[11].offset_number = Some(100);
    assert!(review_nc(&s, &post).is_err());
}

#[test]
fn drilling_and_grooves_use_fixed_rpm_and_invalid_settings_never_create_nc() {
    let p = NlPost::default();
    assert_eq!(p.spindle(K::Drill).mode, SpeedMode::Rpm);
    assert_eq!(p.spindle(K::OdGrooveFinish).mode, SpeedMode::Rpm);
    for post in [
        NlPost { program_number: 0, ..p.clone() },
        NlPost { work_offset: 90, ..p.clone() },
        NlPost { rough_surface_m_min: f64::NAN, ..p.clone() },
        NlPost { drill_rpm: 0, ..p.clone() },
    ] {
        assert!(review_nc(&setup(), &post).is_err());
    }
    let mut s = setup();
    s.operations[0].spindle = Some(Spindle { mode: SpeedMode::Rpm, speed: f64::INFINITY, max_rpm: 1000 });
    assert!(review_nc(&s, &p).is_err());
    s.operations[0].spindle = None;
    s.operations[3].kind = K::OdThread;
    assert!(review_nc(&s, &p).is_err());
}

#[test]
fn old_plans_keep_legacy_output_and_new_settings_roundtrip() {
    let plan = TurningPlan::nl2500();
    let mut old = serde_json::to_value(&plan).unwrap();
    old["setups"][0].as_object_mut().unwrap().remove("post");
    let restored: TurningPlan = serde_json::from_value(old).unwrap();
    assert!(restored.setups[0].post.is_none());
    assert_eq!(serde_json::from_str::<TurningPlan>(&serde_json::to_string(&plan).unwrap()).unwrap(), plan);
}

#[test]
fn drill_and_groove_spans_emit_g97_and_every_cutting_move() {
    use crate::machining::sequence::{Cutting, Drill, Groove, Limits};
    let mut s = setup();
    s.od_job = None;
    let limits = Limits { stock_front_z_mm: 1., chuck_front_z_mm: -45., clearance_mm: 2. };
    let drill = Drill { limits: limits.clone(), diameter_mm: 8., tip_end_z_mm: -30., peck_mm: 4., feed_mm_rev: 0.08, ..Default::default() };
    let groove = Groove {
        limits,
        front_z_mm: -8.,
        width_mm: 4.,
        bottom_diameter_mm: 24.,
        tool_width_mm: 2.,
        radial_depth_mm: 1.,
        feed_mm_rev: 0.06,
        ..Default::default()
    };
    s.operations =
        [(K::Drill, Cutting::Drill(drill)), (K::OdGrooveRough, Cutting::OdGroove(groove.clone())), (K::OdGrooveFinish, Cutting::OdGroove(groove))]
            .into_iter()
            .enumerate()
            .map(|(index, (kind, cutting))| Operation {
                id: format!("local-{index}"),
                name: "analytic".into(),
                kind,
                station: kind.suggested_station(),
                cutting: Some(cutting),
                spindle: None,
                manual_separation: None,
            })
            .collect();
    let path = sequence::calculate(&s).unwrap();
    let text = review_nc(&s, &NlPost::default()).unwrap();
    assert_eq!(text.matches("G97S1000M3").count(), 1);
    assert_eq!(text.matches("G97S700M3").count(), 2);
    assert!(!text.contains("G96S"));
    assert_eq!(text.lines().filter(|l| l.starts_with("G1X")).count(), path.moves.iter().filter(|m| m.motion == Motion::Feed).count());
}

#[test]
fn id_motion_is_mapped_to_negative_diameter_and_no_stock_face_is_explicit() {
    let mut s = setup();
    s.id_job = s.od_job.take();
    let job = s.id_job.as_mut().unwrap();
    for p in &mut job.profile {
        p.diameter_mm = 16.;
    }
    s.stock.as_mut().unwrap().inner_diameter_mm = 10.;
    s.operations.retain(|o| matches!(o.kind, K::OdRough | K::OdFinish));
    for (op, kind) in s.operations.iter_mut().zip([K::IdRough, K::IdFinish]) {
        op.kind = kind;
        op.station = kind.suggested_station();
    }
    let path = sequence::calculate(&s).unwrap();
    let text = review_nc(&s, &NlPost::default()).unwrap();
    let feed: Vec<_> = text.lines().filter(|l| l.starts_with("G1X")).collect();
    let expected: Vec<_> = path.moves.iter().filter(|m| m.motion == Motion::Feed).collect();
    assert_eq!(feed.len(), expected.len());
    for (line, movement) in feed.into_iter().zip(expected) {
        assert!((word(line, 'X') + movement.to.diameter_mm).abs() < 1e-8);
    }
    assert!(text.contains("G0T0505") && text.contains("G0T0707"));
    let mut s = setup();
    s.od_job.as_mut().unwrap().stock_front_z_mm = 0.;
    s.od_job.as_mut().unwrap().axial_stock_to_leave_mm = 0.;
    let text = review_nc(&s, &NlPost::default()).unwrap();
    assert!(text.contains("TANMEN,ARA - NO MATERIAL TO REMOVE"));
}
