//! Small, manually checked dimensional reference set from Kyocera 2024–2026 catalogues.
//! Numeric facts only; no manufacturer CAD, artwork, chipbreaker geometry or grade recommendations.
use crate::{Result, ToolRole, ToolSlot, insert::Insert, invalid};
use serde::{Deserialize, Serialize};

pub const CHECKED: &str = "2026-10-18";
pub const CAD_URL: &str = "https://kyocera.salessupportserver.com/asp-productive/Account/CustomerAutoLogin";
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Seat {
    Tc09,
    Tc11,
    Tc16,
    Tn16,
}
#[derive(Clone, Debug, Serialize)]
pub struct InsertRecord {
    pub model: &'static str,
    pub seat: Seat,
    pub ic: f64,
    pub thickness: f64,
    pub hole: f64,
    pub nose: f64,
    pub clearance_deg: f64,
    pub page: u16,
}
#[derive(Clone, Debug, Serialize)]
pub struct HolderRecord {
    pub model: &'static str,
    pub seat: Seat,
    pub inner: bool,
    pub length: f64,
    pub width: f64,
    pub height: f64,
    pub head_length: f64,
    pub wf: f64,
    pub min_bore: Option<f64>,
    pub page: u16,
}
pub fn source(chapter: char, page: u16) -> String {
    format!("https://tool-global.kyocera.com/prdct/tool/cms_files/2024-2026_{chapter}.pdf#page={page}")
}
pub const INSERTS: &[InsertRecord] = &[
    InsertRecord { model: "TCMT090202HQ", seat: Seat::Tc09, ic: 5.56, thickness: 2.38, hole: 2.5, nose: 0.2, clearance_deg: 7.0, page: 85 },
    InsertRecord { model: "TCMT090204HQ", seat: Seat::Tc09, ic: 5.56, thickness: 2.38, hole: 2.5, nose: 0.4, clearance_deg: 7.0, page: 85 },
    InsertRecord { model: "TCMT110202HQ", seat: Seat::Tc11, ic: 6.35, thickness: 2.38, hole: 2.8, nose: 0.2, clearance_deg: 7.0, page: 85 },
    InsertRecord { model: "TCMT110204HQ", seat: Seat::Tc11, ic: 6.35, thickness: 2.38, hole: 2.8, nose: 0.4, clearance_deg: 7.0, page: 85 },
    InsertRecord { model: "TCMT110208HQ", seat: Seat::Tc11, ic: 6.35, thickness: 2.38, hole: 2.8, nose: 0.8, clearance_deg: 7.0, page: 85 },
    InsertRecord { model: "TCMT16T304HQ", seat: Seat::Tc16, ic: 9.525, thickness: 3.97, hole: 4.4, nose: 0.4, clearance_deg: 7.0, page: 85 },
    InsertRecord { model: "TCMT16T308HQ", seat: Seat::Tc16, ic: 9.525, thickness: 3.97, hole: 4.4, nose: 0.8, clearance_deg: 7.0, page: 85 },
    InsertRecord { model: "TCMT16T312HQ", seat: Seat::Tc16, ic: 9.525, thickness: 3.97, hole: 4.4, nose: 1.2, clearance_deg: 7.0, page: 85 },
    InsertRecord { model: "TNMG160404HQ", seat: Seat::Tn16, ic: 9.525, thickness: 4.76, hole: 3.81, nose: 0.4, clearance_deg: 0.0, page: 38 },
    InsertRecord { model: "TNMG160408HQ", seat: Seat::Tn16, ic: 9.525, thickness: 4.76, hole: 3.81, nose: 0.8, clearance_deg: 0.0, page: 38 },
    InsertRecord { model: "TNMG160412HQ", seat: Seat::Tn16, ic: 9.525, thickness: 4.76, hole: 3.81, nose: 1.2, clearance_deg: 0.0, page: 38 },
    InsertRecord { model: "TNMG160404PQ", seat: Seat::Tn16, ic: 9.525, thickness: 4.76, hole: 3.81, nose: 0.4, clearance_deg: 0.0, page: 38 },
    InsertRecord { model: "TNMG160402PP", seat: Seat::Tn16, ic: 9.525, thickness: 4.76, hole: 3.81, nose: 0.2, clearance_deg: 0.0, page: 38 },
    InsertRecord { model: "TNMG160404PP", seat: Seat::Tn16, ic: 9.525, thickness: 4.76, hole: 3.81, nose: 0.4, clearance_deg: 0.0, page: 38 },
    InsertRecord { model: "TNMG160408PP", seat: Seat::Tn16, ic: 9.525, thickness: 4.76, hole: 3.81, nose: 0.8, clearance_deg: 0.0, page: 38 },
    InsertRecord { model: "TNMG160412PP", seat: Seat::Tn16, ic: 9.525, thickness: 4.76, hole: 3.81, nose: 1.2, clearance_deg: 0.0, page: 38 },
    InsertRecord { model: "TNMG160402GP", seat: Seat::Tn16, ic: 9.525, thickness: 4.76, hole: 3.81, nose: 0.2, clearance_deg: 0.0, page: 38 },
    InsertRecord { model: "TNMG160404GP", seat: Seat::Tn16, ic: 9.525, thickness: 4.76, hole: 3.81, nose: 0.4, clearance_deg: 0.0, page: 38 },
    InsertRecord { model: "TNMG160408GP", seat: Seat::Tn16, ic: 9.525, thickness: 4.76, hole: 3.81, nose: 0.8, clearance_deg: 0.0, page: 38 },
    InsertRecord { model: "TNMG160408PQ", seat: Seat::Tn16, ic: 9.525, thickness: 4.76, hole: 3.81, nose: 0.8, clearance_deg: 0.0, page: 38 },
    InsertRecord { model: "TNMG160412PQ", seat: Seat::Tn16, ic: 9.525, thickness: 4.76, hole: 3.81, nose: 1.2, clearance_deg: 0.0, page: 38 },
    InsertRecord { model: "TNMG160404CQ", seat: Seat::Tn16, ic: 9.525, thickness: 4.76, hole: 3.81, nose: 0.4, clearance_deg: 0.0, page: 38 },
    InsertRecord { model: "TNMG160408CQ", seat: Seat::Tn16, ic: 9.525, thickness: 4.76, hole: 3.81, nose: 0.8, clearance_deg: 0.0, page: 38 },
    InsertRecord { model: "TNMG160412CQ", seat: Seat::Tn16, ic: 9.525, thickness: 4.76, hole: 3.81, nose: 1.2, clearance_deg: 0.0, page: 38 },
    InsertRecord { model: "TNMG160404PMG", seat: Seat::Tn16, ic: 9.525, thickness: 4.76, hole: 3.81, nose: 0.4, clearance_deg: 0.0, page: 39 },
    InsertRecord { model: "TNMG160408PMG", seat: Seat::Tn16, ic: 9.525, thickness: 4.76, hole: 3.81, nose: 0.8, clearance_deg: 0.0, page: 39 },
    InsertRecord { model: "TNMG160412PMG", seat: Seat::Tn16, ic: 9.525, thickness: 4.76, hole: 3.81, nose: 1.2, clearance_deg: 0.0, page: 39 },
    InsertRecord { model: "TNMG160404GS", seat: Seat::Tn16, ic: 9.525, thickness: 4.76, hole: 3.81, nose: 0.4, clearance_deg: 0.0, page: 39 },
    InsertRecord { model: "TNMG160408GS", seat: Seat::Tn16, ic: 9.525, thickness: 4.76, hole: 3.81, nose: 0.8, clearance_deg: 0.0, page: 39 },
    InsertRecord { model: "TNMG160404PG", seat: Seat::Tn16, ic: 9.525, thickness: 4.76, hole: 3.81, nose: 0.4, clearance_deg: 0.0, page: 39 },
    InsertRecord { model: "TNMG160408PG", seat: Seat::Tn16, ic: 9.525, thickness: 4.76, hole: 3.81, nose: 0.8, clearance_deg: 0.0, page: 39 },
    InsertRecord { model: "TNMG160412PG", seat: Seat::Tn16, ic: 9.525, thickness: 4.76, hole: 3.81, nose: 1.2, clearance_deg: 0.0, page: 39 },
    InsertRecord { model: "TNMG160404PS", seat: Seat::Tn16, ic: 9.525, thickness: 4.76, hole: 3.81, nose: 0.4, clearance_deg: 0.0, page: 39 },
    InsertRecord { model: "TNMG160408PS", seat: Seat::Tn16, ic: 9.525, thickness: 4.76, hole: 3.81, nose: 0.8, clearance_deg: 0.0, page: 39 },
    InsertRecord { model: "TNMG160412PS", seat: Seat::Tn16, ic: 9.525, thickness: 4.76, hole: 3.81, nose: 1.2, clearance_deg: 0.0, page: 39 },
    InsertRecord { model: "TNMG160408PT", seat: Seat::Tn16, ic: 9.525, thickness: 4.76, hole: 3.81, nose: 0.8, clearance_deg: 0.0, page: 39 },
    InsertRecord { model: "TNMG160412PT", seat: Seat::Tn16, ic: 9.525, thickness: 4.76, hole: 3.81, nose: 1.2, clearance_deg: 0.0, page: 39 },
    InsertRecord { model: "TNMG160408GT", seat: Seat::Tn16, ic: 9.525, thickness: 4.76, hole: 3.81, nose: 0.8, clearance_deg: 0.0, page: 39 },
    InsertRecord { model: "TNMG160412GT", seat: Seat::Tn16, ic: 9.525, thickness: 4.76, hole: 3.81, nose: 1.2, clearance_deg: 0.0, page: 39 },
];
pub const HOLDERS: &[HolderRecord] = &[
    HolderRecord {
        model: "DTGNR2020K-16",
        seat: Seat::Tn16,
        inner: false,
        length: 125.0,
        width: 20.0,
        height: 20.0,
        head_length: 25.0,
        wf: 25.0,
        min_bore: None,
        page: 22,
    },
    HolderRecord {
        model: "DTGNR2525M-16",
        seat: Seat::Tn16,
        inner: false,
        length: 150.0,
        width: 25.0,
        height: 25.0,
        head_length: 25.0,
        wf: 32.0,
        min_bore: None,
        page: 22,
    },
    HolderRecord {
        model: "WTJNR2020K-16N",
        seat: Seat::Tn16,
        inner: false,
        length: 125.0,
        width: 20.0,
        height: 20.0,
        head_length: 32.0,
        wf: 25.0,
        min_bore: None,
        page: 27,
    },
    HolderRecord {
        model: "WTJNR2525M-16N",
        seat: Seat::Tn16,
        inner: false,
        length: 150.0,
        width: 25.0,
        height: 25.0,
        head_length: 32.0,
        wf: 32.0,
        min_bore: None,
        page: 27,
    },
    HolderRecord {
        model: "S08X-STLCR09-10A",
        seat: Seat::Tc09,
        inner: true,
        length: 120.0,
        width: 8.0,
        height: 7.0,
        head_length: 16.0,
        wf: 5.0,
        min_bore: Some(10.0),
        page: 89,
    },
    HolderRecord {
        model: "S10L-STLCR09-12A",
        seat: Seat::Tc09,
        inner: true,
        length: 140.0,
        width: 10.0,
        height: 9.0,
        head_length: 20.0,
        wf: 6.2,
        min_bore: Some(12.0),
        page: 89,
    },
    HolderRecord {
        model: "S10L-STLCR11-12A",
        seat: Seat::Tc11,
        inner: true,
        length: 140.0,
        width: 10.0,
        height: 9.0,
        head_length: 20.0,
        wf: 6.2,
        min_bore: Some(12.0),
        page: 89,
    },
    HolderRecord {
        model: "S12M-STLCR11-14A",
        seat: Seat::Tc11,
        inner: true,
        length: 150.0,
        width: 12.0,
        height: 11.0,
        head_length: 24.0,
        wf: 7.2,
        min_bore: Some(14.0),
        page: 89,
    },
    HolderRecord {
        model: "S16Q-STLCR11-18A",
        seat: Seat::Tc11,
        inner: true,
        length: 180.0,
        width: 16.0,
        height: 15.0,
        head_length: 30.0,
        wf: 9.2,
        min_bore: Some(18.0),
        page: 89,
    },
    HolderRecord {
        model: "S20R-STLCR11-22A",
        seat: Seat::Tc11,
        inner: true,
        length: 200.0,
        width: 20.0,
        height: 19.0,
        head_length: 36.0,
        wf: 11.2,
        min_bore: Some(22.0),
        page: 89,
    },
    HolderRecord {
        model: "WTKNR2020K-16N",
        seat: Seat::Tn16,
        inner: false,
        length: 125.0,
        width: 20.0,
        height: 20.0,
        head_length: 32.0,
        wf: 25.0,
        min_bore: None,
        page: 27,
    },
    HolderRecord {
        model: "WTKNR2525M-16N",
        seat: Seat::Tn16,
        inner: false,
        length: 150.0,
        width: 25.0,
        height: 25.0,
        head_length: 32.0,
        wf: 32.0,
        min_bore: None,
        page: 27,
    },
    HolderRecord {
        model: "PTGNR1616H-16",
        seat: Seat::Tn16,
        inner: false,
        length: 100.0,
        width: 16.0,
        height: 16.0,
        head_length: 24.0,
        wf: 20.0,
        min_bore: None,
        page: 24,
    },
    HolderRecord {
        model: "PTGNR2020H-16",
        seat: Seat::Tn16,
        inner: false,
        length: 100.0,
        width: 20.0,
        height: 20.0,
        head_length: 24.0,
        wf: 25.0,
        min_bore: None,
        page: 24,
    },
    HolderRecord {
        model: "PTGNR2020K-16",
        seat: Seat::Tn16,
        inner: false,
        length: 125.0,
        width: 20.0,
        height: 20.0,
        head_length: 24.0,
        wf: 25.0,
        min_bore: None,
        page: 24,
    },
    HolderRecord {
        model: "PTGNR2525M-16",
        seat: Seat::Tn16,
        inner: false,
        length: 150.0,
        width: 25.0,
        height: 25.0,
        head_length: 24.0,
        wf: 32.0,
        min_bore: None,
        page: 24,
    },
    HolderRecord {
        model: "A08X-STLCR09-10AE",
        seat: Seat::Tc09,
        inner: true,
        length: 120.0,
        width: 8.0,
        height: 7.0,
        head_length: 16.0,
        wf: 5.0,
        min_bore: Some(10.0),
        page: 88,
    },
    HolderRecord {
        model: "A10L-STLCR09-12AE",
        seat: Seat::Tc09,
        inner: true,
        length: 140.0,
        width: 10.0,
        height: 9.0,
        head_length: 20.0,
        wf: 6.2,
        min_bore: Some(12.0),
        page: 88,
    },
    HolderRecord {
        model: "A10L-STLCR11-12AE",
        seat: Seat::Tc11,
        inner: true,
        length: 140.0,
        width: 10.0,
        height: 9.0,
        head_length: 20.0,
        wf: 6.2,
        min_bore: Some(12.0),
        page: 88,
    },
    HolderRecord {
        model: "A12M-STLCR11-14AE",
        seat: Seat::Tc11,
        inner: true,
        length: 150.0,
        width: 12.0,
        height: 11.0,
        head_length: 24.0,
        wf: 7.2,
        min_bore: Some(14.0),
        page: 88,
    },
    HolderRecord {
        model: "A16Q-STLCR11-18AE",
        seat: Seat::Tc11,
        inner: true,
        length: 180.0,
        width: 16.0,
        height: 15.0,
        head_length: 30.0,
        wf: 9.2,
        min_bore: Some(18.0),
        page: 88,
    },
    HolderRecord {
        model: "A20R-STLCR11-22AE",
        seat: Seat::Tc11,
        inner: true,
        length: 200.0,
        width: 20.0,
        height: 19.0,
        head_length: 36.0,
        wf: 11.2,
        min_bore: Some(22.0),
        page: 88,
    },
    HolderRecord {
        model: "A25R-DTFNR16-32",
        seat: Seat::Tn16,
        inner: true,
        length: 200.0,
        width: 25.0,
        height: 23.0,
        head_length: 42.0,
        wf: 17.0,
        min_bore: Some(32.0),
        page: 143,
    },
    HolderRecord {
        model: "A32S-DTFNR16-40",
        seat: Seat::Tn16,
        inner: true,
        length: 250.0,
        width: 32.0,
        height: 30.0,
        head_length: 50.0,
        wf: 22.0,
        min_bore: Some(40.0),
        page: 143,
    },
];
/// References identify catalogue selections, not verified mounted assemblies. Current
/// editable simulation dimensions may differ. Unknown future IDs remain readable.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub insert: Option<String>,
    pub holder: Option<String>,
}
impl Selection {
    pub fn validate(&self) -> Result<()> {
        for id in [&self.insert, &self.holder].into_iter().flatten() {
            if id.is_empty() || id.len() > 100 || !id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-') {
                return invalid("invalid tool catalogue reference");
            }
        }
        Ok(())
    }
}
pub fn assign(slot: &mut ToolSlot, selection: Selection) -> Result<()> {
    assign_impl(slot, selection, false)
}
/// Manual model references carry no manufacturer dimensional certification.
pub fn assign_custom(slot: &mut ToolSlot, selection: Selection) -> Result<()> {
    assign_impl(slot, selection, true)
}
fn assign_impl(slot: &mut ToolSlot, selection: Selection, custom: bool) -> Result<()> {
    selection.validate()?;
    let insert = selection.insert.as_deref().and_then(|id| INSERTS.iter().find(|v| v.model == id));
    let holder = selection.holder.as_deref().and_then(|id| HOLDERS.iter().find(|v| v.model == id));
    if !custom && ((selection.insert.is_some() && insert.is_none()) || (selection.holder.is_some() && holder.is_none())) {
        return invalid("unknown catalogue tool; use manual model registration");
    }
    let inner = matches!(slot.role, ToolRole::IdRough | ToolRole::IdFinish);
    if !matches!(slot.role, ToolRole::IdRough | ToolRole::IdFinish | ToolRole::OdRough | ToolRole::OdFinish) {
        return invalid("catalogue selection requires an OD or ID turning station");
    }
    if selection.insert.is_none() && selection.holder.is_none() {
        return invalid("select an insert or holder");
    }
    if holder.is_some_and(|h| h.inner != inner) {
        return invalid("holder does not match the station's OD/ID role");
    }
    if insert.zip(holder).is_some_and(|(i, h)| i.seat != h.seat) {
        return invalid("insert and holder seats do not match");
    }
    // Construct everything before mutating. A newly selected holder must never inherit
    // the old holder's geometry or verification flag.
    let mut tool = slot.simulation.clone().unwrap_or_default();
    if let Some(i) = insert {
        let orientation_rad = tool.insert.as_ref().map_or(std::f64::consts::FRAC_PI_4, |v| v.orientation_rad);
        tool.triangular_insert = true;
        tool.nose_radius_mm = Some(i.nose);
        tool.insert = Some(Insert { inscribed_diameter_mm: i.ic, thickness_mm: i.thickness, orientation_rad, inner });
    } else {
        // Holder-only registration cannot silently keep an unidentified old insert.
        tool.insert = None;
        tool.nose_radius_mm = None;
        tool.triangular_insert = false;
    }
    if slot.catalog.as_ref().and_then(|s| s.holder.as_ref()) != selection.holder.as_ref() {
        tool.holder = None;
    }
    tool.holder_verified = false;
    tool.validate()?;
    if slot.catalog.as_ref().and_then(|s| s.insert.as_ref()) != selection.insert.as_ref() {
        slot.insert_cad = None;
    }
    if slot.catalog.as_ref().and_then(|s| s.holder.as_ref()) != selection.holder.as_ref() {
        slot.holder_cad = None;
    }
    slot.tool_name = Some([selection.holder.as_deref(), selection.insert.as_deref()].into_iter().flatten().collect::<Vec<_>>().join(" + "));
    slot.catalog = Some(selection);
    slot.simulation = Some(tool);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalogue_dimensions_and_seats_are_checked_without_certifying_a_holder() {
        let mut ids = std::collections::BTreeSet::new();
        for i in INSERTS {
            assert!(ids.insert(i.model));
            Insert { inscribed_diameter_mm: i.ic, thickness_mm: i.thickness, ..Default::default() }.validate(i.nose).unwrap();
        }
        for h in HOLDERS {
            assert!(ids.insert(h.model));
            assert!(h.length > h.head_length && h.width > 0. && h.height > 0.);
        }
        let mut slot = crate::nl2500_turret().into_iter().find(|s| s.station == 5).unwrap();
        let choice = Selection { insert: Some("TCMT110204HQ".into()), holder: Some("S12M-STLCR11-14A".into()) };
        assign(&mut slot, choice).unwrap();
        assert_eq!(slot.simulation.as_ref().unwrap().insert.as_ref().unwrap().inscribed_diameter_mm, 6.35);
        assert!(!slot.simulation.as_ref().unwrap().holder_verified);
        assert!(slot.simulation.as_ref().unwrap().holder.is_none());
        let before = slot.clone();
        assert!(assign(&mut slot, Selection { insert: Some("TNMG160404HQ".into()), holder: Some("S12M-STLCR11-14A".into()) }).is_err());
        assert_eq!(slot, before);
        assert!(assign(&mut slot, Selection { insert: None, holder: Some("DTGNR2525M-16".into()) }).is_err());
        assert_eq!(slot, before);
    }
}
