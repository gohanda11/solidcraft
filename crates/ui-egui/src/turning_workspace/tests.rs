use super::*;
use crate::scenario::Harness;

#[test]
fn flat_stock_display_keeps_removed_grooves_and_radial_gaps() {
    use cam::simulation::Slab;
    let bands = flat::joined_bands(vec![
        Slab { z0: -3., z1: -2., inner: 2., outer: 5. },
        Slab { z0: -2., z1: -1., inner: 2., outer: 5. },
        Slab { z0: 0., z1: 1., inner: 2., outer: 5. },
        Slab { z0: -3., z1: -2., inner: 7., outer: 9. },
        Slab { z0: -2., z1: -1., inner: 7., outer: 9. },
    ]);
    assert_eq!(bands.len(), 3);
    assert!(bands.iter().all(|s| s.z1 <= -1. || s.z0 >= 0.));
    assert!(bands.iter().all(|s| s.outer <= 5. || s.inner >= 7.));
}

fn click(h: &mut Harness, handle: &str) {
    h.frames(2);
    if let Some(rect) = crate::scenario::rect_of(handle) {
        let at = rect.center();
        h.step(&json!({"click":[at.x, at.y]})).unwrap();
    } else {
        h.step(&json!({"click":{"handle":handle}})).unwrap();
    }
    h.frames(2);
}

fn edit_tool(h: &mut Harness, station: u8) {
    click(h, "cam_step_2");
    h.frames(2);
    let at = crate::scenario::rect_of(&format!("cam_tool_{station}")).unwrap().center();
    h.step(&json!({"click":[at.x,at.y],"double":true})).unwrap();
    h.frames(2);
    assert_eq!(h.app.turning_workspace.tool_editor, Some(station));
}

#[test]
fn staged_flat_workflow_edits_lines_tools_and_resets_without_losing_tools() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1600,"height":1200}));
    let script: serde_json::Value = serde_json::from_str(include_str!("../../../../examples/turning-toolpath.json")).unwrap();
    for c in script["commands"].as_array().unwrap() {
        h.app.run(c["command"].as_str().unwrap(), c["params"].clone()).unwrap();
    }
    h.app.home.open = false;
    h.frames(3);
    let camera = h.app.cam;
    enter(&mut h.app, "turning").unwrap();
    click(&mut h, "cam_model_read");
    assert!(crate::scenario::rect_of("cam_tool_12").is_none());
    assert!(crate::scenario::rect_of("cam_operation_0").is_none());
    assert!(crate::scenario::rect_of("cam_flat_canvas").is_some());
    assert_eq!(h.app.cam, camera, "CAM navigation must not change the design camera");
    let old = h.app.session.doc.turning.as_ref().unwrap().setups[0].od_job.as_ref().unwrap().profile.len();
    h.step(&json!({"click":{"handle":"cam_segment_0"},"double":true})).unwrap();
    h.frames(2);
    assert_eq!(h.app.turning_workspace.segment_editor, Some(0));
    let canvas = crate::scenario::rect_of("cam_flat_canvas").unwrap();
    for (name, at) in crate::scenario::handles("cam_segment_") {
        if name.strip_prefix("cam_segment_").and_then(|s| s.parse::<usize>().ok()).is_some() {
            assert!(canvas.contains(at), "opening the line editor must keep {name} visible");
        }
    }
    click(&mut h, "cam_segment_split");
    assert_eq!(h.app.session.doc.turning.as_ref().unwrap().setups[0].od_job.as_ref().unwrap().profile.len(), old + 1);
    h.app.run("cam.turning.toolpath", json!({"setup":"setup-1"})).unwrap();
    for _ in 0..2 {
        click(&mut h, "cam_model_reverse");
        assert!(h.app.session.doc.turning.as_ref().unwrap().setups[0].od_job.as_ref().unwrap().model_source.as_ref().unwrap().reverse);
        h.app.run("edit.undo", json!({})).unwrap();
        h.frames(3);
        assert!(!h.app.session.doc.turning.as_ref().unwrap().setups[0].od_job.as_ref().unwrap().model_source.as_ref().unwrap().reverse);
    }
    click(&mut h, "cam_step_0");
    assert!(crate::scenario::rect_of("cam_tool_12").is_none());
    assert!(crate::scenario::rect_of("cam_operation_0").is_none());
    edit_tool(&mut h, 12);
    assert!(crate::scenario::rect_of("cam_tool_edit_close").is_some());
    let canvas = crate::scenario::rect_of("cam_flat_canvas").unwrap();
    let editor = crate::scenario::rect_of("cam_tool_edit_close").unwrap();
    assert!(canvas.right() <= editor.left(), "tool controls must not expand underneath the 2D canvas");
    click(&mut h, "cam_step_4");
    click(&mut h, "cam_sim_calculate");
    assert!(simulation_ui::current(&h.app).is_some());
    assert!(h.widget("O9001 (SOLIDCRAFT REVIEW ONLY)").is_some());
    assert!(crate::scenario::rect_of("cam_tool_12").is_none());
    assert!(crate::scenario::rect_of("cam_operation_0").is_some());
    let before = h.app.session.doc.turning.clone();
    click(&mut h, "cam_reset");
    assert_eq!(h.app.turning_workspace.workflow, 1);
    assert!(h.app.turning_workspace.machining.preview.is_none());
    let plan = h.app.session.doc.turning.as_ref().unwrap();
    assert!(plan.setups[0].operations.is_empty() && plan.setups[0].od_job.is_none() && plan.setups[0].stock.is_none());
    assert_eq!(plan.setups[0].turret, before.as_ref().unwrap().setups[0].turret);
    h.app.run("edit.undo", json!({})).unwrap();
    h.frames(3);
    assert_eq!(h.app.session.doc.turning, before);
}

fn start() -> Harness {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1440,"height":1000}));
    h.app.home.open = false;
    assert_eq!(h.call("ui.workspace", json!({"workspace":"turning"}))["ok"], true);
    h.app.turning_workspace.advanced = true;
    h.frames(3);
    click(&mut h, "cam_create_plan");
    h
}

fn choose_operation(h: &mut Harness, kind: OperationKind) {
    click(h, "cam_add_operation");
    let group = match kind {
        OperationKind::FaceRough | OperationKind::FaceFinish => 0,
        OperationKind::OdRough | OperationKind::OdFinish => 1,
        OperationKind::IdRough | OperationKind::IdFinish => 2,
        OperationKind::CenterDrill | OperationKind::Drill => 3,
        OperationKind::PartOff | OperationKind::PartingFinish => 5,
        _ => 4,
    };
    click(h, &format!("cam_operation_group_{group}"));
    click(h, &format!("cam_operation_kind_{kind:?}"));
}

#[test]
fn turning_guide_starts_with_model_without_inventing_operations() {
    let mut h = Harness::new();
    h.app.home.open = false;
    enter(&mut h.app, "turning").unwrap();
    click(&mut h, "cam_create_plan");
    assert_eq!(h.app.turning_workspace.workflow, 1);
    assert!(h.app.session.doc.turning.as_ref().unwrap().setups[0].operations.is_empty());
    click(&mut h, "cam_guide_next");
    assert_eq!(h.app.turning_workspace.workflow, 1);
    click(&mut h, "cam_step_0");
    h.app.turning_workspace.stock = Stock { outer_diameter_mm: 50., inner_diameter_mm: 0., length_mm: 80. };
    click(&mut h, "cam_set_stock");
    click(&mut h, "cam_guide_next");
    assert_eq!(h.app.turning_workspace.workflow, 2);
    click(&mut h, "cam_tool_12");
    assert!(h.app.turning_workspace.selection.editing);
    click(&mut h, "cam_selection_model");
    assert_eq!(h.app.turning_workspace.workflow, 1);
    assert!(h.app.session.doc.turning.as_ref().unwrap().setups[0].operations.is_empty());
}

#[test]
fn turning_tool_contour_preview_then_explicit_add() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.call("ui.resize", json!({"width":1600,"height":1200}));
    let script: serde_json::Value = serde_json::from_str(include_str!("../../../../examples/turning-toolpath.json")).unwrap();
    for c in script["commands"].as_array().unwrap() {
        h.app.run(c["command"].as_str().unwrap(), c["params"].clone()).unwrap();
    }
    let mut plan = h.app.session.doc.turning.as_deref().unwrap().clone();
    plan.setups[0].operations.clear();
    h.app.run("cam.turning.set", json!({"plan":plan})).unwrap();
    enter(&mut h.app, "turning").unwrap();
    click(&mut h, "cam_step_2");
    click(&mut h, "cam_tool_12");
    click(&mut h, "cam_select_all");
    assert!(h.app.turning_workspace.selection.difference.is_none(), "tool/range selection must not run a residual calculation");
    let before = h.app.session.doc.turning.clone();
    click(&mut h, "cam_selection_preview");
    assert!(h.app.turning_workspace.selection.preview.is_some(), "{:?}", h.app.turning_workspace.selection);
    assert_eq!(h.app.session.doc.turning, before);
    click(&mut h, "cam_selection_add");
    let setup = &h.app.session.doc.turning.as_ref().unwrap().setups[0];
    assert_eq!(setup.operations.len(), 1);
    assert!(matches!(setup.operations[0].cutting, Some(cam::machining::sequence::Cutting::Contour(_))));
    click(&mut h, "cam_tool_6");
    click(&mut h, "cam_select_all");
    click(&mut h, "cam_selection_preview");
    assert!(!h.app.turning_workspace.selection.preview.as_ref().unwrap().2.rough_then_finish);
    click(&mut h, "cam_selection_add");
    assert_eq!(h.app.session.doc.turning.as_ref().unwrap().setups[0].operations.len(), 2);
}

#[test]
fn turning_material_candidates_apply_explicitly_and_insert_preset_is_persistent() {
    let mut h = Harness::new();
    let script: serde_json::Value = serde_json::from_str(include_str!("../../../../examples/turning-toolpath.json")).unwrap();
    for c in script["commands"].as_array().unwrap() {
        h.app.run(c["command"].as_str().unwrap(), c["params"].clone()).unwrap();
    }
    h.app.home.open = false;
    enter(&mut h.app, "turning").unwrap();
    click(&mut h, "cam_step_2");
    click(&mut h, "cam_operation_material_conditions");
    let before = h.app.session.doc.turning.as_ref().unwrap().setups[0].od_job.clone().unwrap();
    click(&mut h, "cam_material_list");
    click(&mut h, "cam_material_SUS304");
    assert_eq!(h.app.session.doc.turning.as_ref().unwrap().material, "SUS304");
    assert_eq!(h.app.session.doc.turning.as_ref().unwrap().setups[0].od_job.as_ref().unwrap().rough_feed_mm_rev, before.rough_feed_mm_rev);
    click(&mut h, "cam_condition_level");
    click(&mut h, "cam_level_gentle");
    click(&mut h, "cam_conditions_apply");
    let after = h.app.session.doc.turning.as_ref().unwrap().setups[0].od_job.as_ref().unwrap();
    assert_eq!(after.radial_depth_mm, 0.3);
    assert_eq!(after.radial_stock_to_leave_mm, before.radial_stock_to_leave_mm);
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.turning.as_ref().unwrap().setups[0].od_job, Some(before));
    edit_tool(&mut h, 12);
    click(&mut h, "cam_insert_all");
    let plan = h.app.session.doc.turning.as_ref().unwrap();
    let tool = plan.setups[0].turret[11].simulation.as_ref().unwrap();
    assert_eq!(tool.nose_radius_mm, Some(0.4));
    assert!(tool.insert.is_some() && !tool.holder_verified);
    let roundtrip = solidcraft_engine::doc::Document::from_json(&h.app.session.doc.to_json()).unwrap();
    assert_eq!(roundtrip.turning, h.app.session.doc.turning);
    click(&mut h, "cam_step_4");
    click(&mut h, "cam_sim_calculate");
    assert_eq!(simulation_ui::current(&h.app).unwrap().insert_stations, vec![6, 12]);
}

#[test]
fn review_step_simulates_stock_without_silently_certifying_unknown_holders() {
    let mut h = Harness::new();
    let script: serde_json::Value = serde_json::from_str(include_str!("../../../../examples/turning-toolpath.json")).unwrap();
    for c in script["commands"].as_array().unwrap() {
        h.app.run(c["command"].as_str().unwrap(), c["params"].clone()).unwrap();
    }
    h.app.run("cam.turning.profile.apply", json!({"body":"段付き軸","setup":"setup-1","expected_revision":h.app.session.revision})).unwrap();
    h.app.home.open = false;
    enter(&mut h.app, "turning").unwrap();
    click(&mut h, "cam_step_4");
    assert_eq!(h.app.turning_workspace.workflow, 4);
    click(&mut h, "cam_sim_calculate");
    let report = simulation_ui::current(&h.app).unwrap();
    assert!(report.checked_stations.is_empty());
    assert_eq!(report.missing_stations, vec![6, 12]);
    assert!(spatial::stage_scene(&h.app).is_some());
    let revision = h.app.session.revision;
    let mut plan = h.app.session.doc.turning.as_deref().unwrap().clone();
    plan.setups[0].turret[11].simulation =
        Some(cam::simulation::ToolAssembly { triangular_insert: true, nose_radius_mm: Some(0.4), ..Default::default() });
    h.app.run("cam.turning.set", json!({"plan":plan})).unwrap();
    h.frames(2);
    assert!(h.app.session.revision > revision);
    assert!(simulation_ui::current(&h.app).is_none());
    assert!(spatial::stage_scene(&h.app).is_none());
    click(&mut h, "cam_sim_calculate");
    assert!(simulation_ui::current(&h.app).unwrap().checked_stations.is_empty());
    let revision = h.app.session.revision;
    edit_tool(&mut h, 12);
    click(&mut h, "cam_sim_tool_settings");
    assert_eq!(h.app.session.revision, revision, "opening unknown holder dimensions must be read-only");
    click(&mut h, "cam_step_4");
    assert!(simulation_ui::current(&h.app).is_some());
    // Synthetic envelope deliberately intersects stock; replay must stop and focus it.
    let mut plan = h.app.session.doc.turning.as_deref().unwrap().clone();
    let tool = plan.setups[0].turret[11].simulation.as_mut().unwrap();
    tool.holder = Some(cam::simulation::Holder { length_mm: 30., width_mm: 20., height_mm: 20., ..Default::default() });
    tool.holder_verified = true;
    h.app.run("cam.turning.set", json!({"plan":plan})).unwrap();
    click(&mut h, "cam_sim_calculate");
    let first = simulation_ui::current(&h.app).unwrap().contacts[0].move_index;
    click(&mut h, "cam_play");
    h.frames(120);
    assert!(!h.app.turning_workspace.spatial.playing);
    assert_eq!(h.app.turning_workspace.simulation.focus_contact, Some(0));
    assert_eq!(h.app.turning_workspace.machining.step, first);
    click(&mut h, "cam_sim_contact_0");
    assert_eq!(h.app.turning_workspace.simulation.focus_contact, Some(0));
    click(&mut h, "cam_review_edit");
    assert_eq!(h.app.turning_workspace.workflow, 2);
}

#[test]
fn turning_workspace_buttons_create_edit_and_undo_saved_plan() {
    let mut h = start();
    assert!(h.app.session.is_dirty());
    assert_eq!(h.app.session.doc.turning.as_ref().unwrap().setups[0].turret.len(), 12);
    choose_operation(&mut h, OperationKind::OdRough);
    click(&mut h, "cam_operation_confirm");
    let plan = h.app.session.doc.turning.as_ref().unwrap();
    assert_eq!(plan.setups[0].operations[0].kind, OperationKind::OdRough);
    assert_eq!(plan.setups[0].operations[0].station, Some(12));
    click(&mut h, "cam_advanced");
    click(&mut h, "cam_add_setup");
    assert_eq!(h.app.session.doc.turning.as_ref().unwrap().setups[1].previous_setup.as_deref(), Some("setup-1"));
    h.app.run("edit.undo", json!({})).unwrap();
    h.frames(2);
    assert_eq!(h.app.session.doc.turning.as_ref().unwrap().setups.len(), 1);
    click(&mut h, "cam_operation_0");
    click(&mut h, "cam_remove_operation");
    assert!(h.app.session.doc.turning.as_ref().unwrap().setups[0].operations.is_empty());
    h.app.run("edit.undo", json!({})).unwrap();
    let path = h.dir.join("turning.solidcraft");
    h.app.run("file.save_as", json!({"path":path})).unwrap();
    let saved = h.app.session.doc.turning.clone();
    h.app.run("file.new", json!({})).unwrap();
    h.app.run("doc.open", json!({"path":path})).unwrap();
    assert_eq!(h.app.session.doc.turning, saved);
}

#[test]
fn turning_manual_separation_needs_bore_and_remains_last() {
    let mut h = start();
    let before = h.app.session.doc.turning.clone();
    choose_operation(&mut h, OperationKind::PartingFinish);
    click(&mut h, "cam_operation_confirm");
    assert_eq!(h.app.session.doc.turning, before);
    assert!(h.app.turning_workspace.operation_draft.is_some());
    h.app.turning_workspace.operation_draft.as_mut().unwrap().bore = 20.0;
    click(&mut h, "cam_operation_confirm");
    assert!(h.app.turning_workspace.error.is_none());
    choose_operation(&mut h, OperationKind::IdFinish);
    click(&mut h, "cam_operation_confirm");
    let setup = &h.app.session.doc.turning.as_ref().unwrap().setups[0];
    assert_eq!(setup.operations[0].kind, OperationKind::IdFinish);
    assert_eq!(setup.operations[1].manual_separation, Some(ManualSeparation { finished_bore_diameter_mm: 20.0, stop_diameter_mm: 20.0 }));
    assert_eq!(setup.planned_outcome(), PlannedOutcome::ManualSeparationRequired);
    let before = h.app.session.doc.turning.clone();
    choose_operation(&mut h, OperationKind::PartOff);
    click(&mut h, "cam_operation_confirm");
    assert_eq!(h.app.session.doc.turning, before);
}

#[test]
fn turning_operation_composer_is_read_only_until_confirm_and_keeps_rough_before_finish() {
    let mut h = start();
    h.app.turning_workspace.advanced = false;
    click(&mut h, "cam_step_2");
    let revision = h.app.session.revision;
    choose_operation(&mut h, OperationKind::OdFinish);
    assert_eq!(h.app.session.revision, revision);
    click(&mut h, "cam_operation_group_2");
    click(&mut h, "cam_operation_confirm");
    assert_eq!(h.app.session.revision, revision, "changing group must clear the old type");
    click(&mut h, "cam_operation_cancel");
    assert_eq!(h.app.session.revision, revision);
    assert!(h.app.turning_workspace.operation_draft.is_none());
    choose_operation(&mut h, OperationKind::OdFinish);
    click(&mut h, "cam_operation_confirm");
    choose_operation(&mut h, OperationKind::OdRough);
    click(&mut h, "cam_operation_draft_station");
    click(&mut h, "cam_operation_draft_station_6");
    click(&mut h, "cam_operation_confirm");
    let setup = &h.app.session.doc.turning.as_ref().unwrap().setups[0];
    assert_eq!(setup.operations.iter().map(|o| o.kind).collect::<Vec<_>>(), vec![OperationKind::OdRough, OperationKind::OdFinish]);
    assert_eq!(setup.operations[0].station, Some(6));
    assert_eq!(h.app.turning_workspace.operation, 0);
    assert_eq!(h.app.turning_workspace.workflow, 2);
    assert!(!h.app.turning_workspace.advanced);
    assert!(h.app.turning_workspace.operation_draft.is_none());
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.turning.as_ref().unwrap().setups[0].operations.len(), 1);
    h.app.run("edit.redo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.turning.as_ref().unwrap().setups[0].operations.len(), 2);
    let revision = h.app.session.revision;
    choose_operation(&mut h, OperationKind::Drill);
    click(&mut h, "cam_step_1");
    assert!(h.app.turning_workspace.operation_draft.is_none());
    assert_eq!(h.app.session.revision, revision);
}

#[test]
fn turning_survives_ribbon_context_sync_in_both_shell_layouts() {
    for layout in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.ui.layout = layout;
        h.frames(3);
        enter(&mut h.app, "turning").unwrap();
        h.frames(3);
        assert!(active(&h.app));
        assert_eq!(crate::workspace::visible_tabs(&h.app), vec!["TURNING"]);
        assert!(h.app.shell.cards.is_empty());
        assert!(h.app.shell.panels.is_empty());
        assert!(h.app.shell.content.is_none());
        click(&mut h, "cam_create_plan");
        assert!(h.app.session.doc.turning.is_some());
        enter(&mut h.app, "design").unwrap();
        h.frames(3);
        assert_eq!(h.app.ui.tab, "MODEL");
        assert_eq!(h.app.ui.layout, layout);
    }
}

#[test]
fn turning_workspace_switching_and_document_state_are_isolated() {
    let mut h = start();
    click(&mut h, "cam_add_setup");
    h.app.turning_workspace.bore = 20.0;
    crate::documents::open_tab(&mut h.app, solidcraft_engine::Session::default()).unwrap();
    assert_eq!(h.app.turning_workspace.bore, 0.0);
    assert!(h.app.session.doc.turning.is_none());
    crate::documents::switch(&mut h.app, 0);
    assert_eq!(h.app.turning_workspace.bore, 20.0);
    assert_eq!(h.app.turning_workspace.setup, 1);
    let plan = h.app.session.doc.turning.clone();
    assert_eq!(h.call("ui.workspace", json!({"workspace":"render"}))["ok"], true);
    assert_eq!(h.call("ui.workspace", json!({"workspace":"turning"}))["ok"], true);
    assert_eq!(h.app.session.doc.turning, plan);
    h.app.run("file.new", json!({})).unwrap();
    assert_eq!(h.app.turning_workspace.bore, 0.0);
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    assert_eq!(h.call("ui.workspace", json!({"workspace":"turning"}))["ok"], false);
    assert_eq!(h.call("ui.set", json!({"tab":"TURNING"}))["ok"], false);
}

#[test]
fn turning_workspace_menu_works_in_english_and_japanese() {
    for language in ["en", "ja"] {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.preferences.language = language.into();
        h.frames(3);
        click(&mut h, "workspace_switcher");
        click(&mut h, "menu:ui.workspace.turning");
        assert!(active(&h.app));
        click(&mut h, "cam_create_plan");
        assert!(h.app.session.doc.turning.is_some());
        assert!(h.widget("④ シミュレーション・NC").is_some());
    }
}

#[test]
fn turning_form_edits_reach_document_and_invalid_stock_is_rejected() {
    let mut h = start();
    click(&mut h, "cam.material");
    h.step(&json!({"text":"SUS304"})).unwrap();
    h.frames(2);
    assert_eq!(h.app.session.doc.turning.as_ref().unwrap().material, "SUS304");
    h.app.turning_workspace.stock = Stock { outer_diameter_mm: 60.0, inner_diameter_mm: 70.0, length_mm: 80.0 };
    click(&mut h, "cam_set_stock");
    assert!(h.app.session.doc.turning.as_ref().unwrap().setups[0].stock.is_none());
    h.app.turning_workspace.stock.inner_diameter_mm = 0.0;
    click(&mut h, "cam_set_stock");
    assert_eq!(h.app.session.doc.turning.as_ref().unwrap().setups[0].stock.as_ref().unwrap().outer_diameter_mm, 60.0);
    click(&mut h, "cam_section_1");
    click(&mut h, "cam_tool_name_1");
    h.step(&json!({"text":"Center tool"})).unwrap();
    h.frames(2);
    assert_eq!(h.app.session.doc.turning.as_ref().unwrap().setups[0].turret[0].tool_name.as_deref(), Some("Center tool"));
    h.app.run("edit.undo", json!({})).unwrap();
    h.frames(2);
    assert!(h.app.session.doc.turning.as_ref().unwrap().setups[0].turret[0].tool_name.is_none());
}

#[test]
fn turning_path_preview_export_and_revision_invalidation_use_real_buttons() {
    let mut h = Harness::new();
    let script: serde_json::Value = serde_json::from_str(include_str!("../../../../examples/turning-toolpath.json")).unwrap();
    for c in script["commands"].as_array().unwrap() {
        h.app.run(c["command"].as_str().unwrap(), c["params"].clone()).unwrap();
    }
    h.app.home.open = false;
    enter(&mut h.app, "turning").unwrap();
    click(&mut h, "cam_step_4");
    click(&mut h, "cam_sim_calculate");
    assert!(h.app.turning_workspace.machining.preview.is_some());
    assert!(machining_ui::active(&h.app));
    click(&mut h, "cam_path_nc_view");
    assert!(h.widget("O9001 (SOLIDCRAFT REVIEW ONLY)").is_some());
    click(&mut h, "cam_path_export");
    h.frames(4);
    let nc = std::fs::read_to_string(h.dir.join("turning-review.nc")).unwrap();
    assert!(nc.contains("M30") && nc.contains("G01"));
    click(&mut h, "cam_path_nc_view");
    let old_revision = h.app.session.revision;
    let mut plan = h.app.session.doc.turning.as_deref().unwrap().clone();
    plan.setups[0].od_job.as_mut().unwrap().radial_stock_to_leave_mm = 0.4;
    h.app.run("cam.turning.set", json!({"plan":plan})).unwrap();
    h.frames(3);
    assert!(h.app.session.revision > old_revision);
    assert!(h.widget("Preview is out of date").is_some());
    click(&mut h, "cam_sim_calculate");
    let preview = h.app.turning_workspace.machining.preview.as_ref().unwrap();
    assert_eq!(preview.0, h.app.session.revision);
    assert_eq!(preview.2.job.radial_stock_to_leave_mm, 0.4);
}

#[test]
fn turning_nl_output_format_is_saved_and_applies_to_export_from_the_review_step() {
    let mut h = Harness::new();
    let script: serde_json::Value = serde_json::from_str(include_str!("../../../../examples/turning-toolpath.json")).unwrap();
    for c in script["commands"].as_array().unwrap() {
        h.app.run(c["command"].as_str().unwrap(), c["params"].clone()).unwrap();
    }
    h.app.home.open = false;
    enter(&mut h.app, "turning").unwrap();
    click(&mut h, "cam_step_4");
    click(&mut h, "cam_nc_settings");
    click(&mut h, "cam_post_format");
    click(&mut h, "cam_post_nl");
    assert!(h.app.session.doc.turning.as_ref().unwrap().setups[0].post.is_some());
    click(&mut h, "cam_sim_calculate");
    click(&mut h, "cam_path_nc_view");
    assert!(h.widget("O9001 (SOLIDCRAFT NL SAMPLE REVIEW)").is_some());
    click(&mut h, "cam_path_export");
    h.frames(4);
    let nc = std::fs::read_to_string(h.dir.join("turning-review.nc")).unwrap();
    assert!(nc.contains("G0T1212") && nc.contains("G96S150.000M3") && nc.contains("G1X-"));
    assert!(nc.find("M30").unwrap() < nc.find("G0T").unwrap());
}

#[test]
fn turning_model_profile_buttons_fill_an_empty_profile_and_calculate() {
    let mut h = Harness::new();
    let script: serde_json::Value = serde_json::from_str(include_str!("../../../../examples/turning-toolpath.json")).unwrap();
    for c in script["commands"].as_array().unwrap() {
        h.app.run(c["command"].as_str().unwrap(), c["params"].clone()).unwrap();
    }
    let mut plan = h.app.session.doc.turning.as_deref().unwrap().clone();
    plan.setups[0].od_job.as_mut().unwrap().profile.clear();
    h.app.run("cam.turning.set", json!({"plan":plan})).unwrap();
    h.app.home.open = false;
    enter(&mut h.app, "turning").unwrap();
    click(&mut h, "cam_step_1");
    let revision = h.app.session.revision;
    click(&mut h, "cam_model_read");
    assert!(h.app.session.revision > revision, "combined extraction is one undoable edit");
    let job = h.app.session.doc.turning.as_ref().unwrap().setups[0].od_job.as_ref().unwrap();
    assert_eq!(job.profile.len(), 6);
    assert_eq!(job.model_source.as_ref().unwrap().body, "段付き軸");
    assert_eq!(job.radial_stock_to_leave_mm, 0.1);
    click(&mut h, "cam_step_4");
    click(&mut h, "cam_sim_calculate");
    assert!(h.app.turning_workspace.machining.preview.is_some());
    h.app.run("edit.undo", json!({})).unwrap();
    assert!(h.app.session.doc.turning.as_ref().unwrap().setups[0].od_job.as_ref().unwrap().profile.is_empty());
}

#[test]
fn turning_model_profile_inner_buttons_apply_and_preview_both_paths() {
    let mut h = Harness::new();
    let script: serde_json::Value = serde_json::from_str(include_str!("../../../../examples/turning-inner-profile.json")).unwrap();
    for c in script["commands"].as_array().unwrap() {
        h.app.run(c["command"].as_str().unwrap(), c["params"].clone()).unwrap();
    }
    h.app
        .run("cam.turning.profile.apply", json!({"body":"段付き貫通穴","setup":"setup-1","inner":true,"expected_revision":h.app.session.revision}))
        .unwrap();
    h.app.home.open = false;
    enter(&mut h.app, "turning").unwrap();
    click(&mut h, "cam_step_1");
    click(&mut h, "cam_model_read");
    let setup = &h.app.session.doc.turning.as_ref().unwrap().setups[0];
    assert_eq!(setup.id_job.as_ref().unwrap().profile.len(), 5);
    assert!(setup.drawing.is_some());
    // The new flow leaves operation creation to stage 3.
    let revision = h.app.session.revision;
    h.app.run("cam.turning.profile.apply", json!({"body":"段付き貫通穴","setup":"setup-1","inner":true,"expected_revision":revision})).unwrap();
    click(&mut h, "cam_step_4");
    click(&mut h, "cam_sim_calculate");
    assert!(h.app.turning_workspace.machining.preview.as_ref().unwrap().2.id_path.is_some());
    click(&mut h, "cam_path_nc_view");
    assert!(h.widget("Gコード").is_some());
}

#[test]
fn turning_catalogue_selection_preview_and_registration_use_real_buttons() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1440,"height":1000}));
    let script: serde_json::Value = serde_json::from_str(include_str!("../../../../examples/turning-toolpath.json")).unwrap();
    for c in script["commands"].as_array().unwrap() {
        h.app.run(c["command"].as_str().unwrap(), c["params"].clone()).unwrap();
    }
    h.app.home.open = false;
    enter(&mut h.app, "turning").unwrap();
    edit_tool(&mut h, 12);
    let revision = h.app.session.revision;
    click(&mut h, "cam_catalog_open");
    click(&mut h, "cam_catalog_search");
    h.step(&json!({"text":"TNMG"})).unwrap();
    click(&mut h, "cam_catalog_insert");
    click(&mut h, "cam_catalog_insert_8");
    click(&mut h, "cam_catalog_preview_insert");
    assert!(catalog_ui::scene(&h.app).is_some());
    assert!(catalogue_bounds(&h.app).is_some());
    assert_eq!(h.app.session.revision, revision);
    click(&mut h, "cam_catalog_preview_close");
    click(&mut h, "cam_catalog_holder");
    click(&mut h, "cam_catalog_holder_1");
    click(&mut h, "cam_catalog_preview_holder");
    assert!(catalog_ui::scene(&h.app).is_some());
    assert!(spatial::section_plane(&h.app).is_none());
    click(&mut h, "cam_catalog_apply");
    let slot = &h.app.session.doc.turning.as_ref().unwrap().setups[0].turret[11];
    assert_eq!(slot.catalog.as_ref().unwrap().holder.as_deref(), Some("DTGNR2525M-16"));
    assert_eq!(slot.simulation.as_ref().unwrap().nose_radius_mm, Some(0.4));
    assert!(!slot.simulation.as_ref().unwrap().holder_verified);
    click(&mut h, "cam_catalog_station");
    click(&mut h, "cam_catalog_station_5");
    assert_eq!(h.app.turning_workspace.tool_editor, Some(5));
    assert!(h.app.turning_workspace.catalog.insert.is_none());
    assert!(h.app.turning_workspace.catalog.holder.is_none());
    assert!(catalog_ui::preview(&h.app).is_none());
    h.app.run("edit.undo", json!({})).unwrap();
    assert!(h.app.session.doc.turning.as_ref().unwrap().setups[0].turret[11].catalog.is_none());
}

#[test]
fn turning_holder_cad_buttons_import_preview_and_remove() {
    let mut h = start();
    h.call("ui.resize", json!({"width":1440,"height":1200}));
    h.app.turning_workspace.advanced = false;
    h.app
        .run(
            "cam.turning.catalog.apply",
            json!({"setup":"setup-1","station":12,"expected_revision":h.app.session.revision,
        "selection":{"holder":"DTGNR2525M-16","insert":null}}),
        )
        .unwrap();
    let path = h.dir.join("analytic-holder.step");
    let mut source = solidcraft_engine::Session::default();
    source.execute("solid.box", &json!({"length":25,"width":25,"height":150})).unwrap();
    source.execute("file.export", &json!({"path":path})).unwrap();
    edit_tool(&mut h, 12);
    click(&mut h, "cam_catalog_open");
    click(&mut h, "cam_holder_cad_path");
    h.step(&json!({"text":path.to_string_lossy()})).unwrap();
    click(&mut h, "cam_holder_cad_import");
    assert!(h.app.turning_workspace.error.is_none(), "{:?}", h.app.turning_workspace.error);
    let cad = h.app.session.doc.turning.as_ref().unwrap().setups[0].turret[11].holder_cad.as_ref().unwrap();
    assert_eq!(cad.file_name, "analytic-holder.step");
    assert!(!cad.triangles.is_empty());
    let bounds = catalogue_bounds(&h.app).unwrap();
    assert!((bounds.max.z - bounds.min.z - 150.).abs() < 1e-6);
    assert!(catalog_ui::scene(&h.app).is_some());
    click(&mut h, "cam_catalog_preview_close");
    let revision = h.app.session.revision;
    click(&mut h, "cam_holder_cad_preview");
    assert_eq!(h.app.session.revision, revision);
    click(&mut h, "cam_holder_cad_remove");
    assert!(h.app.session.doc.turning.as_ref().unwrap().setups[0].turret[11].holder_cad.is_none());
    h.app.run("edit.undo", json!({})).unwrap();
    assert!(h.app.session.doc.turning.as_ref().unwrap().setups[0].turret[11].holder_cad.is_some());
}

#[test]
fn turning_custom_insert_registration_and_step_preview_buttons() {
    let mut h = start();
    h.call("ui.resize", json!({"width":1440,"height":1400}));
    h.app.turning_workspace.advanced = false;
    let path = h.dir.join("test-insert.step");
    let mut source = solidcraft_engine::Session::default();
    source.execute("solid.box", &json!({"length":8,"width":6,"height":2})).unwrap();
    source.execute("file.export", &json!({"path":path})).unwrap();
    edit_tool(&mut h, 12);
    click(&mut h, "cam_catalog_open");
    click(&mut h, "cam_custom_open");
    click(&mut h, "cam_custom_insert");
    h.step(&json!({"text":"TEST-INSERT"})).unwrap();
    click(&mut h, "cam_custom_apply");
    assert!(h.app.turning_workspace.error.is_none(), "{:?}", h.app.turning_workspace.error);
    assert_eq!(h.app.session.doc.turning.as_ref().unwrap().setups[0].turret[11].catalog.as_ref().unwrap().insert.as_deref(), Some("TEST-INSERT"));
    click(&mut h, "cam_insert_cad_path");
    h.step(&json!({"text":path.to_string_lossy()})).unwrap();
    click(&mut h, "cam_insert_cad_import");
    assert!(h.app.turning_workspace.error.is_none(), "{:?}", h.app.turning_workspace.error);
    assert!(catalog_ui::scene(&h.app).is_some());
    let b = catalogue_bounds(&h.app).unwrap();
    assert!((b.max.z - b.min.z - 2.).abs() < 1e-6);
    assert_eq!(catalog_ui::preview(&h.app), Some((false, "TEST-INSERT")));
    click(&mut h, "cam_catalog_preview_close");
    let revision = h.app.session.revision;
    click(&mut h, "cam_insert_cad_preview");
    assert_eq!(h.app.session.revision, revision);
    click(&mut h, "cam_insert_cad_remove");
    assert!(h.app.session.doc.turning.as_ref().unwrap().setups[0].turret[11].insert_cad.is_none());
    assert!(catalog_ui::preview(&h.app).is_none());
    h.app.run("edit.undo", json!({})).unwrap();
    assert!(h.app.session.doc.turning.as_ref().unwrap().setups[0].turret[11].insert_cad.is_some());
}

#[test]
fn turning_library_previews_unmounted_step_without_assigning_a_station() {
    let mut h = start();
    h.call("ui.resize", json!({"width":1440,"height":1200}));
    h.app.turning_workspace.advanced = false;
    let setups = h.app.session.doc.turning.as_ref().unwrap().setups.clone();
    let path = h.dir.join("library-holder.step");
    let mut source = solidcraft_engine::Session::default();
    source.execute("solid.box", &json!({"length":10,"width":6,"height":2})).unwrap();
    source.execute("file.export", &json!({"path":path})).unwrap();
    h.app
        .run("cam.turning.tool_cad.import", json!({"kind":"holder","model":"WTJNR2525M-16N","path":path,"expected_revision":h.app.session.revision}))
        .unwrap();
    let revision = h.app.session.revision;
    edit_tool(&mut h, 12);
    click(&mut h, "cam_catalog_open");
    click(&mut h, "cam_catalog_holder");
    let index = cam::catalog::HOLDERS.iter().position(|v| v.model == "WTJNR2525M-16N").unwrap();
    click(&mut h, &format!("cam_catalog_holder_{index}"));
    click(&mut h, "cam_catalog_preview_holder");
    let bounds = catalogue_bounds(&h.app).unwrap();
    assert!((bounds.max.z - bounds.min.z - 2.).abs() < 1e-6);
    assert!(catalog_ui::scene(&h.app).is_some());
    assert_eq!(h.app.session.revision, revision);
    assert_eq!(h.app.session.doc.turning.as_ref().unwrap().setups, setups);
    click(&mut h, "cam_catalog_preview_close");
    click(&mut h, "cam_catalog_station");
    click(&mut h, "cam_catalog_station_5");
    assert_eq!(h.app.session.doc.turning.as_ref().unwrap().tool_cad_library.len(), 1);
}

#[test]
fn turning_drilling_and_grooves_have_operation_specific_settings() {
    let mut h = start();
    for (kind, key, station) in [
        (OperationKind::Drill, "cam.drill_tip_z", 2),
        (OperationKind::OdGrooveRough, "cam.groove_tool_width", 11),
        (OperationKind::OdGrooveFinish, "cam.groove_bottom", 8),
    ] {
        choose_operation(&mut h, kind);
        click(&mut h, "cam_operation_confirm");
        let setup = &h.app.session.doc.turning.as_ref().unwrap().setups[0];
        let selected = &setup.operations[h.app.turning_workspace.operation];
        assert_eq!(selected.kind, kind);
        assert_eq!(selected.station, Some(station));
        assert!(selected.cutting.is_some());
        assert!(crate::scenario::rect_of(key).is_some());
    }
    click(&mut h, "cam_groove_copy_0");
    let revision = h.app.session.revision;
    h.frames(8);
    assert_eq!(revision, h.app.session.revision);
}
