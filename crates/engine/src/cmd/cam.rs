//! Turning planning commands. All updates are validated before mutation and participate in undo.
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Value, json};
use solidcraft_doc::cam::TurningPlan;

use super::CommandSpec;
use crate::params::bad;
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("cam.turning.selection.preview", "Preview Selected Contour Operation", selection_preview).icon("inspect").doc_only().noundo()
        .params("setup, station, contour, expected_revision. Proposes rough/finish from allowances, plans the preceding operations and selected range. Does not mutate the document."),
    CommandSpec::new("cam.turning.selection.add", "Add Selected Contour Operation", selection_add).icon("new").doc_only()
        .params("Same request as selection.preview. Recalculates and atomically inserts the reviewed operations. Rejects a stale revision. Undoable."),
    CommandSpec::new("cam.turning.difference", "Inspect Stock Difference", difference).icon("inspect").doc_only().noundo()
        .params("setup, expected_revision, remaining?: false. Conservative nominal axial stock cells and approximate excess volume; initial stock or after all supported operations."),
    CommandSpec::new("cam.turning.tool_cad.import", "Add STEP to Job Tool Library", library_cad_import).icon("import").doc_only()
        .params("kind: holder or insert, model, path: .step/.stp, expected_revision. Saves an unmounted display model in the current job library. Replaces the same kind/model; does not change turret assignments, cutting dimensions or collision geometry. Undoable."),
    CommandSpec::new("cam.turning.catalog.custom", "Register Manual Tool Models", catalogue_custom).icon("settings").doc_only()
        .params("setup, station, expected_revision, selection: {insert, holder}. Allows model IDs outside the curated list. Unknown inserts clear analytic cutting dimensions. Changed holder/insert IDs clear their old STEP snapshots; mounting remains unverified. Undoable."),
    CommandSpec::new("cam.turning.insert_cad.import", "Import Insert STEP Preview", insert_cad_import).icon("import").doc_only()
        .params("setup, station, insert: registered model, path: .step/.stp, expected_revision. Saves an independent insert display mesh. Does not derive cutting dimensions, nose compensation or mounting from the STEP. Undoable."),
    CommandSpec::new("cam.turning.holder_cad.import", "Import Holder STEP Preview", holder_cad_import).icon("import").doc_only()
        .params("setup, station, holder: registered model, path: .step/.stp, expected_revision. Imports a bounded display mesh in file coordinates, saved with the plan. Refuses partial STEP reads. Clears holder verification; never changes the workpiece or collision envelope. Undoable."),
    CommandSpec::new("cam.tool_catalog.list", "List Turning Tool Catalogue", catalogue).icon("list").noundo()
        .params("No parameters. Curated Kyocera dimensional references, sources and check date. No manufacturer CAD or grade/cutting-condition certification."),
    CommandSpec::new("cam.turning.catalog.apply", "Assign Catalogue Tool", catalogue_apply).icon("settings").doc_only()
        .params("setup, station, expected_revision, selection: {insert: model or null, holder: model or null}. Replaces catalogue selection atomically, checks OD/ID and seat compatibility, applies insert dimensions, clears changed holder geometry and never confirms mounting. Undoable."),
    CommandSpec::new("cam.turning.simulate", "Simulate Turning Cutting and Holders", simulate).icon("play").doc_only().noundo()
        .params("setup, expected_revision, resolution_mm?: 0.1 (0.1..2). Regenerates paths; optional rounded-triangle sweeps replay stock cutting at axial slice midpoints. No automatic nose compensation. Checks confirmed holder envelopes against pre-move stock and the chuck-front half-space. Returns insert/nominal stations, contact candidates and missing holders. Not machine verification."),
    CommandSpec::new("cam.turning.range", "Set Turning End Plane", cut_range).icon("settings").doc_only()
        .params("setup, end_z_mm: negative Z or null for the full profile, expected_revision. Shared OD/ID cut end from front Z0. Validated, undoable; full source profile is retained for model-change detection."),
    CommandSpec::new("cam.turning.profile", "Read Turning Profile from Model", profile).icon("inspect").doc_only().noundo()
        .params("body: evaluated body name, reverse?: false, inner?: false. Recognizes coaxial cylinders/cones/circular fillets and perpendicular planes; returns exterior or through-bore profile, front frame and revision without changing the plan. Other geometry is refused."),
    CommandSpec::new("cam.turning.profile.apply", "Use Model Turning Profile", apply_profile).icon("settings").doc_only()
        .params("body, reverse?: false, inner?: false, setup, expected_revision. Re-extracts and atomically replaces the selected OD/ID profile and its source; preserves existing cutting conditions and stock. First ID apply copies OD conditions and inserts ID rough/finish at stations 5/7. Undoable."),
    CommandSpec::new("cam.turning.toolpath", "Calculate Turning Toolpath", toolpath).icon("inspect").doc_only().noundo()
        .params("setup: setup id. Calculates configured face/OD/ID profiles, axial drilling and rectangular OD groove rough/finish in operation order. All roughing and drilling precede finishing. ID needs an existing bore or an earlier drill reaching the full profile depth. Nominal cutting references; no automatic nose compensation."),
    CommandSpec::new("cam.turning.nc", "Export Turning Review NC", nc).icon("export").doc_only().noundo()
        .params("setup: id, expected_revision: current session revision, path?: .nc file, overwrite?: false. Regenerates and exports a DISARMED review fragment, never machine-ready. Rejects stale review and unsupported operations."),
    CommandSpec::new("cam.turning.defaults", "Turning Plan Template", defaults).icon("settings")
        .doc_only()
        .noundo()
        .params("No parameters. Returns the NL2500 planning template with the owner's 12-station turret; does not change the document."),
    CommandSpec::new("cam.turning.get", "Inspect Turning Plan", get).icon("list").doc_only().noundo().params(
        "No parameters. Returns {plan, planned_outcomes, nc_export_available:false, review_nc_supported:true}. Machine-ready NC remains unavailable; toolpath and nc commands provide nominal-tip review only.",
    ),
    CommandSpec::new("cam.turning.set", "Save Turning Plan", set).icon("save")
        .doc_only()
        .params("plan: full TurningPlan or null to remove. Atomic validated replacement; saved with the design; undoable. See docs/turning-cam.md."),
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectionRequest {
    setup: String,
    station: u8,
    contour: solidcraft_doc::cam::machining::selection::Contour,
    expected_revision: u64,
}
fn selection_proposal(s: &Session, p: &Value) -> Result<(SelectionRequest, solidcraft_doc::cam::machining::selection::Proposal)> {
    let cmd = "cam.turning.selection.preview";
    let req: SelectionRequest = serde_json::from_value(p.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    if req.expected_revision != s.revision {
        return Err(bad(cmd, "モデル・工程が変わりました。提案を再計算してください"));
    }
    let setup = s.doc.turning.as_ref().and_then(|p| p.setups.iter().find(|t| t.id == req.setup)).ok_or_else(|| bad(cmd, "setup not found"))?;
    validate_sources(s, setup)?;
    let proposal = solidcraft_doc::cam::machining::selection::propose(setup, req.station, &req.contour).map_err(|e| bad(cmd, e.to_string()))?;
    Ok((req, proposal))
}
fn selection_preview(s: &mut Session, p: &Value) -> Result<Value> {
    let (req, proposal) = selection_proposal(s, p)?;
    Ok(json!({"revision":s.revision,"setup":req.setup,"proposal":proposal}))
}
fn selection_add(s: &mut Session, p: &Value) -> Result<Value> {
    let (req, proposal) = selection_proposal(s, p)?;
    let mut plan = s.doc.turning.as_deref().cloned().ok_or_else(|| bad("cam.turning.selection.add", "missing plan"))?;
    let setup = plan.setups.iter_mut().find(|t| t.id == req.setup).ok_or_else(|| bad("cam.turning.selection.add", "missing setup"))?;
    setup.operations.splice(proposal.insert_at..proposal.insert_at, proposal.operations.clone());
    set(s, &json!({"plan":plan}))?;
    Ok(json!({"revision":s.revision,"insert_at":proposal.insert_at,"count":proposal.operations.len()}))
}
fn difference(s: &mut Session, p: &Value) -> Result<Value> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Request {
        setup: String,
        expected_revision: u64,
        #[serde(default)]
        remaining: bool,
    }
    let cmd = "cam.turning.difference";
    let req: Request = serde_json::from_value(p.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    if req.expected_revision != s.revision {
        return Err(bad(cmd, "design changed"));
    }
    let setup = s.doc.turning.as_ref().and_then(|p| p.setups.iter().find(|t| t.id == req.setup)).ok_or_else(|| bad(cmd, "setup not found"))?;
    validate_sources(s, setup)?;
    let result = if req.remaining {
        solidcraft_doc::cam::machining::selection::remaining(setup)
    } else {
        solidcraft_doc::cam::machining::selection::difference(setup)
    }
    .map_err(|e| bad(cmd, e.to_string()))?;
    Ok(json!({"revision":s.revision,"setup":req.setup,"residual":result}))
}

fn holder_cad_import(s: &mut Session, p: &Value) -> Result<Value> {
    tool_cad_import(s, p, true)
}
fn insert_cad_import(s: &mut Session, p: &Value) -> Result<Value> {
    tool_cad_import(s, p, false)
}
fn tool_cad_import(s: &mut Session, p: &Value, holder: bool) -> Result<Value> {
    let cmd = if holder { "cam.turning.holder_cad.import" } else { "cam.turning.insert_cad.import" };
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Request {
        setup: String,
        station: u8,
        #[serde(alias = "holder", alias = "insert")]
        model: String,
        path: String,
        expected_revision: u64,
    }
    let req: Request = serde_json::from_value(p.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    if req.expected_revision != s.revision {
        return Err(bad(cmd, "design changed; refresh the holder selection"));
    }
    let mut plan = s.doc.turning.as_deref().cloned().ok_or_else(|| bad(cmd, "create a turning plan first"))?;
    let slot = plan
        .setups
        .iter_mut()
        .find(|v| v.id == req.setup)
        .and_then(|v| v.turret.iter_mut().find(|t| t.station == req.station))
        .ok_or_else(|| bad(cmd, "station not found"))?;
    if slot.catalog.as_ref().and_then(|c| if holder { c.holder.as_deref() } else { c.insert.as_deref() }) != Some(req.model.as_str()) {
        return Err(bad(cmd, "register the selected tool model before importing its STEP"));
    }
    let (cad, bodies) = read_tool_cad(&req.path, &req.model, cmd)?;
    let result = cad_result(&cad, bodies);
    if holder {
        slot.holder_cad = Some(Arc::new(cad));
    } else {
        slot.insert_cad = Some(Arc::new(cad));
    }
    if let Some(sim) = &mut slot.simulation {
        sim.holder_verified = false;
    }
    set(s, &json!({"plan":plan}))?;
    Ok(result)
}

fn read_tool_cad(file: &str, model: &str, cmd: &str) -> Result<(solidcraft_doc::cam::holder_cad::HolderCad, usize)> {
    use solidcraft_doc::cam::holder_cad::{HolderCad, MAX_TRIANGLES, MAX_VERTICES};
    let path = std::path::Path::new(file);
    if file.len() > 4096
        || !path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("step") || e.eq_ignore_ascii_case("stp"))
    {
        return Err(bad(cmd, "choose a .step or .stp file"));
    }
    let bytes = solidcraft_io::vfs::read(file, 4 * 1024 * 1024).map_err(|e| bad(cmd, e.to_string()))?;
    let source = std::str::from_utf8(&bytes).map_err(|e| bad(cmd, e.to_string()))?;
    let imported = solidcraft_kernel::step_import_shared(source).map_err(|e| bad(cmd, e.to_string()))?;
    if imported.bodies.is_empty() || imported.bodies.len() > 32 || !imported.warnings.is_empty() || imported.bodies.iter().any(|b| !b.closed) {
        return Err(bad(cmd, "tool STEP is empty, oversized, open or partially imported; preview was not replaced"));
    }
    let mut cad = HolderCad {
        model: model.into(),
        file_name: path.file_name().and_then(|n| n.to_str()).unwrap_or("tool.stp").into(),
        tessellation_mm: 0.1,
        positions: Vec::new(),
        triangles: Vec::new(),
    };
    for body in &imported.bodies {
        // Share the CAD viewport's display mesher, including trimmed inner loops
        // and its fallback for surfaces unsupported by the display plan.
        let mesh = body.body.display_mesh(cad.tessellation_mm).map_err(|e| bad(cmd, e.to_string()))?;
        if cad.positions.len() + mesh.positions.len() > MAX_VERTICES || cad.triangles.len() + mesh.triangles.len() > MAX_TRIANGLES {
            return Err(bad(cmd, "tool STEP display mesh is too large"));
        }
        let base = cad.positions.len() as u32;
        if mesh.triangles.iter().flatten().any(|i| *i as usize >= mesh.positions.len()) {
            return Err(bad(cmd, "tool STEP tessellation has invalid indices"));
        }
        cad.positions.extend(mesh.positions.iter().map(|p| [p.x, p.y, p.z]));
        cad.triangles.extend(mesh.triangles.iter().map(|t| t.map(|i| i + base)));
    }
    cad.validate().map_err(|e| bad(cmd, e.to_string()))?;
    Ok((cad, imported.bodies.len()))
}
fn cad_result(cad: &solidcraft_doc::cam::holder_cad::HolderCad, bodies: usize) -> Value {
    json!({"model":cad.model,"file_name":cad.file_name,"bounds_mm":cad.bounds(),"triangles":cad.triangles.len(),"bodies":bodies,"collision_geometry":false})
}
fn library_cad_import(s: &mut Session, p: &Value) -> Result<Value> {
    use solidcraft_doc::cam::holder_cad::{Kind, LibraryEntry};
    let cmd = "cam.turning.tool_cad.import";
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Request {
        kind: Kind,
        model: String,
        path: String,
        expected_revision: u64,
    }
    let req: Request = serde_json::from_value(p.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    if req.expected_revision != s.revision {
        return Err(bad(cmd, "design changed; refresh tool library"));
    }
    let mut plan = s.doc.turning.as_deref().cloned().ok_or_else(|| bad(cmd, "create a turning plan first"))?;
    let (cad, bodies) = read_tool_cad(&req.path, &req.model, cmd)?;
    let result = cad_result(&cad, bodies);
    let entry = LibraryEntry { kind: req.kind, cad: Arc::new(cad) };
    if let Some(existing) = plan.tool_cad_library.iter_mut().find(|e| e.kind == entry.kind && e.cad.model == entry.cad.model) {
        *existing = entry;
    } else {
        plan.tool_cad_library.push(entry);
    }
    set(s, &json!({"plan":plan}))?;
    Ok(result)
}

fn catalogue(_: &mut Session, _: &Value) -> Result<Value> {
    use solidcraft_doc::cam::catalog::*;
    Ok(
        json!({"manufacturer":"Kyocera","checked":CHECKED,"inserts":INSERTS,"holders":HOLDERS,"insert_source":source('B',1),"external_holder_source":source('D',1),"internal_holder_source":source('F',1),"cad_url":CAD_URL,"manufacturer_cad_included":false}),
    )
}
fn catalogue_apply(s: &mut Session, p: &Value) -> Result<Value> {
    catalogue_assign(s, p, false)
}
fn catalogue_custom(s: &mut Session, p: &Value) -> Result<Value> {
    catalogue_assign(s, p, true)
}
fn catalogue_assign(s: &mut Session, p: &Value, custom: bool) -> Result<Value> {
    let cmd = if custom { "cam.turning.catalog.custom" } else { "cam.turning.catalog.apply" };
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Request {
        setup: String,
        station: u8,
        expected_revision: u64,
        selection: solidcraft_doc::cam::catalog::Selection,
    }
    let req: Request = serde_json::from_value(p.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    if req.expected_revision != s.revision {
        return Err(bad(cmd, "design changed; refresh tool selection"));
    }
    let mut plan = s.doc.turning.as_deref().cloned().ok_or_else(|| bad(cmd, "create a turning plan first"))?;
    let slot = plan
        .setups
        .iter_mut()
        .find(|v| v.id == req.setup)
        .and_then(|v| v.turret.iter_mut().find(|t| t.station == req.station))
        .ok_or_else(|| bad(cmd, "station not found"))?;
    if custom { solidcraft_doc::cam::catalog::assign_custom(slot, req.selection) } else { solidcraft_doc::cam::catalog::assign(slot, req.selection) }
        .map_err(|e| bad(cmd, e.to_string()))?;
    set(s, &json!({"plan":plan}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathRequest {
    setup: String,
}

fn simulate(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.turning.simulate";
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Request {
        setup: String,
        expected_revision: u64,
        resolution_mm: Option<f64>,
    }
    let req: Request = serde_json::from_value(p.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    if req.expected_revision != s.revision {
        return Err(bad(cmd, "design changed; regenerate simulation"));
    }
    let path = calculate(s, &req.setup)?;
    let setup = s.doc.turning.as_ref().and_then(|p| p.setups.iter().find(|t| t.id == req.setup)).ok_or_else(|| bad(cmd, "setup not found"))?;
    let report = solidcraft_doc::cam::simulation::simulate(setup, &path, req.resolution_mm.unwrap_or(0.1)).map_err(|e| bad(cmd, e.to_string()))?;
    Ok(json!({"revision":s.revision,"setup":req.setup,"toolpath":path,"simulation":report}))
}

fn cut_range(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.turning.range";
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Request {
        setup: String,
        end_z_mm: Option<f64>,
        expected_revision: u64,
    }
    let req: Request = serde_json::from_value(p.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    if req.expected_revision != s.revision || p.get("end_z_mm").is_none() {
        return Err(bad(cmd, "current revision and an explicit end_z_mm (or null) are required"));
    }
    let mut plan = s.doc.turning.as_deref().cloned().ok_or_else(|| bad(cmd, "create a turning plan first"))?;
    let setup = plan.setups.iter_mut().find(|t| t.id == req.setup).ok_or_else(|| bad(cmd, "setup not found"))?;
    setup.cut_end_z_mm = req.end_z_mm;
    set(s, &json!({"plan":plan}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileRequest {
    body: String,
    #[serde(default)]
    reverse: bool,
    #[serde(default)]
    inner: bool,
}

fn extract(
    s: &Session,
    body: &str,
    reverse: bool,
    inner: bool,
) -> Result<(solidcraft_doc::cam::machining::ModelSource, Vec<solidcraft_doc::cam::machining::ProfilePoint>)> {
    use solidcraft_doc::cam::machining::{ModelSource, ProfilePoint};
    if body.len() > 512 {
        return Err(bad("cam.turning.profile", "body name is too long"));
    }
    let world = s.world_state();
    let selected = world.body(body).ok_or_else(|| bad("cam.turning.profile", "source body no longer exists; select a model"))?;
    let outline = if inner {
        solidcraft_kernel::turning_inner_outline(&selected.body, reverse)
    } else {
        solidcraft_kernel::turning_outline(&selected.body, reverse)
    }
    .map_err(|e| bad("cam.turning.profile", e.to_string()))?;
    let arr = |p: solidcraft_geom::Vec3| [p.x, p.y, p.z];
    let source =
        ModelSource { body: body.into(), reverse, origin: arr(outline.origin), axis: arr(outline.axis), approximation_mm: outline.approximation_mm };
    let profile = outline.points.into_iter().map(|(z_mm, diameter_mm)| ProfilePoint { z_mm, diameter_mm }).collect();
    Ok((source, profile))
}

fn profile(s: &mut Session, p: &Value) -> Result<Value> {
    let req: ProfileRequest = serde_json::from_value(p.clone()).map_err(|e| bad("cam.turning.profile", e.to_string()))?;
    let (source, profile) = extract(s, &req.body, req.reverse, req.inner)?;
    Ok(json!({"source":source,"profile":profile,"revision":s.revision,"inner":req.inner,"scope":if req.inner {"through_bore"} else {"exterior"}}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyProfile {
    body: String,
    #[serde(default)]
    reverse: bool,
    setup: String,
    expected_revision: u64,
    #[serde(default)]
    inner: bool,
    #[serde(default = "default_true")]
    create_operations: bool,
}
fn default_true() -> bool {
    true
}

fn apply_profile(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.turning.profile.apply";
    let req: ApplyProfile = serde_json::from_value(p.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    if req.expected_revision != s.revision {
        return Err(bad(cmd, "design changed; read the model profile again"));
    }
    let (source, profile) = extract(s, &req.body, req.reverse, req.inner)?;
    let mut plan = s.doc.turning.as_deref().cloned().ok_or_else(|| bad(cmd, "create a turning plan first"))?;
    let setup = plan.setups.iter_mut().find(|s| s.id == req.setup).ok_or_else(|| bad(cmd, "setup not found"))?;
    if req.inner && setup.id_job.is_none() {
        let mut job = setup.od_job.clone().unwrap_or_default();
        job.finish = true;
        setup.id_job = Some(job);
        use solidcraft_doc::cam::{Operation, OperationKind};
        for (kind, name) in [(OperationKind::IdRough, "内径荒取り"), (OperationKind::IdFinish, "内径仕上げ")] {
            if req.create_operations && !setup.operations.iter().any(|o| o.kind == kind) {
                let id = (1..=1000)
                    .map(|n| format!("{}-id-{n}", setup.id))
                    .find(|id| !setup.operations.iter().any(|o| &o.id == id))
                    .ok_or_else(|| bad(cmd, "operation id limit"))?;
                let op = Operation { cutting: None, id, name: name.into(), kind, station: kind.suggested_station(), manual_separation: None };
                let index = if kind == OperationKind::IdRough {
                    setup
                        .operations
                        .iter()
                        .position(|o| matches!(o.kind, OperationKind::FaceFinish | OperationKind::OdFinish | OperationKind::IdFinish))
                        .unwrap_or(setup.operations.len())
                } else {
                    setup.operations.len()
                };
                setup.operations.insert(index, op);
            }
        }
    }
    let job = if req.inner { setup.id_job.get_or_insert_with(Default::default) } else { setup.od_job.get_or_insert_with(Default::default) };
    job.profile = profile;
    job.model_source = Some(source);
    set(s, &json!({"plan":plan}))
}

fn check_source(s: &Session, job: &solidcraft_doc::cam::machining::OdJob, inner: bool) -> Result<()> {
    let Some(saved) = &job.model_source else { return Ok(()) };
    let (source, points) = extract(s, &saved.body, saved.reverse, inner)?;
    if points.len() != job.profile.len()
        || (source.approximation_mm - saved.approximation_mm).abs() > 1e-9
        || source.origin.iter().chain(&source.axis).zip(saved.origin.iter().chain(&saved.axis)).any(|(a, b)| (a - b).abs() > 1e-6)
        || points.iter().zip(&job.profile).any(|(a, b)| (a.z_mm - b.z_mm).abs() > 1e-6 || (a.diameter_mm - b.diameter_mm).abs() > 1e-6)
    {
        return Err(bad("cam.turning.toolpath", "model/profile changed since import; read and apply the model profile again"));
    }
    Ok(())
}

fn validate_sources(s: &Session, setup: &solidcraft_doc::cam::Setup) -> Result<()> {
    if setup.previous_setup.is_some() {
        return Err(bad("cam.turning.toolpath", "regrip stock transfer is not supported; use an independent setup with explicit stock"));
    }
    for (job, inner) in [(setup.od_job.as_ref(), false), (setup.id_job.as_ref(), true)] {
        if let Some(job) = job {
            check_source(s, job, inner)?;
        }
    }
    if let Some((a, b)) = setup.od_job.as_ref().and_then(|j| j.model_source.as_ref()).zip(setup.id_job.as_ref().and_then(|j| j.model_source.as_ref()))
        && (a.body != b.body
            || a.reverse != b.reverse
            || a.origin.iter().chain(&a.axis).zip(b.origin.iter().chain(&b.axis)).any(|(x, y)| (x - y).abs() > 1e-6))
    {
        return Err(bad("cam.turning.toolpath", "OD and ID must use the same model, axis and front face"));
    }
    Ok(())
}
fn calculate(s: &Session, setup_id: &str) -> Result<solidcraft_doc::cam::machining::Toolpath> {
    let setup = s
        .doc
        .turning
        .as_ref()
        .and_then(|p| p.setups.iter().find(|t| t.id == setup_id))
        .ok_or_else(|| bad("cam.turning.toolpath", "setup not found"))?;
    validate_sources(s, setup)?;
    solidcraft_doc::cam::machining::sequence::calculate(setup).map_err(|e| bad("cam.turning.toolpath", e.to_string()))
}

fn toolpath(s: &mut Session, p: &Value) -> Result<Value> {
    let req: PathRequest = serde_json::from_value(p.clone()).map_err(|e| bad("cam.turning.toolpath", e.to_string()))?;
    let path = calculate(s, &req.setup)?;
    Ok(json!({"revision":s.revision,"setup":req.setup,"toolpath":path}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NcRequest {
    setup: String,
    expected_revision: u64,
    path: Option<String>,
    #[serde(default)]
    overwrite: bool,
}

fn nc(s: &mut Session, p: &Value) -> Result<Value> {
    let req: NcRequest = serde_json::from_value(p.clone()).map_err(|e| bad("cam.turning.nc", e.to_string()))?;
    if req.expected_revision != s.revision {
        return Err(bad("cam.turning.nc", "design changed since preview; recalculate before exporting"));
    }
    let path = calculate(s, &req.setup)?;
    let text = solidcraft_doc::cam::machining::review_nc(&path).map_err(|e| bad("cam.turning.nc", e.to_string()))?;
    if let Some(destination) = &req.path {
        if destination.len() > 4096 || destination.chars().any(char::is_control) || !destination.to_ascii_lowercase().ends_with(".nc") {
            return Err(bad("cam.turning.nc", "a bounded .nc destination is required"));
        }
        solidcraft_io::vfs::check(destination).map_err(|e| bad("cam.turning.nc", e.to_string()))?;
        #[cfg(target_arch = "wasm32")]
        {
            if solidcraft_io::vfs::exists(destination) && !req.overwrite {
                return Err(bad("cam.turning.nc", "destination exists; choose a new filename or explicitly allow overwrite"));
            }
            solidcraft_io::vfs::write(destination, text.as_bytes()).map_err(|e| bad("cam.turning.nc", e.to_string()))?;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let destination = std::path::Path::new(destination);
            let result = if req.overwrite {
                solidcraft_io::write_atomic(destination, text.as_bytes(), false)
            } else {
                solidcraft_io::safe_write::create_atomic(destination, text.as_bytes())
            };
            result.map_err(|e| bad("cam.turning.nc", e.to_string()))?;
        }
    }
    Ok(
        json!({"text":text,"path":req.path,"machine_ready":false,"disarmed":true,"revision":s.revision,"scope":"configured_face_od_id_drilling_od_grooves"}),
    )
}

fn empty(command: &str, params: &Value) -> Result<()> {
    if !params.as_object().is_some_and(|object| object.is_empty()) {
        return Err(bad(command, "expected an empty parameter object"));
    }
    Ok(())
}

fn defaults(_: &mut Session, params: &Value) -> Result<Value> {
    empty("cam.turning.defaults", params)?;
    Ok(json!({"plan": TurningPlan::nl2500(), "nc_export_available": false}))
}

fn get(s: &mut Session, params: &Value) -> Result<Value> {
    empty("cam.turning.get", params)?;
    let outcomes: Vec<_> = s
        .doc
        .turning
        .iter()
        .flat_map(|plan| plan.setups.iter())
        .map(|setup| json!({"setup": setup.id, "outcome": setup.planned_outcome()}))
        .collect();
    Ok(json!({"plan": s.doc.turning, "planned_outcomes": outcomes, "nc_export_available": false, "review_nc_supported": true}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetPlan {
    // Value makes the key required; a missing key must not silently delete a saved plan.
    plan: Value,
}

fn set(s: &mut Session, params: &Value) -> Result<Value> {
    let command = "cam.turning.set";
    let request: SetPlan = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    let plan: Option<TurningPlan> = serde_json::from_value(request.plan).map_err(|e| bad(command, e.to_string()))?;
    if let Some(plan) = &plan {
        plan.validate().map_err(|e| bad(command, e.to_string()))?;
    }
    if s.doc.turning.as_deref() != plan.as_ref() {
        s.doc_mut().turning = plan.map(Arc::new);
    }
    get(s, &json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_contour_preview_add_revision_and_roundtrip() {
        use solidcraft_doc::cam::machining::{selection::Contour, sequence::Limits};
        let mut s = path_example();
        let mut plan = s.doc.turning.as_deref().unwrap().clone();
        let setup = &mut plan.setups[0];
        setup.operations.clear();
        let j = setup.od_job.as_ref().unwrap();
        let c = Contour {
            limits: Limits { stock_front_z_mm: j.stock_front_z_mm, chuck_front_z_mm: j.chuck_front_z_mm, clearance_mm: j.clearance_mm },
            inner: false,
            first: 0,
            last: j.profile.len() - 1,
            profile_snapshot: j.profile.clone(),
            source_snapshot: j.model_source.clone(),
            radial_leave_mm: 0.,
            axial_leave_mm: 0.,
            depth_mm: 0.3,
            feed_mm_rev: 0.1,
        };
        s.execute("cam.turning.set", &json!({"plan":plan})).unwrap();
        let before = s.doc.turning.clone();
        let revision = s.revision;
        let request = json!({"setup":"setup-1","station":6,"contour":c,"expected_revision":revision});
        let p = s.execute("cam.turning.selection.preview", &request).unwrap();
        assert_eq!(p["proposal"]["rough_then_finish"], true);
        assert_eq!(s.doc.turning, before);
        assert_eq!(s.revision, revision);
        s.execute("cam.turning.selection.add", &request).unwrap();
        assert_eq!(s.doc.turning.as_ref().unwrap().setups[0].operations.len(), 2);
        assert!(s.execute("cam.turning.selection.add", &request).is_err());
        assert_eq!(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap().turning, s.doc.turning);
        assert!(s.execute("cam.turning.toolpath", &json!({"setup":"setup-1"})).is_ok());
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc.turning, before);
    }

    #[test]
    fn catalogue_assignment_is_atomic_saved_and_undoable() {
        let mut s = path_example();
        let before = s.doc.turning.clone();
        let catalog = s.execute("cam.tool_catalog.list", &json!({})).unwrap();
        assert_eq!(catalog["inserts"].as_array().unwrap().len(), solidcraft_doc::cam::catalog::INSERTS.len());
        assert_eq!(catalog["holders"].as_array().unwrap().len(), solidcraft_doc::cam::catalog::HOLDERS.len());
        let request =
            json!({"setup":"setup-1","station":12,"expected_revision":s.revision,"selection":{"insert":"TNMG160404HQ","holder":"DTGNR2525M-16"}});
        s.execute("cam.turning.catalog.apply", &request).unwrap();
        let slot = &s.doc.turning.as_ref().unwrap().setups[0].turret[11];
        assert_eq!(slot.catalog.as_ref().unwrap().insert.as_deref(), Some("TNMG160404HQ"));
        assert!(!slot.simulation.as_ref().unwrap().holder_verified);
        let encoded = serde_json::to_value(&s.doc.turning).unwrap();
        let decoded: Option<TurningPlan> = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded.as_ref(), s.doc.turning.as_deref());
        assert!(s.execute("cam.turning.catalog.apply", &request).is_err());
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc.turning, before);
    }

    #[test]
    fn insert_cad_and_custom_models_are_independent_of_holder_and_cutting_geometry() {
        let mut s = path_example();
        s.execute(
            "cam.turning.catalog.apply",
            &json!({"setup":"setup-1","station":12,"expected_revision":s.revision,
            "selection":{"holder":"DTGNR2525M-16","insert":"TNMG160404HQ"}}),
        )
        .unwrap();
        let dir = solidcraft_io::vfs::temp_dir().join(format!("cam-insert-cad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("analytic-insert.step");
        let mut source = Session::default();
        source.execute("solid.box", &json!({"length":8,"width":6,"height":2})).unwrap();
        source.execute("file.export", &json!({"path":path})).unwrap();
        s.execute(
            "cam.turning.holder_cad.import",
            &json!({"setup":"setup-1","station":12,"holder":"DTGNR2525M-16","path":path,"expected_revision":s.revision}),
        )
        .unwrap();
        let holder = s.doc.turning.as_ref().unwrap().setups[0].turret[11].holder_cad.clone();
        let sim = s.doc.turning.as_ref().unwrap().setups[0].turret[11].simulation.clone();
        let before = s.doc.turning.clone();
        let req = json!({"setup":"setup-1","station":12,"insert":"TNMG160404HQ","path":path,"expected_revision":s.revision});
        s.execute("cam.turning.insert_cad.import", &req).unwrap();
        assert!(s.execute("cam.turning.insert_cad.import", &req).is_err());
        let slot = &s.doc.turning.as_ref().unwrap().setups[0].turret[11];
        assert_eq!(slot.holder_cad, holder);
        assert_eq!(slot.simulation, sim, "display STEP must not redefine cutting dimensions");
        assert_eq!(slot.insert_cad.as_ref().unwrap().model, "TNMG160404HQ");
        assert_eq!(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap().turning, s.doc.turning);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc.turning, before);
        s.execute("edit.redo", &json!({})).unwrap();
        s.execute(
            "cam.turning.catalog.custom",
            &json!({"setup":"setup-1","station":12,"expected_revision":s.revision,
            "selection":{"holder":"DTGNR2525M-16","insert":"CUSTOM-INSERT"}}),
        )
        .unwrap();
        let slot = &s.doc.turning.as_ref().unwrap().setups[0].turret[11];
        assert_eq!(slot.holder_cad, holder);
        assert!(slot.insert_cad.is_none());
        assert!(slot.simulation.as_ref().unwrap().insert.is_none());
        assert!(slot.simulation.as_ref().unwrap().nose_radius_mm.is_none());
        s.execute(
            "cam.turning.insert_cad.import",
            &json!({"setup":"setup-1","station":12,"insert":"CUSTOM-INSERT","path":path,"expected_revision":s.revision}),
        )
        .unwrap();
        assert_eq!(s.doc.turning.as_ref().unwrap().setups[0].turret[11].insert_cad.as_ref().unwrap().model, "CUSTOM-INSERT");
        let valid = s.doc.turning.clone();
        assert!(
            s.execute(
                "cam.turning.catalog.custom",
                &json!({"setup":"setup-1","station":12,"expected_revision":s.revision,
            "selection":{"holder":null,"insert":"../invalid"}})
            )
            .is_err()
        );
        assert_eq!(s.doc.turning, valid);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn turning_tool_step_keeps_the_central_hole_open() {
        use solidcraft_geom::{Loop2, Plane, Region2, Vec2};
        let profile = Region2 {
            outer: Loop2::polygon(&[Vec2::new(-10., -8.), Vec2::new(10., -8.), Vec2::new(0., 12.)]),
            holes: vec![Loop2::circle(Vec2::ZERO, 2.).reversed()],
        };
        let bodies = solidcraft_kernel::extrude(&Plane::XY, &[profile], 0., 4.).unwrap();
        let source = solidcraft_kernel::step_export(&[&bodies[0]], "analytic insert with bore").unwrap();
        let path = solidcraft_io::vfs::temp_dir().join(format!("cam-tool-hole-{}.step", std::process::id()));
        std::fs::write(&path, source).unwrap();
        let (cad, _) = super::read_tool_cad(path.to_str().unwrap(), "ANALYTIC-INSERT", "test").unwrap();
        let mut top_area = 0.;
        for t in &cad.triangles {
            let [a, b, c] = t.map(|i| cad.positions[i as usize]);
            if [a, b, c].iter().all(|p| (p[2] - 4.).abs() < 1e-6) {
                let cross = |p: [f64; 3], q: [f64; 3]| p[0] * q[1] - p[1] * q[0];
                let signs = [cross(a, b), cross(b, c), cross(c, a)];
                assert!(!(signs.iter().all(|v| *v >= 0.) || signs.iter().all(|v| *v <= 0.)), "triangle covers bore center");
                top_area += ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() * 0.5;
            }
        }
        assert!((top_area - (200. - 4. * std::f64::consts::PI)).abs() < 1.0, "top area {top_area}");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn turning_tool_library_preserves_assignments_and_rejects_stale_duplicate_models() {
        let mut s = path_example();
        let setups = s.doc.turning.as_ref().unwrap().setups.clone();
        let dir = solidcraft_io::vfs::temp_dir().join(format!("cam-library-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("analytic-library.step");
        let mut source = Session::default();
        source.execute("solid.box", &json!({"length":10,"width":6,"height":2})).unwrap();
        source.execute("file.export", &json!({"path":path})).unwrap();
        let req = json!({"kind":"holder","model":"WTJNR2525M-16N","path":path,"expected_revision":s.revision});
        s.execute("cam.turning.tool_cad.import", &req).unwrap();
        assert!(s.execute("cam.turning.tool_cad.import", &req).is_err());
        let plan = s.doc.turning.as_ref().unwrap();
        assert_eq!(plan.setups, setups);
        assert_eq!(plan.tool_cad_library.len(), 1);
        assert!(solidcraft_doc::cam::holder_cad::find(&plan.tool_cad_library, true, "WTJNR2525M-16N").is_some());
        assert!(solidcraft_doc::cam::holder_cad::find(&plan.tool_cad_library, false, "WTJNR2525M-16N").is_none());
        assert_eq!(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap().turning, s.doc.turning);
        let mut invalid = plan.as_ref().clone();
        invalid.tool_cad_library.push(invalid.tool_cad_library[0].clone());
        assert!(invalid.validate().is_err());
        let before = s.doc.turning.clone();
        assert!(
            s.execute("cam.turning.tool_cad.import", &json!({"kind":"holder","model":"../bad","path":path,"expected_revision":s.revision})).is_err()
        );
        assert_eq!(s.doc.turning, before);
        s.execute("cam.turning.tool_cad.import", &json!({"kind":"holder","model":"WTJNR2525M-16N","path":path,"expected_revision":s.revision}))
            .unwrap();
        assert_eq!(s.doc.turning.as_ref().unwrap().tool_cad_library.len(), 1);
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.doc.turning.as_ref().unwrap().tool_cad_library.is_empty());
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(s.doc.turning, before);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn holder_cad_import_preserves_workpiece_checks_identity_and_survives_reload() {
        let mut s = path_example();
        let workpiece = s.doc.features.clone();
        s.execute(
            "cam.turning.catalog.apply",
            &json!({"setup":"setup-1","station":12,"expected_revision":s.revision,
            "selection":{"holder":"DTGNR2525M-16","insert":"TNMG160404HQ"}}),
        )
        .unwrap();
        let before = s.doc.turning.clone();
        let dir = solidcraft_io::vfs::temp_dir().join(format!("cam-holder-cad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("analytic-holder.step");
        let mut tool = Session::default();
        tool.execute("solid.box", &json!({"length":25,"width":25,"height":150})).unwrap();
        tool.execute("file.export", &json!({"path":path})).unwrap();
        let mut req = json!({"setup":"setup-1","station":12,"holder":"DTGNR2525M-16","path":path,"expected_revision":s.revision});
        let result = s.execute("cam.turning.holder_cad.import", &req).unwrap();
        assert_eq!(result["bodies"], 1);
        assert_eq!(result["collision_geometry"], false);
        assert_eq!(s.doc.features, workpiece);
        let cad = s.doc.turning.as_ref().unwrap().setups[0].turret[11].holder_cad.as_ref().unwrap();
        let (lo, hi) = cad.bounds().unwrap();
        assert!((hi[2] - lo[2] - 150.).abs() < 1e-6);
        assert_eq!(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap().turning, s.doc.turning);
        let imported = s.doc.turning.clone();
        assert!(s.execute("cam.turning.holder_cad.import", &req).is_err());
        req["expected_revision"] = json!(s.revision);
        req["holder"] = json!("S08X-STLCR09-10A");
        assert!(s.execute("cam.turning.holder_cad.import", &req).is_err());
        assert_eq!(s.doc.turning, imported);
        req["holder"] = json!("DTGNR2525M-16");
        std::fs::write(&path, "not a STEP").unwrap();
        assert!(s.execute("cam.turning.holder_cad.import", &req).is_err());
        assert_eq!(s.doc.turning, imported);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc.turning, before);
        s.execute("edit.redo", &json!({})).unwrap();
        s.execute(
            "cam.turning.catalog.apply",
            &json!({"setup":"setup-1","station":12,"expected_revision":s.revision,
            "selection":{"holder":"DTGNR2020K-16","insert":"TNMG160404HQ"}}),
        )
        .unwrap();
        assert!(s.doc.turning.as_ref().unwrap().setups[0].turret[11].holder_cad.is_none());
        std::fs::remove_file(path).unwrap();
    }

    fn path_example() -> Session {
        let mut s = Session::default();
        let script: Value = serde_json::from_str(include_str!("../../../../examples/turning-toolpath.json")).unwrap();
        for c in script["commands"].as_array().unwrap() {
            s.execute(c["command"].as_str().unwrap(), &c["params"]).unwrap();
        }
        s
    }

    #[test]
    fn model_profile_inner_apply_paths_nc_undo_and_model_change() {
        use solidcraft_doc::cam::OperationKind;
        let mut s = Session::default();
        let script: Value = serde_json::from_str(include_str!("../../../../examples/turning-inner-profile.json")).unwrap();
        for c in script["commands"].as_array().unwrap() {
            s.execute(c["command"].as_str().unwrap(), &c["params"]).unwrap();
        }
        let original = s.doc.turning.clone();
        let read = s.execute("cam.turning.profile", &json!({"body":"段付き貫通穴","inner":true})).unwrap();
        assert_eq!(read["profile"].as_array().unwrap().len(), 5);
        s.execute("cam.turning.profile.apply", &json!({"body":"段付き貫通穴","inner":true,"setup":"setup-1","expected_revision":s.revision}))
            .unwrap();
        let setup = &s.doc.turning.as_ref().unwrap().setups[0];
        assert_eq!(
            setup.operations.iter().map(|o| o.kind).collect::<Vec<_>>(),
            [
                OperationKind::FaceRough,
                OperationKind::OdRough,
                OperationKind::IdRough,
                OperationKind::FaceFinish,
                OperationKind::OdFinish,
                OperationKind::IdFinish
            ]
        );
        assert_eq!(setup.operations[2].station, Some(5));
        assert_eq!(setup.operations[5].station, Some(7));
        assert!(setup.id_job.as_ref().unwrap().model_source.is_some());
        assert_eq!(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap().turning, s.doc.turning);
        let path = s.execute("cam.turning.toolpath", &json!({"setup":"setup-1"})).unwrap();
        assert_eq!(path["toolpath"]["id_path"]["job"]["profile"].as_array().unwrap().len(), 5);
        let nc = s.execute("cam.turning.nc", &json!({"setup":"setup-1","expected_revision":s.revision})).unwrap();
        assert!(nc["text"].as_str().unwrap().contains("IdFinish"));
        s.execute("solid.move", &json!({"bodies":["段付き貫通穴"],"translate":[3,0,0]})).unwrap();
        assert!(s.execute("cam.turning.toolpath", &json!({"setup":"setup-1"})).is_err());
        assert!(s.execute("cam.turning.nc", &json!({"setup":"setup-1","expected_revision":s.revision})).is_err());
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc.turning, original);
    }

    #[test]
    fn holder_simulation_is_revision_bound_and_keeps_unknown_geometry_unchecked() {
        let mut s = path_example();
        let revision = s.revision;
        let first = s.execute("cam.turning.simulate", &json!({"setup":"setup-1","expected_revision":revision})).unwrap();
        assert_eq!(first["simulation"]["checked_stations"], json!([]));
        assert_eq!(first["simulation"]["missing_stations"], json!([6, 12]));
        assert_eq!(s.revision, revision);
        let mut plan = s.doc.turning.as_deref().unwrap().clone();
        use solidcraft_doc::cam::simulation::{Holder, HolderKind, ToolAssembly};
        plan.setups[0].turret[11].simulation = Some(ToolAssembly {
            triangular_insert: true,
            nose_radius_mm: Some(0.4),
            holder: Some(Holder {
                kind: HolderKind::Rectangular,
                z_start_mm: 0.,
                length_mm: 30.,
                r_center_mm: 0.,
                t_center_mm: 0.,
                width_mm: 20.,
                height_mm: 20.,
            }),
            holder_verified: true,
            insert: None,
        });
        s.execute("cam.turning.set", &json!({"plan":plan})).unwrap();
        assert!(s.execute("cam.turning.simulate", &json!({"setup":"setup-1","expected_revision":revision})).is_err());
        assert_eq!(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap().turning, s.doc.turning);
        let result = s.execute("cam.turning.simulate", &json!({"setup":"setup-1","expected_revision":s.revision})).unwrap();
        assert_eq!(result["simulation"]["checked_stations"], json!([12]));
        assert!(!result["simulation"]["contacts"].as_array().unwrap().is_empty());
        assert_eq!(result["simulation"]["machine_ready"], false);
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.doc.turning.as_ref().unwrap().setups[0].turret[11].simulation.is_none());
    }

    #[test]
    fn selected_range_limits_paths_and_nc_without_detaching_model() {
        let mut s = path_example();
        s.execute("cam.turning.profile.apply", &json!({"body":"段付き軸","setup":"setup-1","expected_revision":s.revision})).unwrap();
        let full = s.doc.turning.clone();
        let old_revision = s.revision;
        s.execute("cam.turning.range", &json!({"setup":"setup-1","end_z_mm":-35.,"expected_revision":s.revision})).unwrap();
        assert_eq!(s.doc.turning.as_ref().unwrap().setups[0].od_job, full.as_ref().unwrap().setups[0].od_job);
        let path = calculate(&s, "setup-1").unwrap();
        assert_eq!(path.job.profile.last().unwrap().z_mm, -35.);
        assert!(path.moves.iter().all(|m| m.from.z_mm >= -35. - 1e-8 && m.to.z_mm >= -35. - 1e-8));
        assert!(s.execute("cam.turning.nc", &json!({"setup":"setup-1","expected_revision":old_revision})).is_err());
        let nc = s.execute("cam.turning.nc", &json!({"setup":"setup-1","expected_revision":s.revision})).unwrap();
        assert!(nc["text"].as_str().unwrap().contains("Z-35"));
        assert_eq!(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap().turning, s.doc.turning);
        let saved = s.doc.turning.clone();
        for end in [0., -999.] {
            assert!(s.execute("cam.turning.range", &json!({"setup":"setup-1","end_z_mm":end,"expected_revision":s.revision})).is_err());
            assert_eq!(s.doc.turning, saved);
        }
        s.execute("solid.move", &json!({"bodies":["段付き軸"],"translate":[3,0,0]})).unwrap();
        assert!(calculate(&s, "setup-1").is_err());
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc.turning, full);
    }

    #[test]
    fn model_profile_is_atomic_preserves_conditions_persists_and_rejects_stale_source() {
        let mut s = path_example();
        let original = s.doc.turning.as_deref().unwrap().clone();
        let old_job = original.setups[0].od_job.as_ref().unwrap();
        s.mark_saved();
        let read = s.execute("cam.turning.profile", &json!({"body":"段付き軸"})).unwrap();
        assert!(!s.is_dirty());
        let points: Vec<solidcraft_doc::cam::machining::ProfilePoint> = serde_json::from_value(read["profile"].clone()).unwrap();
        assert_eq!(points.len(), old_job.profile.len());
        for (a, b) in points.iter().zip(&old_job.profile) {
            assert!((a.z_mm - b.z_mm).abs() < 1e-6 && (a.diameter_mm - b.diameter_mm).abs() < 1e-6);
        }
        s.execute("cam.turning.profile.apply", &json!({"body":"段付き軸","setup":"setup-1","expected_revision":s.revision})).unwrap();
        let saved = s.doc.turning.as_deref().unwrap().clone();
        let job = saved.setups[0].od_job.as_ref().unwrap();
        assert!(job.model_source.is_some());
        assert_eq!(job.radial_stock_to_leave_mm, old_job.radial_stock_to_leave_mm);
        assert_eq!(job.axial_stock_to_leave_mm, old_job.axial_stock_to_leave_mm);
        assert_eq!(job.rough_feed_mm_rev, old_job.rough_feed_mm_rev);
        assert_eq!(saved.setups[0].stock, original.setups[0].stock);
        assert_eq!(saved.setups[0].operations, original.setups[0].operations);
        assert_eq!(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap().turning, s.doc.turning);
        s.execute("cam.turning.toolpath", &json!({"setup":"setup-1"})).unwrap();
        s.execute("solid.move", &json!({"bodies":["段付き軸"],"translate":[3,0,0]})).unwrap();
        assert!(s.execute("cam.turning.toolpath", &json!({"setup":"setup-1"})).is_err());
        assert!(s.execute("cam.turning.nc", &json!({"setup":"setup-1","expected_revision":s.revision})).is_err());
        let before = s.doc.clone();
        assert!(s.execute("cam.turning.profile.apply", &json!({"body":"段付き軸","setup":"setup-1","expected_revision":read["revision"]})).is_err());
        assert_eq!(s.doc, before);
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("cam.turning.toolpath", &json!({"setup":"setup-1"})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc.turning.as_deref(), Some(&original));
        s.execute("solid.box", &json!({"length":10,"width":20,"height":30,"body_name":"Unsupported"})).unwrap();
        let before = s.doc.clone();
        assert!(s.execute("cam.turning.profile.apply", &json!({"body":"Unsupported","setup":"setup-1","expected_revision":s.revision})).is_err());
        assert_eq!(s.doc, before);
    }

    #[test]
    fn face_finish_is_explicit_and_cannot_precede_od_roughing() {
        use solidcraft_doc::cam::OperationKind;
        let mut s = path_example();
        let plan = s.doc.turning.as_deref().unwrap().clone();
        assert_eq!(
            plan.setups[0].operations.iter().map(|o| o.kind).collect::<Vec<_>>(),
            [OperationKind::FaceRough, OperationKind::OdRough, OperationKind::FaceFinish, OperationKind::OdFinish]
        );
        assert_eq!(plan.setups[0].operations[2].station, Some(6));
        let mut interleaved = plan.clone();
        interleaved.setups[0].operations.swap(1, 2);
        s.execute("cam.turning.set", &json!({"plan":interleaved})).unwrap();
        assert!(s.execute("cam.turning.toolpath", &json!({"setup":"setup-1"})).is_err());
        let mut rough_only = plan.clone();
        rough_only.setups[0].operations.truncate(2);
        rough_only.setups[0].od_job.as_mut().unwrap().finish = false;
        s.execute("cam.turning.set", &json!({"plan":rough_only})).unwrap();
        s.execute("cam.turning.toolpath", &json!({"setup":"setup-1"})).unwrap();
        let mut legacy = plan;
        legacy.setups[0].operations[0].kind = OperationKind::Face;
        s.execute("cam.turning.set", &json!({"plan":legacy})).unwrap();
        assert!(s.execute("cam.turning.toolpath", &json!({"setup":"setup-1"})).is_err());
    }

    #[test]
    fn path_preview_export_rejects_stale_geometry_and_never_dirties_document() {
        let mut s = path_example();
        s.mark_saved();
        let preview = s.execute("cam.turning.toolpath", &json!({"setup":"setup-1"})).unwrap();
        let old_rev = preview["revision"].as_u64().unwrap();
        let nc = s.execute("cam.turning.nc", &json!({"setup":"setup-1","expected_revision":old_rev})).unwrap();
        assert_eq!(nc["machine_ready"], false);
        assert_eq!(nc["disarmed"], true);
        assert!(!s.is_dirty());
        let saved = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
        assert_eq!(saved.turning, s.doc.turning);
        s.execute("solid.box", &json!({"length":1,"width":1,"height":1})).unwrap();
        assert!(s.execute("cam.turning.nc", &json!({"setup":"setup-1","expected_revision":old_rev})).is_err());
        let mut plan = s.doc.turning.as_deref().unwrap().clone();
        plan.setups[0].operations[1].kind = solidcraft_doc::cam::OperationKind::IdRough;
        s.execute("cam.turning.set", &json!({"plan":plan})).unwrap();
        assert!(s.execute("cam.turning.toolpath", &json!({"setup":"setup-1"})).is_err());
        assert!(s.execute("cam.turning.nc", &json!({"setup":"setup-1","expected_revision":s.revision})).is_err());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn nc_export_is_atomic_no_clobber_and_respects_file_scope() {
        let mut s = path_example();
        let dir = solidcraft_io::vfs::temp_dir().join(format!("cam-nc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("review.nc");
        std::fs::write(&path, b"existing NC").unwrap();
        let request = json!({"setup":"setup-1","expected_revision":s.revision,"path":path});
        assert!(s.execute("cam.turning.nc", &request).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"existing NC");
        let mut replace = request.clone();
        replace["overwrite"] = json!(true);
        let exported = s.execute("cam.turning.nc", &replace).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), exported["text"].as_str().unwrap());
        let denied = solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::default()), || s.execute("cam.turning.nc", &replace));
        assert!(denied.is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), exported["text"].as_str().unwrap());
        assert!(s.execute("cam.turning.nc", &json!({"setup":"setup-1","expected_revision":s.revision,"path":dir.join("bad.txt")})).is_err());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn turning_plan_is_persistent_undoable_and_noop_safe() {
        let mut s = Session::default();
        let preset = s.execute("cam.turning.defaults", &json!({})).unwrap();
        assert!(s.doc.turning.is_none());
        assert!(!s.is_dirty());
        s.execute("cam.turning.set", &json!({"plan": preset["plan"]})).unwrap();
        assert!(s.is_dirty());
        let undo_count = s.undo.len();
        let revision = s.revision;
        s.execute("cam.turning.set", &json!({"plan": preset["plan"]})).unwrap();
        assert_eq!(s.undo.len(), undo_count);
        assert_eq!(s.revision, revision);
        let saved = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
        assert_eq!(saved.turning, s.doc.turning);
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.doc.turning.is_none());
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(s.doc.turning, saved.turning);
        s.execute("cam.turning.set", &json!({"plan":null})).unwrap();
        assert!(s.doc.turning.is_none());
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc.turning, saved.turning);
    }

    #[test]
    fn invalid_updates_do_not_erase_a_plan_or_change_history() {
        let mut s = Session::default();
        s.execute("cam.turning.set", &json!({"plan": TurningPlan::nl2500()})).unwrap();
        let before = s.doc.clone();
        let undo_count = s.undo.len();
        for params in [json!({}), json!({"plan":{"machine":"NL2500"}}), json!({"plan":null,"typo":true})] {
            assert!(s.execute("cam.turning.set", &params).is_err());
            assert_eq!(s.doc, before);
            assert_eq!(s.undo.len(), undo_count);
        }
        let mut invalid = TurningPlan::nl2500();
        invalid.setups[0].turret[0].station = 12;
        assert!(s.execute("cam.turning.set", &json!({"plan":invalid})).is_err());
        assert_eq!(s.doc, before);
        assert_eq!(s.undo.len(), undo_count);
        let mut saved = serde_json::to_value(&*before).unwrap();
        saved["turning"] = serde_json::to_value(invalid).unwrap();
        assert!(solidcraft_doc::Document::from_json(&saved.to_string()).is_err());
    }

    #[test]
    fn older_designs_have_no_implicit_cam_and_export_stays_unavailable() {
        let mut s = Session::default();
        assert!(!s.doc.to_json().contains("\"turning\""));
        let old = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
        assert!(old.turning.is_none());
        let reply = s.execute("cam.turning.get", &json!({})).unwrap();
        assert_eq!(reply["nc_export_available"], false);
        assert!(reply["plan"].is_null());
    }

    #[test]
    fn example_keeps_geometry_and_reports_manual_separation() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length":10,"width":20,"height":5})).unwrap();
        let features = s.doc.features.clone();
        let script: Value = serde_json::from_str(include_str!("../../../../examples/turning-plan.json")).unwrap();
        for command in script["commands"].as_array().unwrap() {
            s.execute(command["command"].as_str().unwrap(), &command["params"]).unwrap();
        }
        assert_eq!(s.doc.features, features);
        let result = s.execute("cam.turning.get", &json!({})).unwrap();
        assert_eq!(result["planned_outcomes"][0]["outcome"], "manual_separation_required");
        assert_eq!(result["nc_export_available"], false);
        let restored = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
        assert_eq!(restored.turning, s.doc.turning);
        assert_eq!(restored.features, features);
    }
    #[test]
    fn drilling_and_groove_commands_roundtrip_and_regenerate_review() {
        use solidcraft_doc::cam::{
            Operation, OperationKind as K,
            machining::sequence::{Cutting, Drill, Groove},
        };
        let mut s = path_example();
        let mut plan = s.doc.turning.as_deref().unwrap().clone();
        let setup = &mut plan.setups[0];
        let drill = Drill { diameter_mm: 4., tip_end_z_mm: -10., feed_mm_rev: 0.08, peck_mm: 3., ..Drill::default() };
        let groove = Groove {
            front_z_mm: -5.,
            width_mm: 5.,
            bottom_diameter_mm: 16.,
            tool_width_mm: 2.,
            radial_depth_mm: 1.,
            feed_mm_rev: 0.08,
            ..Groove::default()
        };
        let op = |id: &str, kind: K, cutting| Operation {
            id: id.into(),
            name: id.into(),
            kind,
            station: kind.suggested_station(),
            manual_separation: None,
            cutting: Some(cutting),
        };
        setup.operations.insert(2, op("drill", K::Drill, Cutting::Drill(drill)));
        setup.operations.insert(3, op("gr", K::OdGrooveRough, Cutting::OdGroove(groove.clone())));
        setup.operations.push(op("gf", K::OdGrooveFinish, Cutting::OdGroove(groove)));
        s.execute("cam.turning.set", &json!({"plan":plan})).unwrap();
        assert_eq!(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap().turning, s.doc.turning);
        let path = s.execute("cam.turning.toolpath", &json!({"setup":"setup-1"})).unwrap();
        assert_eq!(path["toolpath"]["operation_spans"].as_array().unwrap().len(), 7);
        let revision = s.revision;
        let nc = s.execute("cam.turning.nc", &json!({"setup":"setup-1","expected_revision":revision})).unwrap();
        assert!(nc["text"].as_str().unwrap().contains("OdGrooveFinish"));
        assert_eq!(nc["machine_ready"], false);
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.execute("cam.turning.nc", &json!({"setup":"setup-1","expected_revision":revision})).is_err());
    }
}
