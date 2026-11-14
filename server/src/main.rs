use axum::{
    extract::{Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::{
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;
use tracing::{error, info, warn};
use walkdir::WalkDir;

// ==================== Player ====================

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
    stdin: Option<ChildStdin>,
    state: PlayState,
    current_path: Option<PathBuf>,
    volume: u8,
    audio_device: String,
    // queue
    queue: Vec<PathBuf>,
    queue_index: Option<usize>,
}

impl Player {
    fn new(audio_device: String) -> Self {
        Self {
            child: None,
            stdin: None,
            state: PlayState::Idle,
            current_path: None,
            volume: 80,
            audio_device,
            queue: Vec::new(),
            queue_index: None,
        }
    }

    fn stop_internal(&mut self) {
        // try graceful quit first
        if let Some(stdin) = self.stdin.as_mut() {
            let _ = writeln!(stdin, "QUIT");
            let _ = stdin.flush();
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.stdin = None;
        self.state = PlayState::Stopped;
    }

    /// Start mpg123 in remote control mode (-R)
    fn spawn_remote(&mut self, path: &Path) -> Result<(), String> {
        self.stop_internal();

        let mut cmd = Command::new("mpg123");
        cmd.arg("-a")
            .arg(&self.audio_device)
            .arg("-R") // remote control mode (stdin commands)
            .arg("--no-control") // we drive it
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .stdin(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| format!("spawn mpg123 failed: {}", e))?;

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "failed to take stdin".to_string())?;

        // LOAD the file
        writeln!(stdin, "LOAD {}", path.display())
            .map_err(|e| format!("write LOAD failed: {}", e))?;
        stdin.flush().ok();

        // set volume (0-100 → mpg123 scale roughly)
        let vol = self.volume.min(100);
        let _ = writeln!(stdin, "VOLUME {}", vol);
        let _ = stdin.flush();

        self.child = Some(child);
        self.stdin = Some(stdin);
        self.current_path = Some(path.to_path_buf());
        self.state = PlayState::Playing;
        Ok(())
    }

    fn play_path(&mut self, path: PathBuf) -> Result<(), String> {
        if !path.exists() {
            return Err(format!("file not found: {}", path.display()));
        }
        self.spawn_remote(&path)
    }

    fn pause(&mut self) -> Result<(), String> {
        if self.state != PlayState::Playing {
            return Ok(());
        }
        if let Some(stdin) = self.stdin.as_mut() {
            writeln!(stdin, "PAUSE").map_err(|e| e.to_string())?;
            stdin.flush().ok();
            self.state = PlayState::Paused;
            Ok(())
        } else {
            Err("no active player".into())
        }
    }

    fn resume(&mut self) -> Result<(), String> {
        if self.state != PlayState::Paused {
            // if stopped/idle but we have a path, just replay
            if let Some(path) = self.current_path.clone() {
                return self.play_path(path);
            }
            return Ok(());
        }
        if let Some(stdin) = self.stdin.as_mut() {
            // PAUSE toggles in mpg123 remote mode
            writeln!(stdin, "PAUSE").map_err(|e| e.to_string())?;
            stdin.flush().ok();
            self.state = PlayState::Playing;
            Ok(())
        } else {
            Err("no active player".into())
        }
    }

    fn stop(&mut self) {
        self.stop_internal();
        self.current_path = None;
        self.state = PlayState::Stopped;
    }

    // ----- queue -----

    fn set_queue(&mut self, paths: Vec<PathBuf>, start_index: usize) {
        self.queue = paths;
        self.queue_index = if self.queue.is_empty() {
            None
        } else {
            Some(start_index.min(self.queue.len() - 1))
        };
    }

    fn play_index(&mut self, idx: usize) -> Result<(), String> {
        if idx >= self.queue.len() {
            return Err("index out of range".into());
        }
        let path = self.queue[idx].clone();
        self.queue_index = Some(idx);
        self.play_path(path)
    }

    fn next(&mut self) -> Result<(), String> {
        let idx = match self.queue_index {
            Some(i) if i + 1 < self.queue.len() => i + 1,
            _ => return Err("no next track".into()),
        };
        self.play_index(idx)
    }

    fn prev(&mut self) -> Result<(), String> {
        let idx = match self.queue_index {
            Some(i) if i > 0 => i - 1,
            _ => return Err("no previous track".into()),
        };
        self.play_index(idx)
    }

    fn status(&mut self) -> StatusResponse {
        // reap exited process
        if let Some(child) = self.child.as_mut() {
            match child.try_wait() {
                Ok(Some(_)) => {
                    self.child = None;
                    self.stdin = None;
                    if self.state == PlayState::Playing {
                        self.state = PlayState::Stopped;
                    }
                }
                Ok(None) => {}
                Err(_) => {
                    self.child = None;
                    self.stdin = None;
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
            queue_len: self.queue.len(),
            queue_index: self.queue_index,
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
    path: String,
    rel_path: String,
    name: String,
    size: u64,
    mtime: u64,
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
                t.name.to_lowercase().contains(&q) || t.rel_path.to_lowercase().contains(&q)
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

// ==================== App State & Types ====================

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
    queue_len: usize,
    queue_index: Option<usize>,
}

#[derive(Deserialize)]
struct PlayRequest {
    path: String,
}

#[derive(Deserialize)]
struct VolumeRequest {
    volume: u8,
}

#[derive(Deserialize)]
struct QueueRequest {
    /// absolute paths
    paths: Vec<String>,
    /// start playing from this index (default 0)
    #[serde(default)]
    start: usize,
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
    // single play also becomes the queue of one
    player.set_queue(vec![path.clone()], 0);
    match player.play_path(path) {
        Ok(()) => Ok(Json(player.status())),
        Err(e) => Err((StatusCode::BAD_REQUEST, Json(ApiError { error: e }))),
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

async fn resume(
    State(state): State<AppState>,
) -> Result<Json<StatusResponse>, (StatusCode, Json<ApiError>)> {
    let mut player = state.player.lock().await;
    player.resume().map_err(|e| {
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
    // try live volume change
    if let Some(stdin) = player.stdin.as_mut() {
        let _ = writeln!(stdin, "VOLUME {}", req.volume);
        let _ = stdin.flush();
    }
    Ok(Json(player.status()))
}

async fn next(
    State(state): State<AppState>,
) -> Result<Json<StatusResponse>, (StatusCode, Json<ApiError>)> {
    let mut player = state.player.lock().await;
    player.next().map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError { error: e }),
        )
    })?;
    Ok(Json(player.status()))
}

async fn prev(
    State(state): State<AppState>,
) -> Result<Json<StatusResponse>, (StatusCode, Json<ApiError>)> {
    let mut player = state.player.lock().await;
    player.prev().map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError { error: e }),
        )
    })?;
    Ok(Json(player.status()))
}

async fn queue_set(
    State(state): State<AppState>,
    Json(req): Json<QueueRequest>,
) -> Result<Json<StatusResponse>, (StatusCode, Json<ApiError>)> {
    if req.paths.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "paths empty".into(),
            }),
        ));
    }
    let paths: Vec<PathBuf> = req.paths.into_iter().map(PathBuf::from).collect();
    let mut player = state.player.lock().await;
    player.set_queue(paths, req.start);
    let idx = player.queue_index.unwrap_or(0);
    player.play_index(idx).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError { error: e }),
        )
    })?;
    Ok(Json(player.status()))
}

// ----- library -----

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

    let state = AppState {
        player: Arc::new(Mutex::new(Player::new(audio_device))),
        library: Arc::new(Mutex::new(Library::new(PathBuf::from(music_root)))),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/v1/status", get(status))
        .route("/api/v1/play", post(play))
        .route("/api/v1/pause", post(pause))
        .route("/api/v1/resume", post(resume))
        .route("/api/v1/stop", post(stop))
        .route("/api/v1/volume", post(volume))
        .route("/api/v1/next", post(next))
        .route("/api/v1/prev", post(prev))
        .route("/api/v1/queue", post(queue_set))
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
