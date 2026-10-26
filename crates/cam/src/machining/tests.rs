use super::*;

fn fixture() -> (OdJob, Stock) {
    let job = OdJob {
        model_source: None,
        profile: vec![p(0.0, 20.0), p(-20.0, 20.0), p(-20.0, 40.0), p(-60.0, 40.0)],
        stock_front_z_mm: 2.0,
        chuck_front_z_mm: -80.0,
        radial_stock_to_leave_mm: 0.2,
        axial_stock_to_leave_mm: 0.3,
        radial_depth_mm: 2.0,
        face_depth_mm: 1.0,
        clearance_mm: 2.0,
        rough_feed_mm_rev: 0.2,
        finish_feed_mm_rev: 0.1,
        finish: true,
    };
    (job, Stock { outer_diameter_mm: 60.0, inner_diameter_mm: 0.0, length_mm: 90.0 })
}
fn p(z: f64, d: f64) -> ProfilePoint {
    ProfilePoint { z_mm: z, diameter_mm: d }
}

#[test]
fn all_roughing_precedes_both_finish_phases_and_default_allowance_is_point_one() {
    let defaults = OdJob::default();
    near(defaults.radial_stock_to_leave_mm, 0.1);
    near(defaults.axial_stock_to_leave_mm, 0.1);
    let (mut job, stock) = fixture();
    job.radial_stock_to_leave_mm = defaults.radial_stock_to_leave_mm;
    job.axial_stock_to_leave_mm = defaults.axial_stock_to_leave_mm;
    let path = calculate(&job, &stock).unwrap();
    let mut phases: Vec<_> = path.moves.iter().map(|m| m.phase).collect();
    phases.dedup();
    assert_eq!(phases, [Phase::FaceRough, Phase::OdRough, Phase::RoughContour, Phase::FaceFinish, Phase::OdFinish]);
    near(diameter_at(&path.rough_profile, -10.0).unwrap(), 20.2);
    near(path.rough_profile.first().unwrap().z_mm, 0.1);
    let nc = review_nc(&path).unwrap();
    assert!(nc.find("(RoughContour)").unwrap() < nc.find("(FaceFinish)").unwrap());
    assert!(nc.find("(FaceFinish)").unwrap() < nc.find("(OdFinish)").unwrap());
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-7, "{a} != {b}");
}

#[test]
fn allowance_is_radial_and_axial_shoulders_do_not_shorten_the_back_cut() {
    let (job, stock) = fixture();
    let path = calculate(&job, &stock).unwrap();
    near(diameter_at(&path.rough_profile, -10.0).unwrap(), 20.4);
    near(diameter_at(&path.rough_profile, -19.8).unwrap(), 40.4);
    near(path.rough_profile.last().unwrap().z_mm, -60.0);
    near(diameter_at(&path.rough_profile, -59.9).unwrap(), 40.4);
    // Analytic volumes of two cylinders, including the front allowance and shifted shoulder.
    near(path.finish_removed_volume_mm3, std::f64::consts::PI * 524.812);
    let final_material = std::f64::consts::PI * (10.0_f64.powi(2) * 20.0 + 20.0_f64.powi(2) * 40.0);
    near(path.rough_removed_volume_mm3 + path.finish_removed_volume_mm3, std::f64::consts::PI * 30.0_f64.powi(2) * 62.0 - final_material);
    assert!(path.allowance_audit.iter().any(|r| r.radial_remaining_mm > 10.0));
}

#[test]
fn taper_residual_includes_the_axial_allowance_component() {
    let (mut job, stock) = fixture();
    job.profile = vec![p(0.0, 20.0), p(-60.0, 40.0)];
    let path = calculate(&job, &stock).unwrap();
    near((diameter_at(&path.rough_profile, -30.0).unwrap() - 30.0) / 2.0, 0.25);
    // Radial 0.2 + axial 0.3 * (radial slope 10 / 60) = 0.25.
    near(material_volume(&job.profile, 0.0), std::f64::consts::PI * 60.0 * (100.0 + 200.0 + 400.0) / 3.0);
}

// Independent in-process stock envelope from cuts, not from the planner's rough-profile report.
fn stock_diameter_at(z: f64, stock: &Stock, job: &OdJob, previous: &[Move]) -> f64 {
    if z > job.stock_front_z_mm || z < job.stock_front_z_mm - stock.length_mm {
        return 0.0;
    }
    let mut d = stock.outer_diameter_mm;
    for m in previous.iter().filter(|m| m.motion == Motion::Feed) {
        if matches!(m.phase, Phase::FaceRough | Phase::FaceFinish)
            && (m.from.z_mm - m.to.z_mm).abs() < EPS
            && m.to.diameter_mm <= stock.inner_diameter_mm + EPS
            && z > m.to.z_mm + EPS
        {
            return 0.0;
        }
        let lo = m.from.z_mm.min(m.to.z_mm);
        let hi = m.from.z_mm.max(m.to.z_mm);
        if hi - lo > EPS && z >= lo - EPS && z <= hi + EPS {
            let t = (z - m.from.z_mm) / (m.to.z_mm - m.from.z_mm);
            d = d.min(m.from.diameter_mm + t * (m.to.diameter_mm - m.from.diameter_mm));
        }
    }
    d
}

#[test]
fn independent_stock_replay_checks_rapid_clearance_gouging_and_remaining_stock() {
    for taper in [false, true] {
        let (mut job, stock) = fixture();
        if taper {
            job.profile = vec![p(0.0, 20.0), p(-60.0, 40.0)];
        }
        let path = calculate(&job, &stock).unwrap();
        for (index, m) in path.moves.iter().enumerate() {
            for step in 1..=31 {
                let t = step as f64 / 31.0;
                let z = m.from.z_mm + t * (m.to.z_mm - m.from.z_mm);
                let d = m.from.diameter_mm + t * (m.to.diameter_mm - m.from.diameter_mm);
                if m.motion == Motion::Rapid {
                    let material = stock_diameter_at(z, &stock, &job, &path.moves[..index]);
                    assert!(d >= material - 1e-7, "rapid crosses stock at Z{z} X{d}, material X{material}, move {index}");
                }
                if z < -1e-6 && z > -60.0 + 1e-6 {
                    // At a shoulder the lower OD bounds the exposed end face.
                    let a = diameter_at(&job.profile, z + 1e-7).unwrap();
                    let b = diameter_at(&job.profile, z - 1e-7).unwrap();
                    assert!(d >= a.min(b) - 1e-6, "nominal tip gouges finished part at {z} {d}");
                }
            }
        }
        let rough_end = path.moves.iter().position(|m| matches!(m.phase, Phase::FaceFinish | Phase::OdFinish)).unwrap();
        for n in 0..600 {
            let z = -(n as f64 + 0.37) / 10.0;
            near(stock_diameter_at(z, &stock, &job, &path.moves[..rough_end]), diameter_at(&path.rough_profile, z).unwrap());
            near(stock_diameter_at(z, &stock, &job, &path.moves), diameter_at(&job.profile, z).unwrap());
        }
    }
}

#[test]
fn invalid_stock_undercuts_and_unbounded_conditions_are_refused() {
    let (base, stock) = fixture();
    for field in 0..10 {
        let mut job = base.clone();
        match field {
            0 => job.radial_depth_mm = 0.0,
            1 => job.rough_feed_mm_rev = f64::NAN,
            2 => job.profile[2].diameter_mm = 10.0,
            3 => job.radial_stock_to_leave_mm = 20.0,
            4 => job.axial_stock_to_leave_mm = 3.0,
            5 => job.chuck_front_z_mm = -59.0,
            6 => job.profile[3].z_mm = -100.0,
            7 => job.profile[0].z_mm = 1.0,
            8 => job.profile[2].z_mm = -10.0,
            _ => job.radial_depth_mm = 0.001,
        }
        assert!(calculate(&job, &stock).is_err(), "case {field}");
    }
}

#[test]
fn standalone_od_paths_and_nc_refuse_proven_stock_shortages_without_a_setup() {
    let (mut job, stock) = fixture();
    job.radial_stock_to_leave_mm = 0.;
    job.profile = vec![p(0., 20.), p(-59.999, 40.), p(-60., 60.01)];
    assert!(calculate(&job, &stock).is_err(), "a narrow rear taper exceeding the OD is still missing material");
    let (job, stock) = fixture();
    let mut path = calculate(&job, &stock).unwrap();
    assert!(path.source_setup.is_none());
    path.stock.length_mm = 40.;
    assert!(review_nc(&path).is_err(), "standalone NC must recheck axial stock instead of trusting cached moves");
    path.stock = stock;
    path.stock.inner_diameter_mm = 21.;
    assert!(review_nc(&path).is_err(), "an OD inside the original bore has no stock");
}

#[test]
fn zero_allowance_and_zero_front_stock_do_not_add_a_face_cut() {
    let (mut job, stock) = fixture();
    job.radial_stock_to_leave_mm = 0.0;
    job.axial_stock_to_leave_mm = 0.0;
    job.stock_front_z_mm = 0.0;
    let path = calculate(&job, &stock).unwrap();
    assert!(!path.moves.iter().any(|m| matches!(m.phase, Phase::FaceRough | Phase::FaceFinish)));
    near(path.finish_removed_volume_mm3, 0.0);
    assert!(path.allowance_audit.iter().all(|r| r.radial_remaining_mm.abs() < EPS));
}

#[test]
fn rough_only_leaves_allowance_and_preserves_an_existing_bore() {
    let (mut job, mut stock) = fixture();
    job.finish = false;
    stock.inner_diameter_mm = 10.0;
    let path = calculate(&job, &stock).unwrap();
    assert!(!path.moves.iter().any(|m| matches!(m.phase, Phase::FaceFinish | Phase::OdFinish)));
    assert!(path.moves.iter().all(|m| m.to.diameter_mm >= 10.0));
    near(path.finish_removed_volume_mm3, 0.0);
}

#[test]
fn nc_fragment_roundtrips_motion_and_cannot_hide_changed_output() {
    let (job, stock) = fixture();
    let mut path = calculate(&job, &stock).unwrap();
    let nc = review_nc(&path).unwrap();
    assert!(nc.find("M30").unwrap() < nc.find("G00").unwrap());
    assert!(!nc.contains("G71") && !nc.contains("M03"));
    let mut at = path.moves.first().unwrap().from;
    let lines: Vec<_> = nc.lines().filter(|s| s.starts_with("G00") || s.starts_with("G01")).collect();
    assert_eq!(lines.len(), path.moves.len());
    for (line, m) in lines.iter().zip(&path.moves) {
        for word in line.split_whitespace().skip(1) {
            let v: f64 = word[1..].parse().unwrap();
            match &word[..1] {
                "X" => at.diameter_mm = v,
                "Z" => at.z_mm = v,
                "F" => near(v, m.feed_mm_rev.unwrap()),
                _ => panic!("unexpected word"),
            }
        }
        assert!((at.z_mm - m.to.z_mm).abs() <= 0.0001001);
        assert!((at.diameter_mm - m.to.diameter_mm).abs() <= 0.0001001);
    }
    path.moves.clear();
    assert_eq!(review_nc(&path).unwrap(), nc, "export must regenerate, never trust cached moves");
}
