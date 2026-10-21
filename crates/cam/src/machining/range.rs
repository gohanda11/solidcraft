//! Clip an already ordered profile without changing its saved model association.
use super::*;

impl OdJob {
    pub fn clipped(&self, end: f64) -> Result<Self> {
        self.validate_draft()?;
        let Some(last) = self.profile.last() else { return invalid("read a profile before selecting its end") };
        if !end.is_finite() || end > -0.001 || end < last.z_mm - EPS || end < -1e6 {
            return invalid("cut end must lie within the profile, behind the finished front face");
        }
        if self.profile.windows(2).any(|w| w.first().zip(w.get(1)).is_some_and(|(a, b)| b.z_mm > a.z_mm)) {
            return invalid("profile must run from the front towards negative Z");
        }
        let mut clipped = self.clone();
        clipped.profile = self.profile.iter().copied().take_while(|p| p.z_mm >= end).collect();
        if clipped.profile.last().is_none_or(|p| (p.z_mm - end).abs() > EPS) {
            let Some((a, b)) = self.profile.windows(2).find_map(|w| {
                let (a, b) = w.first().zip(w.get(1))?;
                (a.z_mm > end && b.z_mm < end).then_some((a, b))
            }) else {
                return invalid("cut end does not intersect the profile");
            };
            let t = (end - a.z_mm) / (b.z_mm - a.z_mm);
            clipped.profile.push(ProfilePoint { z_mm: end, diameter_mm: a.diameter_mm + t * (b.diameter_mm - a.diameter_mm) });
        }
        if clipped.profile.len() < 2 {
            return invalid("cut range is empty");
        }
        Ok(clipped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clipping_interpolates_tapers_and_preserves_shoulders() {
        let job = OdJob {
            profile: vec![
                ProfilePoint { z_mm: 0., diameter_mm: 20. },
                ProfilePoint { z_mm: -10., diameter_mm: 30. },
                ProfilePoint { z_mm: -10., diameter_mm: 40. },
                ProfilePoint { z_mm: -20., diameter_mm: 40. },
            ],
            ..Default::default()
        };
        assert_eq!(job.clipped(-5.).unwrap().profile.last().unwrap().diameter_mm, 25.);
        assert_eq!(job.clipped(-10.).unwrap().profile.len(), 3);
        assert_eq!(job.profile.len(), 4);
        for end in [0., 1., -21., f64::NAN, f64::INFINITY] {
            assert!(job.clipped(end).is_err());
        }
    }
}
