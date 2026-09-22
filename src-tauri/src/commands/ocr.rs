use std::path::Path;
use std::sync::{Arc, Mutex};

use tokio::io::AsyncWriteExt;

use serde::Serialize;
use tauri::Emitter;

use crate::commands::lock::lock_state;
use crate::config::ConfigStore;
use crate::ocr::{self, OcrEngine, OcrWorkerManager, PpOcrEngine, TesseractOcrEngine};
use crate::storage::{Database, OcrRepository, StoragePaths};

pub fn configured_ppocr_model(config: &ConfigStore) -> &'static ocr::models::PpOcrModelSpec {
    ocr::models::model_spec(config.ppocr_model_variant()).unwrap_or_else(|| {
        crate::log_event!(
            "[ocr] unsupported configured PP-OCR model variant '{}', using small",
            config.ppocr_model_variant()
        );
        ocr::models::default_model_spec()
    })
}

pub fn ocr_config_response(config: &ConfigStore) -> OcrConfigResponse {
    OcrConfigResponse {
        engine: config.ocr_engine().to_string(),
        tesseract_languages: config.tesseract_languages().to_string(),
        ppocr_model_variant: configured_ppocr_model(config).id.to_owned(),
        det_score_threshold: config.det_score_threshold(),
        det_box_threshold: config.det_box_threshold(),
        det_unclip_ratio: config.det_unclip_ratio(),
    }
}

pub fn apply_ocr_runtime_settings(
    config: &Mutex<ConfigStore>,
    paths: &StoragePaths,
    worker: &OcrWorkerManager,
    update: OcrConfigUpdate,
) -> Result<OcrConfigResponse, String> {
    // Snapshot everything the probe and the write-back need, then release
    // the config lock BEFORE spawning any child process. The tesseract
    // probes (`is_available` + `with_languages` → `detect_tesseract_version`)
    // each allow up to PROBE_TIMEOUT (10s); holding the global config lock
    // across them would stall every config-dependent command. This mirrors
    // the release-before-probe pattern already used by `get_ocr_status` and
    // `check_ppocr_status`. A concurrent writer between the snapshot and the
    // write-back resolves as last-write-wins, matching single-lock semantics.
    struct ResolvedOcrSettings {
        engine: String,
        model: &'static ocr::models::PpOcrModelSpec,
        score_threshold: f32,
        box_threshold: f32,
        unclip_ratio: f32,
        tesseract_languages: String,
    }
    let resolved = {
        let cfg = lock_state(&config, "config lock poisoned")?;
        let engine = update.engine.unwrap_or_else(|| cfg.ocr_engine().to_owned());
        let model = match update.ppocr_model_variant {
            Some(variant) => ocr::models::model_spec(&variant)
                .ok_or_else(|| format!("unsupported PP-OCR model variant: {variant}"))?,
            None => configured_ppocr_model(&cfg),
        };
        ResolvedOcrSettings {
            engine,
            model,
            score_threshold: update
                .det_score_threshold
                .unwrap_or_else(|| cfg.det_score_threshold()),
            box_threshold: update
                .det_box_threshold
                .unwrap_or_else(|| cfg.det_box_threshold()),
            unclip_ratio: update
                .det_unclip_ratio
                .unwrap_or_else(|| cfg.det_unclip_ratio()),
            tesseract_languages: cfg.tesseract_languages().to_owned(),
        }
    };

    // Probe and construct the runtime engine with no locks held. `worker.restart`
    // joins the OCR thread, which may be busy with a long inference; that must
    // also stay outside the critical section (see previous comment).
    let runtime_engine: Arc<dyn OcrEngine> = match resolved.engine.as_str() {
        "ppocr" => {
            let ppocr = PpOcrEngine::new(
                ocr::models::models_dir(&paths.storage),
                resolved.model,
                resolved.score_threshold,
                resolved.box_threshold,
                resolved.unclip_ratio,
            );
            if !ppocr.is_available() {
                return Err(format!(
                    "PP-OCR {} model files are not installed",
                    resolved.model.id
                ));
            }
            Arc::new(ppocr)
        }
        "tesseract" if TesseractOcrEngine::is_available() => Arc::new(
            TesseractOcrEngine::with_languages(resolved.tesseract_languages),
        ),
        "tesseract" => return Err("Tesseract is not available".to_owned()),
        _ => return Err(format!("unsupported OCR engine: {}", resolved.engine)),
    };
    let database = Database::open(&paths.database).map_err(|e| e.to_string())?;

    let response = {
        let mut cfg = lock_state(&config, "config lock poisoned")?;
        cfg.set_ocr_settings(
            resolved.engine,
            resolved.model.id.to_owned(),
            resolved.score_threshold,
            resolved.box_threshold,
            resolved.unclip_ratio,
        )
        .map_err(|e| e.to_string())?;
        ocr_config_response(&cfg)
    };

    worker.restart(runtime_engine, Arc::new(database));

    Ok(response)
}

#[tauri::command]
pub fn get_ocr_status(
    database: tauri::State<'_, Database>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
    paths: tauri::State<'_, StoragePaths>,
) -> Result<OcrStatusInfo, String> {
    let pending = database.count_pending_ocr().map_err(|e| e.to_string())?;
    let completed = database.count_completed_ocr().map_err(|e| e.to_string())?;
    let failed = database.count_failed_ocr().map_err(|e| e.to_string())?;
    // Read everything that needs the config, then release the lock before
    // probing engines: `TesseractOcrEngine::is_available` spawns a process,
    // which must not hold the global config lock.
    let (engine, model, installed_variants, ppocr_available) = {
        let cfg = lock_state(&config, "config lock poisoned")?;
        let engine = cfg.ocr_engine().to_string();
        let models_dir = ocr::models::models_dir(&paths.storage);
        let model = configured_ppocr_model(&cfg);
        let installed_variants = ocr::models::installed_model_variants(&models_dir);
        let ppocr_available = ocr::models::model_is_installed(&models_dir, model);
        (engine, model, installed_variants, ppocr_available)
    };
    let tesseract_available = TesseractOcrEngine::is_available();
    let engine_available = match engine.as_str() {
        "ppocr" => ppocr_available,
        "tesseract" => tesseract_available,
        _ => false,
    };

    Ok(OcrStatusInfo {
        total_tasks: pending.saturating_add(completed).saturating_add(failed),
        pending_tasks: pending,
        completed_tasks: completed,
        failed_tasks: failed,
        tesseract_available,
        ppocr_available,
        has_engine: tesseract_available || !installed_variants.is_empty(),
        engine_available,
        engine,
        ppocr_model_variant: model.id.to_owned(),
        installed_variants,
    })
}

#[tauri::command]
pub fn get_ocr_config(
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<OcrConfigResponse, String> {
    let config = lock_state(&config, "config lock poisoned")?;
    Ok(ocr_config_response(&config))
}

#[tauri::command]
pub fn set_ocr_config(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    paths: tauri::State<'_, StoragePaths>,
    worker: tauri::State<'_, OcrWorkerManager>,
    settings: OcrConfigUpdate,
) -> Result<OcrConfigResponse, String> {
    if settings.engine.is_none() {
        return Err("OCR engine is required".to_owned());
    }
    apply_ocr_runtime_settings(&config, &paths, &worker, settings)
}

#[tauri::command]
pub fn restart_ocr_engine(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    paths: tauri::State<'_, StoragePaths>,
    worker: tauri::State<'_, OcrWorkerManager>,
) -> Result<(), String> {
    apply_ocr_runtime_settings(&config, &paths, &worker, OcrConfigUpdate::default())?;
    Ok(())
}

#[tauri::command]
pub async fn install_ppocr(
    app: tauri::AppHandle,
    paths: tauri::State<'_, StoragePaths>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
    worker: tauri::State<'_, OcrWorkerManager>,
    variant: String,
) -> Result<String, String> {
    let model = ocr::models::model_spec(&variant)
        .ok_or_else(|| format!("unsupported PP-OCR model variant: {variant}"))?;

    // Model download is an explicit, user-initiated network fetch — the one
    // OCR exception to "fully offline". It must still respect the stricter
    // local-only privacy switch (same policy as update checks).
    {
        let config = lock_state(&config, "config lock poisoned")?;
        if config.privacy_local_only() {
            return Err(
                "本地模式已开启：下载 OCR 模型需要访问网络，请先在隐私设置中关闭“仅本地模式”"
                    .to_owned(),
            );
        }
    }

    let models_dir = ocr::models::models_dir(&paths.storage);
    tokio::fs::create_dir_all(&models_dir)
        .await
        .map_err(|e| e.to_string())?;

    let client = reqwest::Client::builder()
        .user_agent("clipboard-desktop")
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| format!("create client: {e}"))?;

    for model_file in model.files() {
        download_ppocr_file(&app, &client, &models_dir, model_file).await?;
    }

    apply_ocr_runtime_settings(
        &config,
        &paths,
        &worker,
        OcrConfigUpdate {
            engine: Some("ppocr".to_owned()),
            ppocr_model_variant: Some(model.id.to_owned()),
            ..Default::default()
        },
    )?;

    Ok(format!(
        "PP-OCRv6 {} model installed and activated",
        model.id
    ))
}

#[tauri::command]
pub fn check_ppocr_status(
    paths: tauri::State<'_, StoragePaths>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<PpOcrStatus, String> {
    let models_dir = ocr::models::models_dir(&paths.storage);
    // `configured_ppocr_model` returns a 'static spec, so the guard can be
    // released before the (spawning) availability probe.
    let active_model = {
        let config = lock_state(&config, "config lock poisoned")?;
        configured_ppocr_model(&config)
    };
    Ok(PpOcrStatus {
        available: ocr::models::model_is_installed(&models_dir, active_model),
        tesseract_available: TesseractOcrEngine::is_available(),
        active_variant: active_model.id.to_owned(),
        installed_variants: ocr::models::installed_model_variants(&models_dir),
    })
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PpOcrDownloadProgress {
    filename: String,
    label: String,
    current: u64,
    total: u64,
    percentage: f64,
}

async fn download_ppocr_file(
    app: &tauri::AppHandle,
    client: &reqwest::Client,
    models_dir: &Path,
    model_file: ocr::models::PpOcrModelFile,
) -> Result<(), String> {
    if ocr::models::model_file_is_installed(models_dir, &model_file) {
        // Trust-on-first-use integrity: when we recorded a digest at install
        // time, re-verify the on-disk file before trusting it. A mismatch
        // (corruption or replacement) forces a fresh download.
        if ocr::models::model_digest_matches(models_dir, &model_file) {
            return Ok(());
        }
        crate::log_event!(
            "[ocr] {} no longer matches its recorded digest; redownloading",
            model_file.filename
        );
        let _ = tokio::fs::remove_file(models_dir.join(model_file.filename)).await;
    }

    let destination = models_dir.join(model_file.filename);
    let temporary = models_dir.join(format!("{}.part", model_file.filename));
    if tokio::fs::try_exists(&temporary).await.unwrap_or(false) {
        tokio::fs::remove_file(&temporary)
            .await
            .map_err(|e| format!("remove stale {}: {e}", temporary.display()))?;
    }

    if let Err(error) = app.emit(
        "ppocr-download-progress",
        PpOcrDownloadProgress {
            filename: model_file.filename.to_owned(),
            label: model_file.label.to_owned(),
            current: 0,
            total: model_file.size_bytes,
            percentage: 0.0,
        },
    ) {
        crate::log_event!("[ocr] failed to emit download progress: {error}");
    }

    let mut response = client
        .get(model_file.url)
        .send()
        .await
        .map_err(|e| format!("download {}: {e}", model_file.filename))?
        .error_for_status()
        .map_err(|e| format!("download {}: {e}", model_file.filename))?;
    let total = response.content_length().unwrap_or(model_file.size_bytes);
    let mut file = tokio::fs::File::create(&temporary)
        .await
        .map_err(|e| e.to_string())?;
    let mut downloaded = 0u64;

    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("download {}: {e}", model_file.filename))?
    {
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
        downloaded += chunk.len() as u64;
        let percentage = if total > 0 {
            (downloaded as f64 / total as f64) * 100.0
        } else {
            -1.0
        };
        if let Err(error) = app.emit(
            "ppocr-download-progress",
            PpOcrDownloadProgress {
                filename: model_file.filename.to_owned(),
                label: model_file.label.to_owned(),
                current: downloaded,
                total,
                percentage,
            },
        ) {
            crate::log_event!("[ocr] failed to emit download progress: {error}");
        }
    }

    file.flush().await.map_err(|e| e.to_string())?;
    file.sync_all().await.map_err(|e| e.to_string())?;
    drop(file);

    if downloaded != model_file.size_bytes {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(format!(
            "downloaded {} has unexpected size: expected {}, got {}",
            model_file.filename, model_file.size_bytes, downloaded
        ));
    }
    if tokio::fs::try_exists(&destination).await.unwrap_or(false) {
        tokio::fs::remove_file(&destination)
            .await
            .map_err(|e| format!("replace {}: {e}", destination.display()))?;
    }
    tokio::fs::rename(&temporary, &destination)
        .await
        .map_err(|e| format!("install {}: {e}", model_file.filename))?;

    // Pin the downloaded bytes trust-on-first-use so future installs can
    // detect on-disk corruption without maintaining upstream hash pins.
    let models_dir_owned = models_dir.to_path_buf();
    let digest_result = tauri::async_runtime::spawn_blocking(move || {
        ocr::models::record_model_digest(&models_dir_owned, &model_file)
    })
    .await
    .map_err(|e| format!("record {} digest: {e}", model_file.filename))?;
    digest_result.map_err(|e| format!("record {} digest: {e}", model_file.filename))?;

    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrStatusInfo {
    total_tasks: u64,
    pending_tasks: u64,
    completed_tasks: u64,
    failed_tasks: u64,
    tesseract_available: bool,
    ppocr_available: bool,
    has_engine: bool,
    engine_available: bool,
    engine: String,
    ppocr_model_variant: String,
    installed_variants: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrConfigResponse {
    engine: String,
    tesseract_languages: String,
    det_score_threshold: f32,
    det_box_threshold: f32,
    det_unclip_ratio: f32,
    ppocr_model_variant: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PpOcrStatus {
    available: bool,
    tesseract_available: bool,
    active_variant: String,
    installed_variants: Vec<&'static str>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OcrConfigUpdate {
    engine: Option<String>,
    ppocr_model_variant: Option<String>,
    det_score_threshold: Option<f32>,
    det_box_threshold: Option<f32>,
    det_unclip_ratio: Option<f32>,
}
