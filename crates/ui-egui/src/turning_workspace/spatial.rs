//! The shared CAD viewport is the CAM canvas. X is a diameter, never a world radius.
//! Planned envelopes and nominal-tip stock replay share the same model coordinate frame.
use super::*;
use crate::{
    gpu::GpuScene,
    viewport::{Hit, Proj},
};
use cam::machining::{ModelSource, Motion, Phase, ProfilePoint, Toolpath};
use egui::{Align2, FontId, Pos2, Stroke};
use solidcraft_engine::geom::Vec3;

#[derive(Clone, Debug, Default)]
pub(super) struct View {
    pub section: bool,
    pub(super) stage: usize,
    pick_end: bool,
    pub(super) two_d: bool,
    pub(super) playing: bool,
    last_tick: f64,
    pub operation: Option<OperationKind>,
}

#[derive(Clone, Copy)]
struct Frame {
    origin: Vec3,
    axis: Vec3,
    radial: Vec3,
    normal: Vec3,
}

impl Frame {
    fn new(source: &ModelSource) -> Option<Self> {
        let [x, y, z] = source.origin;
        let origin = Vec3::new(x, y, z);
        let [x, y, z] = source.axis;
        if !source.origin.iter().chain(&source.axis).all(|v| v.is_finite()) {
            return None;
        }
        let axis = Vec3::new(x, y, z).normalized()?;
        let up = if axis.dot(Vec3::Z).abs() < 0.9 { Vec3::Z } else { Vec3::Y };
        let radial = (up - axis * axis.dot(up)).normalized()?;
        Some(Self { origin, axis, radial, normal: axis.cross(radial) })
    }
    fn point(self, p: ProfilePoint) -> Vec3 {
        self.origin + self.axis * p.z_mm + self.radial * (p.diameter_mm * 0.5)
    }
    fn ring(self, z: f64, diameter: f64, angle: f64) -> Vec3 {
        self.origin + self.axis * z + (self.radial * angle.cos() + self.normal * angle.sin()) * (diameter * 0.5)
    }
}

fn setup(app: &SolveApp) -> Option<&Setup> {
    app.session.doc.turning.as_ref()?.setups.get(app.turning_workspace.setup)
}

fn profile(app: &SolveApp) -> Option<(&ModelSource, &[ProfilePoint])> {
    let state = &app.turning_workspace;
    if state.workflow == 1
        && let Some(read) = state.model_profile.current_preview(app.session.revision)
    {
        return Some(read);
    }
    let setup = setup(app)?;
    let job = if state.model_profile.inner { setup.id_job.as_ref() } else { setup.od_job.as_ref() }?;
    Some((job.model_source.as_ref()?, &job.profile))
}

fn path(app: &SolveApp) -> Option<&Toolpath> {
    let (revision, id, path) = app.turning_workspace.machining.preview.as_ref()?;
    (*revision == app.session.revision && setup(app).is_some_and(|s| &s.id == id)).then_some(path)
}

fn frame(app: &SolveApp) -> Option<Frame> {
    model_frame(app).or_else(|| {
        setup(app)?.stock.as_ref()?;
        Some(Frame { origin: Vec3::ZERO, axis: Vec3::Z, radial: Vec3::X, normal: Vec3::Y })
    })
}

fn model_frame(app: &SolveApp) -> Option<Frame> {
    if app.turning_workspace.spatial.stage == 1
        && let Some(path) = path(app)
    {
        return path.job.model_source.as_ref().or_else(|| path.id_path.as_ref()?.job.model_source.as_ref()).and_then(Frame::new);
    }
    if app.turning_workspace.spatial.stage == 2 {
        return setup(app)?.od_job.iter().chain(&setup(app)?.id_job).find_map(|j| j.model_source.as_ref().and_then(Frame::new));
    }
    profile(app)
        .and_then(|(source, _)| Frame::new(source))
        .or_else(|| setup(app)?.od_job.iter().chain(&setup(app)?.id_job).find_map(|j| j.model_source.as_ref().and_then(Frame::new)))
}

fn can_pick_end(app: &SolveApp) -> bool {
    profile(app).is_some() && app.turning_workspace.model_profile.current_preview(app.session.revision).is_none()
}

pub fn scene_key(
    app: &SolveApp,
) -> (bool, usize, usize, Option<(u64, u64)>, Option<[u64; 6]>, Option<(u64, usize, Option<usize>, u64)>, Option<(bool, String, u64, u8)>) {
    (
        super::active(app),
        app.turning_workspace.spatial.stage,
        app.turning_workspace.setup,
        app.turning_workspace.machining.preview.as_ref().map(|p| (p.0, app.turning_workspace.machining.generation)),
        frame(app).map(|f| [f.origin.x, f.origin.y, f.origin.z, f.axis.x, f.axis.y, f.axis.z].map(f64::to_bits)),
        super::simulation_ui::current(app).map(|_| {
            (
                app.session.revision,
                app.turning_workspace.machining.step,
                app.turning_workspace.simulation.focus_contact,
                app.turning_workspace.simulation.resolution.to_bits(),
            )
        }),
        super::catalog_ui::preview(app).map(|(h, i)| (h, i.to_owned(), app.session.revision, app.turning_workspace.simulation.selected_station)),
    )
}

pub fn replacing_model(app: &SolveApp) -> bool {
    if super::catalog_ui::preview(app).is_some() {
        return true;
    }
    if super::active(app) && super::simulation_ui::current(app).is_some() && frame(app).is_some() {
        return true;
    }
    super::active(app)
        && app.turning_workspace.spatial.stage != 0
        && frame(app).is_some()
        && setup(app).is_some_and(|s| s.stock.is_some() && (s.od_job.is_some() || s.id_job.is_some()))
        && (app.turning_workspace.spatial.stage != 1 || path(app).is_some())
}

pub fn section_plane(app: &SolveApp) -> Option<(Vec3, Vec3)> {
    if super::catalog_ui::preview(app).is_some() {
        return None;
    }
    if !super::active(app) || !app.turning_workspace.spatial.section {
        return None;
    }
    let frame = frame(app)?;
    Some((frame.origin, frame.normal))
}

fn button(ui: &mut egui::Ui, id: &str, key: &str, selected: bool) -> bool {
    let r = controls::pick(ui, selected, tr(key));
    crate::scenario::publish_rect(id, r.rect);
    crate::scenario::publish_handle(id, r.rect.center());
    r.clicked()
}

pub fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    if super::catalog_ui::preview(app).is_some() {
        super::catalog_ui::canvas(app, ui);
        return;
    }
    ui.painter().rect_filled(ui.max_rect(), 0., Tokens::get().panel);
    let available_frame = frame(app).is_some();
    let valid = path(app).is_some();
    let two_d_available = super::machining_ui::active(app);
    if !two_d_available {
        app.turning_workspace.spatial.two_d = false;
        app.turning_workspace.machining.show_nc = false;
    }
    if !valid || !app.turning_workspace.machining.show {
        app.turning_workspace.spatial.playing = false;
    }
    let simulation_mode = app.turning_workspace.workflow == 4;
    if simulation_mode && super::simulation_ui::current(app).is_none() {
        app.turning_workspace.spatial.playing = false;
    }
    ui.horizontal_wrapped(|ui| {
        let view = &mut app.turning_workspace.spatial;
        if button(ui, "cam_3d", "cam.spatial_3d", !view.two_d) {
            view.two_d = false;
        }
        ui.add_enabled_ui(two_d_available, |ui| {
            if button(ui, "cam_2d", "cam.spatial_2d", view.two_d) {
                view.two_d = true;
                view.playing = false;
            }
        });
        ui.separator();
        ui.add_enabled_ui(available_frame, |ui| {
            if button(ui, "cam_half_section", "cam.spatial_section", view.section) {
                view.section = !view.section;
            }
            for (stage, key) in [(0, "cam.spatial_part"), (1, "cam.spatial_rough"), (2, "cam.spatial_stock")].into_iter().filter(|_| !simulation_mode)
            {
                ui.add_enabled_ui(stage != 1 || valid, |ui| {
                    if button(ui, &format!("cam_stage_{stage}"), key, view.stage == stage) {
                        view.stage = stage;
                    }
                });
            }
        });
    });
    if app.turning_workspace.spatial.two_d && super::machining_ui::active(app) {
        super::machining_ui::show(app, ui);
        return;
    }
    if !available_frame {
        ui.label(tr("cam.spatial_need_model"));
    }
    if simulation_mode {
        ui.small(tr("cam.sim_playback_hint"));
    }
    if !simulation_mode && app.turning_workspace.spatial.stage == 1 {
        ui.small(tr("cam.spatial_envelope"));
    }
    if app.turning_workspace.workflow == 1 {
        range_controls(app, ui);
    }
    if app.turning_workspace.workflow == 2 {
        let v = &app.turning_workspace.selection;
        ui.strong(if v.editing {
            if v.first.is_none() {
                "輪郭の始点をクリック"
            } else if v.last.is_none() {
                "輪郭の終点をクリック"
            } else {
                "右側で経路を確認して、工程へ追加"
            }
        } else {
            "左の工具を選ぶ → 中央で加工範囲を選ぶ"
        });
    }
    if valid && app.turning_workspace.machining.show && (!simulation_mode || super::simulation_ui::current(app).is_some()) {
        playback(app, ui);
    } else if app.turning_workspace.machining.preview.is_some() && !valid {
        app.turning_workspace.spatial.playing = false;
        ui.colored_label(Tokens::get().warning, tr("cam.path_stale"));
    }
    ui.separator();
    let rect = ui.available_rect_before_wrap();
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        crate::viewport::show(app, ui);
        overlay(app, ui);
    });
    if app.turning_workspace.machining.show_nc {
        let mut open = true;
        egui::Window::new(tr("cam.path_nc_view"))
            .id(egui::Id::new("cam_nc_window"))
            .open(&mut open)
            .default_pos(egui::pos2(rect.right() - 100., rect.top() + 50.))
            .default_width(440.)
            .default_height(380.)
            .resizable(true)
            .show(ui.ctx(), |ui| {
                super::machining_ui::contents(app, ui);
            });
        if !open {
            app.turning_workspace.machining.show_nc = false;
        }
    }
}

fn range_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    let Some(setup) = setup(app) else { return };
    let id = setup.id.clone();
    let end = setup.cut_end_z_mm;
    let available = can_pick_end(app);
    let mut reset = false;
    ui.horizontal_wrapped(|ui| {
        ui.label(tr("cam.spatial_pick_body"));
        ui.add_enabled_ui(available, |ui| {
            let view = &mut app.turning_workspace.spatial;
            if button(ui, "cam_pick_end", "cam.spatial_pick_end", view.pick_end) {
                view.pick_end = !view.pick_end;
                view.stage = 0;
            }
        });
        if end.is_some() {
            reset = ui_kit::button(ui, tr("cam.spatial_full_range"), ui_kit::Button::Normal).clicked();
        }
    });
    if let Some(end) = end {
        ui.monospace(format!("Z 0 → {end:.3} mm"));
    }
    if app.turning_workspace.spatial.pick_end {
        ui.colored_label(Tokens::get().accent, tr("cam.spatial_pick_hint"));
    }
    if let Some(error) = &app.turning_workspace.machining.error {
        ui.colored_label(Tokens::get().warning, error);
    }
    if reset {
        let result = app.run("cam.turning.range", json!({"setup":id,"end_z_mm":null,"expected_revision":app.session.revision}));
        app.turning_workspace.machining.error = result.err();
    }
}

fn playback(app: &mut SolveApp, ui: &mut egui::Ui) {
    let count = path(app).map_or(0, |p| p.moves.len());
    let state = &mut app.turning_workspace;
    ui.horizontal_wrapped(|ui| {
        if button(ui, "cam_play", if state.spatial.playing { "cam.spatial_pause" } else { "cam.spatial_play" }, state.spatial.playing) {
            state.spatial.playing = !state.spatial.playing;
            if state.simulation.focus_contact.take().is_some() && state.spatial.playing {
                state.machining.step = state.machining.step.saturating_add(1).min(count);
            }
            state.spatial.last_tick = ui.input(|i| i.time);
            if state.machining.step >= count {
                state.machining.step = 0;
            }
        }
        for (i, key) in ["cam.path_all", "cam.path_rough", "cam.path_finish_only"].iter().enumerate() {
            if controls::pick(ui, state.spatial.operation.is_none() && state.machining.phase == i, tr(key)).clicked() {
                state.machining.phase = i;
                state.spatial.operation = None;
            }
        }
        ui_kit::checkbox(ui, tr("cam.path_rapid"), &mut state.machining.show_rapid);
    });
    ui.horizontal(|ui| {
        if ui_kit::slider(
            ui,
            &mut state.machining.step,
            0..=count,
            ui_kit::Slider { integer: true, step: Some(1.0), format: |n| format!("{n:.0}"), ..Default::default() },
        )
        .changed()
        {
            state.simulation.focus_contact = None;
        }
        if let Some(kind) = state.spatial.operation {
            ui.label(tr(super::kind_label(kind)));
        }
    });
    if state.workflow != 4 {
        ui.small(tr("cam.spatial_nominal"));
    }
    if state.spatial.playing {
        let now = ui.input(|i| i.time);
        let advance = ((now - state.spatial.last_tick).max(0.) * 12.).floor() as usize;
        if advance > 0 {
            let next = state.machining.step.saturating_add(advance).min(count);
            let contact = if state.workflow == 4 && state.simulation.stop_on_contact {
                state
                    .simulation
                    .report
                    .as_ref()
                    .and_then(|(_, _, r)| r.contacts.iter().enumerate().find(|(_, c)| c.move_index >= state.machining.step && c.move_index < next))
                    .map(|(index, c)| (index, c.move_index))
            } else {
                None
            };
            if let Some((index, step)) = contact {
                state.machining.step = step;
                state.simulation.focus_contact = Some(index);
                state.spatial.playing = false;
            } else {
                state.machining.step = next;
            }
            state.spatial.last_tick = now;
        }
        state.spatial.playing = state.spatial.playing && state.machining.step < count;
        if state.spatial.playing {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
        }
    }
}

fn matches_operation(kind: OperationKind, phase: Phase) -> bool {
    matches!(
        (kind, phase),
        (OperationKind::CenterDrill, Phase::CenterDrill)
            | (OperationKind::Drill, Phase::Drill)
            | (OperationKind::OdGrooveRough, Phase::OdGrooveRough)
            | (OperationKind::OdGrooveFinish, Phase::OdGrooveFinish)
            | (OperationKind::FaceRough, Phase::FaceRough)
            | (OperationKind::FaceFinish, Phase::FaceFinish)
            | (OperationKind::OdRough, Phase::OdRough | Phase::RoughContour)
            | (OperationKind::OdFinish, Phase::OdFinish)
            | (OperationKind::IdRough, Phase::IdRough | Phase::IdRoughContour)
            | (OperationKind::IdFinish, Phase::IdFinish)
    )
}

fn simulation_pose(app: &SolveApp) -> Option<(ProfilePoint, Option<u8>, bool)> {
    let report = super::simulation_ui::current(app)?;
    let state = &app.turning_workspace;
    if let Some(index) = state.simulation.focus_contact
        && let Some(c) = report.contacts.get(index)
    {
        return Some((c.tip, Some(c.station), true));
    }
    let path = path(app)?;
    let m = if state.machining.step == 0 { path.moves.first()? } else { path.moves.get(state.machining.step - 1)? };
    let station = cam::machining::sequence::operation_at(path, setup(app)?, state.machining.step.saturating_sub(1))
        .map(|o| o.station)
        .unwrap_or_else(|| setup(app).and_then(|s| cam::simulation::station(s, m.phase)));
    Some((if state.machining.step == 0 { m.from } else { m.to }, station, false))
}

fn simulation_scene(app: &SolveApp) -> Option<GpuScene> {
    let report = super::simulation_ui::current(app)?;
    let frame = frame(app)?;
    let state = &app.turning_workspace;
    let slabs = report.stock_at(state.machining.step);
    let mut scene = GpuScene::default();
    stock_surface(&mut scene, frame, &slabs, Tokens::get().body.to_array());
    if let Some((tip, station, contact)) = simulation_pose(app) {
        let index =
            state.simulation.focus_contact.and_then(|i| report.contacts.get(i)).map_or(state.machining.step.saturating_sub(1), |c| c.move_index);
        let cutting = path(app)
            .and_then(|p| cam::machining::sequence::operation_at(p, setup(app)?, index))
            .and_then(|o| o.cutting.as_ref())
            .filter(|c| !matches!(c, cam::machining::sequence::Cutting::Contour(_)));
        if let Some(cutting) = cutting {
            local_cutter(&mut scene, frame, tip, cutting);
        }
        let tool = setup(app)?.turret.iter().find(|s| Some(s.station) == station).and_then(|s| s.simulation.as_ref());
        if let Some((insert, radius)) = tool.filter(|_| cutting.is_none()).and_then(|t| t.insert.as_ref().zip(t.nose_radius_mm))
            && let Ok(points) = insert.outline(radius)
        {
            let local = |p: cam::insert::Point, t| {
                frame.origin + frame.axis * (tip.z_mm + p[0]) + frame.radial * (tip.diameter_mm * 0.5 + p[1]) + frame.normal * t
            };
            let center = points.iter().fold([0., 0.], |a, p| [a[0] + p[0] / points.len() as f64, a[1] + p[1] / points.len() as f64]);
            let color = if contact { Tokens::get().error } else { Tokens::get().warning };
            for (a, b) in points.iter().zip(points.iter().cycle().skip(1)).take(points.len()) {
                let (a0, b0, a1, b1) = (local(*a, 0.), local(*b, 0.), local(*a, -insert.thickness_mm), local(*b, -insert.thickness_mm));
                for tri in [[local(center, 0.), a0, b0], [local(center, -insert.thickness_mm), b1, a1], [a0, a1, b1], [a0, b1, b0]] {
                    for p in tri {
                        scene.guide_tri(p.to_f32(), color.to_array());
                    }
                }
                for (a, b) in [(a0, b0), (a1, b1)] {
                    scene.guide_line(a.to_f32(), b.to_f32(), Tokens::get().text.to_array(), 1.);
                }
            }
        }
        if let Some(tool) = tool
            && let Some(h) = tool.holder.as_ref().filter(|h| h.ready())
        {
            let t = Tokens::get();
            let color = if contact {
                t.error
            } else if tool.holder_verified {
                t.accent
            } else {
                t.warning
            };
            let local = |z, r, y| frame.origin + frame.axis * (tip.z_mm + z) + frame.radial * (tip.diameter_mm * 0.5 + r) + frame.normal * y;
            let mut ring0 = vec![];
            let mut ring1 = vec![];
            match h.kind {
                cam::simulation::HolderKind::Round => {
                    for i in 0..32 {
                        let a = i as f64 * std::f64::consts::TAU / 32.;
                        let r = h.r_center_mm + h.width_mm * 0.5 * a.cos();
                        let y = h.t_center_mm + h.width_mm * 0.5 * a.sin();
                        ring0.push(local(h.z_start_mm, r, y));
                        ring1.push(local(h.z_start_mm + h.length_mm, r, y));
                    }
                }
                cam::simulation::HolderKind::Rectangular => {
                    for (r, y) in [(-1., -1.), (1., -1.), (1., 1.), (-1., 1.)] {
                        let r = h.r_center_mm + r * h.width_mm * 0.5;
                        let y = h.t_center_mm + y * h.height_mm * 0.5;
                        ring0.push(local(h.z_start_mm, r, y));
                        ring1.push(local(h.z_start_mm + h.length_mm, r, y));
                    }
                }
            }
            let centers = [local(h.z_start_mm, h.r_center_mm, h.t_center_mm), local(h.z_start_mm + h.length_mm, h.r_center_mm, h.t_center_mm)];
            for i in 0..ring0.len() {
                let next = (i + 1) % ring0.len();
                let Some((a, b)) = ring0.get(i).zip(ring0.get(next)) else { continue };
                let Some((c, d)) = ring1.get(i).zip(ring1.get(next)) else { continue };
                let [front, back] = centers;
                for triangle in [[*a, *c, *d], [*a, *d, *b], [front, *b, *a], [back, *c, *d]] {
                    for p in triangle {
                        scene.guide_tri(p.to_f32(), color.gamma_multiply(0.8).to_array());
                    }
                }
                for (a, b) in [(a, b), (c, d), (a, c)] {
                    scene.guide_line(a.to_f32(), b.to_f32(), t.text.to_array(), 1.);
                }
            }
        }
    }
    Some(scene)
}

/// Wire representation of the same idealised cutting envelope used by stock replay.
fn local_cutter(scene: &mut GpuScene, frame: Frame, tip: ProfilePoint, cutting: &cam::machining::sequence::Cutting) {
    use cam::machining::sequence::Cutting;
    let color = Tokens::get().warning.to_array();
    let mut line = |a: Vec3, b: Vec3| scene.guide_line(a.to_f32(), b.to_f32(), color, 2.);
    match cutting {
        Cutting::Contour(_) => {}
        Cutting::Drill(d) => {
            let shoulder = tip.z_mm + d.point_length();
            let back = shoulder + 10.; // Display stub only; flute length is not a collision model.
            for i in 0..32 {
                let a = i as f64 * std::f64::consts::TAU / 32.;
                let b = (i + 1) as f64 * std::f64::consts::TAU / 32.;
                line(frame.ring(shoulder, d.diameter_mm, a), frame.ring(shoulder, d.diameter_mm, b));
                line(frame.ring(back, d.diameter_mm, a), frame.ring(back, d.diameter_mm, b));
                if i % 4 == 0 {
                    line(frame.point(tip), frame.ring(shoulder, d.diameter_mm, a));
                    line(frame.ring(shoulder, d.diameter_mm, a), frame.ring(back, d.diameter_mm, a));
                }
            }
        }
        Cutting::OdGroove(g) => {
            let a = frame.point(tip);
            let b = a - frame.axis * g.tool_width_mm;
            line(a, b);
            line(a, a + frame.radial * 5.);
            line(b, b + frame.radial * 5.);
            line(a + frame.radial * 5., b + frame.radial * 5.);
        }
    }
}

// Build the boundary of the union, including separate radial bands. Joining unrelated
// bands into a single OD/ID outline creates self intersections and broken section caps.
fn stock_edges(slabs: &[cam::simulation::Slab]) -> Vec<([f64; 2], [f64; 2])> {
    let mut groups: Vec<&[cam::simulation::Slab]> = vec![];
    let mut start = 0;
    while let Some(first) = slabs.get(start) {
        let end = start + slabs.iter().skip(start).take_while(|s| s.z0 == first.z0 && s.z1 == first.z1).count();
        if let Some(group) = slabs.get(start..end) {
            groups.push(group);
        }
        start = end;
    }
    let mut edges = vec![];
    for (index, group) in groups.iter().enumerate() {
        for s in *group {
            if s.outer <= s.inner {
                continue;
            }
            edges.push(([s.z1, s.outer], [s.z0, s.outer]));
            edges.push(([s.z0, s.inner], [s.z1, s.inner]));
            for front in [false, true] {
                let z = if front { s.z1 } else { s.z0 };
                let neighbor = if front { groups.get(index + 1) } else { index.checked_sub(1).and_then(|i| groups.get(i)) };
                let mut exposed = vec![[s.inner, s.outer]];
                for n in neighbor.into_iter().flat_map(|g| g.iter()).filter(|n| ((if front { n.z0 } else { n.z1 }) - z).abs() < 1e-8) {
                    let mut next = vec![];
                    for [lo, hi] in exposed {
                        if hi <= n.inner || lo >= n.outer {
                            next.push([lo, hi]);
                        } else {
                            if lo < n.inner {
                                next.push([lo, n.inner]);
                            }
                            if hi > n.outer {
                                next.push([n.outer, hi]);
                            }
                        }
                    }
                    exposed = next;
                }
                for [lo, hi] in exposed {
                    edges.push(if front { ([z, lo], [z, hi]) } else { ([z, hi], [z, lo]) });
                }
            }
        }
    }
    edges
}

fn stock_surface(scene: &mut GpuScene, frame: Frame, slabs: &[cam::simulation::Slab], color: [u8; 4]) {
    for (a, b) in stock_edges(slabs) {
        for i in 0..48 {
            let theta = i as f64 * std::f64::consts::TAU / 48.;
            let next = (i + 1) as f64 * std::f64::consts::TAU / 48.;
            let [a, b, c, d] = [
                frame.ring(a[0], a[1] * 2., theta),
                frame.ring(b[0], b[1] * 2., theta),
                frame.ring(b[0], b[1] * 2., next),
                frame.ring(a[0], a[1] * 2., next),
            ];
            for [a, b, c] in [[a, b, c], [a, c, d]] {
                let Some(n) = (b - a).cross(c - a).normalized() else { continue };
                for p in [a, b, c] {
                    scene.tri(p.to_f32(), n.to_f32(), color);
                }
            }
        }
    }
}

fn overlay(app: &SolveApp, ui: &egui::Ui) {
    let Some(frame) = frame(app) else { return };
    let Some(rect) = app.viewport.rect else { return };
    let proj = crate::viewport::projection(app, rect);
    let paint = ui.painter_at(rect);
    let t = Tokens::get();
    let line = |a: ProfilePoint, b: ProfilePoint, color, width| {
        if let (Some(a), Some(b)) = (proj.to_screen(frame.point(a)), proj.to_screen(frame.point(b))) {
            paint.line_segment([a, b], Stroke::new(width, color));
        }
    };
    let selection = &app.turning_workspace.selection;
    if selection.show_difference && app.turning_workspace.workflow != 4 {
        let residual = selection
            .preview
            .as_ref()
            .filter(|(r, id, _)| *r == app.session.revision && setup(app).is_some_and(|s| s.id == *id))
            .map(|(_, _, p)| &p.residual)
            .or_else(|| {
                selection
                    .difference
                    .as_ref()
                    .filter(|(r, id, _)| *r == app.session.revision && setup(app).is_some_and(|s| s.id == *id))
                    .map(|(_, _, d)| d)
            });
        if let (Some(residual), Some(s)) = (residual, setup(app)) {
            for slab in &residual.slabs {
                let z = (slab.z0 + slab.z1) * 0.5;
                let od = s.od_job.as_ref().and_then(|j| cam::machining::diameter_at(&j.profile, z)).map(|d| d * 0.5);
                let id = s.id_job.as_ref().and_then(|j| cam::machining::diameter_at(&j.profile, z)).map(|d| d * 0.5);
                let bands = [
                    od.map(|r| (r.max(slab.inner), slab.outer)).or_else(|| (z > 0.).then_some((slab.inner, slab.outer))),
                    id.map(|r| (slab.inner, r.min(slab.outer))),
                ];
                for (a, b) in bands.into_iter().flatten().filter(|(a, b)| b > a) {
                    let points = [(slab.z0, a), (slab.z1, a), (slab.z1, b), (slab.z0, b)]
                        .into_iter()
                        .filter_map(|(z, r)| proj.to_screen(frame.point(ProfilePoint { z_mm: z, diameter_mm: 2. * r })))
                        .collect::<Vec<_>>();
                    if points.len() == 4 {
                        paint.add(egui::Shape::convex_polygon(points, t.warning.gamma_multiply(0.25), Stroke::NONE));
                    }
                }
            }
            paint.text(
                rect.left_top() + egui::vec2(12., 12.),
                Align2::LEFT_TOP,
                "橙：削る部分（名目断面の近似）",
                FontId::proportional(Tokens::get().font_small),
                t.warning,
            );
        }
    }
    if let Some((_, profile)) = profile(app) {
        if let Some(last) = profile.last() {
            let front = setup(app).and_then(|s| s.od_job.as_ref().or(s.id_job.as_ref())).map_or(5., |j| j.stock_front_z_mm + j.clearance_mm + 3.);
            if let (Some(a), Some(b)) = (
                proj.to_screen(frame.point(ProfilePoint { z_mm: front, diameter_mm: 0. })),
                proj.to_screen(frame.point(ProfilePoint { z_mm: last.z_mm, diameter_mm: 0. })),
            ) {
                paint.arrow(a, b - a, Stroke::new(1., t.text_dim));
                paint.text(a, Align2::LEFT_BOTTOM, "進入側 → −Z", FontId::proportional(Tokens::get().font_small), t.text_dim);
            }
        }
        for pair in profile.windows(2) {
            if let [a, b] = pair {
                let selected = setup(app).and_then(|s| s.cut_end_z_mm).is_none_or(|end| b.z_mm >= end - 1e-8);
                line(*a, *b, if selected { t.accent } else { t.text_dim.gamma_multiply(0.4) }, 2.);
            }
        }
        if app.turning_workspace.spatial.pick_end {
            for p in profile.iter().filter(|p| p.z_mm < -0.001) {
                if let Some(pos) = proj.to_screen(frame.point(*p)) {
                    paint.circle_filled(pos, 4., t.accent);
                }
            }
        }
        if app.turning_workspace.workflow == 2 && selection.editing {
            let range = selection.first.zip(selection.last).map(|(a, b)| (a.min(b), a.max(b)));
            for (i, pair) in profile.windows(2).enumerate() {
                if range.is_some_and(|(a, b)| i >= a && i < b)
                    && let [a, b] = pair
                {
                    line(*a, *b, t.axis_y, 4.);
                }
            }
            for (i, p) in profile.iter().enumerate() {
                if let Some(pos) = proj.to_screen(frame.point(*p)) {
                    let selected = selection.first == Some(i) || selection.last == Some(i);
                    paint.circle_filled(pos, if selected { 6. } else { 4. }, if selected { t.axis_y } else { t.accent });
                    paint.text(
                        pos + egui::vec2(6., -8.),
                        Align2::LEFT_BOTTOM,
                        format!("{}", i + 1),
                        FontId::proportional(Tokens::get().font_small),
                        t.text,
                    );
                    crate::scenario::publish_handle(&format!("cam_contour_point_{i}"), pos);
                }
            }
        }
    }
    if let Some(setup) = setup(app)
        && let Some(stock) = &setup.stock
    {
        let radius = stock.outer_diameter_mm * 0.5;
        for z in [0., setup.cut_end_z_mm.unwrap_or_else(|| profile(app).and_then(|(_, p)| p.last()).map_or(0., |p| p.z_mm))] {
            let a = ProfilePoint { z_mm: z, diameter_mm: -2. * radius };
            let b = ProfilePoint { z_mm: z, diameter_mm: 2. * radius };
            line(a, b, t.text_dim, 1.);
            if let Some(p) = proj.to_screen(frame.point(b)) {
                paint.text(p, Align2::LEFT_BOTTOM, format!("Z {z:.3}"), FontId::proportional(Tokens::get().font_small), t.text);
            }
        }
    }
    let state = &app.turning_workspace;
    if !state.machining.show {
        return;
    }
    let Some(path) = path(app) else { return };
    if let Some((tip, station, contact)) = simulation_pose(app) {
        let color = if contact { t.error } else { t.accent };
        if let Some(p) = proj.to_screen(frame.point(tip)) {
            paint.circle_stroke(p, 8., Stroke::new(2., color));
        }
        paint.text(
            rect.left_bottom() + egui::vec2(12., -12.),
            Align2::LEFT_BOTTOM,
            format!(
                "{}  X Ø{:.3}  Z {:.3}",
                station.map(|n| format!("T{n:02}")).unwrap_or_else(|| tr("cam.sim_tool_change").into()),
                tip.diameter_mm,
                tip.z_mm
            ),
            FontId::monospace(Tokens::get().font_body),
            color,
        );
    }
    for (index, m) in path.moves.iter().take(state.machining.step).enumerate() {
        if state.workflow == 2
            && !state.selection.editing
            && !path.operation_spans.is_empty()
            && let Some(setup) = setup(app)
            && let Some(selected) = setup.operations.get(state.operation)
            && cam::machining::sequence::operation_at(path, setup, index).is_none_or(|op| op.id != selected.id)
        {
            continue;
        }
        let finish = cam::machining::interior::is_finish(m.phase);
        if (state.machining.phase == 1 && finish)
            || (state.machining.phase == 2 && !finish)
            || (m.motion == Motion::Rapid && !state.machining.show_rapid)
            || state.spatial.operation.is_some_and(|kind| !matches_operation(kind, m.phase))
        {
            continue;
        }
        line(
            m.from,
            m.to,
            if m.motion == Motion::Rapid {
                t.text_dim.gamma_multiply(0.6)
            } else if finish {
                t.axis_y
            } else {
                t.warning
            },
            if finish { 2. } else { 1. },
        );
    }
    if state.workflow != 4
        && let Some(m) = state.machining.step.checked_sub(1).and_then(|n| path.moves.get(n))
    {
        if let Some(p) = proj.to_screen(frame.point(m.to)) {
            paint.circle_filled(p, 5., t.accent);
            paint.circle_stroke(p, 8., Stroke::new(1.5, t.text));
        }
        paint.text(
            rect.left_bottom() + egui::vec2(12., -12.),
            Align2::LEFT_BOTTOM,
            format!("X Ø{:.3}   Z {:.3}", m.to.diameter_mm, m.to.z_mm),
            FontId::monospace(Tokens::get().font_body),
            t.text,
        );
    }
}

/// Intercept only CAM picks; ordinary navigation remains viewport::navigation::handle.
pub fn click(app: &mut SolveApp, hit: Option<&Hit>, pos: Pos2, proj: &Proj) -> bool {
    if super::catalog_ui::preview(app).is_some() {
        return true;
    }
    if !super::active(app) {
        return false;
    }
    if app.turning_workspace.workflow == 2 && app.turning_workspace.selection.editing && app.turning_workspace.selection.picking {
        let picked = profile(app).and_then(|(source, points)| {
            let frame = Frame::new(source)?;
            points
                .iter()
                .enumerate()
                .filter_map(|(i, p)| Some((i, proj.to_screen(frame.point(*p))?.distance(pos))))
                .filter(|(_, d)| *d < 18.)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i)
        });
        if let Some(i) = picked {
            super::selection_ui::pick(&mut app.turning_workspace, i);
        }
        return true;
    }
    if app.turning_workspace.spatial.pick_end && app.turning_workspace.workflow == 1 {
        if !can_pick_end(app) {
            app.turning_workspace.spatial.pick_end = false;
            return true;
        }
        let picked = profile(app).and_then(|(source, points)| {
            let frame = Frame::new(source)?;
            points
                .iter()
                .filter(|p| p.z_mm < -0.001)
                .filter_map(|p| Some((p.z_mm, proj.to_screen(frame.point(*p))?.distance(pos))))
                .filter(|(_, d)| *d < 18.)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|p| p.0)
        });
        if let Some(end) = picked
            && let Some(id) = setup(app).map(|s| s.id.clone())
        {
            let result = app.run("cam.turning.range", json!({"setup":id,"end_z_mm":end,"expected_revision":app.session.revision}));
            app.turning_workspace.spatial.pick_end = result.is_err();
            app.turning_workspace.machining.error = result.err();
        }
        return true;
    }
    if app.turning_workspace.spatial.stage != 0 {
        return true;
    }
    if app.turning_workspace.workflow == 1 {
        let body = match hit {
            Some(Hit::Face { body, .. } | Hit::Edge { body, .. } | Hit::Vertex { body, .. }) => Some(body),
            _ => None,
        };
        if let Some(body) = body {
            app.turning_workspace.model_profile.pick_body(body);
        }
        return true;
    }
    false
}

/// Closed planned envelope, bounded to 96 angular slices per profile segment.
/// Cached by the viewport; simulation stock includes the playhead in its scene key.
pub fn stage_scene(app: &SolveApp) -> Option<GpuScene> {
    if super::catalog_ui::preview(app).is_some() {
        return super::catalog_ui::scene(app);
    }
    if !super::active(app) {
        return None;
    }
    if app.turning_workspace.workflow == 4 {
        return simulation_scene(app);
    }
    let stage = app.turning_workspace.spatial.stage;
    if stage == 0 {
        return None;
    }
    let frame = frame(app)?;
    let setup = setup(app)?;
    if stage == 1
        && let Some(path) = path(app)
        && path.rough_profile.is_empty()
        && let Some(source) = &path.source_setup
    {
        let residual = cam::machining::selection::remaining(source).ok()?;
        let mut scene = GpuScene::default();
        stock_surface(&mut scene, frame, &residual.slabs, Tokens::get().body.to_array());
        return Some(scene);
    }
    let stock = setup.stock.as_ref()?;
    let job = setup.od_job.as_ref().or(setup.id_job.as_ref())?;
    let front = job.stock_front_z_mm;
    let back = front - stock.length_mm;
    let pp = |z_mm, diameter_mm| ProfilePoint { z_mm, diameter_mm };
    let (mut od, mut id) = if stage == 1 {
        let path = path(app)?;
        let front = if path.inner { front } else { path.rough_profile.first()?.z_mm };
        let od = if path.inner { vec![pp(front, stock.outer_diameter_mm), pp(back, stock.outer_diameter_mm)] } else { path.rough_profile.clone() };
        let id = if path.inner {
            path.rough_profile.clone()
        } else if let Some(inner) = &path.id_path {
            inner.rough_profile.clone()
        } else {
            vec![pp(front, stock.inner_diameter_mm), pp(back, stock.inner_diameter_mm)]
        };
        (od, id)
    } else {
        (
            vec![pp(front, stock.outer_diameter_mm), pp(back, stock.outer_diameter_mm)],
            vec![pp(front, stock.inner_diameter_mm), pp(back, stock.inner_diameter_mm)],
        )
    };
    for (points, diameter) in [(&mut od, stock.outer_diameter_mm), (&mut id, stock.inner_diameter_mm)] {
        if let Some(last) = points.last().copied()
            && last.z_mm > back + 1e-8
        {
            points.push(pp(last.z_mm, diameter));
            points.push(pp(back, diameter));
        }
    }
    // An ID-only operation does not face the stock: the entrance stays bored up to its raw front.
    if let Some(outer_front) = od.first()
        && let Some(inner_front) = id.first().copied()
        && inner_front.z_mm < outer_front.z_mm
    {
        id.insert(0, pp(outer_front.z_mm, inner_front.diameter_mm));
    }
    // Traverse the outside towards the chuck and the inside back towards the front.
    let mut outline = od;
    outline.extend(id.into_iter().rev());
    if let Some(first) = outline.first().copied() {
        outline.push(first);
    }
    let mut scene = GpuScene::default();
    let color = if stage == 1 { Tokens::get().warning } else { Tokens::get().body }.to_array();
    for pair in outline.windows(2) {
        let [a, b] = pair else { continue };
        for i in 0..96 {
            let theta = i as f64 * std::f64::consts::TAU / 96.;
            let next = (i + 1) as f64 * std::f64::consts::TAU / 96.;
            let points = [
                frame.ring(a.z_mm, a.diameter_mm, theta),
                frame.ring(b.z_mm, b.diameter_mm, theta),
                frame.ring(b.z_mm, b.diameter_mm, next),
                frame.ring(a.z_mm, a.diameter_mm, next),
            ];
            let [a, b, c, d] = points;
            for [a, b, c] in [[a, b, c], [a, c, d]] {
                let Some(normal) = (b - a).cross(c - a).normalized() else { continue };
                for p in [a, b, c] {
                    scene.tri([p.x as f32, p.y as f32, p.z as f32], [normal.x as f32, normal.y as f32, normal.z as f32], color);
                }
            }
        }
    }
    Some(scene)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Harness;

    #[test]
    fn turning_stock_surface_preserves_disconnected_bands_without_internal_caps() {
        use cam::simulation::Slab;
        let slabs = [
            Slab { z0: -2., z1: -1., inner: 2., outer: 10. },
            Slab { z0: -1., z1: 0., inner: 2., outer: 6. },
            Slab { z0: -1., z1: 0., inner: 8., outer: 10. },
            Slab { z0: 0., z1: 1., inner: 2., outer: 10. },
        ];
        let edges = stock_edges(&slabs);
        let caps: Vec<_> = edges.iter().filter(|(a, b)| a[0] == b[0] && a[0].abs() < 1.5).copied().collect();
        assert!(caps.contains(&([-1., 6.], [-1., 8.])));
        assert!(caps.contains(&([0., 8.], [0., 6.])));
        assert_eq!(caps.len(), 3); // two groove walls and the front end
        // Integral of radius squared along the oriented boundary gives annular volume / pi.
        let volume: f64 = edges.iter().map(|(a, b)| -(b[0] - a[0]) * a[1] * a[1]).sum();
        assert!((volume - (96. * 3. - 28.)).abs() < 1e-8);
    }

    fn click_button(h: &mut Harness, id: &str) {
        h.frames(2);
        let p = crate::scenario::rect_of(id).unwrap().center();
        h.step(&json!({"click":[p.x,p.y]})).unwrap();
        h.frames(2);
    }

    #[test]
    fn three_d_range_section_stages_and_nc_use_the_same_revision() {
        let mut h = Harness::new();
        let script: serde_json::Value = serde_json::from_str(include_str!("../../../../examples/turning-inner-profile.json")).unwrap();
        for c in script["commands"].as_array().unwrap() {
            h.app.run(c["command"].as_str().unwrap(), c["params"].clone()).unwrap();
        }
        for inner in [true, false] {
            h.app
                .run(
                    "cam.turning.profile.apply",
                    json!({"body":"段付き貫通穴","inner":inner,"setup":"setup-1","expected_revision":h.app.session.revision}),
                )
                .unwrap();
            if inner {
                assert!(frame(&h.app).is_some(), "a linked ID supplies the shared frame even when OD is manual");
            }
        }
        h.app.home.open = false;
        super::super::enter(&mut h.app, "turning").unwrap();
        click_button(&mut h, "cam_step_3");
        click_button(&mut h, "cam_path_calculate");
        assert!(!h.app.turning_workspace.spatial.two_d);
        click_button(&mut h, "cam_half_section");
        assert!(section_plane(&h.app).is_some());
        assert!(h.app.session.section.is_none());
        click_button(&mut h, "cam_stage_1");
        assert!(stage_scene(&h.app).is_some());
        let key = scene_key(&h.app);
        click_button(&mut h, "cam_play");
        assert_eq!(scene_key(&h.app), key, "playhead must not invalidate the GPU stage mesh");
        click_button(&mut h, "cam_play");
        click_button(&mut h, "cam_path_nc_view");
        assert!(h.widget("O9001 (SOLIDCRAFT REVIEW ONLY)").is_some());
        click_button(&mut h, "cam_path_back_paths");
        click_button(&mut h, "cam_step_1");
        click_button(&mut h, "cam_pick_end");
        let (source, points) = profile(&h.app).unwrap();
        let point = points.iter().find(|p| p.z_mm < -0.001 && p.z_mm > -59.).copied().unwrap();
        let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
        let pos = proj.to_screen(Frame::new(source).unwrap().point(point)).unwrap();
        assert!(click(&mut h.app, None, pos, &proj));
        assert_eq!(setup(&h.app).unwrap().cut_end_z_mm, Some(point.z_mm));
        assert!(path(&h.app).is_none());
        h.app.turning_workspace.spatial.stage = 1;
        assert!(stage_scene(&h.app).is_none(), "stale rough shape must disappear");
        click_button(&mut h, "cam_step_3");
        click_button(&mut h, "cam_path_calculate");
        let path = path(&h.app).unwrap();
        assert_eq!(path.job.profile.last().unwrap().z_mm, point.z_mm);
        assert_eq!(path.id_path.as_ref().unwrap().job.profile.last().unwrap().z_mm, point.z_mm);
        assert!(path.moves.iter().all(|m| m.to.z_mm >= point.z_mm - 1e-8));
        super::super::enter(&mut h.app, "design").unwrap();
        assert!(section_plane(&h.app).is_none());
        assert!(h.app.session.section.is_none());
    }

    #[test]
    fn diameter_world_frame_and_reverse_are_respected() {
        for axis in [[1., 0., 0.], [-1., 0., 0.], [0., 0., 1.]] {
            let frame =
                Frame::new(&ModelSource { body: "test".into(), reverse: false, origin: [10., 20., 30.], axis, approximation_mm: 0. }).unwrap();
            let p = frame.point(ProfilePoint { z_mm: -12., diameter_mm: 40. }) - frame.origin;
            assert!((p.dot(frame.axis) + 12.).abs() < 1e-9);
            assert!((p.dot(frame.radial) - 20.).abs() < 1e-9);
            assert!(p.dot(frame.normal).abs() < 1e-9);
        }
    }
}
