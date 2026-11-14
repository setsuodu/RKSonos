use axum::{
    extract::{Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;
use tracing::{error, info, warn};
use walkdir::WalkDir;

// ==================== Player (P1) ====================

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum PlayState {
    Idle,
    Playing,
    Paused,
    Stopped,
}

struct Player {
    child: Option<Child>,
    state: PlayState,
    current_path: Option<PathBuf>,
    volume: u8,
    audio_device: String,
}

impl Player {
    fn new(audio_device: String) -> Self {
        Self {
            child: None,
            state: PlayState::Idle,
            current_path: None,
            volume: 80,
            audio_device,
        }
    }

    fn stop_internal(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.state = PlayState::Stopped;
    }

    fn play(&mut self, path: PathBuf) -> Result<(), String> {
        if !path.exists() {
            return Err(format!("file not found: {}", path.display()));
        }
        self.stop_internal();
        let mut cmd = Command::new("mpg123");
        cmd.arg("-a")
            .arg(&self.audio_device)
            .arg("-q")
            .arg(&path)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        match cmd.spawn() {
            Ok(child) => {
                self.child = Some(child);
                self.current_path = Some(path);
                self.state = PlayState::Playing;
                Ok(())
            }
            Err(e) => {
                error!("failed to spawn mpg123: {}", e);
                Err(format!("spawn mpg123 failed: {}", e))
            }
        }
    }

    fn pause(&mut self) -> Result<(), String> {
        // MVP: stop as pause (true pause in later iteration)
        if self.state == PlayState::Playing {
            self.stop_internal();
            self.state = PlayState::Paused;
        }
        Ok(())
    }

    fn stop(&mut self) {
        self.stop_internal();
        self.current_path = None;
        self.state = PlayState::Stopped;
    }

    fn status(&mut self) -> StatusResponse {
        if let Some(child) = self.child.as_mut() {
            match child.try_wait() {
                Ok(Some(_)) => {
                    self.child = None;
                    if self.state == PlayState::Playing {
                        self.state = PlayState::Stopped;
                    }
                }
                Ok(None) => {}
                Err(_) => {
                    self.child = None;
                    self.state = PlayState::Stopped;
                }
            }
        }
        StatusResponse {
            state: self.state.clone(),
            path: self
                .current_path
                .as_ref()
                .map(|p| p.display().to_string()),
            volume: self.volume,
            audio_device: self.audio_device.clone(),
        }
    }
}

// ==================== Library (P2) ====================

const AUDIO_EXTS: &[&str] = &[
    "mp3", "flac", "m4a", "aac", "ogg", "wav", "ape", "wma", "aiff", "opus",
];

#[derive(Debug, Clone, Serialize)]
struct Track {
    id: u64,
    path: String,       // absolute path
    rel_path: String,   // relative to MUSIC_ROOT
    name: String,       // filename
    size: u64,
    mtime: u64,         // unix seconds
}

#[derive(Debug, Default)]
struct Library {
    root: PathBuf,
    tracks: Vec<Track>,
    scanning: bool,
    last_scan_ms: u64,
    last_count: usize,
    last_error: Option<String>,
}

impl Library {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            ..Default::default()
        }
    }

    fn scan(&mut self) -> Result<usize, String> {
        if self.scanning {
            return Err("scan already running".into());
        }
        if !self.root.exists() {
            return Err(format!("MUSIC_ROOT not found: {}", self.root.display()));
        }

        self.scanning = true;
        self.last_error = None;
        let start = now_ms();
        let mut tracks = Vec::new();
        let mut id: u64 = 1;

        for entry in WalkDir::new(&self.root)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let ext = path
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.to_lowercase())
                .unwrap_or_default();
            if !AUDIO_EXTS.contains(&ext.as_str()) {
                continue;
            }

            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);

            let abs = path.to_string_lossy().to_string();
            let rel = path
                .strip_prefix(&self.root)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| abs.clone());
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            tracks.push(Track {
                id,
                path: abs,
                rel_path: rel,
                name,
                size: meta.len(),
                mtime,
            });
            id += 1;
        }

        let count = tracks.len();
        self.tracks = tracks;
        self.scanning = false;
        self.last_scan_ms = now_ms() - start;
        self.last_count = count;
        info!("library scan done: {} tracks in {}ms", count, self.last_scan_ms);
        Ok(count)
    }

    fn search(&self, q: &str) -> Vec<&Track> {
        if q.is_empty() {
            return self.tracks.iter().collect();
        }
        let q = q.to_lowercase();
        self.tracks
            .iter()
            .filter(|t| {
                t.name.to_lowercase().contains(&q)
                    || t.rel_path.to_lowercase().contains(&q)
            })
            .collect()
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ==================== App State & API types ====================

#[derive(Clone)]
struct AppState {
    player: Arc<Mutex<Player>>,
    library: Arc<Mutex<Library>>,
}

#[derive(Serialize)]
struct StatusResponse {
    state: PlayState,
    path: Option<String>,
    volume: u8,
    audio_device: String,
}

#[derive(Deserialize)]
struct PlayRequest {
    path: String,
}

#[derive(Deserialize)]
struct VolumeRequest {
    volume: u8,
}

#[derive(Serialize)]
struct ApiError {
    error: String,
}

#[derive(Serialize)]
struct ScanResponse {
    status: String,
    count: usize,
    took_ms: u64,
}

#[derive(Serialize)]
struct LibraryStatus {
    root: String,
    scanning: bool,
    track_count: usize,
    last_scan_ms: u64,
    last_error: Option<String>,
}

#[derive(Deserialize)]
struct LibraryQuery {
    q: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
}

// ==================== Handlers ====================

async fn health() -> &'static str {
    "ok"
}

async fn status(State(state): State<AppState>) -> Json<StatusResponse> {
    let mut player = state.player.lock().await;
    Json(player.status())
}

async fn play(
    State(state): State<AppState>,
    Json(req): Json<PlayRequest>,
) -> Result<Json<StatusResponse>, (StatusCode, Json<ApiError>)> {
    let path = PathBuf::from(&req.path);
    let mut player = state.player.lock().await;
    match player.play(path) {
        Ok(()) => Ok(Json(player.status())),
        Err(e) => Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError { error: e }),
        )),
    }
}

async fn pause(
    State(state): State<AppState>,
) -> Result<Json<StatusResponse>, (StatusCode, Json<ApiError>)> {
    let mut player = state.player.lock().await;
    player.pause().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError { error: e }),
        )
    })?;
    Ok(Json(player.status()))
}

async fn stop(State(state): State<AppState>) -> Json<StatusResponse> {
    let mut player = state.player.lock().await;
    player.stop();
    Json(player.status())
}

async fn volume(
    State(state): State<AppState>,
    Json(req): Json<VolumeRequest>,
) -> Result<Json<StatusResponse>, (StatusCode, Json<ApiError>)> {
    if req.volume > 100 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "volume must be 0-100".into(),
            }),
        ));
    }
    let mut player = state.player.lock().await;
    player.volume = req.volume;
    Ok(Json(player.status()))
}

// ----- P2 library -----

async fn library_scan(
    State(state): State<AppState>,
) -> Result<Json<ScanResponse>, (StatusCode, Json<ApiError>)> {
    let mut lib = state.library.lock().await;
    match lib.scan() {
        Ok(count) => Ok(Json(ScanResponse {
            status: "ok".into(),
            count,
            took_ms: lib.last_scan_ms,
        })),
        Err(e) => {
            lib.last_error = Some(e.clone());
            Err((StatusCode::BAD_REQUEST, Json(ApiError { error: e })))
        }
    }
}

async fn library_status(State(state): State<AppState>) -> Json<LibraryStatus> {
    let lib = state.library.lock().await;
    Json(LibraryStatus {
        root: lib.root.display().to_string(),
        scanning: lib.scanning,
        track_count: lib.tracks.len(),
        last_scan_ms: lib.last_scan_ms,
        last_error: lib.last_error.clone(),
    })
}

async fn library_list(
    State(state): State<AppState>,
    Query(query): Query<LibraryQuery>,
) -> Json<Vec<Track>> {
    let lib = state.library.lock().await;
    let q = query.q.as_deref().unwrap_or("");
    let mut list: Vec<Track> = lib.search(q).into_iter().cloned().collect();
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(200).min(1000);
    if offset < list.len() {
        list = list.into_iter().skip(offset).take(limit).collect();
    } else {
        list.clear();
    }
    Json(list)
}

// ==================== main ====================

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let audio_device =
        std::env::var("AUDIO_DEVICE").unwrap_or_else(|_| "plughw:2,0".to_string());
    let bind = std::env::var("BIND").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    let music_root = std::env::var("MUSIC_ROOT").unwrap_or_else(|_| "/mnt/nas".to_string());

    info!("AUDIO_DEVICE={}", audio_device);
    info!("BIND={}", bind);
    info!("MUSIC_ROOT={}", music_root);

    let library = Library::new(PathBuf::from(&music_root));
    // optional: scan on startup (comment out if NAS slow)
    // let _ = library.scan();

    let state = AppState {
        player: Arc::new(Mutex::new(Player::new(audio_device))),
        library: Arc::new(Mutex::new(library)),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/v1/status", get(status))
        .route("/api/v1/play", post(play))
        .route("/api/v1/pause", post(pause))
        .route("/api/v1/stop", post(stop))
        .route("/api/v1/volume", post(volume))
        // P2
        .route("/api/v1/library/scan", post(library_scan))
        .route("/api/v1/library/status", get(library_status))
        .route("/api/v1/library", get(library_list))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&bind).await?;
    info!("listening on {}", bind);
    axum::serve(listener, app).await?;
    Ok(())
}
