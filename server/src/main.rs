use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::Arc,
};
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;
use tracing::{error, info};

/// Playback state
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
    volume: u8, // 0-100, best-effort via mpg123 -f later
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

        // mpg123 -a <device> -q <file>
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
        // mpg123 does not support true pause via signal easily without --control;
        // MVP: stop (P1.5 can switch to long-running control mode)
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
        // Reap exited child
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

#[derive(Clone)]
struct AppState {
    player: Arc<Mutex<Player>>,
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
    /// Absolute path on device, e.g. /mnt/nas/hero_yoshiki.mp3
    path: String,
}

#[derive(Deserialize)]
struct VolumeRequest {
    /// 0-100
    volume: u8,
}

#[derive(Serialize)]
struct ApiError {
    error: String,
}

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
    // MVP: volume stored only; real ALSA/mpg123 volume in next iteration
    Ok(Json(player.status()))
}

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

    info!("AUDIO_DEVICE={}", audio_device);
    info!("BIND={}", bind);

    let state = AppState {
        player: Arc::new(Mutex::new(Player::new(audio_device))),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/v1/status", get(status))
        .route("/api/v1/play", post(play))
        .route("/api/v1/pause", post(pause))
        .route("/api/v1/stop", post(stop))
        .route("/api/v1/volume", post(volume))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&bind).await?;
    info!("listening on {}", bind);
    axum::serve(listener, app).await?;
    Ok(())
}
