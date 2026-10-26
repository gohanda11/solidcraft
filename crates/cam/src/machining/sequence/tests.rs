use super::*;

fn limits() -> Limits {
    Limits { stock_front_z_mm: 2., chuck_front_z_mm: -75., clearance_mm: 2. }
}
fn drill() -> Drill {
    Drill { limits: limits(), diameter_mm: 10., tip_end_z_mm: -65., peck_mm: 5., feed_mm_rev: 0.08, ..Drill::default() }
}
fn groove() -> Groove {
    Groove {
        limits: limits(),
        front_z_mm: -10.,
        width_mm: 6.,
        bottom_diameter_mm: 30.,
        tool_width_mm: 2.,
        radial_depth_mm: 1.,
        feed_mm_rev: 0.08,
        ..Groove::default()
    }
}
fn op(id: &str, kind: K, cutting: Option<Cutting>) -> crate::Operation {
    crate::Operation { spindle: None, id: id.into(), name: id.into(), kind, station: kind.suggested_station(), cutting, manual_separation: None }
}
fn setup() -> Setup {
    let mut p = crate::TurningPlan::nl2500();
    let s = &mut p.setups[0];
    s.stock = Some(Stock { outer_diameter_mm: 50., inner_diameter_mm: 0., length_mm: 82. });
    s.clone()
}
fn profile(d: f64) -> OdJob {
    OdJob {
        profile: vec![ProfilePoint { z_mm: 0., diameter_mm: d }, ProfilePoint { z_mm: -60., diameter_mm: d }],
        stock_front_z_mm: 2.,
        chuck_front_z_mm: -75.,
        clearance_mm: 2.,
        radial_depth_mm: 1.,
        face_depth_mm: 1.,
        rough_feed_mm_rev: 0.1,
        finish_feed_mm_rev: 0.06,
        finish: true,
        ..OdJob::default()
    }
}

#[test]
fn shared_limits_validate_placement_without_requiring_cutting_conditions() {
    let mut s = setup();
    s.operations = vec![op("drill", K::Drill, Some(Cutting::Drill(Drill { limits: limits(), ..Drill::default() })))];
    assert_eq!(shared_limits(&s).unwrap(), limits());
    assert!(validate_local(&s, &s.operations[0]).is_err());
    s.operations.push(op("groove", K::OdGrooveRough, Some(Cutting::OdGroove(Groove { limits: limits(), ..Groove::default() }))));
    assert_eq!(shared_limits(&s).unwrap(), limits());
    for front in [f64::NAN, f64::INFINITY, -1., 1e6 + 1.] {
        let mut invalid = s.clone();
        if let Some(Cutting::Drill(d)) = &mut invalid.operations[0].cutting {
            d.limits.stock_front_z_mm = front;
        }
        assert!(shared_limits(&invalid).is_err());
    }
    if let Some(Cutting::OdGroove(g)) = &mut s.operations[1].cutting {
        g.limits.clearance_mm = f64::NAN;
    }
    assert!(shared_limits(&s).is_err());
    // Common profile placement deliberately supersedes hidden local snapshots.
    s.od_job = Some(profile(40.));
    assert_eq!(shared_limits(&s).unwrap(), limits());
}

fn model_groove_setup() -> Setup {
    let mut s = setup();
    let mut job = profile(40.);
    job.model_source =
        Some(ModelSource { body: "analytic grooved shaft".into(), reverse: false, origin: [0.; 3], axis: [0., 0., 1.], approximation_mm: 0. });
    job.profile = [(0., 40.), (-10., 40.), (-10., 30.), (-16., 30.), (-16., 40.), (-60., 40.)]
        .into_iter()
        .map(|(z_mm, diameter_mm)| ProfilePoint { z_mm, diameter_mm })
        .collect();
    s.od_job = Some(job);
    s
}

#[test]
fn model_rectangular_groove_keeps_finished_target_and_separate_turning_envelope() {
    let mut s = model_groove_setup();
    let exact = s.od_job.clone().unwrap();
    s.operations = vec![op("face", K::FaceRough, None), op("od", K::OdRough, None), op("ff", K::FaceFinish, None), op("of", K::OdFinish, None)];
    let path = calculate(&s).unwrap();
    assert_eq!(path.job, exact);
    assert_eq!(s.od_job.as_ref().unwrap(), &exact);
    assert!(path.moves.iter().filter(|m| m.phase == Phase::OdFinish).all(|m| m.to.diameter_mm >= 40.));
    let before = super::super::selection::remaining(&s).unwrap();
    assert!(before.slabs.iter().filter(|s| s.z0 >= -16. && s.z1 <= -10.).all(|s| (s.outer - 20.).abs() < EPS));
    assert!(before.remaining_mm3 >= std::f64::consts::PI * (400. - 225.) * 6. - 0.001);
    let g = groove();
    s.operations.insert(2, op("gr", K::OdGrooveRough, Some(Cutting::OdGroove(g.clone()))));
    s.operations.push(op("gf", K::OdGrooveFinish, Some(Cutting::OdGroove(g))));
    let path = calculate(&s).unwrap();
    assert_eq!(path.job, exact);
    let after = super::super::selection::remaining(&s).unwrap();
    assert!(after.slabs.iter().filter(|s| s.z0 >= -16. && s.z1 <= -10.).all(|s| (s.outer - 15.).abs() < EPS));
    let report = crate::simulation::simulate(&s, &path, 0.5).unwrap();
    assert!(report.contacts.iter().all(|c| c.target != "cutter_gouge"));
    assert!(review_nc(&s).unwrap().contains("OdGrooveFinish"));
}

#[test]
fn model_groove_ranges_keep_original_indices_and_reject_partial_recesses() {
    let s = model_groove_setup();
    let job = s.od_job.as_ref().unwrap();
    let mut c = super::super::selection::Contour {
        limits: limits(),
        inner: false,
        first: 0,
        last: 5,
        profile_snapshot: job.profile.clone(),
        source_snapshot: job.model_source.clone(),
        radial_leave_mm: 0.1,
        axial_leave_mm: 0.1,
        depth_mm: 1.,
        feed_mm_rev: 0.1,
    };
    let points = c.points(&s).unwrap();
    assert_eq!(points, vec![job.profile[0], job.profile[1], job.profile[4], job.profile[5]]);
    assert_eq!(c.profile_snapshot, job.profile);
    for (first, last) in [(0, 2), (2, 3), (3, 5), (1, 3)] {
        c.first = first;
        c.last = last;
        assert!(c.points(&s).is_err(), "partial recess {first}..{last}");
    }
    c.first = 4;
    c.last = 5;
    assert_eq!(c.points(&s).unwrap(), job.profile[4..=5]);
}

#[test]
fn model_groove_dimensions_must_match_and_manual_intent_stays_explicit() {
    let mut s = model_groove_setup();
    let g = groove();
    s.operations = vec![op("gr", K::OdGrooveRough, Some(Cutting::OdGroove(g.clone())))];
    assert!(validate_local(&s, &s.operations[0]).is_ok());
    for changed in [Groove { front_z_mm: -9., ..g.clone() }, Groove { width_mm: 7., ..g.clone() }, Groove { bottom_diameter_mm: 29., ..g.clone() }] {
        s.operations[0].cutting = Some(Cutting::OdGroove(changed));
        assert!(validate_local(&s, &s.operations[0]).is_err());
        assert!(calculate(&s).is_err());
    }
    s.operations[0].cutting = Some(Cutting::OdGroove(g));
    s.od_job.as_mut().unwrap().profile = profile(40.).profile;
    assert!(calculate(&s).is_err()); // No groove exists in the model.
    s.od_job.as_mut().unwrap().model_source = None;
    let path = calculate(&s).unwrap();
    assert!(path.warnings.iter().any(|w| w.contains("手入力")));
    assert!(crate::simulation::simulate(&s, &path, 0.5).unwrap().contacts.iter().all(|c| c.target != "cutter_gouge"));
}

#[test]
fn model_solid_and_blind_hole_drilling_check_the_actual_finished_inner_boundary() {
    let mut s = model_groove_setup();
    let mut d = drill();
    d.tip_end_z_mm = -10.;
    s.operations = vec![op("d", K::Drill, Some(Cutting::Drill(d.clone())))];
    assert!(validate_local(&s, &s.operations[0]).is_err());
    assert!(calculate(&s).is_err());
    let inner = [(0., 20.), (-20., 20.), (-20., 0.), (-60., 0.)].into_iter().map(|(z_mm, diameter_mm)| ProfilePoint { z_mm, diameter_mm }).collect();
    s.drawing = Some(crate::drawing::Drawing::new(&s.od_job.as_ref().unwrap().profile, inner));
    assert!(validate_local(&s, &s.operations[0]).is_ok());
    assert!(calculate(&s).is_ok());
    d.tip_end_z_mm = -25.;
    s.operations[0].cutting = Some(Cutting::Drill(d));
    assert!(validate_local(&s, &s.operations[0]).is_err());
    assert!(calculate(&s).is_err());
}

#[test]
fn local_model_operations_use_shared_limits_instead_of_hidden_legacy_fields() {
    let mut s = model_groove_setup();
    let mut g = groove();
    g.limits.clearance_mm = 7.;
    s.operations = vec![op("gr", K::OdGrooveRough, Some(Cutting::OdGroove(g)))];
    assert!(validate_local(&s, &s.operations[0]).is_ok());
    let path = calculate(&s).unwrap();
    assert_eq!(path.moves.first().unwrap().from.z_mm, 4.);
    s.od_job.as_mut().unwrap().model_source = None;
    let mut d = drill();
    d.limits.chuck_front_z_mm = -100.;
    s.operations = vec![op("drill", K::Drill, Some(Cutting::Drill(d)))];
    assert!(validate_local(&s, &s.operations[0]).is_ok());
    let path = calculate(&s).unwrap();
    assert_eq!(path.moves.first().unwrap().from.z_mm, 4.);
}

#[test]
fn selected_groove_cannot_be_broken_through_by_a_later_drill() {
    let mut s = setup();
    let job = profile(40.);
    let c = super::super::selection::Contour {
        limits: limits(),
        inner: false,
        first: 0,
        last: 1,
        profile_snapshot: job.profile.clone(),
        source_snapshot: None,
        radial_leave_mm: 0.1,
        axial_leave_mm: 0.1,
        depth_mm: 1.,
        feed_mm_rev: 0.1,
    };
    s.od_job = Some(job);
    let mut d = drill();
    d.diameter_mm = 32.;
    s.operations = vec![
        op("groove", K::OdGrooveRough, Some(Cutting::OdGroove(groove()))),
        op("drill", K::Drill, Some(Cutting::Drill(d))),
        op("range", K::OdRough, Some(Cutting::Contour(c))),
    ];
    assert!(validate_local(&s, &s.operations[0]).is_err());
    assert!(calculate(&s).is_err());
    if let Some(Cutting::Drill(d)) = &mut s.operations[1].cutting {
        d.diameter_mm = 20.;
    }
    assert!(calculate(&s).is_ok());
}

#[test]
fn sequence_nc_mapping_includes_each_drill_and_transfer_move_once() {
    let mut s = setup();
    s.operations = vec![op("drill", K::Drill, Some(Cutting::Drill(drill()))), op("groove", K::OdGrooveRough, Some(Cutting::OdGroove(groove())))];
    let path = calculate(&s).unwrap();
    let program = review_program(&s).unwrap();
    assert_eq!(program.text, review_nc(&s).unwrap());
    assert_eq!(program.move_lines.len(), path.moves.len());
    let lines: Vec<_> = program.text.lines().collect();
    let mut previous = None;
    for (movement, line) in path.moves.iter().zip(program.move_lines) {
        let line = line.unwrap();
        assert!(previous.is_none_or(|previous| line > previous));
        assert_eq!(lines[line].starts_with("G00"), movement.motion == Motion::Rapid);
        previous = Some(line);
    }
}

#[test]
fn drilling_pecks_retract_and_conical_tip_does_not_invent_full_depth() {
    let mut s = setup();
    let d = drill();
    s.operations.push(op("drill", K::Drill, Some(Cutting::Drill(d.clone()))));
    let path = calculate(&s).unwrap();
    assert_eq!(path.moves.iter().filter(|m| m.motion == Motion::Feed).count(), 14);
    for pair in path.moves.as_chunks::<2>().0 {
        assert_eq!(pair[0].phase, Phase::Drill);
        assert_eq!(pair[1].to.z_mm, 4.);
        assert_eq!(pair[1].to.diameter_mm, 0.);
    }
    assert_eq!(path.moves.last().unwrap().from.z_mm, -65.);
    let sim = crate::simulation::simulate(&s, &path, 0.1).unwrap();
    let stock = sim.stock_at(path.moves.len());
    assert!((stock.iter().find(|s| s.z0 <= -60. && s.z1 > -60.).unwrap().inner - 5.).abs() < 1e-8);
    let tip = stock.iter().find(|s| s.z0 <= -64. && s.z1 > -64.).unwrap();
    assert!(tip.inner > 0. && tip.inner < 5.);
    assert_eq!(stock.iter().find(|s| s.z0 <= -66. && s.z1 > -66.).unwrap().inner, 0.);
    let nc = review_nc(&s).unwrap();
    assert!(nc.find("M30").unwrap() < nc.find("G01").unwrap());
    assert!(nc.contains("FULL DIA Z"));
}

#[test]
fn groove_rough_keeps_both_walls_and_radial_allowance_then_finishes() {
    let mut s = setup();
    let g = groove();
    s.operations = vec![op("rough", K::OdGrooveRough, Some(Cutting::OdGroove(g.clone())))];
    let rough = calculate(&s).unwrap();
    for m in rough.moves.iter().filter(|m| m.motion == Motion::Feed) {
        assert!(m.to.diameter_mm >= 30.2 - 1e-8);
        assert!(m.to.z_mm <= -10.1 + 1e-8);
        assert!(m.to.z_mm - g.tool_width_mm >= -15.9 - 1e-8);
    }
    let report = crate::simulation::simulate(&s, &rough, 0.1).unwrap();
    let stock = report.stock_at(rough.moves.len());
    assert!((stock.iter().find(|s| s.z0 <= -12. && s.z1 > -12.).unwrap().outer - 15.1).abs() < 1e-8);
    assert_eq!(stock.iter().find(|s| s.z0 <= -10.01 && s.z1 > -10.01).unwrap().outer, 25.);
    s.operations.push(op("finish", K::OdGrooveFinish, Some(Cutting::OdGroove(g))));
    let path = calculate(&s).unwrap();
    let report = crate::simulation::simulate(&s, &path, 0.1).unwrap();
    let stock = report.stock_at(path.moves.len());
    for slab in stock.iter().filter(|s| (s.z0 + s.z1) * 0.5 > -16. && (s.z0 + s.z1) * 0.5 < -10.) {
        assert!((slab.outer - 15.).abs() < 1e-8);
    }
    assert_eq!(path.operation_spans.len(), 2);
    assert!(review_nc(&s).unwrap().contains("OdGrooveFinish"));
}

#[test]
fn full_width_groove_finish_removes_both_wall_allowances_without_overcut() {
    // Exercise both the dimension-only sequence and the model-recess selection replay.
    for model_linked in [false, true] {
        let mut s = if model_linked { model_groove_setup() } else { setup() };
        if let Some(job) = &mut s.od_job {
            for point in &mut job.profile {
                if point.z_mm == -16. {
                    point.z_mm = -14.;
                }
            }
        }
        let rough = Groove { width_mm: 4., ..groove() };
        let finish = Groove { tool_width_mm: 4., ..rough.clone() };
        s.operations = vec![
            op("rough", K::OdGrooveRough, Some(Cutting::OdGroove(rough.clone()))),
            op("finish", K::OdGrooveFinish, Some(Cutting::OdGroove(finish.clone()))),
        ];
        validate_local(&s, &s.operations[1]).unwrap();
        let path = calculate(&s).unwrap();
        let feeds: Vec<_> = path.moves.iter().filter(|m| m.phase == Phase::OdGrooveFinish && m.motion == Motion::Feed).collect();
        assert_eq!(feeds.len(), 2); // Full-width plunge and radial withdrawal; no axial overrun.
        assert!((feeds[0].to.diameter_mm - finish.bottom_diameter_mm).abs() < EPS);
        assert!((feeds[1].from.diameter_mm - finish.bottom_diameter_mm).abs() < EPS);
        for movement in feeds {
            for point in [movement.from, movement.to] {
                assert!((point.z_mm - finish.front_z_mm).abs() < EPS);
                assert!((point.z_mm - finish.tool_width_mm - (finish.front_z_mm - finish.width_mm)).abs() < EPS);
                assert!(point.diameter_mm >= finish.bottom_diameter_mm - EPS);
            }
        }
        let report = crate::simulation::simulate(&s, &path, 0.1).unwrap();
        let finish_start = path.operation_spans.iter().find(|span| span.operation_id == "finish").unwrap().start;
        let before = report.stock_at(finish_start);
        // Roughing preserves the 0.1 mm wall and radial allowances.
        for z in [-10.05, -13.95] {
            assert_eq!(before.iter().find(|slab| slab.z0 <= z && slab.z1 > z).unwrap().outer, 25.);
        }
        assert!((before.iter().find(|slab| slab.z0 <= -12. && slab.z1 > -12.).unwrap().outer - 15.1).abs() < EPS);
        let after = report.stock_at(path.moves.len());
        for slab in after.iter().filter(|slab| {
            let z = (slab.z0 + slab.z1) * 0.5;
            z > -14. && z < -10.
        }) {
            assert!((slab.outer - 15.).abs() < EPS);
        }
        for z in [-9.95, -14.05] {
            assert_eq!(after.iter().find(|slab| slab.z0 <= z && slab.z1 > z).unwrap().outer, 25.);
        }
        assert!(report.contacts.iter().all(|contact| contact.target != "cutter_gouge"));
        assert!(review_nc(&s).unwrap().contains("OdGrooveFinish"));

        let wide = Groove { tool_width_mm: 4.01, ..finish.clone() };
        s.operations[1].cutting = Some(Cutting::OdGroove(wide));
        assert!(validate_local(&s, &s.operations[1]).is_err());
        assert!(calculate(&s).is_err());
        s.operations.truncate(1);
        s.operations[0].cutting = Some(Cutting::OdGroove(finish));
        assert!(validate_local(&s, &s.operations[0]).is_err());
        assert!(calculate(&s).is_err()); // The same full-width tool still cannot leave rough wall stock.
    }
}

#[test]
fn drilling_supplies_bore_only_after_it_reaches_required_id_depth() {
    let mut s = setup();
    s.od_job = Some(profile(40.));
    s.id_job = Some(profile(20.));
    s.operations = vec![
        op("face", K::FaceRough, None),
        op("od", K::OdRough, None),
        op("drill", K::Drill, Some(Cutting::Drill(drill()))),
        op("id", K::IdRough, None),
        op("ff", K::FaceFinish, None),
        op("of", K::OdFinish, None),
        op("if", K::IdFinish, None),
    ];
    let path = calculate(&s).unwrap();
    assert_eq!(path.stock.inner_diameter_mm, 0.);
    let face_min = path.moves.iter().filter(|m| m.phase == Phase::FaceRough).map(|m| m.to.diameter_mm).fold(f64::INFINITY, f64::min);
    assert_eq!(face_min, 0.); // Face runs before the hole exists.
    let ids: Vec<_> = path.operation_spans.iter().map(|s| s.operation_id.as_str()).collect();
    assert_eq!(ids, ["face", "od", "drill", "id", "ff", "of", "if"]);
    assert!(super::super::review_nc(&path).is_ok());
    let mut shallow = s.clone();
    if let Some(Cutting::Drill(d)) = &mut shallow.operations[2].cutting {
        d.tip_end_z_mm = -60.;
    }
    assert!(calculate(&shallow).is_err());
    let mut late = s;
    late.operations.swap(2, 3);
    assert!(calculate(&late).is_err());
}

#[test]
fn invalid_width_unroughed_finish_chuck_and_excessive_passes_are_rejected() {
    let mut s = setup();
    s.operations = vec![op("finish", K::OdGrooveFinish, Some(Cutting::OdGroove(groove())))];
    assert!(calculate(&s).is_err());
    s.operations[0].kind = K::OdGrooveRough;
    let mut wide = groove();
    wide.tool_width_mm = 5.9;
    s.operations[0].cutting = Some(Cutting::OdGroove(wide));
    assert!(calculate(&s).is_err());
    let mut back = groove();
    back.front_z_mm = -70.;
    s.operations[0].cutting = Some(Cutting::OdGroove(back));
    assert!(calculate(&s).is_err());
    let mut tiny = drill();
    tiny.peck_mm = 1e-10;
    s.operations = vec![op("drill", K::Drill, Some(Cutting::Drill(tiny)))];
    assert!(calculate(&s).is_err());
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1., 1e10] {
        let mut d = drill();
        d.diameter_mm = value;
        s.operations[0].cutting = Some(Cutting::Drill(d));
        assert!(calculate(&s).is_err());
    }
}
