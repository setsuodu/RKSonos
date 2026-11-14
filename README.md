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

## Env

| Var | Default | Meaning |
|-----|---------|---------|
| `AUDIO_DEVICE` | `plughw:2,0` | ALSA device |
| `BIND` | `0.0.0.0:8080` | listen address |
| `MUSIC_ROOT` | `/mnt/nas` | library root for scan |
| `STATE_FILE` | `data/state.json` | queue / volume / mode persistence |
| `RUST_LOG` | `info` | log level |

## API call order（推荐顺序）

日常从曲库听歌：

```text
1. POST /api/v1/library/scan          # 扫库（首次或曲库有变动时）
2. POST /api/v1/library/play          # 指定一首，整库进队列并开始播
3. POST /api/v1/mode                  # 可选：repeat_all / shuffle / ...
4. POST /api/v1/next | /prev          # 切歌
5. POST /api/v1/pause | /resume       # 暂停 / 继续（不从头）
6. POST /api/v1/volume                # 调音量
7. GET  /api/v1/status                # 随时查状态
```

自己拼播放列表：

```text
1. POST /api/v1/queue                 # 整表替换并播放
   或 POST /api/v1/queue/add          # 追加
2. POST /api/v1/next | /prev
3. POST /api/v1/queue/remove          # 按 index 删一首
4. POST /api/v1/queue/clear
```

单曲试播（队列只有一首，next 会失败是正常的）：

```text
POST /api/v1/play   {"path":"/mnt/nas/xxx.mp3"}
```

> **注意**：扫库只建索引，**不会**自动进播放队列。  
> 要用 next/prev，必须先 `library/play` 或 `queue`。

## API 一览

### 播放控制

| Method | Path | Body | Description |
|--------|------|------|-------------|
| GET | `/health` | — | liveness |
| GET | `/api/v1/status` | — | state / path / volume / queue / mode |
| POST | `/api/v1/play` | `{"path":"/mnt/nas/a.mp3"}` | 单曲播放（队列=1） |
| POST | `/api/v1/pause` | — | 暂停（mpg123 remote） |
| POST | `/api/v1/resume` | — | 继续（不从头） |
| POST | `/api/v1/stop` | — | 停止 |
| POST | `/api/v1/volume` | `{"volume":80}` | 0–100，写入 mpg123 + 尝试 amixer |
| POST | `/api/v1/next` | — | 下一首（受 mode 影响） |
| POST | `/api/v1/prev` | — | 上一首 |
| POST | `/api/v1/mode` | `{"mode":"repeat_all"}` | `normal` / `repeat_one` / `repeat_all` / `shuffle` |

### 队列

| Method | Path | Body | Description |
|--------|------|------|-------------|
| GET | `/api/v1/queue` | — | 当前队列 + index |
| POST | `/api/v1/queue` | `{"paths":["..."],"start":0}` | 整表替换并播放 |
| POST | `/api/v1/queue/add` | `{"paths":["..."]}` | 追加 |
| POST | `/api/v1/queue/remove` | `{"index":3}` | 按索引删除 |
| POST | `/api/v1/queue/clear` | — | 清空 |

### 曲库

| Method | Path | Body | Description |
|--------|------|------|-------------|
| POST | `/api/v1/library/scan` | — | 全量扫描 `MUSIC_ROOT` |
| GET | `/api/v1/library/status` | — | 扫描状态 / 曲目数 |
| GET | `/api/v1/library?q=&limit=&offset=` | — | 列表 / 搜索 |
| POST | `/api/v1/library/play` | `{"path":"..."}` 或 `{"id":42}` | **整库进队列并从指定曲开始播** |

## 板上示例

```bash
# 扫库
curl -s -X POST http://127.0.0.1:8080/api/v1/library/scan | jq

# 从某首开始听整库
curl -s -X POST http://127.0.0.1:8080/api/v1/library/play \
  -H 'Content-Type: application/json' \
  -d '{"path":"/mnt/nas/hero_yoshiki.mp3"}' | jq

# 循环整库
curl -s -X POST http://127.0.0.1:8080/api/v1/mode \
  -H 'Content-Type: application/json' \
  -d '{"mode":"repeat_all"}' | jq

# 下一首
curl -s -X POST http://127.0.0.1:8080/api/v1/next | jq

# 暂停 / 继续
curl -s -X POST http://127.0.0.1:8080/api/v1/pause | jq
curl -s -X POST http://127.0.0.1:8080/api/v1/resume | jq
```

## Build / 部署

GitHub Actions 构建 `aarch64-unknown-linux-gnu` release artifact。

```bash
chmod +x rksonos-server
AUDIO_DEVICE=plughw:2,0 MUSIC_ROOT=/mnt/nas ./rksonos-server
```

本地开发：

```bash
cd server
cargo run
```

## NFS auto-mount (board)

```bash
# /etc/fstab
192.168.1.8:/volume1/music  /mnt/nas  nfs  vers=3,defaults,_netdev  0  0
```

## Roadmap

- **P0** ✅ local + NAS play via mpg123
- **P1** ✅ REST server + aarch64 CI
- **P2** ✅ library scan, queue, pause/resume, mode, persistence
- **P3** mDNS discovery, Kotlin / WPF clients
- **P4** DLNA, Docker / deb packaging

## License

MIT
