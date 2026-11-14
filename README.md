# RKSonos

RK3566 WiFi speaker (Sonos-like) monorepo.

**MVP focus: Server first.**

| Component | Stack |
|-----------|--------|
| Server | Rust + Axum + mpg123 |
| App (later) | Kotlin |
| Windows (later) | WPF |

## Hardware (current)

- RK3566 / Ubuntu Server 20.04 (vendor image)
- 2G RAM / 16G ROM
- USB speaker → ALSA `plughw:2,0`
- NAS via NFS: `/mnt/nas` → `192.168.1.8:/volume1/music`

> Vendor kernel `5.10.226` has `CONFIG_CIFS` disabled. Use NFS for now.

## Server (P1)

### API

| Method | Path | Body | Description |
|--------|------|------|-------------|
| GET | `/health` | — | liveness |
| GET | `/api/v1/status` | — | state / path / volume |
| POST | `/api/v1/play` | `{"path":"/mnt/nas/hero_yoshiki.mp3"}` | play file |
| POST | `/api/v1/pause` | — | pause (MVP = stop) |
| POST | `/api/v1/stop` | — | stop |
| POST | `/api/v1/volume` | `{"volume":80}` | 0–100 (stored only) |

### Env

| Var | Default | Meaning |
|-----|---------|---------|
| `AUDIO_DEVICE` | `plughw:2,0` | ALSA device |
| `BIND` | `0.0.0.0:8080` | listen address |
| `RUST_LOG` | `info` | log level |

Same binary works for native / Docker / future Debian packaging (config via env).

### Build (CI)

GitHub Actions builds `aarch64-unknown-linux-gnu` release artifact on every push.

Download artifact `rksonos-server-aarch64` from Actions, scp to board:

```bash
chmod +x rksonos-server
AUDIO_DEVICE=plughw:2,0 ./rksonos-server
```

### Test on board

```bash
# status
curl -s http://127.0.0.1:8080/api/v1/status | jq

# play NAS file
curl -s -X POST http://127.0.0.1:8080/api/v1/play \
  -H 'Content-Type: application/json' \
  -d '{"path":"/mnt/nas/hero_yoshiki.mp3"}' | jq

# stop
curl -s -X POST http://127.0.0.1:8080/api/v1/stop | jq
```

### Local dev (x86)

```bash
cd server
cargo run
# need mpg123 + a sound device, or just hit /health /status
```

## NFS auto-mount (board)

```bash
# /etc/fstab
192.168.1.8:/volume1/music  /mnt/nas  nfs  vers=3,defaults,_netdev  0  0
```

## Roadmap

- **P0** ✅ local + NAS play via mpg123
- **P1** ✅ REST server + aarch64 CI
- **P1.5** real pause, volume via ALSA, queue
- **P2** library scan, mDNS discovery
- **P3** Kotlin / WPF clients, DLNA

## License

MIT
