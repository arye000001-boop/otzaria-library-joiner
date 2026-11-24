#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use joiner_core as core;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{Emitter, Manager, State};

/// התיקייה שבה הקובץ רץ. זו התיקייה היחידה שהכלי כותב אליה.
fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

struct AppState {
    cancel: Arc<AtomicBool>,
    busy: Mutex<bool>,
    out_dir: PathBuf,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SetInfo {
    archive: String,
    dir: String,
    parts: Vec<String>,
    total: u64,
    has_manifest: bool,
    problem: Option<String>,
    exists_in_out: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanResult {
    dir: String,
    out_dir: String,
    free: Option<u64>,
    sets: Vec<SetInfo>,
}

fn scan_path(dir: &Path, out_dir: &Path) -> Result<ScanResult, String> {
    let sets = core::find_sets(dir).map_err(|e| e.to_string())?;
    let infos = sets
        .iter()
        .map(|s| SetInfo {
            archive: s.archive.clone(),
            dir: dir.to_string_lossy().to_string(),
            parts: s
                .parts
                .iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
                .collect(),
            total: s.total,
            has_manifest: s.manifest.is_some(),
            problem: core::precheck(s).err().map(|e| e.to_string()),
            exists_in_out: out_dir.join(&s.archive).exists(),
        })
        .collect();
    Ok(ScanResult {
        dir: dir.to_string_lossy().to_string(),
        out_dir: out_dir.to_string_lossy().to_string(),
        free: core::free_space(out_dir),
        sets: infos,
    })
}

#[tauri::command]
fn scan(state: State<AppState>) -> Result<ScanResult, String> {
    scan_path(&exe_dir(), &state.out_dir)
}

#[tauri::command]
fn scan_dir(path: String, state: State<AppState>) -> Result<ScanResult, String> {
    scan_path(Path::new(&path), &state.out_dir)
}

#[tauri::command]
async fn pick_folder(window: tauri::WebviewWindow) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || {
        rfd::FileDialog::new()
            .set_title("בחירת התיקייה שבה נמצאים חלקי הספרייה")
            .set_directory(exe_dir())
            .set_parent(&window)
            .pick_folder()
            .map(|p| p.to_string_lossy().to_string())
    })
    .await
    .ok()
    .flatten()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileInfo {
    path: String,
    name: String,
    size: u64,
    exists_in_out: bool,
    free: Option<u64>,
    out_dir: String,
}

#[tauri::command]
async fn pick_file(window: tauri::WebviewWindow, state: State<'_, AppState>) -> Result<Option<FileInfo>, String> {
    let out_dir = state.out_dir.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        rfd::FileDialog::new()
            .set_title("בחירת הקובץ לדחיסה")
            .set_directory(exe_dir())
            .set_parent(&window)
            .pick_file()
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(picked.map(|p| {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        FileInfo {
            size: std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0),
            exists_in_out: out_dir.join(format!("{name}.zst")).exists(),
            free: core::free_space(&out_dir),
            out_dir: out_dir.to_string_lossy().to_string(),
            path: p.to_string_lossy().to_string(),
            name,
        }
    }))
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ProgressEvt {
    phase: &'static str,
    done: u64,
    total: u64,
    part_index: usize,
    part_count: usize,
    speed: f64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct DoneEvt {
    kind: &'static str,
    output: String,
    name: String,
    size: u64,
    sha256: Option<String>,
    verified: bool,
    input_size: Option<u64>,
    seconds: f64,
    parts_deletable: bool,
}

#[derive(Serialize, Clone)]
struct FailEvt {
    cancelled: bool,
    message: String,
}

fn begin(state: &State<AppState>) -> Result<Arc<AtomicBool>, String> {
    let mut busy = state.busy.lock().unwrap();
    if *busy {
        return Err("פעולה אחרת כבר רצה".into());
    }
    *busy = true;
    state.cancel.store(false, Ordering::Relaxed);
    Ok(state.cancel.clone())
}

fn reporter(app: tauri::AppHandle, started: Instant) -> impl FnMut(core::Progress) {
    let mut last = Instant::now() - std::time::Duration::from_secs(1);
    move |p: core::Progress| {
        if last.elapsed().as_millis() < 100 && p.done < p.total {
            return;
        }
        last = Instant::now();
        let secs = started.elapsed().as_secs_f64().max(0.001);
        let _ = app.emit(
            "progress",
            ProgressEvt {
                phase: p.phase,
                done: p.done,
                total: p.total,
                part_index: p.part_index,
                part_count: p.part_count,
                speed: p.done as f64 / secs,
            },
        );
    }
}

#[tauri::command]
fn start_join(app: tauri::AppHandle, state: State<AppState>, dir: String, archive: String, overwrite: bool) -> Result<(), String> {
    let cancel = begin(&state)?;
    let out_dir = state.out_dir.clone();
    std::thread::spawn(move || {
        let started = Instant::now();
        let result = (|| {
            let sets = core::find_sets(Path::new(&dir))?;
            let set = sets
                .into_iter()
                .find(|s| s.archive == archive)
                .ok_or_else(|| core::Error::Msg("קבוצת החלקים לא נמצאה יותר בתיקייה".into()))?;
            let mut rep = reporter(app.clone(), started);
            core::join(&set, &out_dir, overwrite, &cancel, &mut rep)
        })();
        finish(&app, result.map(|r| DoneEvt {
            kind: "join",
            name: r.output.file_name().unwrap().to_string_lossy().to_string(),
            output: r.output.to_string_lossy().to_string(),
            size: r.size,
            sha256: Some(r.sha256),
            verified: r.verified,
            input_size: None,
            seconds: started.elapsed().as_secs_f64(),
            parts_deletable: r.verified,
        }));
    });
    Ok(())
}

#[tauri::command]
fn start_compress(app: tauri::AppHandle, state: State<AppState>, path: String, overwrite: bool) -> Result<(), String> {
    let cancel = begin(&state)?;
    let out_dir = state.out_dir.clone();
    std::thread::spawn(move || {
        let started = Instant::now();
        let workers = std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(1).clamp(1, 16);
        let mut rep = reporter(app.clone(), started);
        let result = core::compress(Path::new(&path), &out_dir, workers, overwrite, &cancel, &mut rep);
        finish(&app, result.map(|r| DoneEvt {
            kind: "compress",
            name: r.output.file_name().unwrap().to_string_lossy().to_string(),
            output: r.output.to_string_lossy().to_string(),
            size: r.output_size,
            sha256: None,
            verified: true,
            input_size: Some(r.input_size),
            seconds: started.elapsed().as_secs_f64(),
            parts_deletable: false,
        }));
    });
    Ok(())
}

fn finish(app: &tauri::AppHandle, result: Result<DoneEvt, core::Error>) {
    if let Some(state) = app.try_state::<AppState>() {
        *state.busy.lock().unwrap() = false;
    }
    match result {
        Ok(d) => {
            let _ = app.emit("finished", d);
        }
        Err(core::Error::Cancelled) => {
            let _ = app.emit("failed", FailEvt { cancelled: true, message: "הפעולה נעצרה. לא נשאר שום קובץ חלקי.".into() });
        }
        Err(e) => {
            let _ = app.emit("failed", FailEvt { cancelled: false, message: e.to_string() });
        }
    }
}

#[tauri::command]
fn cancel(state: State<AppState>) {
    state.cancel.store(true, Ordering::Relaxed);
}

/// מוחק את חלקי המקור (והמניפסט) אחרי הרכבה מאומתת, לבקשת המשתמש בלבד.
#[tauri::command]
fn delete_parts(dir: String, archive: String, state: State<AppState>) -> Result<u32, String> {
    if *state.busy.lock().unwrap() {
        return Err("פעולה רצה כרגע".into());
    }
    // לא מוחקים אלא אם הקובץ המורכב קיים ותואם בגודל למניפסט
    let sets = core::find_sets(Path::new(&dir)).map_err(|e| e.to_string())?;
    let set = sets.into_iter().find(|s| s.archive == archive).ok_or("לא נמצאו חלקים")?;
    let m = set.manifest.as_ref().ok_or("אין מניפסט — לא מוחקים בלי אימות")?;
    let joined = state.out_dir.join(&archive);
    let len = std::fs::metadata(&joined).map_err(|_| "הקובץ המורכב לא נמצא".to_string())?.len();
    if len != m.size {
        return Err("הקובץ המורכב אינו תואם למניפסט — לא מוחקים".into());
    }
    let mut n = 0;
    for p in set.parts.iter().chain(set.manifest_path.iter()) {
        if joined == *p {
            continue;
        }
        if std::fs::remove_file(p).is_ok() {
            n += 1;
        }
    }
    Ok(n)
}

#[tauri::command]
fn reveal(path: String) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("explorer")
            .raw_arg(format!("/select,\"{}\"", path))
            .spawn();
    }
    #[cfg(not(windows))]
    {
        let dir = Path::new(&path).parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
    }
}

#[tauri::command]
fn app_info(state: State<AppState>) -> serde_json::Value {
    serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "outDir": state.out_dir.to_string_lossy(),
        "cpus": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
    })
}

fn main() {
    let out_dir = exe_dir();
    // WebView2 שומר פרופיל. מפנים אותו לתיקייה סמוכה לקובץ כדי שלא ייכתב דבר ל-AppData
    let data_dir = out_dir.join(".otzaria-joiner-data");
    let cleanup = data_dir.clone();

    let app = tauri::Builder::default()
        .manage(AppState { cancel: Arc::new(AtomicBool::new(false)), busy: Mutex::new(false), out_dir })
        .invoke_handler(tauri::generate_handler![
            scan, scan_dir, pick_folder, pick_file, start_join, start_compress, cancel, delete_parts, reveal, app_info
        ])
        .setup(move |app| {
            let _ = std::fs::create_dir_all(&data_dir);
            #[cfg(windows)]
            {
                // מסתיר את התיקייה כדי שלא תפריע בתצוגה
                use std::os::windows::process::CommandExt;
                let _ = std::process::Command::new("attrib").arg("+h").arg(&data_dir).creation_flags(0x0800_0000).status();
            }
            tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::App("index.html".into()))
                .title("מרכיב הספרייה של אוצריא")
                .inner_size(400.0, 660.0)
                .resizable(false)
                .maximizable(false)
                .decorations(false)
                .center()
                .data_directory(data_dir.clone())
                .build()?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build app");

    app.run(move |_h, event| {
        if let tauri::RunEvent::Exit = event {
            let _ = std::fs::remove_dir_all(&cleanup);
        }
    });
}
