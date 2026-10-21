use super::*;
use crate::gpu::GpuScene;
use cam::catalog::{self, HOLDERS, INSERTS, Selection};
use solidcraft_engine::geom::{Aabb3, Vec3};

#[derive(Clone, Debug, Default)]
pub(super) struct View {
    pub open: bool,
    pub search: String,
    pub insert: Option<usize>,
    pub holder: Option<usize>,
    pub preview: Option<(bool, String)>,
    pub fit: bool,
    pub path: String,
    pub insert_path: String,
    pub custom_holder: String,
    pub custom_insert: String,
    import: Option<Import>,
}
#[derive(Clone, Debug)]
struct Import {
    holder: bool,
    setup: String,
    station: u8,
    model: String,
    path: String,
}
fn button(ui: &mut egui::Ui, id: &str, key: &str) -> bool {
    let r = ui_kit::button(ui, tr(key), ui_kit::Button::Normal);
    crate::scenario::publish_rect(id, r.rect);
    crate::scenario::publish_handle(id, r.rect.center());
    r.clicked()
}
fn note(ui: &mut egui::Ui, key: &str) {
    ui.add(egui::Label::new(egui::RichText::new(tr(key)).small()).wrap());
}
pub(super) fn show(ui: &mut egui::Ui, setup: &mut Setup, state: &mut State, library: &[cam::holder_cad::LibraryEntry]) {
    let was_open = state.catalog.open;
    let previous_station = state.simulation.selected_station;
    if button(ui, "cam_catalog_open", "cam.catalog_open") {
        state.catalog.open = !state.catalog.open;
    }
    if !state.catalog.open {
        return;
    }
    let view = &mut state.catalog;
    ui.strong(format!("Kyocera · 2024–2026 · {} + {}", INSERTS.len(), HOLDERS.len()));
    note(ui, "cam.catalog_scope");
    if !library.is_empty() {
        ui.small(format!("{}: {}", tr("cam.catalog_saved_steps"), library.len()));
    }
    ui.horizontal(|ui| {
        ui.label(tr("cam.station"));
        let response = controls::dropdown(ui, "cam_catalog_station", (format!("T{:02}", state.simulation.selected_station)).to_string(), |ui| {
            for s in setup.turret.iter().filter(|s| matches!(s.role, ToolRole::OdRough | ToolRole::OdFinish | ToolRole::IdRough | ToolRole::IdFinish))
            {
                let r = controls::value(ui, &mut state.simulation.selected_station, s.station, format!("T{:02}", s.station));
                crate::scenario::publish_rect(&format!("cam_catalog_station_{}", s.station), r.rect);
            }
        });
        crate::scenario::publish_rect("cam_catalog_station", response.response.rect);
    });
    let Some(slot) = setup.turret.iter_mut().find(|s| s.station == state.simulation.selected_station) else { return };
    if !was_open || previous_station != slot.station {
        view.insert = slot.catalog.as_ref().and_then(|c| c.insert.as_deref()).and_then(|id| INSERTS.iter().position(|v| v.model == id));
        view.holder = slot.catalog.as_ref().and_then(|c| c.holder.as_deref()).and_then(|id| HOLDERS.iter().position(|v| v.model == id));
        view.search.clear();
        view.custom_holder = slot.catalog.as_ref().and_then(|c| c.holder.clone()).unwrap_or_default();
        view.custom_insert = slot.catalog.as_ref().and_then(|c| c.insert.clone()).unwrap_or_default();
        if view.preview.take().is_some() {
            view.fit = true;
        }
    }
    let inner = matches!(slot.role, ToolRole::IdRough | ToolRole::IdFinish);
    if let Some(c) = &slot.catalog {
        ui.add(
            egui::Label::new(format!(
                "{}:\n{}\n{}",
                tr("cam.catalog_registered"),
                c.insert.as_deref().unwrap_or("—"),
                c.holder.as_deref().unwrap_or("—")
            ))
            .wrap(),
        );
    }
    let search_response = ui_kit::bounded_cell_editor(ui, ui.next_auto_id(), &mut view.search, tr("cam.catalog_search"), false, 64);
    crate::scenario::publish_rect("cam_catalog_search", search_response.rect);
    let previous_insert = view.insert;
    let search = view.search.to_ascii_uppercase();
    ui.label(tr("cam.catalog_insert"));
    let r = controls::dropdown(ui, "cam_catalog_insert", (view.insert.and_then(|i| INSERTS.get(i)).map_or("—", |v| v.model)).to_string(), |ui| {
        controls::value(ui, &mut view.insert, None, "—");
        for (i, item) in INSERTS.iter().enumerate().filter(|(_, v)| v.model.contains(&search)) {
            let r = controls::value(
                ui,
                &mut view.insert,
                Some(i),
                format!(
                    "{} · R{}{}",
                    item.model,
                    item.nose,
                    if cam::holder_cad::find(library, false, item.model).is_some() { " · STEP" } else { "" }
                ),
            );
            crate::scenario::publish_rect(&format!("cam_catalog_insert_{i}"), r.rect);
        }
    });
    crate::scenario::publish_rect("cam_catalog_insert", r.response.rect);
    if previous_insert != view.insert {
        view.search.clear();
    }
    let search = view.search.to_ascii_uppercase();
    if let Some(item) = view.insert.and_then(|i| INSERTS.get(i)) {
        ui.label(format!("IC {} / S {} / R{} mm", item.ic, item.thickness, item.nose));
        ui.small(format!("D1 {} mm / AN {}°", item.hole, item.clearance_deg));
        ui.hyperlink_to(format!("{} B{}", tr("cam.catalog_source"), item.page), catalog::source('B', item.page));
        if button(ui, "cam_catalog_preview_insert", "cam.catalog_preview_insert") {
            view.preview = Some((false, item.model.into()));
            view.fit = true;
            state.spatial.two_d = false;
            state.spatial.playing = false;
        }
    }
    ui.label(tr("cam.catalog_holder"));
    let r = controls::dropdown(ui, "cam_catalog_holder", (view.holder.and_then(|i| HOLDERS.get(i)).map_or("—", |v| v.model)).to_string(), |ui| {
        controls::value(ui, &mut view.holder, None, "—");
        for (i, item) in HOLDERS.iter().enumerate().filter(|(_, v)| v.inner == inner && v.model.contains(&search)) {
            let r = controls::value(
                ui,
                &mut view.holder,
                Some(i),
                format!("{}{}", item.model, if cam::holder_cad::find(library, true, item.model).is_some() { " · STEP" } else { "" }),
            );
            crate::scenario::publish_rect(&format!("cam_catalog_holder_{i}"), r.rect);
        }
    });
    crate::scenario::publish_rect("cam_catalog_holder", r.response.rect);
    if let Some(h) = view.holder.and_then(|i| HOLDERS.get(i)) {
        ui.label(format!("LF {} / {} {} / H {} mm", h.length, if h.inner { "DCON" } else { "B" }, h.width, h.height));
        ui.small(format!("LH {} / WF {} mm · R", h.head_length, h.wf));
        if let Some(d) = h.min_bore {
            ui.label(format!("{}: φ{}", tr("cam.catalog_min_bore"), d));
        }
        ui.hyperlink_to(
            format!("{} {}{}", tr("cam.catalog_source"), if h.inner { 'F' } else { 'D' }, h.page),
            catalog::source(if h.inner { 'F' } else { 'D' }, h.page),
        );
        if button(ui, "cam_catalog_preview_holder", "cam.catalog_preview_holder") {
            view.preview = Some((true, h.model.into()));
            view.fit = true;
            state.spatial.two_d = false;
            state.spatial.playing = false;
        }
    }
    note(ui, "cam.catalog_assignment");
    if button(ui, "cam_catalog_apply", "cam.catalog_apply") {
        let selection = Selection {
            insert: view.insert.and_then(|i| INSERTS.get(i)).map(|v| v.model.into()),
            holder: view.holder.and_then(|i| HOLDERS.get(i)).map(|v| v.model.into()),
        };
        state.error = catalog::assign(slot, selection).err().map(|e| e.to_string());
        if state.error.is_none() {
            view.preview = None;
            view.fit = true;
        }
    }
    let custom = controls::section(ui, tr("cam.catalog_custom"), |ui| {
        note(ui, "cam.catalog_custom_note");
        ui.label(tr("cam.catalog_holder"));
        let r = ui_kit::bounded_cell_editor(ui, ui.next_auto_id(), &mut view.custom_holder, "", false, 100);
        crate::scenario::publish_rect("cam_custom_holder", r.rect);
        ui.label(tr("cam.catalog_insert"));
        let r = ui_kit::bounded_cell_editor(ui, ui.next_auto_id(), &mut view.custom_insert, "", false, 100);
        crate::scenario::publish_rect("cam_custom_insert", r.rect);
        if button(ui, "cam_custom_apply", "cam.catalog_custom_apply") {
            let id = |v: &str| (!v.trim().is_empty()).then(|| v.trim().to_owned());
            let selection = Selection { holder: id(&view.custom_holder), insert: id(&view.custom_insert) };
            state.error = catalog::assign_custom(slot, selection).err().map(|e| e.to_string());
            if state.error.is_none() {
                view.holder = HOLDERS.iter().position(|h| Some(h.model) == slot.catalog.as_ref().and_then(|c| c.holder.as_deref()));
                view.insert = INSERTS.iter().position(|i| Some(i.model) == slot.catalog.as_ref().and_then(|c| c.insert.as_deref()));
                view.preview = None;
                view.fit = true;
            }
        }
    });
    crate::scenario::publish_rect("cam_custom_open", custom.header_rect);
    for holder in [true, false] {
        let Some(model) = slot.catalog.as_ref().and_then(|c| if holder { c.holder.clone() } else { c.insert.clone() }) else { continue };
        let prefix = if holder { "cam_holder_cad" } else { "cam_insert_cad" };
        let stored = if holder { &mut slot.holder_cad } else { &mut slot.insert_cad };
        ui.separator();
        ui.strong(tr(if holder { "cam.holder_cad_title" } else { "cam.insert_cad_title" }));
        ui.small(&model);
        if let Some(cad) = stored {
            ui.add(egui::Label::new(&cad.file_name).wrap());
            if button(ui, &format!("{prefix}_preview"), "cam.holder_cad_preview") {
                view.preview = Some((holder, cad.model.clone()));
                view.fit = true;
                state.spatial.two_d = false;
                state.spatial.playing = false;
            }
            if button(ui, &format!("{prefix}_remove"), "cam.holder_cad_remove") {
                *stored = None;
                view.preview = None;
                view.fit = true;
            }
        }
        note(ui, "cam.holder_cad_note");
        let path = if holder { &mut view.path } else { &mut view.insert_path };
        let r = ui_kit::bounded_cell_editor(ui, ui.next_auto_id(), path, tr("cam.holder_cad_path"), false, 4096);
        crate::scenario::publish_rect(&format!("{prefix}_path"), r.rect);
        if button(ui, &format!("{prefix}_import"), if holder { "cam.holder_cad_import" } else { "cam.insert_cad_import" }) {
            view.import = Some(Import { holder, setup: setup.id.clone(), station: slot.station, model, path: path.trim().trim_matches('"').into() });
        }
    }
    ui.hyperlink_to(tr("cam.catalog_cad"), catalog::CAD_URL);
    note(ui, "cam.catalog_login");
    ui.separator();
}
pub(super) fn process(app: &mut SolveApp, state: &mut State) {
    let Some(Import { setup, station, holder, model, path }) = state.catalog.import.take() else { return };
    match app.run(
        if holder { "cam.turning.holder_cad.import" } else { "cam.turning.insert_cad.import" },
        json!({"setup":setup,"station":station,"model":model,"path":path,"expected_revision":app.session.revision}),
    ) {
        Ok(_) => {
            state.error = None;
            state.catalog.preview = Some((holder, model));
            state.catalog.fit = true;
            state.spatial.two_d = false;
            state.spatial.playing = false;
        }
        Err(e) => state.error = Some(e),
    }
}
fn imported(app: &SolveApp) -> Option<&cam::holder_cad::HolderCad> {
    let (holder, model) = preview(app)?;
    let state = &app.turning_workspace;
    let plan = app.session.doc.turning.as_ref()?;
    let slot = plan.setups.get(state.setup)?.turret.iter().find(|t| t.station == state.simulation.selected_station)?;
    let cad = if holder { slot.holder_cad.as_deref() } else { slot.insert_cad.as_deref() };
    cad.filter(|c| c.model == model).or_else(|| cam::holder_cad::find(&plan.tool_cad_library, holder, model))
}
pub(super) fn preview(app: &SolveApp) -> Option<(bool, &str)> {
    (super::active(app) && app.turning_workspace.workflow == 4)
        .then_some(app.turning_workspace.catalog.preview.as_ref())
        .flatten()
        .map(|(h, m)| (*h, m.as_str()))
}
pub(super) fn bounds(app: &SolveApp) -> Option<Aabb3> {
    if let Some(cad) = imported(app) {
        let (lo, hi) = cad.bounds()?;
        let p = |v: [f64; 3]| Vec3::new(v[0], v[1], v[2]);
        return Some(Aabb3 { min: p(lo), max: p(hi) });
    }
    let (holder, model) = preview(app)?;
    let (l, w, h) = if holder {
        let v = HOLDERS.iter().find(|v| v.model == model)?;
        (v.length, v.width.max(v.wf), v.height)
    } else {
        let v = INSERTS.iter().find(|v| v.model == model)?;
        (2. * v.ic, 2. * v.ic, v.thickness)
    };
    Some(Aabb3 { min: Vec3::new(-2., -w, -h), max: Vec3::new(l + 2., w, h) })
}
fn tri(scene: &mut GpuScene, points: [Vec3; 3], color: [u8; 4]) {
    let [a, b, c] = points;
    let Some(n) = (b - a).cross(c - a).normalized() else { return };
    for p in points {
        scene.tri(p.to_f32(), n.to_f32(), color);
    }
}
fn prism(scene: &mut GpuScene, outline: &[[f64; 2]], z0: f64, z1: f64, color: [u8; 4]) {
    if outline.len() < 3 {
        return;
    }
    let center = outline.iter().fold([0., 0.], |a, p| [a[0] + p[0] / outline.len() as f64, a[1] + p[1] / outline.len() as f64]);
    let point = |p: [f64; 2], z| Vec3::new(p[0], p[1], z);
    for (a, b) in outline.iter().zip(outline.iter().cycle().skip(1)).take(outline.len()) {
        let (a0, b0, a1, b1) = (point(*a, z0), point(*b, z0), point(*a, z1), point(*b, z1));
        for t in [[point(center, z0), b0, a0], [point(center, z1), a1, b1], [a0, b0, b1], [a0, b1, a1]] {
            tri(scene, t, color);
        }
    }
}
pub(super) fn scene(app: &SolveApp) -> Option<GpuScene> {
    let (holder, model) = preview(app)?;
    let mut scene = GpuScene::default();
    if let Some(cad) = imported(app) {
        let color = if holder { Tokens::get().body } else { Tokens::get().warning }.to_array();
        for ids in &cad.triangles {
            let point = |i: u32| cad.positions.get(i as usize).map(|p| Vec3::new(p[0], p[1], p[2]));
            if let (Some(a), Some(b), Some(c)) = (point(ids[0]), point(ids[1]), point(ids[2])) {
                tri(&mut scene, [a, b, c], color);
            }
        }
        return Some(scene);
    }
    if holder {
        let h = HOLDERS.iter().find(|h| h.model == model)?;
        let color = Tokens::get().body.to_array();
        if h.inner {
            // Shaft only. Flats, sculpted head, clamp and screw are deliberately omitted.
            let radius = h.width * 0.5;
            for i in 0..48 {
                let a = i as f64 * std::f64::consts::TAU / 48.;
                let b = (i + 1) as f64 * std::f64::consts::TAU / 48.;
                let p = |x, t: f64| Vec3::new(x, radius * t.cos(), radius * t.sin());
                for t in [
                    [p(0., a), p(h.length, b), p(h.length, a)],
                    [p(0., a), p(0., b), p(h.length, b)],
                    [Vec3::ZERO, p(0., b), p(0., a)],
                    [Vec3::new(h.length, 0., 0.), p(h.length, a), p(h.length, b)],
                ] {
                    tri(&mut scene, t, color);
                }
            }
        } else {
            prism(
                &mut scene,
                &[[0., -h.width * 0.5], [h.length, -h.width * 0.5], [h.length, h.width * 0.5], [0., h.width * 0.5]],
                -h.height * 0.5,
                h.height * 0.5,
                color,
            );
        }
        // A visible bounding box of the head region, not a fabricated precise head model.
        let c = Tokens::get().warning.to_array();
        let y = h.wf.max(h.width) * 0.5;
        for z in [-h.height * 0.5, h.height * 0.5] {
            for (a, b) in [
                (Vec3::new(0., -y, z), Vec3::new(h.head_length, -y, z)),
                (Vec3::new(h.head_length, -y, z), Vec3::new(h.head_length, y, z)),
                (Vec3::new(h.head_length, y, z), Vec3::new(0., y, z)),
                (Vec3::new(0., y, z), Vec3::new(0., -y, z)),
            ] {
                scene.guide_line(a.to_f32(), b.to_f32(), c, 1.5);
            }
        }
    } else {
        let i = INSERTS.iter().find(|i| i.model == model)?;
        let shape = cam::insert::Insert { inscribed_diameter_mm: i.ic, thickness_mm: i.thickness, orientation_rad: 0., inner: false };
        let points = shape.outline(i.nose).ok()?;
        prism(&mut scene, &points, 0., i.thickness, Tokens::get().warning.to_array());
    }
    Some(scene)
}
pub(super) fn canvas(app: &mut SolveApp, ui: &mut egui::Ui) {
    let Some((holder, name)) = preview(app) else { return };
    ui.painter().rect_filled(ui.max_rect(), 0., Tokens::get().panel);
    let from_step = imported(app).is_some();
    ui.strong(format!("{name} · {}", tr(if from_step { "cam.holder_cad_preview" } else { "cam.catalog_preview" })));
    note(
        ui,
        if from_step {
            "cam.holder_cad_note"
        } else if holder {
            "cam.catalog_holder_shape"
        } else {
            "cam.catalog_insert_shape"
        },
    );
    if button(ui, "cam_catalog_preview_close", "cam.catalog_preview_close") {
        app.turning_workspace.catalog.preview = None;
        app.fit_view();
    }
    let rect = ui.available_rect_before_wrap();
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| crate::viewport::show(app, ui));
}
