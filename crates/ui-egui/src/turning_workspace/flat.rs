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
    fitted: Option<(String, u64, u64, usize)>,
    size: egui::Vec2,
    click_frame: Option<u64>,
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
    ui.horizontal(|ui| {
        ui.strong(match step {
            1 => "輪郭 · 最初に加工する側 →",
            0 => "材料と完成形状",
            2 => "工具 → 始点 → 終点",
            _ => "切削シミュレーション",
        });
        if ui_kit::button(ui, "全体表示", ui_kit::Button::Small).clicked() {
            app.turning_workspace.flat.fitted = None;
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
    if d.segments.is_empty() {
        ui.label("中心軸と輪郭を取得すると、ここに断面が表示されます。");
        return;
    }
    let part_back = d.segments.iter().flat_map(|s| [s.a.z_mm, s.b.z_mm]).fold(0., f64::min);
    let stock = (step != 1).then_some(setup.stock.as_ref()).flatten();
    let job = setup.od_job.as_ref().or(setup.id_job.as_ref());
    let front = job.map_or(0., |j| j.stock_front_z_mm);
    let zmin = part_back.min(stock.map_or(0., |s| front - s.length_mm));
    let diameter =
        d.segments.iter().flat_map(|s| [s.a.diameter_mm, s.b.diameter_mm]).fold(1., f64::max).max(stock.map_or(0., |s| s.outer_diameter_mm));
    let zmax = if step == 1 { 3. } else { job.map_or(3., |j| j.stock_front_z_mm + j.clearance_mm + 3.) };
    let key = (setup.id.clone(), zmin.to_bits(), diameter.to_bits(), step);
    let (rect, response) = ui.allocate_exact_size(ui.available_size().max(vec2(1., 1.)), egui::Sense::click_and_drag());
    crate::scenario::publish_rect("cam_flat_canvas", rect);
    if app.turning_workspace.flat.fitted.as_ref() != Some(&key) || app.turning_workspace.flat.size != rect.size() {
        let camera = &mut app.turning_workspace.flat.camera;
        *camera = Camera {
            yaw: 0.,
            pitch: 0.,
            target: Vec3::new((zmin + zmax) * 0.5, 0., 0.),
            distance: (diameter * 0.8).max((zmax - zmin) * rect.height() as f64 / rect.width().max(1.) as f64) * 1.65,
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
    let difference_color = if app.turning_workspace.selection.show_difference { t.panel.lerp_to_gamma(t.warning, 0.3) } else { remaining_color };
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
        let front = setup.od_job.as_ref().or(setup.id_job.as_ref()).map_or(0., |j| j.stock_front_z_mm);
        slab(front - stock.length_mm, front, stock.inner_diameter_mm * 0.5, stock.outer_diameter_mm * 0.5, difference_color);
        if front - stock.length_mm < part_back {
            slab(front - stock.length_mm, part_back, stock.inner_diameter_mm * 0.5, stock.outer_diameter_mm * 0.5, remaining_color);
        }
    }
    // Fill the outer half-section with trapezoids, then cut out its inner boundary.
    if sim.is_none() {
        for s in &d.segments {
            if !matches!(s.side, Side::Outer | Side::Inner) || (s.a.z_mm - s.b.z_mm).abs() < 1e-8 {
                continue;
            }
            let color = if s.side == Side::Outer {
                t.accent_soft
            } else if step != 1 && setup.stock.is_some() {
                difference_color
            } else {
                t.panel
            };
            for sign in [1., -1.] {
                let a = ProfilePoint { diameter_mm: s.a.diameter_mm * sign, ..s.a };
                let b = ProfilePoint { diameter_mm: s.b.diameter_mm * sign, ..s.b };
                painter.add(egui::Shape::convex_polygon(
                    vec![map(a), map(b), map(ProfilePoint { diameter_mm: 0., ..b }), map(ProfilePoint { diameter_mm: 0., ..a })],
                    color,
                    Stroke::NONE,
                ));
            }
        }
        if step != 1
            && let Some(stock) = &setup.stock
        {
            let front = setup.od_job.as_ref().or(setup.id_job.as_ref()).map_or(0., |j| j.stock_front_z_mm);
            slab(front - stock.length_mm, front, 0., stock.inner_diameter_mm * 0.5, t.panel);
        }
    }
    let axis = [map(ProfilePoint { z_mm: zmin - 3., diameter_mm: 0. }), map(ProfilePoint { z_mm: zmax + 3., diameter_mm: 0. })];
    painter.line_segment(axis, Stroke::new(1., t.text_dim));
    painter.text(axis[1], Align2::RIGHT_TOP, "中心軸 / Z →", FontId::proportional(t.font_small), t.text_dim);
    if step != 1
        && let Some(job) = setup.od_job.as_ref().or(setup.id_job.as_ref())
    {
        let top = map(ProfilePoint { z_mm: job.chuck_front_z_mm, diameter_mm: diameter * 1.15 });
        let bottom = map(ProfilePoint { z_mm: job.chuck_front_z_mm, diameter_mm: -diameter * 1.15 });
        painter.line_segment([top, bottom], Stroke::new(1., t.warning));
        painter.text(top, Align2::LEFT_BOTTOM, "チャック前端", FontId::proportional(t.font_small), t.text_dim);
    }
    let hover = response.hover_pos();
    let nearest = hover.and_then(|p| {
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
    if step == 2 && app.turning_workspace.selection.editing {
        let inner = app.turning_workspace.model_profile.inner;
        if let Some(job) = if inner { &setup.id_job } else { &setup.od_job } {
            let mut picked = None;
            for (i, p) in job.profile.iter().enumerate() {
                let at = map(*p);
                let selected = app.turning_workspace.selection.first == Some(i) || app.turning_workspace.selection.last == Some(i);
                painter.circle_filled(at, if selected { 5. } else { 3.5 }, if selected { t.warning } else { t.text });
                crate::scenario::publish_handle(&format!("cam_contour_point_{i}"), at);
                if response.clicked() && hover.is_some_and(|h| h.distance(at) < 10.) {
                    picked = Some(i);
                }
            }
            if let Some(i) = picked {
                super::selection_ui::pick(&mut app.turning_workspace, i);
            }
        }
    }
    let state = &app.turning_workspace;
    let Some(setup) = app.session.doc.turning.as_ref().and_then(|p| p.setups.get(state.setup)) else { return };
    if let Some((revision, setup_id, path)) = &state.machining.preview
        && *revision == app.session.revision
        && setup_id == &setup.id
        && state.machining.show
        && step >= 2
    {
        for m in path.moves.iter().take(state.machining.step) {
            let finish = cam::machining::interior::is_finish(m.phase);
            if (state.machining.phase == 1 && finish) || (state.machining.phase == 2 && !finish) {
                continue;
            }
            if m.motion == Motion::Rapid && !state.machining.show_rapid {
                continue;
            }
            painter.line_segment([map(m.from), map(m.to)], Stroke::new(1., if m.motion == Motion::Rapid { t.text_dim } else { t.warning }));
        }
        if let Some((tip, station, contact)) = super::spatial::simulation_pose(app) {
            let color = if contact { t.error } else { t.text };
            let index = state
                .simulation
                .focus_contact
                .and_then(|i| super::simulation_ui::current(app).and_then(|s| s.contacts.get(i)))
                .map_or(state.machining.step.saturating_sub(1), |c| c.move_index);
            if let Some(cutting) = cam::machining::sequence::operation_at(path, setup, index).and_then(|o| o.cutting.as_ref()) {
                match cutting {
                    cam::machining::sequence::Cutting::Drill(drill) => {
                        let shoulder = tip.z_mm + drill.point_length();
                        let end = shoulder + drill.diameter_mm;
                        painter.add(egui::Shape::closed_line(
                            vec![
                                map(tip),
                                map(ProfilePoint { z_mm: shoulder, diameter_mm: drill.diameter_mm }),
                                map(ProfilePoint { z_mm: end, diameter_mm: drill.diameter_mm }),
                                map(ProfilePoint { z_mm: end, diameter_mm: -drill.diameter_mm }),
                                map(ProfilePoint { z_mm: shoulder, diameter_mm: -drill.diameter_mm }),
                            ],
                            Stroke::new(2., color),
                        ));
                    }
                    cam::machining::sequence::Cutting::OdGroove(groove) => {
                        painter.line_segment([map(tip), map(ProfilePoint { z_mm: tip.z_mm - groove.tool_width_mm, ..tip })], Stroke::new(4., color));
                    }
                    _ => {}
                }
            }
            if let Some(tool) = station.and_then(|n| setup.turret.iter().find(|s| s.station == n)).and_then(|s| s.simulation.as_ref()) {
                if let Some(holder) = &tool.holder
                    && holder.ready()
                {
                    let half = if holder.kind == cam::simulation::HolderKind::Round { holder.width_mm * 0.5 } else { holder.height_mm * 0.5 };
                    painter.rect_stroke(
                        Rect::from_two_pos(
                            map(ProfilePoint { z_mm: tip.z_mm + holder.z_start_mm, diameter_mm: tip.diameter_mm + 2. * (holder.r_center_mm - half) }),
                            map(ProfilePoint {
                                z_mm: tip.z_mm + holder.z_start_mm + holder.length_mm,
                                diameter_mm: tip.diameter_mm + 2. * (holder.r_center_mm + half),
                            }),
                        ),
                        0.,
                        Stroke::new(2., if tool.holder_verified { color } else { t.warning }),
                        egui::StrokeKind::Inside,
                    );
                }
                if let Some(insert) = &tool.insert
                    && let Ok(points) = insert.outline(tool.nose_radius_mm.unwrap_or(0.))
                {
                    painter.add(egui::Shape::closed_line(
                        points.into_iter().map(|p| map(ProfilePoint { z_mm: tip.z_mm + p[0], diameter_mm: tip.diameter_mm + 2. * p[1] })).collect(),
                        Stroke::new(2., color),
                    ));
                }
            }
            painter.circle_filled(map(tip), 4., color);
        }
    }
    let legend = match step {
        0 if !app.turning_workspace.selection.show_difference => "灰：材料　青：完成形状（差分の強調はオフ）",
        0 => "橙：形状との差分　青：完成形状　灰：背面の残材",
        1 => "実線：断面輪郭　下側：回転対称表示　線をダブルクリックで編集",
        2 => "白点：範囲の端点　橙：選択・工具経路",
        _ => "灰：残材　青：目標輪郭　赤：干渉候補",
    };
    painter.text(rect.left_bottom() + vec2(12., -12.), Align2::LEFT_BOTTOM, legend, FontId::proportional(t.font_small), t.text_dim);
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
