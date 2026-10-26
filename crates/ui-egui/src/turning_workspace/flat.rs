//! Horizontal Z/radius canvas. Navigation is shared with CAD, with orbit disabled.
use super::*;
use cam::{
    drawing::{Drawing, Side},
    machining::{Motion, ProfilePoint},
};
use egui::{Align2, FontId, Rect, Stroke, pos2, vec2};
use solidcraft_engine::{geom::Vec3, render::Camera};

#[derive(Clone, Debug, Default)]
pub(super) struct View {
    camera: Camera,
    fitted: Option<(String, [u64; 4], u64, usize)>,
    size: egui::Vec2,
    click_frame: Option<u64>,
}

impl View {
    pub(super) fn refit(&mut self) {
        self.fitted = None;
    }
}

pub(super) fn drawing(setup: &Setup) -> Drawing {
    if let Some(d) = &setup.drawing {
        return d.clone();
    }
    let outer = setup.od_job.as_ref().map_or(&[][..], |j| j.profile.as_slice());
    let inner = setup.id_job.as_ref().map(|j| j.profile.clone()).unwrap_or_else(|| {
        outer
            .first()
            .zip(outer.last())
            .map(|(a, b)| vec![ProfilePoint { diameter_mm: 0., ..*a }, ProfilePoint { diameter_mm: 0., ..*b }])
            .unwrap_or_default()
    });
    Drawing::new(outer, inner)
}

pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let step = app.turning_workspace.workflow;
    ui.painter().rect_filled(ui.max_rect(), 0., Tokens::get().panel);
    ui.horizontal_wrapped(|ui| {
        ui.strong(match step {
            1 => "輪郭 · 最初に加工する側 →",
            0 => "材料と完成形状",
            2 => "工具 → 加工範囲を選択",
            _ => "切削シミュレーション",
        });
        let fit = ui_kit::button(ui, tr("ui.fit"), ui_kit::Button::Small);
        crate::scenario::publish_rect("cam_flat_fit", fit.rect);
        crate::scenario::publish_handle("cam_flat_fit", fit.rect.center());
        if fit.clicked() {
            app.turning_workspace.flat.refit();
        }
    });
    if step == 4 && super::simulation_ui::current(app).is_some() {
        super::spatial::playback(app, ui);
    }
    let Some(setup) = app.session.doc.turning.as_ref().and_then(|p| p.setups.get(app.turning_workspace.setup)) else {
        ui.label("モデルを用意して、旋削CAMを開始してください。");
        return;
    };
    let d = drawing(setup);
    let has_contour = !d.segments.is_empty();
    let stock = (step != 1).then_some(setup.stock.as_ref()).flatten();
    if !has_contour && stock.is_none() {
        ui.label("中心軸と輪郭を取得すると、ここに断面が表示されます。");
        return;
    }
    let part_back = d.segments.iter().flat_map(|s| [s.a.z_mm, s.b.z_mm]).fold(0., f64::min);
    let job = setup.od_job.as_ref().or(setup.id_job.as_ref());
    // Local-only jobs have the same raw-stock/placement model as the planner. They
    // need a canvas even though there is deliberately no finished contour to draw.
    let placement = job.map(|job| [job.stock_front_z_mm, job.chuck_front_z_mm, job.clearance_mm]).or_else(|| {
        let limits = setup.operations.iter().find_map(|operation| operation.cutting.as_ref())?.limits();
        Some([limits.stock_front_z_mm, limits.chuck_front_z_mm, limits.clearance_mm])
    });
    let front = placement.map_or(0., |[front, _, _]| front);
    let zmin = part_back.min(stock.map_or(0., |s| front - s.length_mm));
    let part_diameter = d.segments.iter().flat_map(|s| [s.a.diameter_mm, s.b.diameter_mm]).fold(1., f64::max);
    let diameter = part_diameter.max(stock.map_or(0., |s| s.outer_diameter_mm));
    let zmax = if step == 1 { 3. } else { placement.map_or(3., |[front, _, clearance]| front + clearance + 3.) };
    if has_contour {
        ui.add(
            egui::Label::new(egui::RichText::new(format!("全長 {:.3} mm · 最大径 Φ{:.3} mm · 右端 Z0", -part_back, part_diameter)).small()).wrap(),
        );
    } else if let Some(stock) = stock {
        let summary = format!(
            "{} {:.3} mm · {} Φ{:.3} mm · {} Φ{:.3} mm",
            tr("cam.stock_length"),
            stock.length_mm,
            tr("cam.stock_od"),
            stock.outer_diameter_mm,
            tr("cam.stock_id"),
            stock.inner_diameter_mm,
        );
        ui.add(egui::Label::new(egui::RichText::new(summary).small()).wrap());
        let response = ui.add(egui::Label::new(tr("cam.flat_no_target")).wrap());
        crate::scenario::publish_rect("cam_flat_no_target", response.rect);
    }
    let mut bounds = [zmin - 3., zmax, -diameter * 0.65, diameter * 0.65];
    if step == 4
        && let Some([tool_zmin, tool_zmax, tool_rmin, tool_rmax]) = super::spatial::simulation_bounds(app)
    {
        bounds = [bounds[0].min(tool_zmin), bounds[1].max(tool_zmax), bounds[2].min(tool_rmin), bounds[3].max(tool_rmax)];
    }
    let generation = if step == 4 { app.turning_workspace.machining.generation } else { 0 };
    let key = (setup.id.clone(), bounds.map(f64::to_bits), generation, step);
    let (rect, response) = ui.allocate_exact_size(ui.available_size().max(vec2(1., 1.)), egui::Sense::click_and_drag());
    crate::scenario::publish_rect("cam_flat_canvas", rect);
    if app.turning_workspace.flat.fitted.as_ref() != Some(&key) || app.turning_workspace.flat.size != rect.size() {
        let camera = &mut app.turning_workspace.flat.camera;
        *camera = Camera {
            yaw: 0.,
            pitch: 0.,
            target: Vec3::new((bounds[0] + bounds[1]) * 0.5, 0., (bounds[2] + bounds[3]) * 0.5),
            distance: (bounds[3] - bounds[2]).max((bounds[1] - bounds[0]) * rect.height() as f64 / rect.width().max(1.) as f64) * 1.5,
            ..Camera::default()
        };
        app.turning_workspace.flat.fitted = Some(key);
        app.turning_workspace.flat.size = rect.size();
    }
    let design_camera = std::mem::replace(&mut app.cam, app.turning_workspace.flat.camera);
    crate::navigation::handle(app, ui, rect, &response, crate::navigation::NavCaps { orbit: false, pick: false });
    app.turning_workspace.flat.camera = std::mem::replace(&mut app.cam, design_camera);
    let camera = app.turning_workspace.flat.camera;
    let scale = rect.height() as f64 / (2. * camera.half_height()).max(1e-6);
    let map = |p: ProfilePoint| {
        pos2(
            rect.center().x + ((p.z_mm - camera.target.x) * scale) as f32,
            rect.center().y - ((p.diameter_mm * 0.5 - camera.target.z) * scale) as f32,
        )
    };
    let painter = ui.painter_at(rect);
    let t = Tokens::get();
    let remaining_color = t.panel.lerp_to_gamma(t.body, 0.3);
    let difference_color =
        if has_contour && app.turning_workspace.selection.show_difference { t.panel.lerp_to_gamma(t.warning, 0.3) } else { remaining_color };
    painter.rect_filled(rect, 0., t.panel);
    let Some(setup) = app.session.doc.turning.as_ref().and_then(|p| p.setups.get(app.turning_workspace.setup)) else { return };
    let slab = |z0: f64, z1: f64, r0: f64, r1: f64, color| {
        for sign in [1., -1.] {
            painter.rect_filled(
                Rect::from_two_pos(
                    map(ProfilePoint { z_mm: z0, diameter_mm: 2. * r0 * sign }),
                    map(ProfilePoint { z_mm: z1, diameter_mm: 2. * r1 * sign }),
                ),
                0.,
                color,
            );
        }
    };
    let sim = super::simulation_ui::current(app);
    if let Some(sim) = sim {
        // Adjacent equal bands are one rectangle: no antialiased seams between stock samples.
        for s in joined_bands(sim.stock_at(app.turning_workspace.machining.step)) {
            slab(s.z0, s.z1, s.inner, s.outer, t.body);
        }
    } else if step != 1
        && let Some(stock) = &setup.stock
    {
        slab(front - stock.length_mm, front, stock.inner_diameter_mm * 0.5, stock.outer_diameter_mm * 0.5, difference_color);
        if has_contour && front - stock.length_mm < part_back {
            slab(front - stock.length_mm, part_back, stock.inner_diameter_mm * 0.5, stock.outer_diameter_mm * 0.5, remaining_color);
        }
    }
    // Fill the outer half-section with trapezoids, then cut out its inner boundary.
    if sim.is_none() {
        for s in &d.segments {
            if !matches!(s.side, Side::Outer | Side::Inner) || (s.a.z_mm - s.b.z_mm).abs() < 1e-8 {
                continue;
            }
            let color = if s.side == Side::Outer { t.accent_soft } else { t.panel };
            for sign in [1., -1.] {
                let a = ProfilePoint { diameter_mm: s.a.diameter_mm * sign, ..s.a };
                let b = ProfilePoint { diameter_mm: s.b.diameter_mm * sign, ..s.b };
                painter.add(egui::Shape::convex_polygon(
                    vec![map(a), map(b), map(ProfilePoint { diameter_mm: 0., ..b }), map(ProfilePoint { diameter_mm: 0., ..a })],
                    color,
                    Stroke::NONE,
                ));
            }
            if s.side == Side::Inner
                && step != 1
                && let Some(stock) = &setup.stock
            {
                let region = cavity_stock_polygon(s.a, s.b, stock, front);
                if region.len() >= 3 {
                    for sign in [1., -1.] {
                        painter.add(egui::Shape::convex_polygon(
                            region.iter().map(|p| map(ProfilePoint { diameter_mm: p.diameter_mm * sign, ..*p })).collect(),
                            difference_color,
                            Stroke::NONE,
                        ));
                    }
                }
            }
        }
        if step != 1
            && let Some(stock) = &setup.stock
        {
            slab(front - stock.length_mm, front, 0., stock.inner_diameter_mm * 0.5, t.panel);
        }
    }
    if step == 0
        && let Some(coverage) = super::stock_ui::coverage(&app.turning_workspace, app.session.revision, &setup.id)
    {
        for shortage in &coverage.shortages {
            for sign in [1., -1.] {
                let polygon = [
                    (shortage.z0_mm, shortage.inner0_mm),
                    (shortage.z1_mm, shortage.inner1_mm),
                    (shortage.z1_mm, shortage.outer1_mm),
                    (shortage.z0_mm, shortage.outer0_mm),
                ]
                .map(|(z_mm, radius)| map(ProfilePoint { z_mm, diameter_mm: radius * 2. * sign }));
                painter.add(egui::Shape::convex_polygon(polygon.to_vec(), t.panel.lerp_to_gamma(t.error, 0.6), Stroke::new(2., t.error)));
            }
        }
        if !coverage.shortages.is_empty() {
            painter.text(
                rect.left_top() + vec2(12., 12.),
                Align2::LEFT_TOP,
                "材料不足：赤い部分は削り出せません",
                FontId::proportional(t.font_body),
                t.error,
            );
        }
    }
    let axis = [map(ProfilePoint { z_mm: zmin - 3., diameter_mm: 0. }), map(ProfilePoint { z_mm: zmax + 3., diameter_mm: 0. })];
    painter.line_segment(axis, Stroke::new(1., t.text_dim));
    painter.text(axis[1], Align2::RIGHT_TOP, "中心軸 / Z →", FontId::proportional(t.font_small), t.text_dim);
    let origin = map(ProfilePoint { z_mm: 0., diameter_mm: 0. });
    painter.line_segment([origin - vec2(5., 0.), origin + vec2(5., 0.)], Stroke::new(2., t.text));
    painter.line_segment([origin - vec2(0., 5.), origin + vec2(0., 5.)], Stroke::new(2., t.text));
    painter.text(origin + vec2(8., 10.), Align2::LEFT_TOP, "Z0 / X0", FontId::proportional(t.font_small), t.text);
    crate::scenario::publish_handle("cam_origin", origin);
    if step == 1 {
        let a = map(ProfilePoint { z_mm: part_back, diameter_mm: -part_diameter }) + vec2(0., 24.);
        let b = map(ProfilePoint { z_mm: 0., diameter_mm: -part_diameter }) + vec2(0., 24.);
        painter.line_segment([a, b], Stroke::new(1., t.text_dim));
        for at in [a, b] {
            painter.line_segment([at - vec2(0., 4.), at + vec2(0., 4.)], Stroke::new(1., t.text_dim));
        }
        painter.text(
            a.lerp(b, 0.5) + vec2(0., 5.),
            Align2::CENTER_TOP,
            format!("全長 {:.3} mm", -part_back),
            FontId::proportional(t.font_small),
            t.text,
        );
    }
    if step != 1
        && let Some([_, chuck, _]) = placement
    {
        let top = map(ProfilePoint { z_mm: chuck, diameter_mm: diameter * 1.15 });
        let bottom = map(ProfilePoint { z_mm: chuck, diameter_mm: -diameter * 1.15 });
        painter.line_segment([top, bottom], Stroke::new(1., t.warning));
        painter.text(top, Align2::LEFT_BOTTOM, "チャック前端", FontId::proportional(t.font_small), t.text_dim);
    }
    let hover = response.hover_pos();
    let nearest = hover.filter(|_| step == 1).and_then(|p| {
        d.segments
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let a = map(s.a);
                let b = map(s.b);
                let ab = b - a;
                let q = a + ab * ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0., 1.);
                (i, p.distance(q))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .filter(|(_, distance)| *distance < 9.)
            .map(|(i, _)| i)
    });
    for (i, s) in d.segments.iter().enumerate() {
        let selected = app.turning_workspace.segment_editor == Some(i) || nearest == Some(i);
        let color = if selected {
            t.warning
        } else if s.thread_pitch_mm.is_some() {
            t.error
        } else {
            t.accent
        };
        painter.line_segment([map(s.a), map(s.b)], Stroke::new(if selected { 3. } else { 2. }, color));
        painter.line_segment(
            [map(ProfilePoint { diameter_mm: -s.a.diameter_mm, ..s.a }), map(ProfilePoint { diameter_mm: -s.b.diameter_mm, ..s.b })],
            Stroke::new(1., t.text_dim),
        );
        crate::scenario::publish_handle(
            &format!("cam_segment_{i}"),
            map(ProfilePoint { z_mm: (s.a.z_mm + s.b.z_mm) * 0.5, diameter_mm: (s.a.diameter_mm + s.b.diameter_mm) * 0.5 }),
        );
    }
    if step == 1
        && (response.double_clicked() || response.triple_clicked())
        && let Some(index) = nearest
        && app.turning_workspace.flat.click_frame != Some(ui.ctx().cumulative_frame_nr())
    {
        app.turning_workspace.flat.click_frame = Some(ui.ctx().cumulative_frame_nr());
        app.turning_workspace.segment_editor = Some(index);
    }
    if step == 2
        && !app.turning_workspace.selection.editing
        && app.turning_workspace.operation_draft.is_none()
        && let Some(cam::machining::sequence::Cutting::Contour(contour)) =
            setup.operations.get(app.turning_workspace.operation).and_then(|o| o.cutting.as_ref())
        && contour.points(setup).is_ok()
        && let Some(points) = contour.profile_snapshot.get(contour.first..=contour.last)
    {
        let screen: Vec<_> = points.iter().copied().map(map).collect();
        for pair in screen.windows(2) {
            if let [a, b] = pair {
                painter.line_segment([*a, *b], Stroke::new(4., t.text));
            }
        }
        if let (Some(first), Some(last)) = (screen.first(), screen.last()) {
            painter.circle_stroke(*first, 6., Stroke::new(2., t.warning));
            painter.rect_stroke(Rect::from_center_size(*last, vec2(12., 12.)), 0., Stroke::new(2., t.warning), egui::StrokeKind::Middle);
            crate::scenario::publish_rect("cam_existing_range", Rect::from_points(&screen));
        }
    }
    let selection = &app.turning_workspace.selection;
    let face_picking = step == 2 && super::selection_ui::face_kind(setup, selection).is_some();
    let face = (step == 2).then(|| super::selection_ui::front_face(setup)).flatten();
    let face_hover = face
        .filter(|_| face_picking)
        .zip(hover)
        .is_some_and(|(points, at)| front_face_hit(points.map(map), at, setup.od_job.iter().flat_map(|job| job.profile.iter()).copied().map(map)));
    let face_selected = step == 2
        && (app.turning_workspace.operation_draft.as_ref().is_some_and(super::operations_ui::Draft::is_front_face)
            || (!selection.editing
                && app.turning_workspace.operation_draft.is_none()
                && setup
                    .operations
                    .get(app.turning_workspace.operation)
                    .is_some_and(|op| matches!(op.kind, OperationKind::Face | OperationKind::FaceRough | OperationKind::FaceFinish))));
    if let Some(points) = face
        && (face_picking || face_selected)
    {
        let [outer, axis] = points.map(map);
        let color = if face_hover || face_selected { t.warning } else { t.text };
        if face_hover || face_selected {
            painter.line_segment([outer, axis], Stroke::new(4., color));
        }
        painter.circle_filled(axis, 6., t.panel);
        painter.circle_stroke(axis, 6., Stroke::new(2., color));
        painter.text(axis + vec2(-8., -10.), Align2::RIGHT_BOTTOM, tr("cam.face_pick"), FontId::proportional(t.font_small), color);
        if face_selected {
            crate::scenario::publish_rect("cam_selected_face", Rect::from_two_pos(outer, axis));
        }
        if face_picking {
            crate::scenario::publish_handle("cam_face_x0", axis);
            crate::scenario::publish_handle("cam_face_span", outer.lerp(axis, 0.5));
        }
    }
    if face_hover
        && response.clicked()
        && app.turning_workspace.flat.click_frame != Some(ui.ctx().cumulative_frame_nr())
        && let Some(plan) = app.session.doc.turning.as_deref()
    {
        app.turning_workspace.flat.click_frame = Some(ui.ctx().cumulative_frame_nr());
        super::selection_ui::pick_face(plan, &mut app.turning_workspace);
    }
    let selection = &app.turning_workspace.selection;
    let turning_tool = selection.update.is_some()
        || selection
            .station
            .and_then(|station| setup.turret.iter().find(|slot| slot.station == station))
            .is_some_and(|slot| matches!(slot.role, ToolRole::OdRough | ToolRole::OdFinish | ToolRole::IdRough | ToolRole::IdFinish));
    if step == 2 && selection.editing && turning_tool && selection.range_is_current(app.session.revision) {
        let inner = app.turning_workspace.model_profile.inner;
        if let Some(job) = if inner { &setup.id_job } else { &setup.od_job } {
            let screen: Vec<_> = job.profile.iter().copied().map(map).collect();
            let view = &app.turning_workspace.selection;
            let visible = selection_points(&screen, view.first, view.last, view.detail_points);
            let picking_hover = hover.filter(|_| view.picking);
            let nearby_span = picking_hover.and_then(|at| nearest_span(&screen, &visible, at));
            let point = picking_hover
                .and_then(|at| nearest_point(&screen, &visible, at))
                .filter(|i| nearby_span.is_none() || hover.zip(screen.get(*i)).is_some_and(|(at, point)| at.distance(*point) <= 6.));
            let span = nearby_span.filter(|_| point.is_none());
            let selected = view.first.zip(view.last).map(|(a, b)| a.min(b)..a.max(b));
            for (i, pair) in screen.windows(2).enumerate() {
                let [a, b] = pair else { continue };
                if selected.as_ref().is_some_and(|range| range.contains(&i)) {
                    painter.line_segment([*a, *b], Stroke::new(4., t.text));
                } else if span.is_some_and(|(first, last)| (first..last).contains(&i)) {
                    painter.line_segment([*a, *b], Stroke::new(4., t.warning));
                }
            }
            for &i in &visible {
                let Some(at) = screen.get(i).copied() else { continue };
                let first = view.first == Some(i);
                let last = view.last == Some(i);
                if !view.picking && !first && !last {
                    continue;
                }
                let color = if first || last || point == Some(i) { t.warning } else { t.text };
                painter.circle_filled(at, if first || last { 6. } else { 4. }, t.panel);
                if last {
                    painter.rect_stroke(Rect::from_center_size(at, vec2(12., 12.)), 0., Stroke::new(2., color), egui::StrokeKind::Middle);
                } else {
                    painter.circle_stroke(at, if first { 6. } else { 4. }, Stroke::new(2., color));
                }
                if first || last {
                    let label = if first { "始点" } else { "終点" };
                    let offset = if first { vec2(8., -10.) } else { vec2(-8., 10.) };
                    let align = if first { Align2::LEFT_BOTTOM } else { Align2::RIGHT_TOP };
                    painter.text(at + offset, align, label, FontId::proportional(t.font_small), t.text);
                }
                if view.picking {
                    crate::scenario::publish_handle(&format!("cam_contour_point_{i}"), at);
                }
            }
            for pair in visible.windows(2).filter(|_| view.picking) {
                if let [first, last] = pair {
                    let mid = (first + last) / 2;
                    if let Some([a, b]) = screen.get(mid..=mid.saturating_add(1)) {
                        crate::scenario::publish_handle(&format!("cam_contour_span_{first}_{last}"), a.lerp(*b, 0.5));
                    }
                }
            }
            if view.picking
                && response.clicked()
                && app.turning_workspace.flat.click_frame != Some(ui.ctx().cumulative_frame_nr())
                && (point.is_some() || span.is_some())
            {
                app.turning_workspace.flat.click_frame = Some(ui.ctx().cumulative_frame_nr());
                if let Some(i) = point {
                    super::selection_ui::pick(&mut app.turning_workspace, i);
                } else if let Some((first, last)) = span {
                    super::selection_ui::pick_span(&mut app.turning_workspace, first, last);
                }
            }
        }
    }
    let state = &app.turning_workspace;
    let Some(setup) = app.session.doc.turning.as_ref().and_then(|p| p.setups.get(state.setup)) else { return };
    if let Some((revision, setup_id, path)) = &state.machining.preview
        && *revision == app.session.revision
        && setup_id == &setup.id
        && state.machining.show
        && !(step == 4 && state.machining.draft_preview)
        && step >= 2
    {
        let proposal = state.selection.preview.as_ref().filter(|(r, id, _)| *r == app.session.revision && id == &setup.id && step == 2);
        let new_ranges = proposal.map(|(_, _, p)| super::selection_ui::added_move_ranges(p));
        for (index, m) in path.moves.iter().take(state.machining.step).enumerate() {
            let added = new_ranges.as_ref().is_none_or(|ranges| ranges.iter().any(|range| range.contains(&index)));
            if !added && !state.selection.show_existing_paths {
                continue;
            }
            let finish = cam::machining::interior::is_finish(m.phase);
            if (state.machining.phase == 1 && finish) || (state.machining.phase == 2 && !finish) {
                continue;
            }
            if m.motion == Motion::Rapid && !state.machining.show_rapid {
                continue;
            }
            let color = if !added || m.motion == Motion::Rapid { t.text_dim } else { t.warning };
            painter.line_segment([map(m.from), map(m.to)], Stroke::new(if added { 1.6 } else { 1. }, color));
        }
        if let Some((tip, station, contact)) = super::spatial::simulation_pose(app) {
            let color = if contact { t.error } else { t.text };
            let index = super::simulation_ui::current_move(state).unwrap_or(0);
            let cutting = cam::machining::sequence::operation_at(path, setup, index).and_then(|o| o.cutting.as_ref());
            let nominal = matches!(cutting, Some(cam::machining::sequence::Cutting::Drill(_) | cam::machining::sequence::Cutting::OdGroove(_)));
            if let Some(cutting) = cutting {
                match cutting {
                    cam::machining::sequence::Cutting::Drill(drill) => {
                        let shoulder = tip.z_mm + drill.point_length();
                        let end = shoulder + drill.diameter_mm;
                        let outline = vec![
                            map(tip),
                            map(ProfilePoint { z_mm: shoulder, diameter_mm: drill.diameter_mm }),
                            map(ProfilePoint { z_mm: end, diameter_mm: drill.diameter_mm }),
                            map(ProfilePoint { z_mm: end, diameter_mm: -drill.diameter_mm }),
                            map(ProfilePoint { z_mm: shoulder, diameter_mm: -drill.diameter_mm }),
                        ];
                        crate::scenario::publish_rect("cam_sim_local_cutter_bounds", Rect::from_points(&outline));
                        painter.add(egui::Shape::closed_line(outline, Stroke::new(2., color)));
                    }
                    cam::machining::sequence::Cutting::OdGroove(groove) => {
                        let edge = [map(tip), map(ProfilePoint { z_mm: tip.z_mm - groove.tool_width_mm, ..tip })];
                        crate::scenario::publish_rect("cam_sim_local_cutter_bounds", Rect::from_points(&edge).expand(2.));
                        painter.line_segment(edge, Stroke::new(4., color));
                    }
                    _ => {}
                }
            }
            if let Some(tool) = station.and_then(|n| setup.turret.iter().find(|s| s.station == n)).and_then(|s| s.simulation.as_ref()) {
                if let Some(holder) = &tool.holder
                    && holder.ready()
                {
                    let half = holder.width_mm * 0.5;
                    let bounds = Rect::from_two_pos(
                        map(ProfilePoint { z_mm: tip.z_mm + holder.z_start_mm, diameter_mm: tip.diameter_mm + 2. * (holder.r_center_mm - half) }),
                        map(ProfilePoint {
                            z_mm: tip.z_mm + holder.z_start_mm + holder.length_mm,
                            diameter_mm: tip.diameter_mm + 2. * (holder.r_center_mm + half),
                        }),
                    );
                    crate::scenario::publish_rect("cam_sim_holder_bounds", bounds);
                    painter.rect_stroke(bounds, 0., Stroke::new(2., if tool.holder_verified { color } else { t.warning }), egui::StrokeKind::Inside);
                }
                if !nominal
                    && let Some(insert) = &tool.insert
                    && let Ok(points) = insert.outline(tool.nose_radius_mm.unwrap_or(0.))
                {
                    let outline: Vec<_> =
                        points.into_iter().map(|p| map(ProfilePoint { z_mm: tip.z_mm + p[0], diameter_mm: tip.diameter_mm + 2. * p[1] })).collect();
                    crate::scenario::publish_rect("cam_sim_insert_bounds", Rect::from_points(&outline));
                    painter.add(egui::Shape::closed_line(outline, Stroke::new(2., color)));
                }
            }
            if nominal {
                painter.text(
                    map(tip) + vec2(8., -10.),
                    egui::Align2::LEFT_BOTTOM,
                    tr("cam.local_cutter_nominal"),
                    FontId::proportional(t.font_small),
                    color,
                );
            }
            painter.circle_filled(map(tip), 4., color);
        }
    }
    let legend = match step {
        _ if !has_contour => tr("cam.flat_stock_legend"),
        0 if !app.turning_workspace.selection.show_difference => "灰：材料　青：完成形状　赤：材料不足（差分の強調はオフ）",
        0 => "橙：削る部分　青：完成形状　灰：背面の残材　赤：材料不足",
        1 => "実線：断面輪郭　下側：回転対称表示　線をダブルクリックで編集",
        2 => "線：区間選択 / 点2つ：範囲指定　太線：選択範囲 / 橙：追加経路",
        _ => "灰：残材　青：目標輪郭　赤：干渉候補",
    };
    let legend = painter.layout(legend.to_owned(), FontId::proportional(t.font_small), t.text_dim, (rect.width() - 24.).max(1.));
    painter.galley(rect.left_bottom() + vec2(12., -12. - legend.size().y), legend, t.text_dim);
}

/// Preserve the shared OD endpoint for contour picking, while making the face
/// center clickable even when a small face projects to only a few screen points.
pub(super) fn front_face_hit([outer, axis]: [egui::Pos2; 2], at: egui::Pos2, mut contour: impl Iterator<Item = egui::Pos2>) -> bool {
    let axis_distance = at.distance(axis);
    if contour.any(|point| at.distance(point) <= 6. && at.distance(point) < axis_distance) {
        return false;
    }
    if axis_distance <= 9. && axis_distance <= at.distance(outer) {
        return true;
    }
    let edge = axis - outer;
    let nearest = outer + edge * ((at - outer).dot(edge) / edge.length_sq().max(1e-6)).clamp(0., 1.);
    at.distance(outer) > 6. && at.distance(nearest) <= 8.
}

/// Only the intersection of a finished bore and the real cylindrical stock is removable.
/// The rest of the cavity stays empty, including regions before/behind short stock.
pub(super) fn cavity_stock_polygon(a: ProfilePoint, b: ProfilePoint, stock: &Stock, front: f64) -> Vec<ProfilePoint> {
    if !front.is_finite()
        || !stock.length_mm.is_finite()
        || stock.length_mm <= 0.
        || !stock.inner_diameter_mm.is_finite()
        || !stock.outer_diameter_mm.is_finite()
        || stock.outer_diameter_mm <= stock.inner_diameter_mm
        || [a, b].iter().any(|p| !p.z_mm.is_finite() || !p.diameter_mm.is_finite())
    {
        return Vec::new();
    }
    let mut polygon = vec![a, b, ProfilePoint { diameter_mm: 0., ..b }, ProfilePoint { diameter_mm: 0., ..a }];
    for (axis_z, limit, direction) in
        [(true, front - stock.length_mm, 1.), (true, front, -1.), (false, stock.inner_diameter_mm, 1.), (false, stock.outer_diameter_mm, -1.)]
    {
        let distance = |p: ProfilePoint| ((if axis_z { p.z_mm } else { p.diameter_mm }) - limit) * direction;
        let Some(mut previous) = polygon.last().copied() else { break };
        let mut clipped = Vec::new();
        for current in polygon {
            let from = distance(previous);
            let to = distance(current);
            if (from >= 0.) != (to >= 0.) {
                let t = (from / (from - to)).clamp(0., 1.);
                let mut intersection = ProfilePoint {
                    z_mm: previous.z_mm + (current.z_mm - previous.z_mm) * t,
                    diameter_mm: previous.diameter_mm + (current.diameter_mm - previous.diameter_mm) * t,
                };
                if axis_z {
                    intersection.z_mm = limit;
                } else {
                    intersection.diameter_mm = limit;
                }
                clipped.push(intersection);
            }
            if to >= 0. {
                clipped.push(current);
            }
            previous = current;
        }
        polygon = clipped;
    }
    polygon
}

/// Suppress crowded smooth-curve samples only on screen. The saved contour is untouched.
/// Corners, collinear split points, endpoints and the current selection remain available.
pub(super) fn selection_points(points: &[egui::Pos2], first: Option<usize>, last: Option<usize>, detailed: bool) -> Vec<usize> {
    const SPACING: f32 = 18.;
    let mut visible = Vec::new();
    for (i, p) in points.iter().enumerate() {
        if !p.is_finite() {
            continue;
        }
        let mandatory = i == 0 || i + 1 == points.len() || first == Some(i) || last == Some(i);
        let boundary = i.checked_sub(1).and_then(|a| points.get(a)).zip(points.get(i + 1)).is_some_and(|(a, b)| {
            let incoming = *p - *a;
            let outgoing = *b - *p;
            let lengths = incoming.length() * outgoing.length();
            let cosine = incoming.dot(outgoing) / lengths.max(1e-8);
            // Straight midpoints include deliberate user splits; do not hide those.
            !(0.94..=1. - 1e-6).contains(&cosine) || incoming.length() >= SPACING || outgoing.length() >= SPACING
        });
        let spaced = visible.last().and_then(|i| points.get(*i)).is_none_or(|last| p.distance(*last) >= SPACING);
        if mandatory || detailed || boundary || spaced {
            visible.push(i);
        }
    }
    visible
}

pub(super) fn nearest_point(points: &[egui::Pos2], visible: &[usize], at: egui::Pos2) -> Option<usize> {
    visible
        .iter()
        .filter_map(|i| points.get(*i).map(|p| (*i, p.distance(at))))
        .filter(|(_, distance)| *distance <= 12.)
        .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
        .map(|(i, _)| i)
}

pub(super) fn nearest_span(points: &[egui::Pos2], visible: &[usize], at: egui::Pos2) -> Option<(usize, usize)> {
    visible
        .windows(2)
        .filter_map(|pair| {
            let [first, last] = pair else { return None };
            let distance = points
                .get(*first..=(*last))?
                .windows(2)
                .filter_map(|pair| {
                    let [a, b] = pair else { return None };
                    let ab = *b - *a;
                    let q = *a + ab * ((at - *a).dot(ab) / ab.length_sq().max(1e-8)).clamp(0., 1.);
                    Some(at.distance(q))
                })
                .fold(f32::INFINITY, f32::min);
            (distance <= 9.).then_some((*first, *last, distance))
        })
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(first, last, _)| (first, last))
}

pub(super) fn joined_bands(mut slabs: Vec<cam::simulation::Slab>) -> Vec<cam::simulation::Slab> {
    slabs.sort_by(|a, b| a.inner.total_cmp(&b.inner).then(a.outer.total_cmp(&b.outer)).then(a.z0.total_cmp(&b.z0)));
    let mut joined: Vec<cam::simulation::Slab> = Vec::new();
    for s in slabs {
        if let Some(last) = joined.last_mut()
            && last.inner == s.inner
            && last.outer == s.outer
            && (last.z1 - s.z0).abs() < 1e-9
        {
            last.z1 = s.z1;
        } else {
            joined.push(s);
        }
    }
    joined
}
