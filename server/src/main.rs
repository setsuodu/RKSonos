use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Path as FsPath, PathBuf},
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
    Normal,
    RepeatOne,
    RepeatAll,
    Shuffle,
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

fn pick_backend(path: &FsPath, has_mpv: bool) -> Result<Backend, String> {
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

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn slug_id(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    let out = out.trim_matches('-').chars().take(32).collect::<String>();
    if out.is_empty() {
        format!("m{}", now_ms() % 1_000_000)
    } else {
        out
    }
}

// ==================== Config (persisted mounts) ====================

/// One music source. Board starts empty; client adds via API.
/// Mount via `gio mount smb://...` (no kernel CIFS).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct MountEntry {
    id: String,
    /// e.g. smb://user@192.168.1.8/music  or  smb://192.168.1.8/share
    uri: String,
    #[serde(default)]
    label: String,
    /// optional; password stored in config for remount on boot (home LAN use)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    password: Option<String>,
    /// last resolved local path under GVFS (informational; re-resolved on mount)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mount_point: Option<String>,
    #[serde(default)]
    enabled: bool,
}

impl Default for MountEntry {
    fn default() -> Self {
        Self {
            id: String::new(),
            uri: String::new(),
            label: String::new(),
            username: None,
            password: None,
            mount_point: None,
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct AppConfig {
    mounts: Vec<MountEntry>,
}

struct ConfigStore {
    path: PathBuf,
    cfg: AppConfig,
}

impl ConfigStore {
    fn load(path: PathBuf) -> Self {
        let cfg = match fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => AppConfig::default(),
        };
        info!(
            "config loaded: {} mounts from {}",
            cfg.mounts.len(),
            path.display()
        );
        Self { path, cfg }
    }

    fn save(&self) {
        if let Some(parent) = self.path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        match serde_json::to_string_pretty(&self.cfg) {
            Ok(json) => {
                if let Err(e) = fs::write(&self.path, json) {
                    warn!("save config failed: {}", e);
                }
            }
            Err(e) => warn!("serialize config failed: {}", e),
        }
    }
}

// ==================== Mount manager (gio / GVFS SMB) ====================

struct MountManager;

impl MountManager {
    fn gio_available() -> bool {
        which("gio")
    }

    /// Percent-encode for embedding in smb URI userinfo.
    fn pct_encode(s: &str) -> String {
        let mut out = String::with_capacity(s.len() * 3);
        for b in s.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                    out.push(b as char)
                }
                _ => out.push_str(&format!("%{:02X}", b)),
            }
        }
        out
    }

    /// Accept smb://…, //host/share, \\host\share → smb://host/share
    fn normalize_smb_uri(uri: &str) -> String {
        let mut u = uri.trim().to_string();
        // Windows UNC → forward slashes
        u = u.replace('\\', "/");
        // strip leading smb:
        let lower = u.to_lowercase();
        if lower.starts_with("smb:") {
            u = u[4..].to_string(); // keep //host/...
        }
        // collapse to //host/share then host/share
        while u.starts_with('/') {
            u = u[1..].to_string();
        }
        // now host/share or user@host/share
        if u.is_empty() {
            return "smb://".into();
        }
        format!("smb://{}", u)
    }

    /// Inject username into smb URI (no password). Path must already be normalized.
    fn build_uri(uri: &str, username: Option<&str>) -> String {
        let uri = Self::normalize_smb_uri(uri);
        if let Some(user) = username {
            let user = user.trim();
            if user.is_empty() {
                return uri;
            }
            let rest = &uri[6..]; // after smb://
            if rest.contains('@') {
                return uri;
            }
            return format!("smb://{}@{}", user, rest);
        }
        uri
    }

    /// smb://user:pass@host/share — domain intentionally omitted (not WORKGROUP).
    fn build_uri_with_password(uri: &str, username: Option<&str>, password: &str) -> String {
        let base = Self::normalize_smb_uri(uri);
        let rest = &base[6..];
        let host_and_path = rest.split_once('@').map(|(_, r)| r).unwrap_or(rest);
        let user = username.unwrap_or("").trim();
        let u = Self::pct_encode(user);
        let p = Self::pct_encode(password);
        if u.is_empty() {
            format!("smb://:{}@{}", p, host_and_path)
        } else {
            format!("smb://{}:{}@{}", u, p, host_and_path)
        }
    }

    /// gvfs prompts vary. Domain must be empty line (user said workgroup is empty).
    fn write_auth_stdin(stdin: &mut impl std::io::Write, username: Option<&str>, password: &str) {
        let user = username.unwrap_or("").trim();
        // Cover: User / Domain / Password  and  Domain / Password  and  Password only
        let _ = writeln!(stdin, "{}", user);
        let _ = writeln!(stdin, ""); // empty domain — do NOT send WORKGROUP
        let _ = writeln!(stdin, "{}", password);
        let _ = writeln!(stdin, "");
        let _ = writeln!(stdin, "{}", password);
        let _ = stdin.flush();
    }

    /// Mount via gio. Returns resolved local path if found.
    fn mount(uri: &str, username: Option<&str>, password: Option<&str>) -> Result<String, String> {
        if !Self::gio_available() {
            return Err(
                "gio not found; install: apt install -y gvfs-backends gvfs-fuse libglib2.0-bin"
                    .into(),
            );
        }
        let full_uri = Self::build_uri(uri, username);
        info!("gio mount target: {}", full_uri);

        if let Some(p) = Self::find_mount_point(&full_uri) {
            info!("already mounted: {} -> {}", full_uri, p);
            return Ok(p);
        }
        // also match host/share without userinfo
        if let Some(p) = Self::find_mount_point(&Self::normalize_smb_uri(uri)) {
            info!("already mounted: {} -> {}", uri, p);
            return Ok(p);
        }

        let mut last_err = String::new();

        // A) credentials in URI
        if let Some(pw) = password {
            if !pw.is_empty() {
                let auth_uri = Self::build_uri_with_password(uri, username, pw);
                // never log password — log host only
                info!("gio mount attempt: URI credentials (host form)");
                match Self::gio_mount_once(&auth_uri, None, None) {
                    Ok(p) => return Ok(p),
                    Err(e) => {
                        last_err = e.clone();
                        info!("URI credentials failed: {}", e);
                    }
                }
            }
        }

        // B) user@host + stdin (empty domain)
        info!("gio mount attempt: stdin auth");
        match Self::gio_mount_once(&full_uri, username, password) {
            Ok(p) => return Ok(p),
            Err(e) => {
                last_err = e.clone();
                info!("stdin auth failed: {}", e);
            }
        }

        // C) bare host/share + stdin including username
        let bare = Self::normalize_smb_uri(uri);
        if bare != full_uri {
            info!("gio mount attempt: bare URI + stdin");
            match Self::gio_mount_once(&bare, username, password) {
                Ok(p) => return Ok(p),
                Err(e) => {
                    last_err = e;
                }
            }
        }

        Err(last_err)
    }

    fn gio_mount_once(
        full_uri: &str,
        username: Option<&str>,
        password: Option<&str>,
    ) -> Result<String, String> {
        let mut cmd = Command::new("gio");
        cmd.arg("mount").arg(full_uri);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("LANG", "C");

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("spawn gio mount failed: {}", e))?;

        if let Some(mut stdin) = child.stdin.take() {
            if let Some(pw) = password {
                Self::write_auth_stdin(&mut stdin, username, pw);
            }
        }

        let output = child
            .wait_with_output()
            .map_err(|e| format!("gio mount wait failed: {}", e))?;

        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let combined = format!("{} {}", stderr, stdout).to_lowercase();

        if !output.status.success() {
            if combined.contains("already mounted") {
                // ok
            } else {
                return Err(format!(
                    "gio mount failed: {} {}",
                    stderr.trim(),
                    stdout.trim()
                ));
            }
        }

        for _ in 0..40 {
            if let Some(p) = Self::find_mount_point(full_uri) {
                info!("mounted {} -> {}", full_uri, p);
                return Ok(p);
            }
            if let Some(rest) = full_uri.strip_prefix("smb://") {
                if let Some((_, hostpart)) = rest.split_once('@') {
                    let alt = format!("smb://{}", hostpart);
                    if let Some(p) = Self::find_mount_point(&alt) {
                        return Ok(p);
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        Err(format!(
            "mounted but path not resolved for {}; stderr={}",
            full_uri,
            stderr.trim()
        ))
    }

    fn unmount(uri: &str) -> Result<(), String> {
        if !Self::gio_available() {
            return Err("gio not found".into());
        }
        let status = Command::new("gio")
            .args(["mount", "-u", uri])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .status()
            .map_err(|e| format!("gio unmount failed: {}", e))?;
        if status.success() {
            Ok(())
        } else {
            // try by mount point
            if let Some(p) = Self::find_mount_point(uri) {
                let st = Command::new("gio")
                    .args(["mount", "-u", &p])
                    .status()
                    .map_err(|e| e.to_string())?;
                if st.success() {
                    return Ok(());
                }
            }
            Err(format!("gio unmount failed for {}", uri))
        }
    }

    /// Extract "Local Mount Path: …" value (path itself may contain ':').
    fn parse_local_mount_path_line(line: &str) -> Option<String> {
        let t = line.trim();
        let lower = t.to_lowercase();
        let key = "local mount path:";
        if !lower.starts_with(key) {
            return None;
        }
        // length of key in original may differ by case; split on first ':' after label
        let idx = t.find(':')?;
        let path = t[idx + 1..].trim().to_string();
        if path.is_empty() {
            None
        } else {
            Some(path)
        }
    }

    /// server + share from smb://[user@]host/share[/…]
    fn host_share_of(uri: &str) -> Option<(String, String)> {
        let u = uri.trim().trim_end_matches('/').to_lowercase();
        let rest = u.strip_prefix("smb://").unwrap_or(&u);
        let rest = rest.split_once('@').map(|(_, r)| r).unwrap_or(rest);
        let mut parts = rest.split('/');
        let host = parts.next()?.to_string();
        let share = parts.next().unwrap_or("").to_string();
        if host.is_empty() {
            return None;
        }
        Some((host, share))
    }

    fn path_matches_hint(path: &str, uri_hint: &str) -> bool {
        let pl = path.to_lowercase();
        let hint = uri_hint.trim_end_matches('/').to_lowercase();
        if pl.contains(&hint) || hint.contains(&pl) {
            return true;
        }
        if let Some((host, share)) = Self::host_share_of(&hint) {
            // gvfs: smb-share:server=HOST,share=SHARE
            let host_ok = pl.contains(&format!("server={}", host))
                || pl.contains(&host);
            let share_ok = share.is_empty()
                || pl.contains(&format!("share={}", share))
                || pl.contains(&share);
            if host_ok && share_ok {
                return true;
            }
        }
        false
    }

    /// Parse `gio mount -l` + scan /run/user/*/gvfs for smb locations.
    fn find_mount_point(uri_hint: &str) -> Option<String> {
        let hint = uri_hint.trim_end_matches('/').to_lowercase();

        if let Ok(out) = Command::new("gio").args(["mount", "-l"]).output() {
            let text = String::from_utf8_lossy(&out.stdout);
            let mut current_uri = String::new();
            for line in text.lines() {
                let t = line.trim();
                if t.starts_with("Mount(") {
                    if let Some(idx) = t.find("->") {
                        current_uri = t[idx + 2..].trim().trim_end_matches('/').to_lowercase();
                    }
                } else if let Some(path) = Self::parse_local_mount_path_line(t) {
                    let matched = Self::path_matches_hint(&path, &hint)
                        || (!current_uri.is_empty()
                            && (Self::path_matches_hint(&current_uri, &hint)
                                || current_uri.contains(
                                    hint.strip_prefix("smb://").unwrap_or(&hint),
                                )
                                || hint.contains(&current_uri)));
                    if matched && FsPath::new(&path).exists() {
                        return Some(path);
                    }
                }
            }
            // any listed path matching host/share
            for line in text.lines() {
                if let Some(path) = Self::parse_local_mount_path_line(line) {
                    if Self::path_matches_hint(&path, &hint) && FsPath::new(&path).exists() {
                        return Some(path);
                    }
                }
            }
        }

        // Fallback: scan gvfs dirs (root often /run/user/0/gvfs)
        let gvfs_roots = [
            "/run/user/0/gvfs",
            "/root/.gvfs",
            "/run/user/1000/gvfs",
        ];
        for root in gvfs_roots {
            let root_path = FsPath::new(root);
            if !root_path.exists() {
                continue;
            }
            if let Ok(rd) = std::fs::read_dir(root_path) {
                for ent in rd.flatten() {
                    let name = ent.file_name().to_string_lossy().to_lowercase();
                    if !name.starts_with("smb-share:") && !name.starts_with("smb:") {
                        continue;
                    }
                    let path = ent.path();
                    let ps = path.to_string_lossy();
                    if Self::path_matches_hint(&ps, &hint) && path.exists() {
                        return Some(ps.to_string());
                    }
                }
            }
        }
        None
    }

    fn list_active() -> Vec<(String, String)> {
        let mut v = Vec::new();
        let out = match Command::new("gio").args(["mount", "-l"]).output() {
            Ok(o) => o,
            Err(_) => return v,
        };
        let text = String::from_utf8_lossy(&out.stdout);
        let mut current_uri = String::new();
        for line in text.lines() {
            let t = line.trim();
            if t.starts_with("Mount(") {
                if let Some(idx) = t.find("->") {
                    current_uri = t[idx + 2..].trim().to_string();
                }
            } else if let Some(path) = Self::parse_local_mount_path_line(t) {
                if !current_uri.is_empty() {
                    v.push((current_uri.clone(), path));
                }
            }
        }
        v
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

    fn spawn_remote(&mut self, path: &FsPath) -> Result<(), String> {
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
        let cmd_json = serde_json::json!({ "command": args });
        let payload = cmd_json.to_string();
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
            Some(Backend::Mpv) | None => {}
        }
        let _ = Command::new("amixer")
            .args(["-q", "sset", "Master", &format!("{}%", self.volume)])
            .status();
        self.save_state();
    }

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

// ==================== Library (multi-root) ====================

const AUDIO_EXTS: &[&str] = &[
    "mp3", "flac", "m4a", "aac", "ogg", "wav", "ape", "wma", "aiff", "opus",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Track {
    id: u64,
    path: String,
    rel_path: String,
    name: String,
    size: u64,
    mtime: u64,
    /// which mount id contributed this track (if known)
    #[serde(skip_serializing_if = "Option::is_none")]
    mount_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct LibraryIndexFile {
    tracks: Vec<Track>,
    last_scan_ms: u64,
    saved_at_ms: u64,
}

#[derive(Debug, Default)]
struct Library {
    /// id -> local path
    roots: HashMap<String, PathBuf>,
    tracks: Vec<Track>,
    scanning: bool,
    last_scan_ms: u64,
    last_error: Option<String>,
    index_path: PathBuf,
}

impl Library {
    fn set_roots(&mut self, roots: HashMap<String, PathBuf>) {
        self.roots = roots;
    }

    fn scan(&mut self) -> Result<usize, String> {
        if self.scanning {
            return Err("scan already running".into());
        }
        if self.roots.is_empty() {
            return Err("no mounts configured; POST /api/v1/mounts first".into());
        }
        self.scanning = true;
        self.last_error = None;
        let start = now_ms();
        let mut tracks = Vec::new();
        let mut id: u64 = 1;

        for (mount_id, root) in &self.roots {
            if !root.exists() {
                warn!("scan skip missing root {} = {}", mount_id, root.display());
                continue;
            }
            for entry in WalkDir::new(root)
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
                    .strip_prefix(root)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| abs.clone());
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                tracks.push(Track {
                    id,
                    path: abs,
                    rel_path: format!("{}/{}", mount_id, rel),
                    name,
                    size: meta.len(),
                    mtime,
                    mount_id: Some(mount_id.clone()),
                });
                id += 1;
            }
        }

        let count = tracks.len();
        self.tracks = tracks;
        self.last_scan_ms = now_ms().saturating_sub(start);
        self.scanning = false;
        self.last_error = None;
        info!("library scan: {} tracks in {}ms", count, self.last_scan_ms);
        self.save_index();
        Ok(count)
    }

    fn save_index(&self) {
        if self.index_path.as_os_str().is_empty() {
            return;
        }
        if let Some(parent) = self.index_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let file = LibraryIndexFile {
            tracks: self.tracks.clone(),
            last_scan_ms: self.last_scan_ms,
            saved_at_ms: now_ms(),
        };
        match serde_json::to_string_pretty(&file) {
            Ok(json) => {
                if let Err(e) = fs::write(&self.index_path, json) {
                    warn!("save library index failed: {}", e);
                } else {
                    info!(
                        "library index saved: {} tracks -> {}",
                        self.tracks.len(),
                        self.index_path.display()
                    );
                }
            }
            Err(e) => warn!("serialize library index failed: {}", e),
        }
    }

    fn load_index(&mut self) {
        if self.index_path.as_os_str().is_empty() {
            return;
        }
        let Ok(s) = fs::read_to_string(&self.index_path) else {
            return;
        };
        match serde_json::from_str::<LibraryIndexFile>(&s) {
            Ok(file) => {
                self.tracks = file.tracks;
                self.last_scan_ms = file.last_scan_ms;
                info!(
                    "library index loaded: {} tracks from {}",
                    self.tracks.len(),
                    self.index_path.display()
                );
            }
            Err(e) => warn!("load library index failed: {}", e),
        }
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
                    || t.path.to_lowercase().contains(&q)
            })
            .collect()
    }

    fn by_id(&self, id: u64) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }

    fn by_path(&self, path: &str) -> Option<&Track> {
        self.tracks.iter().find(|t| t.path == path)
    }
}

// ==================== App State & API types ====================

#[derive(Clone)]
struct AppState {
    player: Arc<Mutex<Player>>,
    library: Arc<Mutex<Library>>,
    config: Arc<Mutex<ConfigStore>>,
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
    roots: Vec<RootInfo>,
    scanning: bool,
    track_count: usize,
    last_scan_ms: u64,
    last_error: Option<String>,
}

#[derive(Serialize)]
struct RootInfo {
    id: String,
    path: String,
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

#[derive(Deserialize)]
struct MountAddRequest {
    /// smb://user@host/share  or  smb://host/share
    uri: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    password: Option<String>,
    /// if true (default), mount immediately
    #[serde(default = "default_true")]
    mount_now: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Serialize)]
struct MountView {
    id: String,
    uri: String,
    label: String,
    username: Option<String>,
    /// password never returned
    mount_point: Option<String>,
    enabled: bool,
    mounted: bool,
}

#[derive(Serialize)]
struct MountListResponse {
    mounts: Vec<MountView>,
    gio: bool,
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
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ApiError { error: e })))?;
    Ok(Json(player.status()))
}

async fn resume(
    State(state): State<AppState>,
) -> Result<Json<StatusResponse>, (StatusCode, Json<ApiError>)> {
    let mut player = state.player.lock().await;
    player
        .resume()
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ApiError { error: e })))?;
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
    // refresh roots from config mount points before scan
    {
        let store = state.config.lock().await;
        let mut roots = HashMap::new();
        for m in &store.cfg.mounts {
            if !m.enabled {
                continue;
            }
            if let Some(ref mp) = m.mount_point {
                if FsPath::new(mp).exists() {
                    roots.insert(m.id.clone(), PathBuf::from(mp));
                    continue;
                }
            }
            if let Some(p) = MountManager::find_mount_point(&m.uri) {
                roots.insert(m.id.clone(), PathBuf::from(p));
            }
        }
        let mut lib = state.library.lock().await;
        lib.set_roots(roots);
    }

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
    let roots = lib
        .roots
        .iter()
        .map(|(id, p)| RootInfo {
            id: id.clone(),
            path: p.display().to_string(),
        })
        .collect();
    Json(LibraryStatus {
        roots,
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
    let (start_path, all_paths) = {
        let lib = state.library.lock().await;
        if lib.tracks.is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    error: "library empty; scan first".into(),
                }),
            ));
        }

        // Resolve start path: prefer id, else path (indexed or raw absolute).
        let start = if let Some(id) = req.id {
            let t = lib.by_id(id).ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ApiError {
                        error: format!("id not found: {}", id),
                    }),
                )
            })?;
            PathBuf::from(&t.path)
        } else if let Some(ref path) = req.path {
            if let Some(t) = lib.by_path(path) {
                PathBuf::from(&t.path)
            } else {
                // allow absolute path even if not in index
                PathBuf::from(path)
            }
        } else {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    error: "path or id required".into(),
                }),
            ));
        };

        let all: Vec<PathBuf> = lib.tracks.iter().map(|t| PathBuf::from(&t.path)).collect();
        (start, all)
    };

    let start_idx = all_paths
        .iter()
        .position(|p| p == &start_path)
        .unwrap_or(0);

    let mut player = state.player.lock().await;
    player.set_queue(all_paths, start_idx);
    player
        .play_index(start_idx)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ApiError { error: e })))?;
    Ok(Json(player.status()))
}

// ----- mounts -----

fn to_view(m: &MountEntry) -> MountView {
    let mounted = m
        .mount_point
        .as_ref()
        .map(|p| FsPath::new(p).exists())
        .unwrap_or(false)
        || MountManager::find_mount_point(&m.uri).is_some();
    MountView {
        id: m.id.clone(),
        uri: m.uri.clone(),
        label: m.label.clone(),
        username: m.username.clone(),
        mount_point: m
            .mount_point
            .clone()
            .or_else(|| MountManager::find_mount_point(&m.uri)),
        enabled: m.enabled,
        mounted,
    }
}

async fn mounts_list(State(state): State<AppState>) -> Json<MountListResponse> {
    let store = state.config.lock().await;
    let mounts = store.cfg.mounts.iter().map(to_view).collect();
    Json(MountListResponse {
        mounts,
        gio: MountManager::gio_available(),
    })
}

async fn mounts_add(
    State(state): State<AppState>,
    Json(req): Json<MountAddRequest>,
) -> Result<Json<MountView>, (StatusCode, Json<ApiError>)> {
    let uri = req.uri.trim().to_string();
    if uri.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "uri required (e.g. smb://user@192.168.1.8/music)".into(),
            }),
        ));
    }
    if !uri.starts_with("smb://") && !uri.contains("://") {
        // allow host/share shorthand → smb://
    }

    let id = req
        .id
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let base = req
                .label
                .chars()
                .chain(uri.chars())
                .collect::<String>();
            slug_id(&base)
        });

    let uri_norm = MountManager::normalize_smb_uri(&uri);
    if !uri_norm.starts_with("smb://") || uri_norm.len() <= 6 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: format!("invalid smb uri: {}", uri),
            }),
        ));
    }

    let mut entry = MountEntry {
        id: id.clone(),
        uri: uri_norm,
        label: if req.label.is_empty() {
            id.clone()
        } else {
            req.label
        },
        username: req.username.clone(),
        password: req.password.clone(),
        mount_point: None,
        enabled: true,
    };

    if req.mount_now {
        match MountManager::mount(
            &entry.uri,
            entry.username.as_deref(),
            entry.password.as_deref(),
        ) {
            Ok(mp) => {
                entry.mount_point = Some(mp.clone());
                // update library root immediately
                let mut lib = state.library.lock().await;
                lib.roots.insert(entry.id.clone(), PathBuf::from(mp));
            }
            Err(e) => {
                return Err((StatusCode::BAD_REQUEST, Json(ApiError { error: e })));
            }
        }
    }

    let view = to_view(&entry);
    {
        let mut store = state.config.lock().await;
        // replace same id
        store.cfg.mounts.retain(|m| m.id != entry.id);
        store.cfg.mounts.push(entry);
        store.save();
    }
    Ok(Json(view))
}

async fn mounts_delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let mut store = state.config.lock().await;
    let pos = store
        .cfg
        .mounts
        .iter()
        .position(|m| m.id == id)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    error: format!("mount id not found: {}", id),
                }),
            )
        })?;
    let entry = store.cfg.mounts.remove(pos);
    store.save();
    drop(store);

    let _ = MountManager::unmount(&entry.uri);
    {
        let mut lib = state.library.lock().await;
        lib.roots.remove(&id);
    }
    Ok(Json(serde_json::json!({ "status": "ok", "id": id })))
}

async fn mounts_remount(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<MountView>, (StatusCode, Json<ApiError>)> {
    let (uri, username, password, mut entry) = {
        let store = state.config.lock().await;
        let m = store
            .cfg
            .mounts
            .iter()
            .find(|m| m.id == id)
            .cloned()
            .ok_or_else(|| {
                (
                    StatusCode::NOT_FOUND,
                    Json(ApiError {
                        error: format!("mount id not found: {}", id),
                    }),
                )
            })?;
        (m.uri.clone(), m.username.clone(), m.password.clone(), m)
    };

    let mp = MountManager::mount(&uri, username.as_deref(), password.as_deref())
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ApiError { error: e })))?;

    entry.mount_point = Some(mp.clone());
    entry.enabled = true;
    {
        let mut store = state.config.lock().await;
        if let Some(m) = store.cfg.mounts.iter_mut().find(|m| m.id == id) {
            m.mount_point = Some(mp.clone());
            m.enabled = true;
            store.save();
        }
    }
    {
        let mut lib = state.library.lock().await;
        lib.roots.insert(id, PathBuf::from(mp));
    }
    Ok(Json(to_view(&entry)))
}

async fn mounts_active() -> Json<serde_json::Value> {
    let list = MountManager::list_active();
    let items: Vec<_> = list
        .into_iter()
        .map(|(uri, path)| serde_json::json!({ "uri": uri, "path": path }))
        .collect();
    Json(serde_json::json!({ "active": items, "gio": MountManager::gio_available() }))
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
    // MUSIC_ROOT is no longer required. Optional legacy single-root bootstrap via env removed
    // in favor of client-driven mounts. Config file holds mounts.
    let state_file = std::env::var("STATE_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("data/state.json"));
    let config_file = std::env::var("CONFIG_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            // same directory as state
            state_file
                .parent()
                .map(|p| p.join("config.json"))
                .unwrap_or_else(|| PathBuf::from("data/config.json"))
        });

    info!("AUDIO_DEVICE={}", audio_device);
    info!("BIND={}", bind);
    info!("STATE_FILE={}", state_file.display());
    info!("CONFIG_FILE={}", config_file.display());
    info!("gio available={}", MountManager::gio_available());

    let mut store = ConfigStore::load(config_file);
    // default: empty mounts — client configures

    // Remount saved entries on boot
    let mut roots = HashMap::new();
    for m in store.cfg.mounts.iter_mut() {
        if !m.enabled {
            continue;
        }
        match MountManager::mount(&m.uri, m.username.as_deref(), m.password.as_deref()) {
            Ok(mp) => {
                m.mount_point = Some(mp.clone());
                roots.insert(m.id.clone(), PathBuf::from(mp));
            }
            Err(e) => {
                warn!("boot remount {} failed: {}", m.id, e);
                m.mount_point = None;
            }
        }
    }
    store.save();

    let mut library = Library {
        index_path: PathBuf::from("data/library.json"),
        ..Library::default()
    };
    library.load_index();
    library.set_roots(roots);

    let state = AppState {
        player: Arc::new(Mutex::new(Player::new(audio_device, state_file))),
        library: Arc::new(Mutex::new(library)),
        config: Arc::new(Mutex::new(store)),
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
        // mounts (client-driven, multi-dir, gio smb://)
        .route("/api/v1/mounts", get(mounts_list))
        .route("/api/v1/mounts", post(mounts_add))
        .route("/api/v1/mounts/active", get(mounts_active))
        .route("/api/v1/mounts/:id", delete(mounts_delete))
        .route("/api/v1/mounts/:id/remount", post(mounts_remount))
        .layer(CorsLayer::permissive())
        .with_state(state);

    if let Err(e) = mdns::start(&bind) {
        warn!("mDNS start failed: {e:#}");
    }

    let listener = tokio::net::TcpListener::bind(&bind).await?;
    info!("listening on {}", bind);
    axum::serve(listener, app).await?;
    Ok(())
}
