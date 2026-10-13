use crate::ocr_poll::ServerStatus;
use crate::routes::projects::forms::{
    CreateProject, IngestQuery, RemoveRequest, ScanConflict, ScanPageResult, ScanRequest,
    SettingsUpdate, AutoAssistRequest, AutoBridgeRequest,
    ScanJobStatus, JobState,
};
use crate::routes::projects::models::{IMPORT_ORDER_GAP, Page, PageDb, StructureDb, HintType};
use crate::routes::projects::{images, storage};
use crate::state::AppState;
use crate::hocr_parser::ParserConfig;
use crate::hocr_parser::navigation::{preceding_block, succeeding_block};
use axum::Json;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse};
use tokio::fs;

pub async fn settings_service_status_get(State(state): State<AppState>) -> impl IntoResponse {
    Json(storage::check_service_status(&state))
}

pub async fn settings_get(State(state): State<AppState>) -> impl IntoResponse {
    Json(storage::read_settings(&state))
}

pub async fn settings_post(
    State(state): State<AppState>,
    Json(payload): Json<SettingsUpdate>,
) -> impl IntoResponse {
    match storage::write_settings(&state, &payload) {
        Ok(()) => Json(storage::read_settings(&state)).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

pub async fn list_projects(State(state): State<AppState>) -> impl IntoResponse {
    Json(storage::read_projects(&state))
}

pub async fn create_project(
    State(state): State<AppState>,
    Json(mut payload): Json<CreateProject>,
) -> impl IntoResponse {
    payload.machine_name = payload.machine_name.trim().to_string();
    payload.name = payload.name.trim().to_string();

    if !storage::is_valid_machine_name(&payload.machine_name) || payload.name.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid name"})),
        )
            .into_response();
    }

    let project_dir = state.projects_dir.join(&payload.machine_name);
    if project_dir.exists() {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "project already exists"})),
        )
            .into_response();
    }

    match storage::create_project_on_disk(&state, &payload.name, &payload.machine_name) {
        Ok(_) => (
            StatusCode::CREATED,
            Json(storage::read_project(&state.projects_dir, &payload.machine_name).unwrap()),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn get_project_metadata(
    State(state): State<AppState>,
    Path(machine_name): Path<String>,
) -> impl IntoResponse {
    let project = match storage::read_project(&state.projects_dir, &machine_name) {
        Ok(project) => project,
        Err(status) => return status.into_response(),
    };

    Json(project).into_response()
}

pub async fn put_project_metadata(
    State(state): State<AppState>,
    Path(machine_name): Path<String>,
    Json(project): Json<crate::routes::projects::models::Project>,
) -> impl IntoResponse {
    let current_project = match storage::read_project(&state.projects_dir, &machine_name) {
        Ok(p) => p,
        Err(status) => return status.into_response(),
    };

    let current_flows: std::collections::HashSet<&String> =
        current_project.flows.iter().map(|f| &f.name).collect();
    let new_flows: std::collections::HashSet<&String> =
        project.flows.iter().map(|f| &f.name).collect();
    let removed_flows: std::collections::HashSet<&String> =
        current_flows.difference(&new_flows).cloned().collect();

    let current_layouts: std::collections::HashSet<&String> =
        current_project.layouts.iter().map(|l| &l.name).collect();
    let new_layouts: std::collections::HashSet<&String> =
        project.layouts.iter().map(|l| &l.name).collect();
    let removed_layouts: std::collections::HashSet<&String> =
        current_layouts.difference(&new_layouts).cloned().collect();

    if !removed_flows.is_empty() || !removed_layouts.is_empty() {
        let pagedb = storage::load_page_db(&state.project_pagesdb_path(&machine_name));
        let mut blocking: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();

        for page in &pagedb.pages {
            if let Some(hocr_path) =
                storage::hocr_active_path(&state.projects_dir, &machine_name, &page.scan)
            {
                if let Ok(html) = std::fs::read_to_string(&hocr_path) {
                    if let Some(hocr_page) = crate::hocr_parser::parse(&html, ParserConfig::default()) {
                        let page_label = if page.name.is_empty() {
                            page.scan.clone()
                        } else {
                            page.name.clone()
                        };
                        for carea in &hocr_page.careas {
                            if let Some(ref flow) = carea.flow {
                                if removed_flows.contains(flow) {
                                    blocking
                                        .entry(flow.clone())
                                        .or_default()
                                        .push(page_label.clone());
                                }
                            }
                            if let Some(ref layout) = carea.layout {
                                if removed_layouts.contains(layout) {
                                    blocking
                                        .entry(layout.clone())
                                        .or_default()
                                        .push(page_label.clone());
                                }
                            }
                        }
                    }
                }
            }
        }

        if !blocking.is_empty() {
            for pages in blocking.values_mut() {
                pages.sort();
                pages.dedup();
            }
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": "Cannot remove items currently in use",
                    "blocking": blocking
                })),
            )
                .into_response();
        }
    }

    match storage::write_project(&state.projects_dir, &machine_name, &project) {
        Ok(_) => (StatusCode::OK, Json(project)).into_response(),
        Err(status) => status.into_response(),
    }
}

pub async fn post_append_images(
    State(state): State<AppState>,
    Path(machine_name): Path<String>,
    Query(query): Query<IngestQuery>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let pages_dir = state.projects_dir.join(&machine_name).join("pages");
    if !pages_dir.exists() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let scans_dir = pages_dir.join("scans");
    let thumbs_dir = pages_dir.join("thumbs");
    if std::fs::create_dir_all(&scans_dir).is_err() || std::fs::create_dir_all(&thumbs_dir).is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let pagedb_path = pages_dir.join("pagedata.json");
    let mut pagedb = storage::load_page_db(&pagedb_path);

    // Collect and validate all files before writing anything, so we can sort
    // by filename and assign contiguous order keys within the batch.
    let mut incoming: Vec<(String, axum::body::Bytes)> = Vec::new();
    while let Ok(Some(field)) = multipart.next_field().await {
        let filename = match field.file_name() {
            Some(name) if !name.is_empty() => name.to_string(),
            _ => continue,
        };
        let ext = std::path::Path::new(&filename)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase());
        if !matches!(
            ext.as_deref(),
            Some("jpg") | Some("jpeg") | Some("png") | Some("tif") | Some("tiff") | Some("webp")
        ) {
            continue;
        }
        let Ok(data) = field.bytes().await else {
            continue;
        };
        incoming.push((filename, data));
    }
    incoming.sort_by(|(a, _), (b, _)| a.cmp(b));

    let batch = pagedb.next_batch;
    pagedb.next_batch += 1;
    let base_import_order = pagedb
        .pages
        .iter()
        .map(|p| p.import_order)
        .max()
        .map_or(0, |max| max + IMPORT_ORDER_GAP);

    let mut new_pages: Vec<Page> = Vec::new();
    for (i, (filename, data)) in incoming.into_iter().enumerate() {
        let final_name = images::resolve_scan_filename(&scans_dir, &filename);
        if std::fs::write(scans_dir.join(&final_name), &data).is_err() {
            continue;
        }
        let stem = std::path::Path::new(&final_name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&final_name);
        let thumb_name = format!("{stem}.jpg");
        let (sw, sh, tw, th) =
            images::generate_thumb(&data, &thumbs_dir.join(&thumb_name)).unwrap_or((0, 0, 0, 0));
        let import_order = base_import_order + (i as u32) * IMPORT_ORDER_GAP;
        new_pages.push(Page {
            index: 0,
            name: String::new(),
            scan: final_name,
            scan_width: sw,
            scan_height: sh,
            thumb: thumb_name,
            thumb_width: tw,
            thumb_height: th,
            batch,
            import_order,
            ..Page::default()
        });
    }

    match (query.after, query.before) {
        (Some(_), Some(_)) => {
            return StatusCode::BAD_REQUEST.into_response();
        }
        (None, None) => {
            pagedb.pages.extend(new_pages);
        }
        (Some(after), None) => {
            let insert_pos = (after + 1).min(pagedb.pages.len());
            for (i, page) in new_pages.into_iter().enumerate() {
                pagedb.pages.insert(insert_pos + i, page);
            }
        }
        (None, Some(before)) => {
            let insert_pos = before.min(pagedb.pages.len());
            for (i, page) in new_pages.into_iter().enumerate() {
                pagedb.pages.insert(insert_pos + i, page);
            }
        }
    }

    storage::reindex(&mut pagedb);
    if storage::save_page_db(&pagedb_path, &pagedb).is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    get_project_pagesdb(State(state), Path(machine_name))
        .await
        .into_response()
}

pub async fn post_remove_images(
    State(state): State<AppState>,
    Path(machine_name): Path<String>,
    Json(payload): Json<RemoveRequest>,
) -> impl IntoResponse {
    let db_path = state
        .projects_dir
        .join(&machine_name)
        .join("pages")
        .join("pagedata.json");
    let mut db = storage::load_page_db(&db_path);
    let to_remove: std::collections::HashSet<usize> = payload.indices.into_iter().collect();
    db.pages.retain(|p| !to_remove.contains(&p.index));
    storage::reindex(&mut db);
    let _ = storage::save_page_db(&db_path, &db);
    get_project_pagesdb(State(state), Path(machine_name))
        .await
        .into_response()
}

pub async fn get_hocr_json(
    State(state): State<AppState>,
    Path((machine_name, stem)): Path<(String, String)>,
    Query(config): Query<ParserConfig>,
) -> impl IntoResponse {
    let Some(hocr_path) = storage::hocr_active_path(&state.projects_dir, &machine_name, &stem)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let html = match tokio::fs::read_to_string(&hocr_path).await {
        Ok(s) => s,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let projects_dir = state.projects_dir().clone();
    let machine_name_clone = machine_name.clone();
    let stem_clone = stem.clone();

    let page = tokio::task::spawn_blocking(move || {
        let mut p = crate::hocr_parser::parse(&html, config.clone())?;
        if config.block_metrics {
            if let Some(provider) =
                crate::hocr_parser::FileSystemPageProvider::new(projects_dir, &machine_name_clone)
            {
                p.calculate_all_metrics(&provider, &stem_clone);
            }
        }
        Some(p)
    })
    .await
    .unwrap_or(None);

    match page {
        Some(mut p) => {
            p.page_id = stem;
            Json(p).into_response()
        }
        None => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": "failed to parse hOCR"})),
        )
            .into_response(),
    }
}

pub async fn get_hocr_status(
    State(state): State<AppState>,
    Path(machine_name): Path<String>,
) -> impl IntoResponse {
    let pagedb = storage::load_page_db(&state.project_pagesdb_path(&machine_name));
    let scanned = storage::list_scanned(&state.projects_dir, &machine_name, &pagedb.pages);
    Json(serde_json::json!({ "scanned": scanned }))
}

async fn run_scan_job(
    state: AppState,
    machine_name: String,
    pages: Vec<Page>,
    language: String,
    processing_settings: Option<crate::routes::projects::models::ProcessingSettings>,
    server: crate::app_settings::OcrServerConfig,
) {
    let projects_dir = state.projects_dir();
    let total_pages = pages.len();

    let mut job_status = ScanJobStatus {
        total_pages,
        processed_pages: 0,
        status: JobState::Running,
        results: Vec::new(),
        error: None,
    };

    // Initial save
    let _ = storage::save_job_status(&projects_dir, &machine_name, &job_status);

    let scans_dir = projects_dir
        .join(&machine_name)
        .join("pages")
        .join("scans");

    for page in pages {
        // Read and process image
        let raw = match tokio::fs::read(scans_dir.join(&page.scan)).await {
            Ok(b) => b,
            Err(e) => {
                job_status.status = JobState::Failed;
                job_status.error = Some(format!("failed to read {}: {}", page.scan, e));
                let _ = storage::save_job_status(&projects_dir, &machine_name, &job_status);
                break;
            }
        };

        let crop = page.crop_edges;
        let hints = page.hints.clone();
        let scan_name = page.scan.clone();
        let settings = processing_settings.clone();

        let bytes_result = tokio::task::spawn_blocking(move || {
            crate::image_utils::apply_crop_mask(&raw, crop, settings.as_ref(), &hints)
        })
        .await;

        let bytes = match bytes_result {
            Ok(Ok(b)) => b,
            Ok(Err(e)) => {
                job_status.results.push(ScanPageResult {
                    scan: scan_name,
                    success: false,
                    error: Some(format!("failed to mask: {}", e)),
                });
                job_status.processed_pages += 1;
                let _ = storage::save_job_status(&projects_dir, &machine_name, &job_status);
                continue;
            }
            Err(_) => {
                job_status.results.push(ScanPageResult {
                    scan: scan_name,
                    success: false,
                    error: Some("image task panicked".to_string()),
                });
                job_status.processed_pages += 1;
                let _ = storage::save_job_status(&projects_dir, &machine_name, &job_status);
                continue;
            }
        };

        // Call OCR for THIS page
        let ocr_result = crate::ocr_client::call_ocr_service(
            &server,
            vec![(page.scan.clone(), bytes)],
            &language,
        )
        .await;

        match ocr_result {
            Ok(mut results) if !results.is_empty() => {
                let ocr = results.remove(0);
                if let Some(original_hocr) = ocr.hocr {
                    // Process hOCR (inject dropcaps/images)
                    let hints = page.hints.clone();
                    let hocr_to_save = if hints.iter().any(|h| {
                        matches!(h.hint_type, HintType::DropCap { .. })
                            || matches!(h.hint_type, HintType::Image)
                    }) {
                        let hocr_str = original_hocr.clone();
                        let processed = tokio::task::spawn_blocking(move || {
                            let mut hocr_page =
                                crate::hocr_parser::parse(&hocr_str, ParserConfig::default())?;
                            let mut dropcaps = Vec::new();
                            let mut images = Vec::new();
                            for h in hints {
                                match h.hint_type {
                                    HintType::DropCap { letter } => {
                                        dropcaps.push(crate::hocr_parser::DropCapInjection {
                                            text: letter,
                                            bbox: crate::hocr_parser::HocrBbox([
                                                h.area.left as i32,
                                                h.area.top as i32,
                                                h.area.right as i32,
                                                h.area.bottom as i32,
                                            ]),
                                        });
                                    }
                                    HintType::Image => {
                                        images.push(crate::hocr_parser::HocrBbox([
                                            h.area.left as i32,
                                            h.area.top as i32,
                                            h.area.right as i32,
                                            h.area.bottom as i32,
                                        ]));
                                    }
                                    _ => {}
                                }
                            }
                            if !dropcaps.is_empty() {
                                hocr_page.inject_dropcaps(dropcaps);
                            }
                            if !images.is_empty() {
                                hocr_page.inject_images(images);
                            }
                            Some(hocr_page.to_hocr_html())
                        })
                        .await;
                        match processed {
                            Ok(Some(html)) => html,
                            _ => original_hocr,
                        }
                    } else {
                        original_hocr
                    };

                    match storage::save_hocr_original(
                        &projects_dir,
                        &machine_name,
                        &page.scan,
                        &hocr_to_save,
                    ) {
                        Ok(()) => {
                            let edited_path = storage::hocr_edited_path(
                                &projects_dir,
                                &machine_name,
                                &page.scan,
                            );
                            let _ = tokio::fs::remove_file(edited_path).await;
                            job_status.results.push(ScanPageResult {
                                scan: page.scan.clone(),
                                success: true,
                                error: None,
                            });
                        }
                        Err(e) => {
                            job_status.results.push(ScanPageResult {
                                scan: page.scan.clone(),
                                success: false,
                                error: Some(e.to_string()),
                            });
                        }
                    }
                } else {
                    job_status.results.push(ScanPageResult {
                        scan: page.scan.clone(),
                        success: false,
                        error: ocr.error,
                    });
                }
            }
            Ok(_) => {
                job_status.results.push(ScanPageResult {
                    scan: page.scan.clone(),
                    success: false,
                    error: Some("No results from OCR service".to_string()),
                });
            }
            Err(e) => {
                job_status.results.push(ScanPageResult {
                    scan: page.scan.clone(),
                    success: false,
                    error: Some(e),
                });
            }
        }

        job_status.processed_pages += 1;
        let _ = storage::save_job_status(&projects_dir, &machine_name, &job_status);
    }

    if job_status.status == JobState::Running {
        job_status.status = JobState::Completed;
        let _ = storage::save_job_status(&projects_dir, &machine_name, &job_status);
    }

    // Clean up active job handle
    let mut active_jobs = state.active_jobs.write().unwrap();
    active_jobs.remove(&machine_name);
}

pub async fn scan_pages_post(
    State(state): State<AppState>,
    Path(machine_name): Path<String>,
    Json(payload): Json<ScanRequest>,
) -> impl IntoResponse {
    // Check if job is already running
    {
        let active_jobs = state.active_jobs.read().unwrap();
        if active_jobs.contains_key(&machine_name) {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({"error": "a scan job is already running for this project"})),
            )
                .into_response();
        }
    }

    // Load pagedb and resolve the requested pages by index
    let pagedb_path = state.project_pagesdb_path(&machine_name);
    let pagedb = storage::load_page_db(&pagedb_path);

    let pages: Vec<Page> = payload
        .indices
        .iter()
        .filter_map(|&idx| pagedb.pages.iter().find(|p| p.index == idx).cloned())
        .collect();

    if pages.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "no valid pages"})),
        )
            .into_response();
    }

    // Conflict check: pages with unsaved edits block a forced re-scan
    if !payload.force {
        let conflicts: Vec<String> = pages
            .iter()
            .filter(|p| storage::has_unsaved_edits(&state.projects_dir, &machine_name, &p.scan))
            .map(|p| {
                if p.name.is_empty() {
                    p.scan.clone()
                } else {
                    p.name.clone()
                }
            })
            .collect();

        if !conflicts.is_empty() {
            return (
                StatusCode::CONFLICT,
                Json(ScanConflict { pages: conflicts }),
            )
                .into_response();
        }
    }

    // Pick the first online OCR server (priority 1 → 2)
    let server = {
        let settings = state.settings.read().unwrap();
        let ocr_status = state.ocr_status.read().unwrap();
        if matches!(ocr_status.server_1, ServerStatus::Online) {
            settings.ocr_server_1.clone()
        } else if matches!(ocr_status.server_2, ServerStatus::Online) {
            settings.ocr_server_2.clone()
        } else {
            None
        }
    };

    let Some(server) = server else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "no OCR server available"})),
        )
            .into_response();
    };

    // Load project metadata to get processing settings
    let project = match storage::read_project(&state.projects_dir, &machine_name) {
        Ok(p) => p,
        Err(status) => return status.into_response(),
    };
    let processing_settings = project.processing.clone();

    let job_status = ScanJobStatus {
        total_pages: pages.len(),
        processed_pages: 0,
        status: JobState::Running,
        results: Vec::new(),
        error: None,
    };

    // Spawn job
    let handle = tokio::spawn(run_scan_job(
        state.clone(),
        machine_name.clone(),
        pages,
        payload.language,
        processing_settings,
        server,
    ));

    // Store handle
    {
        let mut active_jobs = state.active_jobs.write().unwrap();
        active_jobs.insert(machine_name, handle.abort_handle());
    }

    Json(job_status).into_response()
}

pub async fn scan_pages_get(
    State(state): State<AppState>,
    Path(machine_name): Path<String>,
) -> impl IntoResponse {
    let projects_dir = state.projects_dir();
    match storage::load_job_status(&projects_dir, &machine_name) {
        Some(mut status) => {
            if status.status == JobState::Running {
                let active_jobs = state.active_jobs.read().unwrap();
                if !active_jobs.contains_key(&machine_name) {
                    // Job is marked running in file but no handle in memory -> server restart
                    status.status = JobState::Failed;
                    status.error = Some("Job interrupted by server restart".to_string());
                    let _ = storage::save_job_status(&projects_dir, &machine_name, &status);
                }
            }
            Json(status).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

pub async fn scan_pages_delete(
    State(state): State<AppState>,
    Path(machine_name): Path<String>,
) -> impl IntoResponse {
    let projects_dir = state.projects_dir();

    // Abort the task if it's running
    {
        let mut active_jobs = state.active_jobs.write().unwrap();
        if let Some(handle) = active_jobs.remove(&machine_name) {
            handle.abort();
        }
    }

    // Update status file to "Cancelled"
    if let Some(mut status) = storage::load_job_status(&projects_dir, &machine_name) {
        if status.status == JobState::Running {
            status.status = JobState::Cancelled;
            let _ = storage::save_job_status(&projects_dir, &machine_name, &status);
        }
        Json(status).into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

pub async fn get_project_pagesdb(
    State(state): State<AppState>,
    Path(machine_name): Path<String>,
) -> impl IntoResponse {
    let pagedb_path = state.project_pagesdb_path(&machine_name);
    let pagedb = storage::load_page_db(&pagedb_path);
    Json(pagedb).into_response()
}

pub async fn put_project_pagesdb(
    State(state): State<AppState>,
    Path(machine_name): Path<String>,
    Json(pagedb): Json<PageDb>,
) -> impl IntoResponse {
    let pagedb_path = state.project_pagesdb_path(&machine_name);
    if !pagedb_path.exists() {
        return StatusCode::NOT_FOUND.into_response();
    }
    match storage::save_page_db(&pagedb_path, &pagedb) {
        Ok(_) => StatusCode::OK.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn save_all_pages(
    State(state): State<AppState>,
    Path(machine_name): Path<String>,
) -> impl IntoResponse {
    let pagedb_path = state.project_pagesdb_path(&machine_name);
    let pagedb = storage::load_page_db(&pagedb_path);

    let mut success_count = 0;
    let mut failure_count = 0;

    for page in &pagedb.pages {
        if let Some(active_path) = storage::hocr_active_path(&state.projects_dir, &machine_name, &page.scan) {
            match fs::read_to_string(&active_path).await {
                Ok(html) => {
                    if let Some(hocr_page) = crate::hocr_parser::parse(&html, ParserConfig::default()) {
                        let normalized_html = hocr_page.to_hocr_html();
                        match storage::save_hocr_edited(&state.projects_dir, &machine_name, &page.scan, &normalized_html) {
                            Ok(_) => success_count += 1,
                            Err(_) => failure_count += 1,
                        }
                    } else {
                        failure_count += 1;
                    }
                }
                Err(_) => failure_count += 1,
            }
        }
    }

    Json(serde_json::json!({
        "success": true,
        "success_count": success_count,
        "failure_count": failure_count,
    }))
}

pub async fn get_project_structure(
    State(state): State<AppState>,
    Path(machine_name): Path<String>,
) -> impl IntoResponse {
    let path = state.project_structuredb_path(&machine_name);
    let db = storage::load_structure_db(&path);
    Json(db).into_response()
}

pub async fn put_project_structure(
    State(state): State<AppState>,
    Path(machine_name): Path<String>,
    Json(db): Json<StructureDb>,
) -> impl IntoResponse {
    let path = state.project_structuredb_path(&machine_name);
    match storage::save_structure_db(&path, &db) {
        Ok(_) => StatusCode::OK.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn auto_layout(
    State(_state): State<AppState>,
    Path((machine_name, stem)): Path<(String, String)>,
    Json(payload): Json<AutoAssistRequest>,
) -> impl IntoResponse {
    if !payload.stems.is_empty() && payload.carea_ids.as_ref().map_or(false, |v| !v.is_empty()) {
        return (StatusCode::BAD_REQUEST, "Cannot specify both stems and carea_ids").into_response();
    }

    println!("Auto layout for project {}, page {}, selection: {:?}", machine_name, stem, payload.stems);
    StatusCode::OK.into_response()
}

pub async fn auto_flow(
    State(state): State<AppState>,
    Path((machine_name, stem)): Path<(String, String)>,
    Json(payload): Json<AutoAssistRequest>,
) -> impl IntoResponse {
    if !payload.stems.is_empty() && payload.carea_ids.as_ref().map_or(false, |v| !v.is_empty()) {
        return (StatusCode::BAD_REQUEST, "Cannot specify both stems and carea_ids").into_response();
    }

    let project = match storage::read_project(&state.projects_dir, &machine_name) {
        Ok(p) => p,
        Err(status) => return status.into_response(),
    };

    let flows = project.flows.clone();
    let layouts = project.layouts.clone();

    let (target_stems, carea_ids) = if let Some(ref ids) = payload.carea_ids {
        if !ids.is_empty() {
            (vec![stem], Some(ids.clone()))
        } else {
            (payload.stems.clone(), None)
        }
    } else {
        (payload.stems.clone(), None)
    };

    for target_stem in target_stems {
        let hocr_path = match storage::hocr_active_path(&state.projects_dir, &machine_name, &target_stem) {
            Some(path) => path,
            None => {
                println!("auto_flow: active HOCR path not found for stem {}", target_stem);
                continue;
            }
        };

        let html = match fs::read_to_string(&hocr_path).await {
            Ok(s) => s,
            Err(e) => {
                println!("auto_flow: failed to read HOCR for stem {}: {}", target_stem, e);
                continue;
            }
        };

        let flows = flows.clone();
        let layouts = layouts.clone();
        let carea_ids = carea_ids.clone();
        let result = tokio::task::spawn_blocking(move || {
            let mut page = crate::hocr_parser::parse(&html, ParserConfig::default())?;
            page.auto_flow(flows, layouts, true, carea_ids);
            Some(page.to_hocr_html())
        })
        .await;

        match result {
            Ok(Some(new_html)) => {
                if let Err(e) = storage::save_hocr_edited(&state.projects_dir, &machine_name, &target_stem, &new_html) {
                    println!("auto_flow: failed to save edited HOCR for stem {}: {}", target_stem, e);
                }
            }
            Ok(None) => {
                println!("auto_flow: failed to parse HOCR for stem {}", target_stem);
            }
            Err(e) => {
                println!("auto_flow: task panicked for stem {}: {}", target_stem, e);
            }
        }
    }

    StatusCode::OK.into_response()
}


pub async fn auto_bridge_page(
    State(state): State<AppState>,
    Path((machine_name, stem)): Path<(String, String)>,
    Json(payload): Json<AutoBridgeRequest>,
) -> impl IntoResponse {
    let projects_dir = state.projects_dir().clone();
    let result = tokio::task::spawn_blocking(move || {
        let provider =
            crate::hocr_parser::FileSystemPageProvider::new(projects_dir.clone(), &machine_name)?;
        let hocr_path = storage::hocr_active_path(&projects_dir, &machine_name, &stem)?;
        let html = std::fs::read_to_string(&hocr_path).ok()?;
        let mut page = crate::hocr_parser::parse(&html, ParserConfig::default())?;

        page.auto_bridge(&provider, &stem, &payload.thresholds, &payload.flow_set);

        let new_html = page.to_hocr_html();
        storage::save_hocr_edited(&projects_dir, &machine_name, &stem, &new_html).ok()?;
        Some(())
    })
    .await;

    match result {
        Ok(Some(())) => StatusCode::OK.into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub async fn auto_bridge_block(
    State(state): State<AppState>,
    Path((machine_name, stem, block_id)): Path<(String, String, String)>,
    Json(payload): Json<AutoBridgeRequest>,
) -> impl IntoResponse {
    let projects_dir = state.projects_dir().clone();
    let result = tokio::task::spawn_blocking(move || {
        let provider =
            crate::hocr_parser::FileSystemPageProvider::new(projects_dir.clone(), &machine_name)?;
        let hocr_path = storage::hocr_active_path(&projects_dir, &machine_name, &stem)?;
        let html = std::fs::read_to_string(&hocr_path).ok()?;
        let mut page = crate::hocr_parser::parse(&html, ParserConfig::default())?;

        let preceding = preceding_block(&provider, &stem, &block_id);
        let following = succeeding_block(&provider, &stem, &block_id);
        let self_path = page.get_block_path(&block_id);

        if let Some(block) = page
            .careas
            .iter_mut()
            .flat_map(|c| c.blocks.iter_mut())
            .find(|b| b.id == block_id)
        {
            let self_path = self_path?;
            let prec_ctx = preceding.as_ref().map(|(b, p, path)| (b, p.as_str(), *path));
            let foll_ctx = following.as_ref().map(|(b, p, path)| (b, p.as_str(), *path));

            block.update_metrics(prec_ctx, foll_ctx, &stem, self_path);
            block.auto_bridge(prec_ctx, foll_ctx, &stem, self_path, &payload.thresholds);

            let new_html = page.to_hocr_html();
            storage::save_hocr_edited(&projects_dir, &machine_name, &stem, &new_html).ok()?;
            Some(())
        } else {
            None
        }
    })
    .await;

    match result {
        Ok(Some(())) => StatusCode::OK.into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
