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
fn automatic_rough_finish_uses_separate_od_id_rough_conditions_and_keeps_finish_intent() {
    for inner in [false, true] {
        let mut s = setup();
        let od = s.od_job.as_mut().unwrap();
        od.radial_depth_mm = 1.6;
        od.rough_feed_mm_rev = 0.18;
        let mut id = od.clone();
        id.profile = vec![ProfilePoint { z_mm: 0., diameter_mm: 20. }, ProfilePoint { z_mm: -60., diameter_mm: 20. }];
        id.radial_depth_mm = 0.42;
        id.rough_feed_mm_rev = 0.075;
        s.id_job = Some(id);
        let mut c = contour(&s, 0, if inner { 1 } else { 3 }, inner, true);
        c.depth_mm = 0.25;
        c.feed_mm_rev = 0.03;
        let proposal = propose(&s, if inner { 7 } else { 6 }, &c).unwrap();
        assert!(proposal.rough_then_finish);
        assert_eq!(proposal.operations.len(), 2);
        let (expected_depth, expected_feed) = if inner { (0.42, 0.075) } else { (1.6, 0.18) };
        let Some(Cutting::Contour(rough)) = &proposal.operations[0].cutting else { panic!("missing rough contour") };
        assert_eq!((rough.depth_mm, rough.feed_mm_rev), (expected_depth, expected_feed));
        assert_eq!((rough.radial_leave_mm, rough.axial_leave_mm), (0.1, 0.1));
        assert_eq!(proposal.operations[0].station, Some(if inner { 5 } else { 12 }));
        assert_eq!(proposal.operations[1].cutting, Some(Cutting::Contour(c.clone())));
        let phase = if inner { Phase::IdRough } else { Phase::OdRough };
        let feed_moves: Vec<_> = proposal.path.moves.iter().filter(|m| m.phase == phase && m.motion == Motion::Feed).collect();
        assert!(!feed_moves.is_empty());
        assert!(feed_moves.iter().all(|m| m.feed_mm_rev == Some(expected_feed)));
    }
}

#[test]
fn automatic_rough_requires_configured_conditions_but_explicit_rough_keeps_user_values() {
    for inner in [false, true] {
        for missing_depth in [false, true] {
            let mut s = setup();
            let mut id = s.od_job.clone().unwrap();
            id.profile = vec![ProfilePoint { z_mm: 0., diameter_mm: 20. }, ProfilePoint { z_mm: -60., diameter_mm: 20. }];
            s.id_job = Some(id);
            let job = if inner { s.id_job.as_mut() } else { s.od_job.as_mut() }.unwrap();
            if missing_depth {
                job.radial_depth_mm = 0.;
            } else {
                job.rough_feed_mm_rev = 0.;
            }
            let mut c = contour(&s, 0, if inner { 1 } else { 3 }, inner, true);
            let error = propose(&s, if inner { 7 } else { 6 }, &c).unwrap_err().0;
            assert!(error.contains(if inner { "内径荒取り" } else { "外径荒取り" }));
            assert!(error.contains(if missing_depth { "切込み" } else { "送り" }));
            assert!(error.contains("設定"));
            c.radial_leave_mm = 0.1;
            c.axial_leave_mm = 0.1;
            c.depth_mm = 0.47;
            c.feed_mm_rev = 0.087;
            let proposal = propose(&s, if inner { 5 } else { 12 }, &c).unwrap();
            assert!(!proposal.rough_then_finish);
            assert_eq!(proposal.operations.len(), 1);
            assert_eq!(proposal.operations[0].cutting, Some(Cutting::Contour(c)));
        }
    }
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
    assert!(d.coverage.shortages.is_empty());
    assert!(!d.coverage.inner_boundary_known);
}

fn known_tube(outer: Vec<ProfilePoint>, inner: Vec<ProfilePoint>) -> Setup {
    let mut s = setup();
    s.od_job.as_mut().unwrap().profile = outer;
    let mut id = s.od_job.as_ref().unwrap().clone();
    id.profile = inner;
    s.id_job = Some(id);
    s
}

#[test]
fn stock_coverage_finds_oversized_bore_at_exact_taper_crossing() {
    let mut s = known_tube(
        vec![ProfilePoint { z_mm: 0., diameter_mm: 40. }, ProfilePoint { z_mm: -60., diameter_mm: 40. }],
        vec![ProfilePoint { z_mm: 0., diameter_mm: 20. }, ProfilePoint { z_mm: -60., diameter_mm: 16. }],
    );
    s.stock.as_mut().unwrap().inner_diameter_mm = 18.;
    let coverage = stock_coverage(&s).unwrap();
    assert!(coverage.inner_boundary_known && coverage.outer_boundary_known);
    assert_eq!(coverage.shortages.len(), 1);
    let shortage = &coverage.shortages[0];
    assert_eq!(shortage.kind, ShortageKind::InnerDiameter);
    assert_eq!((shortage.z0_mm, shortage.z1_mm), (-60., -30.));
    assert_eq!((shortage.inner0_mm, shortage.inner1_mm), (8., 9.));
    assert_eq!((shortage.outer0_mm, shortage.outer1_mm), (9., 9.));
    assert!((coverage.missing_volume_mm3 - 260. * std::f64::consts::PI).abs() < 1e-7);
    let difference = difference(&s).unwrap();
    assert_eq!(difference.coverage.missing_volume_mm3, coverage.missing_volume_mm3);
    assert!(difference.remaining_mm3 > 0., "material to remove and material missing are independent");
    assert!(propose(&s, 12, &contour(&s, 0, 1, false, false)).unwrap_err().to_string().contains("材料不足"));
    assert!(sequence::calculate(&s).unwrap_err().to_string().contains("材料不足"));
}

#[test]
fn stock_coverage_detects_sub_grid_outer_taper_without_false_exact_fit_warning() {
    let mut s = known_tube(
        vec![ProfilePoint { z_mm: 0., diameter_mm: 40. }, ProfilePoint { z_mm: -60., diameter_mm: 40. }],
        vec![ProfilePoint { z_mm: 0., diameter_mm: 16. }, ProfilePoint { z_mm: -60., diameter_mm: 16. }],
    );
    s.stock = Some(Stock { outer_diameter_mm: 40., inner_diameter_mm: 16., length_mm: 60. });
    s.od_job.as_mut().unwrap().stock_front_z_mm = 0.;
    let fit = stock_coverage(&s).unwrap();
    assert!(fit.shortages.is_empty());
    assert_eq!(fit.missing_volume_mm3, 0.);
    s.stock.as_mut().unwrap().outer_diameter_mm = 50.;
    s.od_job.as_mut().unwrap().profile = vec![
        ProfilePoint { z_mm: 0., diameter_mm: 40. },
        ProfilePoint { z_mm: -59.999, diameter_mm: 40. },
        ProfilePoint { z_mm: -60., diameter_mm: 50.2 },
    ];
    let coverage = stock_coverage(&s).unwrap();
    assert_eq!(coverage.shortages.len(), 1);
    let missing = &coverage.shortages[0];
    assert_eq!(missing.kind, ShortageKind::OuterDiameter);
    assert_eq!(missing.z0_mm, -60.);
    assert!(missing.z1_mm > missing.z0_mm && missing.z1_mm - missing.z0_mm < 0.001);
    assert!((missing.outer1_mm - 25.).abs() < 1e-7);
    assert!(coverage.missing_volume_mm3 > 0.);
}

#[test]
fn stock_coverage_splits_front_and_back_planes_and_does_not_double_count_radial_loss() {
    let mut s = known_tube(
        vec![ProfilePoint { z_mm: 3., diameter_mm: 40. }, ProfilePoint { z_mm: -60., diameter_mm: 40. }],
        vec![ProfilePoint { z_mm: 3., diameter_mm: 10. }, ProfilePoint { z_mm: -60., diameter_mm: 10. }],
    );
    s.stock = Some(Stock { outer_diameter_mm: 38., inner_diameter_mm: 12., length_mm: 52. });
    // Stock occupies -50..2; one mm of the front and ten mm of the back are entirely absent.
    let coverage = stock_coverage(&s).unwrap();
    assert_eq!(coverage.shortages.iter().filter(|s| s.kind == ShortageKind::Front).count(), 1);
    assert_eq!(coverage.shortages.iter().filter(|s| s.kind == ShortageKind::Back).count(), 1);
    let expected = std::f64::consts::PI * (11. * (400. - 25.) + 52. * ((400. - 361.) + (36. - 25.)));
    assert!((coverage.missing_volume_mm3 - expected).abs() < 1e-7);
    assert!(coverage.shortages.iter().any(|s| s.kind == ShortageKind::Front && s.z0_mm == 2. && s.z1_mm == 3.));
    assert!(coverage.shortages.iter().any(|s| s.kind == ShortageKind::Back && s.z0_mm == -60. && s.z1_mm == -50.));
}

#[test]
fn stock_coverage_respects_bore_shoulders_and_blind_section_axis() {
    let mut s = known_tube(
        vec![ProfilePoint { z_mm: 0., diameter_mm: 40. }, ProfilePoint { z_mm: -60., diameter_mm: 40. }],
        vec![
            ProfilePoint { z_mm: 0., diameter_mm: 20. },
            ProfilePoint { z_mm: -20., diameter_mm: 20. },
            ProfilePoint { z_mm: -20., diameter_mm: 16. },
            ProfilePoint { z_mm: -60., diameter_mm: 16. },
        ],
    );
    s.stock.as_mut().unwrap().inner_diameter_mm = 18.;
    let coverage = stock_coverage(&s).unwrap();
    assert_eq!(coverage.shortages.len(), 1);
    assert_eq!((coverage.shortages[0].z0_mm, coverage.shortages[0].z1_mm), (-60., -20.));
    assert!((coverage.missing_volume_mm3 - 680. * std::f64::consts::PI).abs() < 1e-7);

    // A blind section is display-only for machining but its solid back still needs stock.
    s.id_job = None;
    s.stock.as_mut().unwrap().inner_diameter_mm = 10.;
    s.drawing = Some(crate::drawing::Drawing::new(
        &s.od_job.as_ref().unwrap().profile,
        vec![
            ProfilePoint { z_mm: 0., diameter_mm: 20. },
            ProfilePoint { z_mm: -20., diameter_mm: 20. },
            ProfilePoint { z_mm: -20., diameter_mm: 0. },
            ProfilePoint { z_mm: -60., diameter_mm: 0. },
        ],
    ));
    let blind = stock_coverage(&s).unwrap();
    assert!(blind.inner_boundary_known);
    assert!((blind.missing_volume_mm3 - 1000. * std::f64::consts::PI).abs() < 1e-7);
    s.drawing = Some(crate::drawing::Drawing::new(
        &s.od_job.as_ref().unwrap().profile,
        vec![ProfilePoint { z_mm: 0., diameter_mm: 0. }, ProfilePoint { z_mm: -60., diameter_mm: 0. }],
    ));
    assert!((stock_coverage(&s).unwrap().missing_volume_mm3 - 1500. * std::f64::consts::PI).abs() < 1e-7);
}

#[test]
fn stock_coverage_rejects_hostile_geometry_and_does_not_claim_unknown_boundaries() {
    let mut s = setup();
    for n in [f64::NAN, f64::INFINITY, -1., 1e7] {
        s.stock.as_mut().unwrap().outer_diameter_mm = n;
        assert!(stock_coverage(&s).is_err());
    }
    s = setup();
    s.od_job.as_mut().unwrap().profile[0].z_mm = f64::NAN;
    assert!(stock_coverage(&s).is_err());
    s = setup();
    s.od_job.as_mut().unwrap().profile.reverse();
    assert!(stock_coverage(&s).is_err());
    s = setup();
    s.od_job.as_mut().unwrap().profile = vec![ProfilePoint { z_mm: 0., diameter_mm: 40. }; 257];
    assert!(stock_coverage(&s).is_err());
    s = setup();
    s.od_job = None;
    assert!(!stock_coverage(&s).unwrap().outer_boundary_known);
}

#[test]
fn stock_coverage_rejects_truncated_inner_boundaries_instead_of_hiding_missing_material() {
    let mut s = known_tube(
        vec![ProfilePoint { z_mm: 0., diameter_mm: 40. }, ProfilePoint { z_mm: -40., diameter_mm: 40. }],
        vec![
            ProfilePoint { z_mm: 0., diameter_mm: 20. },
            ProfilePoint { z_mm: -40., diameter_mm: 20. },
            ProfilePoint { z_mm: -60., diameter_mm: 16. },
        ],
    );
    s.stock.as_mut().unwrap().inner_diameter_mm = 18.;
    // The sole shortage is in the known bore beyond the OD; do not report coverage.
    assert!(stock_coverage(&s).unwrap_err().to_string().contains("前端・後端"));
    assert!(propose(&s, 12, &contour(&s, 0, 1, false, false)).is_err());
    assert!(difference(&s).is_err());

    // A truncated closed section must not become an unspecified legacy bore either.
    s.id_job = None;
    s.drawing = Some(crate::drawing::Drawing::new(
        &s.od_job.as_ref().unwrap().profile,
        vec![ProfilePoint { z_mm: 0., diameter_mm: 20. }, ProfilePoint { z_mm: -20., diameter_mm: 20. }],
    ));
    assert!(stock_coverage(&s).is_err());
    s.drawing.as_mut().unwrap().inner = vec![ProfilePoint { z_mm: -1., diameter_mm: 20. }, ProfilePoint { z_mm: -40., diameter_mm: 20. }];
    assert!(stock_coverage(&s).is_err());
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

#[test]
fn blind_hole_without_id_job_remains_in_residual_after_od_finishing() {
    for depth in [20., 0.03] {
        let mut s = setup();
        s.stock.as_mut().unwrap().inner_diameter_mm = 0.;
        let job = s.od_job.as_mut().unwrap();
        job.stock_front_z_mm = 0.;
        job.profile = vec![ProfilePoint { z_mm: 0., diameter_mm: 40. }, ProfilePoint { z_mm: -60., diameter_mm: 40. }];
        s.drawing = Some(crate::drawing::Drawing::new(
            &job.profile,
            vec![
                ProfilePoint { z_mm: 0., diameter_mm: 20. },
                ProfilePoint { z_mm: -depth, diameter_mm: 20. },
                ProfilePoint { z_mm: -depth, diameter_mm: 0. },
                ProfilePoint { z_mm: -60., diameter_mm: 0. },
            ],
        ));
        assert!(s.id_job.is_none());
        let proposal = propose(&s, 6, &contour(&s, 0, 1, false, true)).unwrap();
        s.operations = proposal.operations;
        let r = remaining(&s).unwrap();
        assert!((r.remaining_mm3 - 100. * std::f64::consts::PI * depth).abs() < 1e-6, "depth={depth}, remaining={}", r.remaining_mm3);
        assert!(r.slabs.iter().any(|slab| (slab.z0 + depth).abs() < EPS || (slab.z1 + depth).abs() < EPS));
        assert!(r.slabs.iter().all(|slab| slab.inner == 0.)); // OD machining did not invent a drilled hole.
    }
}

#[test]
fn selected_od_and_whole_id_cannot_use_different_chuck_or_transfer_planes() {
    let mut s = setup();
    let od = s.od_job.as_mut().unwrap();
    od.profile = vec![
        ProfilePoint { z_mm: 0., diameter_mm: 40. },
        ProfilePoint { z_mm: -20., diameter_mm: 40. },
        ProfilePoint { z_mm: -60., diameter_mm: 40. },
    ];
    od.chuck_front_z_mm = -40.;
    let mut id = od.clone();
    id.profile = vec![ProfilePoint { z_mm: 0., diameter_mm: 20. }, ProfilePoint { z_mm: -60., diameter_mm: 20. }];
    id.chuck_front_z_mm = -100.;
    s.id_job = Some(id);
    let c = contour(&s, 0, 1, false, false);
    s.operations = vec![
        Operation {
            id: "od-range".into(),
            name: "OD front range".into(),
            kind: K::OdRough,
            station: Some(12),
            cutting: Some(Cutting::Contour(c)),
            spindle: None,
            manual_separation: None,
        },
        Operation {
            id: "whole-id".into(),
            name: "ID entire profile".into(),
            kind: K::IdRough,
            station: Some(5),
            cutting: None,
            spindle: None,
            manual_separation: None,
        },
    ];
    assert!(calculate(&s).unwrap_err().0.contains("共通配置"));
    assert!(sequence::calculate(&s).unwrap_err().0.contains("共通配置"));
    s.id_job.as_mut().unwrap().chuck_front_z_mm = -40.;
    assert!(calculate(&s).is_err()); // Same placement still cannot cut a profile behind the chuck.
    s.od_job.as_mut().unwrap().chuck_front_z_mm = -75.;
    s.id_job.as_mut().unwrap().chuck_front_z_mm = -75.;
    if let Some(Cutting::Contour(c)) = &mut s.operations[0].cutting {
        c.limits.chuck_front_z_mm = -75.;
    }
    assert!(calculate(&s).is_ok());
    for front in [true, false] {
        let mut changed = s.clone();
        let id = changed.id_job.as_mut().unwrap();
        if front {
            id.stock_front_z_mm += 1.;
        } else {
            id.clearance_mm += 1.;
        }
        assert!(calculate(&changed).unwrap_err().0.contains("共通配置"));
    }
}
