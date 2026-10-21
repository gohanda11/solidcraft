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
    crate::Operation { id: id.into(), name: id.into(), kind, station: kind.suggested_station(), cutting, manual_separation: None }
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
