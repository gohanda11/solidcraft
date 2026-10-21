use super::*;

fn fixture() -> (OdJob, Stock) {
    let job = OdJob {
        profile: [(0., 30.), (-20., 30.), (-20., 20.), (-40., 20.), (-60., 16.)]
            .map(|(z_mm, diameter_mm)| ProfilePoint { z_mm, diameter_mm })
            .to_vec(),
        stock_front_z_mm: 2.,
        chuck_front_z_mm: -70.,
        radial_depth_mm: 1.,
        face_depth_mm: 1.,
        clearance_mm: 2.,
        rough_feed_mm_rev: 0.2,
        finish_feed_mm_rev: 0.1,
        finish: true,
        ..OdJob::default()
    };
    (job, Stock { outer_diameter_mm: 50., inner_diameter_mm: 10., length_mm: 82. })
}

#[test]
fn through_id_leaves_radial_and_shoulder_stock_and_retracts_inside_prebore() {
    let (job, stock) = fixture();
    let path = calculate(&job, &stock).unwrap();
    assert!(path.inner);
    assert!(path.allowance_audit.iter().all(|r| r.radial_remaining_mm >= 0.1 - 1e-8));
    assert!(path.rough_profile.iter().any(|p| (p.z_mm + 19.9).abs() < 1e-8 && (p.diameter_mm - 29.8).abs() < 1e-8));
    assert!(path.rough_profile.iter().any(|p| (p.z_mm + 19.9).abs() < 1e-8 && (p.diameter_mm - 19.8).abs() < 1e-8));
    // Independent stock replay at axial stations; ID is empty space below the diameter.
    for n in 0..=600 {
        let z = -(n as f64) * 0.1;
        let mut cleared: f64 = stock.inner_diameter_mm;
        let mut rough_cleared = cleared;
        for m in &path.moves {
            let lo = m.from.z_mm.min(m.to.z_mm);
            let hi = m.from.z_mm.max(m.to.z_mm);
            if z < lo - 1e-9 || z > hi + 1e-9 {
                continue;
            }
            let dia = if hi - lo < 1e-9 {
                m.from.diameter_mm.max(m.to.diameter_mm)
            } else {
                m.from.diameter_mm + (m.to.diameter_mm - m.from.diameter_mm) * (z - m.from.z_mm) / (m.to.z_mm - m.from.z_mm)
            };
            if m.motion == Motion::Rapid {
                assert!(dia <= cleared + 1e-7, "rapid into wall at Z{z}: {m:?}");
            } else {
                cleared = cleared.max(dia);
            }
            if !is_finish(m.phase) {
                rough_cleared = cleared;
            }
        }
        // Exact nominal bore: front 30, rear 20, then taper to 16.
        let target = if z >= -20. {
            30.
        } else if z >= -40. {
            20.
        } else {
            20. + (z + 40.) * 0.2
        };
        assert!(cleared <= target + 1e-7, "gouge at {z}");
        assert!((cleared - target).abs() < 1e-7, "uncut finish at {z}: {cleared}");
        if (z + 20.).abs() > 0.11 {
            assert!(rough_cleared <= target - 0.2 + 1e-7);
        }
    }
    let nc = review_nc(&path).unwrap();
    assert!(nc.find("M30").unwrap() < nc.find("G00").unwrap());
    assert!(nc.contains("IdRough") && nc.contains("IdFinish"));
    assert!(!nc.contains("FaceRough"));
    assert!(nc.contains("G01 X30.0000 Z-20.0000"));
}

#[test]
fn through_id_refuses_missing_oversized_bore_undercut_and_chuck_entry() {
    let (job, stock) = fixture();
    for bore in [0., 4., 15.9, 30., f64::NAN] {
        let mut s = stock.clone();
        s.inner_diameter_mm = bore;
        assert!(calculate(&job, &s).is_err(), "bore {bore}");
    }
    let mut bad = job.clone();
    bad.profile[4].diameter_mm = 25.;
    assert!(calculate(&bad, &stock).is_err());
    bad = job.clone();
    bad.chuck_front_z_mm = -61.;
    assert!(calculate(&bad, &stock).is_err());
    bad = job;
    bad.radial_depth_mm = 0.;
    assert!(calculate(&bad, &stock).is_err());
}

#[test]
fn combined_orders_all_rough_before_finish_and_has_continuous_safe_transfers() {
    let (id, stock) = fixture();
    let mut od = id.clone();
    od.profile = vec![ProfilePoint { z_mm: 0., diameter_mm: 40. }, ProfilePoint { z_mm: -60., diameter_mm: 40. }];
    let path = calculate_setup(Some(&od), Some(&id), &stock).unwrap();
    let mut finishing = false;
    for m in &path.moves {
        if is_finish(m.phase) {
            finishing = true;
        }
        if m.phase != Phase::Transfer && !is_finish(m.phase) {
            assert!(!finishing);
        }
        if m.phase == Phase::Transfer {
            assert_eq!(m.from.z_mm, 4.);
            assert_eq!(m.to.z_mm, 4.);
        }
    }
    for pair in path.moves.windows(2) {
        assert_eq!(pair[0].to, pair[1].from);
    }
    let expected_initial = std::f64::consts::PI / 4. * (50. * 50. - 10. * 10.) * 62.;
    let final_outer = std::f64::consts::PI / 4. * 40. * 40. * 60.;
    let final_hole = std::f64::consts::PI / 4. * (30. * 30. * 20. + 20. * 20. * 20. + 20. * (20. * 20. + 20. * 16. + 16. * 16.) / 3.);
    assert!((path.rough_removed_volume_mm3 + path.finish_removed_volume_mm3 - (expected_initial - final_outer + final_hole)).abs() < 1e-6);
    let nc = review_nc(&path).unwrap();
    assert!(nc.find("IdRough").unwrap() < nc.find("FaceFinish").unwrap());
    assert!(nc.find("OdFinish").unwrap() < nc.find("IdFinish").unwrap());
    od.profile[0].diameter_mm = 25.;
    assert!(calculate_setup(Some(&od), Some(&id), &stock).is_err());
}

#[test]
fn id_nc_quantization_is_inward_and_regenerates_instead_of_trusting_cached_moves() {
    let (mut job, mut stock) = fixture();
    stock.inner_diameter_mm = 10.123456;
    job.clearance_mm = 1.012345;
    job.stock_front_z_mm = 2.123456;
    for p in &mut job.profile {
        p.diameter_mm += 0.123456;
    }
    let mut path = calculate(&job, &stock).unwrap();
    let nc = review_nc(&path).unwrap();
    let mut at = rounded(path.moves.first().unwrap().from, true);
    let lines: Vec<_> = nc.lines().filter(|s| s.starts_with("G00") || s.starts_with("G01")).collect();
    assert_eq!(lines.len(), path.moves.len());
    for (line, m) in lines.iter().zip(&path.moves) {
        for word in line.split_whitespace().skip(1) {
            let v: f64 = word[1..].parse().unwrap();
            match &word[..1] {
                "X" => at.diameter_mm = v,
                "Z" => at.z_mm = v,
                "F" => {}
                _ => panic!("unexpected word"),
            }
        }
        assert!(at.diameter_mm <= m.to.diameter_mm + 1e-8);
        assert!(m.to.diameter_mm - at.diameter_mm < 0.0001001);
        assert!(at.z_mm >= m.to.z_mm - 1e-8 && at.z_mm - m.to.z_mm < 0.0001001);
    }
    path.moves.clear();
    path.rough_profile.clear();
    assert_eq!(review_nc(&path).unwrap(), nc);
}
