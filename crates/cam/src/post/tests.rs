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

// Read the review body independently of the planner's phases/spans. These checks
// use analytic dimensions, not a second rendering of the calculated path. M30 is
// retained in the file; this is a test reader, not a controller or an NC importer.
#[derive(Default)]
struct ReadSection {
    tool: u32,
    mode: u32,
    speed: f64,
    max_rpm: f64,
    entry: Option<ProfilePoint>,
    moves: Vec<ReadMove>,
}
struct ReadMove {
    from: ProfilePoint,
    to: ProfilePoint,
    rapid: bool,
    feed: f64,
}
fn read_review(text: &str) -> Vec<ReadSection> {
    let mut sections: Vec<ReadSection> = Vec::new();
    let mut position = None;
    for line in text.lines().filter(|l| !l.starts_with('(')) {
        if line.starts_with('N') {
            assert_eq!(word(line, 'N') as usize, sections.len() + 1);
            sections.push(ReadSection::default());
            position = None;
            continue;
        }
        let Some(section) = sections.last_mut() else { continue };
        if line.starts_with("G0T") {
            section.tool = word(line, 'T') as u32;
        } else if line.starts_with("G50S") {
            section.max_rpm = word(line, 'S');
        } else if line.starts_with("G96S") || line.starts_with("G97S") {
            section.mode = word(line, 'G') as u32;
            section.speed = word(line, 'S');
        } else if line.starts_with("G0X") || line.starts_with("G1X") {
            let to = ProfilePoint { diameter_mm: -word(line, 'X'), z_mm: word(line, 'Z') };
            assert!(to.diameter_mm.is_finite() && to.z_mm.is_finite());
            if let Some(from) = position {
                let rapid = line.starts_with("G0X");
                section.moves.push(ReadMove { from, to, rapid, feed: if rapid { 0. } else { word(line, 'F') } });
            } else {
                assert!(line.starts_with("G0X"));
                section.entry = Some(to);
            }
            position = Some(to);
        }
    }
    sections
}
fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-7, "{actual} != {expected}");
}
fn read_profile(points: &[(f64, f64)]) -> Vec<ProfilePoint> {
    points.iter().map(|&(z_mm, diameter_mm)| ProfilePoint { z_mm, diameter_mm }).collect()
}
fn sample_cut(section: &ReadSection, check: impl Fn(ProfilePoint)) {
    assert!(!section.moves.is_empty());
    for movement in section.moves.iter().filter(|m| !m.rapid) {
        assert!(movement.feed > 0.);
        for t in [0., 0.25, 0.5, 0.75, 1.] {
            check(ProfilePoint {
                diameter_mm: movement.from.diameter_mm + t * (movement.to.diameter_mm - movement.from.diameter_mm),
                z_mm: movement.from.z_mm + t * (movement.to.z_mm - movement.from.z_mm),
            });
        }
    }
}

#[test]
fn exported_stepped_od_retains_radial_and_shoulder_stock_then_finishes_to_size() {
    let mut s = setup();
    s.od_job.as_mut().unwrap().profile = read_profile(&[(0., 30.), (-12., 30.), (-12., 36.), (-30., 36.)]);
    let sections = read_review(&review_nc(&s, &NlPost::default()).unwrap());
    assert_eq!(sections.iter().map(|s| s.tool).collect::<Vec<_>>(), [1212, 1212, 606, 606]);
    for (i, section) in sections.iter().enumerate() {
        assert_eq!(section.mode, 96);
        close(section.speed, if i < 2 { 150. } else { 200. });
        close(section.max_rpm, 2000.);
        close(section.entry.unwrap().z_mm, 3.);
        for movement in section.moves.iter().filter(|m| !m.rapid) {
            close(movement.feed, if i < 2 { 0.1 } else { 0.06 });
        }
        for movement in section.moves.iter().filter(|m| m.rapid && (m.from.z_mm - m.to.z_mm).abs() > 1e-7) {
            assert!(movement.from.diameter_mm >= 44. - 1e-7 && movement.to.diameter_mm >= 44. - 1e-7);
        }
    }
    sample_cut(&sections[0], |p| assert!(p.z_mm >= 0.1 - 1e-7));
    sample_cut(&sections[1], |p| {
        if p.z_mm < 0.1 - 1e-7 {
            let boundary = if p.z_mm < -11.9 - 1e-7 { 36.2 } else { 30.2 };
            assert!(p.diameter_mm >= boundary - 1e-7, "OD gouge at {p:?}");
        }
        assert!(p.z_mm >= -30. - 1e-7);
    });
    for expected in [(0., 30.), (-12., 30.), (-12., 36.), (-30., 36.)] {
        assert!(sections[3].moves.iter().any(|m| !m.rapid && (m.to.z_mm - expected.0).abs() < 1e-7 && (m.to.diameter_mm - expected.1).abs() < 1e-7));
    }
    assert!(sections[2].moves.iter().any(|m| !m.rapid && m.to.z_mm.abs() < 1e-7));
    assert!(sections[1].moves.iter().any(|m| !m.rapid && (m.to.z_mm + 30.).abs() < 1e-7), "rough the rear extent too");
}

#[test]
fn exported_stepped_id_keeps_stock_on_the_inside_and_retracts_in_the_prebore() {
    let mut s = setup();
    let mut job = s.od_job.take().unwrap();
    job.profile = read_profile(&[(0., 20.), (-10., 20.), (-10., 16.), (-30., 16.)]);
    s.id_job = Some(job);
    s.stock.as_mut().unwrap().inner_diameter_mm = 10.;
    s.operations.retain(|o| matches!(o.kind, K::OdRough | K::OdFinish));
    for (op, kind) in s.operations.iter_mut().zip([K::IdRough, K::IdFinish]) {
        op.kind = kind;
        op.station = kind.suggested_station();
    }
    let sections = read_review(&review_nc(&s, &NlPost::default()).unwrap());
    assert_eq!(sections.iter().map(|s| s.tool).collect::<Vec<_>>(), [505, 707]);
    sample_cut(&sections[0], |p| {
        if p.z_mm < 0.1 - 1e-7 {
            let boundary = if p.z_mm < -9.9 - 1e-7 { 15.8 } else { 19.8 };
            assert!(p.diameter_mm <= boundary + 1e-7, "ID gouge at {p:?}");
        }
        assert!(p.z_mm >= -30. - 1e-7);
    });
    for (i, section) in sections.iter().enumerate() {
        close(section.entry.unwrap().diameter_mm, 6.);
        for movement in section.moves.iter().filter(|m| !m.rapid) {
            close(movement.feed, if i == 0 { 0.1 } else { 0.06 });
        }
        for movement in section.moves.iter().filter(|m| m.rapid && (m.from.z_mm - m.to.z_mm).abs() > 1e-7) {
            assert!(movement.from.diameter_mm <= 6. + 1e-7 && movement.to.diameter_mm <= 6. + 1e-7);
        }
    }
    for expected in [(0., 20.), (-10., 20.), (-10., 16.), (-30., 16.)] {
        assert!(sections[1].moves.iter().any(|m| !m.rapid && (m.to.z_mm - expected.0).abs() < 1e-7 && (m.to.diameter_mm - expected.1).abs() < 1e-7));
    }
}

#[test]
fn exported_drilling_uses_tip_depth_and_fully_retracts_between_pecks() {
    use crate::machining::sequence::{Cutting, Drill, Limits};
    let mut s = setup();
    s.od_job = None;
    let limits = Limits { stock_front_z_mm: 1., chuck_front_z_mm: -45., clearance_mm: 2. };
    s.operations = [(K::CenterDrill, 3., -2., 0., 0.04), (K::Drill, 8., -25., 4., 0.08)]
        .into_iter()
        .enumerate()
        .map(|(index, (kind, diameter_mm, tip_end_z_mm, peck_mm, feed_mm_rev))| Operation {
            id: format!("drill-{index}"),
            name: "analytic".into(),
            kind,
            station: kind.suggested_station(),
            spindle: None,
            manual_separation: None,
            cutting: Some(Cutting::Drill(Drill { limits: limits.clone(), diameter_mm, tip_end_z_mm, peck_mm, feed_mm_rev, ..Default::default() })),
        })
        .collect();
    let sections = read_review(&review_nc(&s, &NlPost::default()).unwrap());
    assert_eq!(sections.iter().map(|s| s.tool).collect::<Vec<_>>(), [101, 202]);
    for (section, expected_feed) in sections.iter().zip([0.04, 0.08]) {
        assert_eq!(section.mode, 97);
        close(section.speed, 1000.);
        for movement in &section.moves {
            close(movement.to.diameter_mm, 0.);
            if movement.rapid {
                close(movement.to.z_mm, 3.);
            } else {
                close(movement.from.z_mm, 3.);
                close(movement.feed, expected_feed);
            }
        }
    }
    let depths: Vec<_> = sections[1].moves.iter().filter(|m| !m.rapid).map(|m| m.to.z_mm).collect();
    assert_eq!(depths, [-3., -7., -11., -15., -19., -23., -25.]);
    let full_diameter_z = depths.last().unwrap() + 4. / 59f64.to_radians().tan();
    assert!(full_diameter_z > -23. && full_diameter_z < -22.);
}

#[test]
fn exported_groove_keeps_both_walls_and_radial_stock_with_the_front_edge_reference() {
    use crate::machining::sequence::{Cutting, Groove, Limits};
    let mut s = setup();
    s.od_job = None;
    let g = Groove {
        limits: Limits { stock_front_z_mm: 1., chuck_front_z_mm: -45., clearance_mm: 2. },
        front_z_mm: -8.,
        width_mm: 4.,
        bottom_diameter_mm: 24.,
        tool_width_mm: 2.,
        radial_depth_mm: 1.,
        feed_mm_rev: 0.06,
        ..Default::default()
    };
    s.operations = [K::OdGrooveRough, K::OdGrooveFinish]
        .into_iter()
        .enumerate()
        .map(|(index, kind)| Operation {
            id: format!("groove-{index}"),
            name: "analytic".into(),
            kind,
            station: kind.suggested_station(),
            spindle: None,
            manual_separation: None,
            cutting: Some(Cutting::OdGroove(g.clone())),
        })
        .collect();
    let sections = read_review(&review_nc(&s, &NlPost::default()).unwrap());
    sample_cut(&sections[0], |p| {
        assert!(p.diameter_mm >= 24.2 - 1e-7);
        assert!(p.z_mm <= -8.1 + 1e-7 && p.z_mm - 2. >= -11.9 - 1e-7);
    });
    assert_eq!(sections.iter().map(|section| section.tool).collect::<Vec<_>>(), [1111, 808]);
    for section in &sections {
        assert_eq!(section.mode, 97);
        close(section.speed, 700.);
        for movement in section.moves.iter().filter(|m| !m.rapid) {
            close(movement.feed, 0.06);
        }
        for m in section.moves.iter().filter(|m| m.rapid && (m.from.z_mm - m.to.z_mm).abs() > 1e-7) {
            assert!(m.from.diameter_mm >= 44. - 1e-7 && m.to.diameter_mm >= 44. - 1e-7);
        }
    }
    for z in [-8., -10.] {
        assert!(sections[1].moves.iter().any(|m| !m.rapid && (m.to.diameter_mm - 24.).abs() < 1e-7 && (m.to.z_mm - z).abs() < 1e-7));
    }
}
