use axum::{
    extract::{Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;
use tracing::{info, warn};
use walkdir::WalkDir;

mod mdns;

// ==================== Types ====================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum PlayState {
    Idle,
    Playing,
    Paused,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum PlayMode {
    Normal,     // play queue once, stop at end
    RepeatOne,  // loop current track
    RepeatAll,  // loop entire queue
    Shuffle,    // random next
}

impl Default for PlayMode {
    fn default() -> Self {
        PlayMode::Normal
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Backend {
    Mpg123,
    Mpv,
}

fn which(bin: &str) -> bool {
    Command::new("which")
        .arg(bin)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn pick_backend(path: &Path, has_mpv: bool) -> Result<Backend, String> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "mp3" => Ok(Backend::Mpg123),
        "flac" | "m4a" | "aac" | "ogg" | "wav" | "opus" | "ape" | "wma" | "aiff" => {
            if has_mpv {
                Ok(Backend::Mpv)
            } else {
                Err(format!(
                    "no player for .{ext}: install mpv (`apt install -y mpv`) then restart server"
                ))
            }
        }
        _ => Err(format!("unsupported format: .{ext}")),
    }
}


// ==================== Player ====================

struct Player {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    state: PlayState,
    current_path: Option<PathBuf>,
    volume: u8,
    audio_device: String,
    queue: Vec<PathBuf>,
    queue_index: Option<usize>,
    mode: PlayMode,
    state_file: PathBuf,
    has_mpv: bool,
    backend: Option<Backend>,
    mpv_ipc: PathBuf,
}

impl Player {
    fn new(audio_device: String, state_file: PathBuf) -> Self {
        let has_mpv = which("mpv");
        if has_mpv {
            info!("player backend: mpg123 (mp3) + mpv (flac/others)");
        } else {
            warn!("mpv not found — only mp3 playable; apt install -y mpv for flac");
        }
        let mut p = Self {
            child: None,
            stdin: None,
            state: PlayState::Idle,
            current_path: None,
            volume: 80,
            audio_device,
            queue: Vec::new(),
            queue_index: None,
            mode: PlayMode::Normal,
            state_file,
            has_mpv,
            backend: None,
            mpv_ipc: PathBuf::from("/tmp/rksonos-mpv.sock"),
        };
        p.load_state();
        p
    }

    fn stop_internal(&mut self) {
        if self.backend == Some(Backend::Mpv) {
            let _ = fs::remove_file(&self.mpv_ipc);
        }
        if let Some(stdin) = self.stdin.as_mut() {
            let _ = writeln!(stdin, "QUIT");
            let _ = stdin.flush();
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.stdin = None;
        self.backend = None;
        self.state = PlayState::Stopped;
    }

    fn spawn_remote(&mut self, path: &Path) -> Result<(), String> {
        if !path.exists() {
            return Err(format!("file not found: {}", path.display()));
        }
        let backend = pick_backend(path, self.has_mpv)?;
        self.stop_internal();

        match backend {
            Backend::Mpg123 => {
                let mut cmd = Command::new("mpg123");
                cmd.arg("-a")
                    .arg(&self.audio_device)
                    .arg("-R")
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .stdin(Stdio::piped());
                let mut child = cmd
                    .spawn()
                    .map_err(|e| format!("spawn mpg123 failed: {}", e))?;
                let mut stdin = child
                    .stdin
                    .take()
                    .ok_or_else(|| "failed to take stdin".to_string())?;
                writeln!(stdin, "LOAD {}", path.display())
                    .map_err(|e| format!("LOAD failed: {}", e))?;
                let _ = writeln!(stdin, "VOLUME {}", self.volume.min(100));
                let _ = stdin.flush();
                self.child = Some(child);
                self.stdin = Some(stdin);
            }
            Backend::Mpv => {
                let _ = fs::remove_file(&self.mpv_ipc);
                // mpv: ALSA device as alsa/plughw:x,y
                let alsa_dev = if self.audio_device.starts_with("alsa/") {
                    self.audio_device.clone()
                } else {
                    format!("alsa/{}", self.audio_device)
                };
                let vol = self.volume.min(100);
                let mut cmd = Command::new("mpv");
                cmd.arg("--no-video")
                    .arg("--really-quiet")
                    .arg(format!("--audio-device={}", alsa_dev))
                    .arg(format!("--volume={}", vol))
                    .arg(format!("--input-ipc-server={}", self.mpv_ipc.display()))
                    .arg(path)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
                let child = cmd
                    .spawn()
                    .map_err(|e| format!("spawn mpv failed: {} (apt install -y mpv?)", e))?;
                // wait for ipc socket
                for _ in 0..20 {
                    if self.mpv_ipc.exists() {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                self.child = Some(child);
                self.stdin = None;
            }
        }

        self.backend = Some(backend);
        self.current_path = Some(path.to_path_buf());
        self.state = PlayState::Playing;
        self.save_state();
        Ok(())
    }

    fn play_path(&mut self, path: PathBuf) -> Result<(), String> {
        self.spawn_remote(&path)
    }

    fn pause(&mut self) -> Result<(), String> {
        if self.state != PlayState::Playing {
            return Ok(());
        }
        match self.backend {
            Some(Backend::Mpg123) => {
                if let Some(stdin) = self.stdin.as_mut() {
                    writeln!(stdin, "PAUSE").map_err(|e| e.to_string())?;
                    let _ = stdin.flush();
                } else {
                    return Err("no active player".into());
                }
            }
            Some(Backend::Mpv) => {
                self.mpv_cmd(&["cycle", "pause"])?;
            }
            None => return Err("no active player".into()),
        }
        self.state = PlayState::Paused;
        self.save_state();
        Ok(())
    }

    fn mpv_cmd(&self, args: &[&str]) -> Result<(), String> {
        // echo '{ "command": ["cycle","pause"] }' | socat - /tmp/rksonos-mpv.sock
        // avoid socat dependency: use mpv --input-ipc-client if available, else bash /dev/tcp style via script
        let cmd_json = serde_json::json!({ "command": args });
        let payload = cmd_json.to_string();
        // prefer socat, fallback to python
        if which("socat") {
            let mut child = Command::new("socat")
                .arg("-")
                .arg(format!("UNIX-CONNECT:{}", self.mpv_ipc.display()))
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("socat failed: {}", e))?;
            if let Some(mut sin) = child.stdin.take() {
                let _ = writeln!(sin, "{}", payload);
            }
            let _ = child.wait();
            return Ok(());
        }
        // printf + nc -U
        if which("nc") {
            let mut child = Command::new("nc")
                .arg("-U")
                .arg(&self.mpv_ipc)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("nc failed: {}", e))?;
            if let Some(mut sin) = child.stdin.take() {
                let _ = writeln!(sin, "{}", payload);
            }
            let _ = child.wait();
            return Ok(());
        }
        Err("need socat or nc to control mpv pause".into())
    }

    fn resume(&mut self) -> Result<(), String> {
        if self.state == PlayState::Paused {
            match self.backend {
                Some(Backend::Mpg123) => {
                    if let Some(stdin) = self.stdin.as_mut() {
                        writeln!(stdin, "PAUSE").map_err(|e| e.to_string())?;
                        let _ = stdin.flush();
                        self.state = PlayState::Playing;
                        self.save_state();
                        return Ok(());
                    }
                }
                Some(Backend::Mpv) => {
                    self.mpv_cmd(&["cycle", "pause"])?;
                    self.state = PlayState::Playing;
                    self.save_state();
                    return Ok(());
                }
                None => {}
            }
        }
        if let Some(path) = self.current_path.clone() {
            return self.play_path(path);
        }
        Ok(())
    }

    fn stop(&mut self) {
        self.stop_internal();
        self.current_path = None;
        self.state = PlayState::Stopped;
        self.save_state();
    }

    fn set_volume(&mut self, v: u8) {
        self.volume = v.min(100);
        match self.backend {
            Some(Backend::Mpg123) => {
                if let Some(stdin) = self.stdin.as_mut() {
                    let _ = writeln!(stdin, "VOLUME {}", self.volume);
                    let _ = stdin.flush();
                }
            }
            Some(Backend::Mpv) => {
                // volume applied on next spawn; amixer below still helps
            }
            None => {}
        }
        let _ = Command::new("amixer")
            .args(["-q", "sset", "Master", &format!("{}%", self.volume)])
            .status();
        self.save_state();
    }

    // ----- queue -----

    fn set_queue(&mut self, paths: Vec<PathBuf>, start_index: usize) {
        self.queue = paths;
        self.queue_index = if self.queue.is_empty() {
            None
        } else {
            Some(start_index.min(self.queue.len().saturating_sub(1)))
        };
        self.save_state();
    }

    fn queue_add(&mut self, paths: Vec<PathBuf>) {
        self.queue.extend(paths);
        if self.queue_index.is_none() && !self.queue.is_empty() {
            self.queue_index = Some(0);
        }
        self.save_state();
    }

    fn queue_remove(&mut self, index: usize) -> Result<(), String> {
        if index >= self.queue.len() {
            return Err("index out of range".into());
        }
        self.queue.remove(index);
        if let Some(qi) = self.queue_index {
            if self.queue.is_empty() {
                self.queue_index = None;
            } else if qi >= self.queue.len() {
                self.queue_index = Some(self.queue.len() - 1);
            } else if index < qi {
                self.queue_index = Some(qi - 1);
            }
        }
        self.save_state();
        Ok(())
    }

    fn queue_clear(&mut self) {
        self.queue.clear();
        self.queue_index = None;
        self.save_state();
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
        if self.queue.is_empty() {
            return Err("queue empty".into());
        }
        let next_idx = match self.mode {
            PlayMode::RepeatOne => self.queue_index.unwrap_or(0),
            PlayMode::Shuffle => {
                let mut rng = rand::thread_rng();
                let indices: Vec<usize> = (0..self.queue.len()).collect();
                *indices.choose(&mut rng).unwrap_or(&0)
            }
            PlayMode::RepeatAll => {
                let cur = self.queue_index.unwrap_or(0);
                (cur + 1) % self.queue.len()
            }
            PlayMode::Normal => {
                let cur = self.queue_index.unwrap_or(0);
                if cur + 1 >= self.queue.len() {
                    return Err("no next track".into());
                }
                cur + 1
            }
        };
        self.play_index(next_idx)
    }

    fn prev(&mut self) -> Result<(), String> {
        if self.queue.is_empty() {
            return Err("queue empty".into());
        }
        let prev_idx = match self.mode {
            PlayMode::RepeatOne => self.queue_index.unwrap_or(0),
            PlayMode::Shuffle => {
                let mut rng = rand::thread_rng();
                let indices: Vec<usize> = (0..self.queue.len()).collect();
                *indices.choose(&mut rng).unwrap_or(&0)
            }
            PlayMode::RepeatAll => {
                let cur = self.queue_index.unwrap_or(0);
                if cur == 0 {
                    self.queue.len() - 1
                } else {
                    cur - 1
                }
            }
            PlayMode::Normal => {
                let cur = self.queue_index.unwrap_or(0);
                if cur == 0 {
                    return Err("no previous track".into());
                }
                cur - 1
            }
        };
        self.play_index(prev_idx)
    }

    fn set_mode(&mut self, mode: PlayMode) {
        self.mode = mode;
        self.save_state();
    }

    fn status(&mut self) -> StatusResponse {
        if let Some(child) = self.child.as_mut() {
            match child.try_wait() {
                Ok(Some(_)) => {
                    self.child = None;
                    self.stdin = None;
                    if self.state == PlayState::Playing {
                        // track ended — auto next according to mode
                        self.state = PlayState::Stopped;
                        if self.mode != PlayMode::Normal
                            || self
                                .queue_index
                                .map(|i| i + 1 < self.queue.len())
                                .unwrap_or(false)
                        {
                            let _ = self.next();
                        }
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
            mode: self.mode.clone(),
        }
    }

    // ----- persistence -----

    fn save_state(&self) {
        let snap = PersistedState {
            volume: self.volume,
            mode: self.mode.clone(),
            queue: self
                .queue
                .iter()
                .map(|p| p.display().to_string())
                .collect(),
            queue_index: self.queue_index,
            current_path: self
                .current_path
                .as_ref()
                .map(|p| p.display().to_string()),
        };
        if let Ok(json) = serde_json::to_string_pretty(&snap) {
            if let Some(parent) = self.state_file.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Err(e) = fs::write(&self.state_file, json) {
                warn!("save state failed: {}", e);
            }
        }
    }

    fn load_state(&mut self) {
        let data = match fs::read_to_string(&self.state_file) {
            Ok(d) => d,
            Err(_) => return,
        };
        let snap: PersistedState = match serde_json::from_str(&data) {
            Ok(s) => s,
            Err(e) => {
                warn!("parse state failed: {}", e);
                return;
            }
        };
        self.volume = snap.volume.min(100);
        self.mode = snap.mode;
        self.queue = snap.queue.into_iter().map(PathBuf::from).collect();
        self.queue_index = snap.queue_index.filter(|&i| i < self.queue.len());
        self.current_path = snap.current_path.map(PathBuf::from);
        // do not auto-resume playback on boot; only restore queue/volume/mode
        self.state = PlayState::Stopped;
        info!(
            "restored state: queue={} mode={:?} volume={}",
            self.queue.len(),
            self.mode,
            self.volume
        );
    }
}

#[derive(Serialize, Deserialize)]
struct PersistedState {
    volume: u8,
    mode: PlayMode,
    queue: Vec<String>,
    queue_index: Option<usize>,
    current_path: Option<String>,
}

// ==================== Library ====================

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
    queue_len: usize,
    queue_index: Option<usize>,
    mode: PlayMode,
}

#[derive(Deserialize)]
struct PlayRequest {
    path: String,
}

#[derive(Deserialize)]
struct LibraryPlayRequest {
    path: Option<String>,
    id: Option<u64>,
}

#[derive(Deserialize)]
struct VolumeRequest {
    volume: u8,
}

#[derive(Deserialize)]
struct QueueRequest {
    paths: Vec<String>,
    #[serde(default)]
    start: usize,
}

#[derive(Deserialize)]
struct QueueAddRequest {
    paths: Vec<String>,
}

#[derive(Deserialize)]
struct QueueRemoveRequest {
    index: usize,
}

#[derive(Deserialize)]
struct ModeRequest {
    mode: PlayMode,
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

#[derive(Serialize)]
struct QueueResponse {
    paths: Vec<String>,
    index: Option<usize>,
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
    player.set_queue(vec![path.clone()], 0);
    player
        .play_path(path)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ApiError { error: e })))?;
    Ok(Json(player.status()))
}

async fn pause(
    State(state): State<AppState>,
) -> Result<Json<StatusResponse>, (StatusCode, Json<ApiError>)> {
    let mut player = state.player.lock().await;
    player
        .pause()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError { error: e })))?;
    Ok(Json(player.status()))
}

async fn resume(
    State(state): State<AppState>,
) -> Result<Json<StatusResponse>, (StatusCode, Json<ApiError>)> {
    let mut player = state.player.lock().await;
    player
        .resume()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError { error: e })))?;
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
    player.set_volume(req.volume);
    Ok(Json(player.status()))
}

async fn next(
    State(state): State<AppState>,
) -> Result<Json<StatusResponse>, (StatusCode, Json<ApiError>)> {
    let mut player = state.player.lock().await;
    player
        .next()
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ApiError { error: e })))?;
    Ok(Json(player.status()))
}

async fn prev(
    State(state): State<AppState>,
) -> Result<Json<StatusResponse>, (StatusCode, Json<ApiError>)> {
    let mut player = state.player.lock().await;
    player
        .prev()
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ApiError { error: e })))?;
    Ok(Json(player.status()))
}

async fn mode_set(
    State(state): State<AppState>,
    Json(req): Json<ModeRequest>,
) -> Json<StatusResponse> {
    let mut player = state.player.lock().await;
    player.set_mode(req.mode);
    Json(player.status())
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
    player
        .play_index(idx)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ApiError { error: e })))?;
    Ok(Json(player.status()))
}

async fn queue_add(
    State(state): State<AppState>,
    Json(req): Json<QueueAddRequest>,
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
    player.queue_add(paths);
    Ok(Json(player.status()))
}

async fn queue_remove(
    State(state): State<AppState>,
    Json(req): Json<QueueRemoveRequest>,
) -> Result<Json<StatusResponse>, (StatusCode, Json<ApiError>)> {
    let mut player = state.player.lock().await;
    player
        .queue_remove(req.index)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ApiError { error: e })))?;
    Ok(Json(player.status()))
}

async fn queue_clear(State(state): State<AppState>) -> Json<StatusResponse> {
    let mut player = state.player.lock().await;
    player.queue_clear();
    Json(player.status())
}

async fn queue_get(State(state): State<AppState>) -> Json<QueueResponse> {
    let player = state.player.lock().await;
    Json(QueueResponse {
        paths: player
            .queue
            .iter()
            .map(|p| p.display().to_string())
            .collect(),
        index: player.queue_index,
    })
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

async fn library_play(
    State(state): State<AppState>,
    Json(req): Json<LibraryPlayRequest>,
) -> Result<Json<StatusResponse>, (StatusCode, Json<ApiError>)> {
    let lib = state.library.lock().await;
    if lib.tracks.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "library empty, call /api/v1/library/scan first".into(),
            }),
        ));
    }
    let start_idx = if let Some(id) = req.id {
        lib.tracks.iter().position(|t| t.id == id).ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    error: format!("track id {} not found", id),
                }),
            )
        })?
    } else if let Some(ref path) = req.path {
        lib.tracks
            .iter()
            .position(|t| t.path == *path || t.rel_path == *path)
            .ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ApiError {
                        error: format!("path not in library: {}", path),
                    }),
                )
            })?
    } else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "provide path or id".into(),
            }),
        ));
    };
    let paths: Vec<PathBuf> = lib.tracks.iter().map(|t| PathBuf::from(&t.path)).collect();
    drop(lib);

    let mut player = state.player.lock().await;
    player.set_queue(paths, start_idx);
    player
        .play_index(start_idx)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ApiError { error: e })))?;
    Ok(Json(player.status()))
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
    let state_file = std::env::var("STATE_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("data/state.json"));

    info!("AUDIO_DEVICE={}", audio_device);
    info!("BIND={}", bind);
    info!("MUSIC_ROOT={}", music_root);
    info!("STATE_FILE={}", state_file.display());

    let state = AppState {
        player: Arc::new(Mutex::new(Player::new(audio_device, state_file))),
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
        .route("/api/v1/mode", post(mode_set))
        .route("/api/v1/queue", post(queue_set))
        .route("/api/v1/queue", get(queue_get))
        .route("/api/v1/queue/add", post(queue_add))
        .route("/api/v1/queue/remove", post(queue_remove))
        .route("/api/v1/queue/clear", post(queue_clear))
        .route("/api/v1/library/scan", post(library_scan))
        .route("/api/v1/library/status", get(library_status))
        .route("/api/v1/library", get(library_list))
        .route("/api/v1/library/play", post(library_play))
        .layer(CorsLayer::permissive())
        .with_state(state);

    // mDNS / DNS-SD: _rksonos._tcp (disable with MDNS=0)
    if let Err(e) = mdns::start(&bind) {
        warn!("mDNS start failed: {e:#}");
    }

    let listener = tokio::net::TcpListener::bind(&bind).await?;
    info!("listening on {}", bind);
    axum::serve(listener, app).await?;
    Ok(())
}
