# AGENTS.md

Guidance for AI coding agents working on RKSonos.

## Project overview

RKSonos is a Sonos-like WiFi speaker monorepo for **RK3566** (Ubuntu Server 20.04 vendor image).

| Component | Stack | Path |
|-----------|--------|------|
| Server (primary) | Rust + Axum + mpg123 / mpv | `server/` |
| Windows client | WPF (.NET 10) | `wpf/` |
| Android (planned) | Kotlin | — |

**MVP focus: Server first.** Playback runs on-device via ALSA; clients only control.

Current version: **0.3.2** (both server and WPF).

Hardware baseline:
- RK3566 / 2G RAM / 16G ROM
- USB speaker → ALSA `plughw:2,0`
- NAS via NFS: `/mnt/nas` → `192.168.1.8:/volume1/music`
- Vendor kernel 5.10.226 has `CONFIG_CIFS` disabled → use NFS, not SMB/CIFS

## Repository layout

```
.
├── server/                 # Rust binary (rksonos-server)
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs         # HTTP API, queue, library, player control
│       └── mdns.rs         # mDNS / DNS-SD (_rksonos._tcp)
├── wpf/
│   └── RkSonos.Client/     # WPF client (discovery + control)
├── docs/
│   ├── api.md              # Full HTTP API + recommended call order
│   ├── mdns.md             # Service type & discovery notes
│   └── roadmap.md          # P0–P4 plan
├── .github/workflows/
│   ├── server-package.yml  # tag server/v* → aarch64-musl binary + Release
│   ├── wpf-publish.yml     # tag wpf/v* → RkSonos.Client.exe + Release
│   └── android-package.yml # reserved
└── README.md
```

## Build & run

### Server (local x86 check)

```bash
cd server
cargo check
cargo build --release
```

Cross-compile for board is done in CI with Docker (`messense/rust-musl-cross:aarch64-musl`).
Do **not** assume local aarch64 toolchain; prefer CI artifact.

Binary name on board / in Release: `rksonos-server-aarch64-linux-musl` (do not rename in deploy scripts).

### Server (on-device)

```bash
AUDIO_DEVICE=plughw:2,0 \
MUSIC_ROOT=/mnt/nas \
STATE_FILE=/root/data/state.json \
BIND=0.0.0.0:8080 \
/root/rksonos-server-aarch64-linux-musl
```

Key env vars: `AUDIO_DEVICE`, `BIND`, `MUSIC_ROOT`, `STATE_FILE`, `RUST_LOG`, `MDNS`, `MDNS_NAME`.
See README for full table.

### WPF client

```bash
cd wpf/RkSonos.Client
dotnet restore
dotnet run -c Release
```

Target: `net10.0-windows`, WPF, package `Zeroconf` for mDNS.

Startup flow (do not change without reason):
1. Open discovery window (mDNS + LAN `/health` probe)
2. Do **not** enter play UI until a device is selected or manual `http://IP:8080` health=ok
3. Then open MainWindow

## Release / tags (strict)

Platform-specific tag prefixes → independent workflows & Releases (same pattern as FeiQ2026):

| Platform | Tag | Workflow | Asset |
|----------|-----|----------|--------|
| Server | `server/vX.Y.Z` | `server-package.yml` | `rksonos-server-aarch64-linux-musl` |
| WPF | `wpf/vX.Y.Z` | `wpf-publish.yml` | `RkSonos.Client.exe` |
| Android (later) | `android/vX.Y.Z` | `android-package.yml` | APK |

```bash
git tag server/v0.3.2 && git push origin server/v0.3.2
git tag wpf/v0.3.2    && git push origin wpf/v0.3.2
```

- Never push bare `v*` tags.
- Manual **Run workflow** only produces artifacts, does not create a Release.

## Coding conventions

- **One binary, env-driven**: no hard-coded device paths or hostnames in code; use env / mounts.
- Server is a single crate; keep `main.rs` + `mdns.rs` structure unless a clear module split is needed.
- Playback backends: mpg123 (primary, `-R` for pause/resume) and mpv (FLAC / m4a / ogg etc.). Prefer existing patterns.
- State persistence: queue, index, volume, mode → `STATE_FILE` (JSON).
- API is versioned under `/api/v1/`. Document changes in `docs/api.md`.
- mDNS service type: `_rksonos._tcp`. See `docs/mdns.md`.
- Prefer small, focused changes. Match existing style (Rust 2021, Axum 0.7, tokio full).
- License: MIT.

## Important docs for agents

- `docs/api.md` — full endpoint table + recommended call order (scan → library/play → mode/next/pause…)
- `docs/roadmap.md` — P0–P4 status; current = P3 done for mDNS+WPF, Kotlin/auth pending; P4 = DLNA/Docker/deb
- `docs/mdns.md` — discovery contract for clients

## Known open work / constraints

- Issue #3: playback progress (WS + position estimation) — not implemented yet.
- Android client and optional auth still open.
- No DLNA yet; keep self-protocol first.
- On board, NFS is required; do not rely on CIFS/SMB in code paths.

## What agents should avoid

- Changing release tag prefix convention without explicit request.
- Hard-coding `plughw:2,0` or NAS paths in source (env only).
- Skipping health/discovery gate in WPF.
- Renaming the aarch64 musl binary name used in Release/deploy docs.
- Large refactors of `main.rs` without clear need; prefer incremental edits.
