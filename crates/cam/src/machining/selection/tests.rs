use super::*;

fn setup() -> Setup {
    let mut s = crate::TurningPlan::nl2500().setups.remove(0);
    s.stock = Some(Stock { outer_diameter_mm: 50., inner_diameter_mm: 10., length_mm: 82. });
    s.od_job = Some(OdJob {
        profile: vec![
            ProfilePoint { z_mm: 0., diameter_mm: 30. },
            ProfilePoint { z_mm: -10., diameter_mm: 30. },
            ProfilePoint { z_mm: -10., diameter_mm: 40. },
            ProfilePoint { z_mm: -60., diameter_mm: 40. },
        ],
        stock_front_z_mm: 2.,
        chuck_front_z_mm: -75.,
        clearance_mm: 2.,
        radial_depth_mm: 1.,
        face_depth_mm: 1.,
        rough_feed_mm_rev: 0.1,
        finish_feed_mm_rev: 0.06,
        finish: true,
        ..OdJob::default()
    });
    s
}
fn contour(s: &Setup, first: usize, last: usize, inner: bool, finish: bool) -> Contour {
    let j = if inner { s.id_job.as_ref() } else { s.od_job.as_ref() }.unwrap();
    Contour {
        limits: Limits { stock_front_z_mm: j.stock_front_z_mm, chuck_front_z_mm: j.chuck_front_z_mm, clearance_mm: j.clearance_mm },
        inner,
        first,
        last,
        profile_snapshot: j.profile.clone(),
        source_snapshot: j.model_source.clone(),
        radial_leave_mm: if finish { 0. } else { 0.1 },
        axial_leave_mm: if finish { 0. } else { 0.1 },
        depth_mm: 0.3,
        feed_mm_rev: 0.1,
    }
}
#[test]
fn selection_cuts_only_its_range_and_preserves_both_allowances() {
    let mut s = setup();
    let c = contour(&s, 0, 3, false, false);
    let p = propose(&s, 12, &c).unwrap();
    s.operations = p.operations;
    let r = remaining(&s).unwrap();
    let at = |z| r.slabs.iter().find(|s| s.z0 < z && s.z1 > z).unwrap();
    assert!((at(-5.05).outer - 15.1).abs() < 1e-8);
    assert!((at(-9.95).outer - 20.1).abs() < 1e-8); // Z0.1 remains on shoulder.
    assert!((at(-59.95).outer - 20.1).abs() < 1e-8); // No thick unroughed rear ring.
    assert_eq!(at(-61.05).outer, 25.);
    assert!(r.slabs.iter().all(|s| s.inner == 5.));
    let c = contour(&setup(), 2, 3, false, false);
    let p = propose(&setup(), 12, &c).unwrap();
    assert!(p.path.moves.iter().filter(|m| m.motion == Motion::Feed).all(|m| m.from.z_mm <= -10. + EPS && m.to.z_mm >= -60. - EPS));
    assert!(p.residual.slabs.iter().filter(|s| s.z0 >= 0.).all(|s| s.outer == 25.));
}
#[test]
fn zero_allowance_proposes_rough_finish_only_when_needed_and_nc_keeps_operations() {
    let mut s = setup();
    let c = contour(&s, 0, 3, false, true);
    let p = propose(&s, 6, &c).unwrap();
    assert!(p.rough_then_finish);
    assert_eq!(p.operations.len(), 2);
    assert_eq!(p.operations[0].station, Some(12));
    assert_eq!(p.operations[1].station, Some(6));
    s.operations = p.operations;
    let nc = sequence::review_nc(&s).unwrap();
    assert!(nc.starts_with("%\nO9001"));
    assert!(nc.contains("M30\n(DISARMED"));
    assert!(nc.contains("STATION T12"));
    assert!(nc.contains("STATION T06"));
    let p = propose(&s, 6, &c).unwrap();
    assert!(!p.rough_then_finish);
    let s = setup();
    let mut bad = s.clone();
    bad.operations = vec![Operation {
        spindle: None,
        id: "finish".into(),
        name: "finish".into(),
        kind: c.kind(),
        station: Some(6),
        cutting: Some(Cutting::Contour(c)),
        manual_separation: None,
    }];
    assert!(calculate(&bad).is_err());
}
#[test]
fn partially_roughed_profile_does_not_authorize_finishing_unselected_stock() {
    let mut s = setup();
    let r = propose(&s, 12, &contour(&s, 2, 3, false, false)).unwrap();
    s.operations = r.operations;
    let p = propose(&s, 6, &contour(&s, 0, 3, false, true)).unwrap();
    assert!(p.rough_then_finish);
    let mut thin = setup();
    thin.stock.as_mut().unwrap().outer_diameter_mm = 40.4;
    let p = propose(&thin, 6, &contour(&thin, 2, 3, false, true)).unwrap();
    assert!(!p.rough_then_finish);
}
#[test]
fn inner_selection_requires_preceding_full_depth_bore() {
    let mut s = setup();
    let mut j = s.od_job.clone().unwrap();
    j.profile = vec![ProfilePoint { z_mm: 0., diameter_mm: 20. }, ProfilePoint { z_mm: -60., diameter_mm: 20. }];
    s.id_job = Some(j);
    let c = contour(&s, 0, 1, true, false);
    let p = propose(&s, 5, &c).unwrap();
    assert!((p.residual.slabs.iter().find(|s| s.z0 < -20.05 && s.z1 > -20.05).unwrap().inner - 9.9).abs() < 1e-8);
    s.stock.as_mut().unwrap().inner_diameter_mm = 0.;
    assert!(propose(&s, 5, &c).is_err());
    let d = sequence::Drill {
        limits: c.limits.clone(),
        diameter_mm: 10.,
        tip_end_z_mm: -65.,
        point_angle_rad: 118f64.to_radians(),
        peck_mm: 5.,
        feed_mm_rev: 0.08,
    };
    s.operations.push(Operation {
        spindle: None,
        id: "drill".into(),
        name: "drill".into(),
        kind: K::Drill,
        station: Some(2),
        cutting: Some(Cutting::Drill(d)),
        manual_separation: None,
    });
    assert!(propose(&s, 5, &c).is_ok());
    if let Some(Cutting::Drill(d)) = &mut s.operations[0].cutting {
        d.tip_end_z_mm = -60.;
    }
    assert!(propose(&s, 5, &c).is_err());
}
#[test]
fn model_changes_hostile_ranges_and_chuck_entry_are_refused() {
    let s = setup();
    let c = contour(&s, 0, 3, false, false);
    let mut changed = s.clone();
    changed.od_job.as_mut().unwrap().profile[1].z_mm = -9.;
    assert!(propose(&changed, 12, &c).is_err());
    for last in [0, 256, usize::MAX] {
        let mut c = c.clone();
        c.last = last;
        assert!(propose(&s, 12, &c).is_err());
    }
    let mut c = c.clone();
    c.feed_mm_rev = f64::NAN;
    assert!(propose(&s, 12, &c).is_err());
    let mut changed = s;
    changed.od_job.as_mut().unwrap().chuck_front_z_mm = -61.;
    let c = contour(&changed, 0, 3, false, false);
    assert!(propose(&changed, 12, &c).is_err());
}
#[test]
fn initial_difference_is_analytic_for_cylinder_and_no_cut_conditions_are_needed() {
    let mut s = setup();
    s.od_job.as_mut().unwrap().profile = vec![ProfilePoint { z_mm: 0., diameter_mm: 40. }, ProfilePoint { z_mm: -60., diameter_mm: 40. }];
    s.od_job.as_mut().unwrap().clearance_mm = 0.;
    let d = difference(&s).unwrap();
    let expected = std::f64::consts::PI * ((25f64.powi(2) - 20f64.powi(2)) * 60. + (25f64.powi(2) - 5f64.powi(2)) * 2.);
    assert!((d.remaining_mm3 - expected).abs() < 1e-6);
}

#[test]
fn face_only_is_allowed_and_finishing_is_not_lost_to_global_job_toggle() {
    let mut s = setup();
    s.od_job.as_mut().unwrap().finish = false;
    let op = |kind: K| Operation {
        spindle: None,
        id: format!("{kind:?}"),
        name: "face".into(),
        kind,
        station: kind.suggested_station(),
        cutting: None,
        manual_separation: None,
    };
    s.operations = vec![op(K::FaceRough), op(K::FaceFinish)];
    let path = sequence::calculate(&s).unwrap();
    assert!(path.moves.iter().any(|m| m.phase == Phase::FaceFinish && m.motion == Motion::Feed));
    assert!(!path.moves.iter().any(|m| matches!(m.phase, Phase::OdRough | Phase::OdFinish)));
    s.operations.remove(0);
    assert!(sequence::calculate(&s).is_err());
}
