# RKSonos

RK3566 WiFi speaker (Sonos-like) monorepo.

**MVP focus: Server first.**

| Component | Stack | 目录 |
|-----------|--------|------|
| Server | Rust + Axum + mpg123 / mpv | `server/` |
| Windows | WPF (.NET) | `wpf/` |
| App (later) | Kotlin | — |

## Hardware (current)

- RK3566 / Ubuntu Server 20.04 (vendor image)
- 2G RAM / 16G ROM
- USB speaker → ALSA `plughw:2,0`
- NAS via NFS: `/mnt/nas` → `192.168.1.8:/volume1/music`

> Vendor kernel `5.10.226` has `CONFIG_CIFS` disabled. Use NFS for now.

## 板上安装（拿到板子之后）

### 1. 系统依赖

```bash
apt update
apt install -y mpg123 nfs-common curl
# FLAC / m4a / ogg 等需要 mpv（可选但推荐）
apt install -y mpv
# mpv 暂停控制（可选）
apt install -y socat
```

确认声卡：

```bash
aplay -l
# 记下 USB 喇叭 card 号，例如 card 2 → AUDIO_DEVICE=plughw:2,0
```

### 2. 挂载 NAS 曲库

```bash
mkdir -p /mnt/nas
mount -t nfs -o vers=3 192.168.1.8:/volume1/music /mnt/nas
ls /mnt/nas   # 能看到歌曲即可
```

开机自动挂载，写入 `/etc/fstab`：

```bash
192.168.1.8:/volume1/music  /mnt/nas  nfs  vers=3,defaults,_netdev  0  0
```

### 3. 安装 server 二进制

从 [Releases](../../releases) 下载 **Server** 资产 `rksonos-server-aarch64-linux-musl`（见下方「发版 Tag SOP」），拷到板子：

```bash
# 在电脑上（文件名以 Release 为准）
scp rksonos-server-aarch64-linux-musl root@板子IP:/root/rksonos-server

# 在板子上
cd /root
chmod +x rksonos-server
mkdir -p /root/data
```

### 4. 启动

```bash
AUDIO_DEVICE=plughw:2,0 \
MUSIC_ROOT=/mnt/nas \
STATE_FILE=/root/data/state.json \
BIND=0.0.0.0:8080 \
./rksonos-server
```

后台常驻可用 `nohup` 或 systemd（自行加 unit）。

### 5. 验收

```bash
curl -s http://127.0.0.1:8080/health
# ok

curl -s -X POST http://127.0.0.1:8080/api/v1/library/scan
# {"status":"ok","count":...,"took_ms":...}

curl -s -X POST http://127.0.0.1:8080/api/v1/library/play \
  -H 'Content-Type: application/json' \
  -d '{"path":"/mnt/nas/某首歌.mp3"}'
```

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
| POST | `/api/v1/pause` | — | 暂停 |
| POST | `/api/v1/resume` | — | 继续（不从头） |
| POST | `/api/v1/stop` | — | 停止 |
| POST | `/api/v1/volume` | `{"volume":80}` | 0–100 |
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

## WPF 客户端

源码在 `wpf/`。本地开发：

```bash
cd wpf/RkSonos.Client
dotnet restore
dotnet run -c Release
```

发行包见下方 Tag SOP 的 **WPF** Release（`RkSonos.Client.exe`，win-x64 单文件）。

---

## 发版 Tag SOP

CI 按**平台拆分**（风格同 FeiQ2026）：**不同前缀的 tag → 不同 workflow → 独立 GitHub Release**，互不覆盖资产。

### Tag 约定

| 平台 | Tag 格式 | Workflow | Release 标题示例 | 主要资产 |
|------|----------|----------|------------------|----------|
| Server | `server/vX.Y.Z` | `server-package.yml` | Server 0.3.1 | `rksonos-server-aarch64-linux-musl` |
| WPF | `wpf/vX.Y.Z` | `wpf-publish.yml` | WPF 0.3.0 | `RkSonos.Client.exe` |
| Android（预留） | `android/vX.Y.Z` | `android-package.yml` | Android 0.1.0 | `*.apk`（工程就绪后） |

- 版本号用 **SemVer**：`X.Y.Z`（可加 `-beta.1` 等后缀，按需）
- **不要**再推裸的 `vX.Y.Z`（旧 `ci.yml` 已废弃；统一用带前缀的 tag）
- 每个 tag 只打**一个**平台；要发两端就打两个 tag

### 发版步骤（推荐）

1. **确认 main 可发**  
   - Server：`server/` 能 `cargo check` / 板上验收过  
   - WPF：本机 `dotnet build -c Release` 通过  

2. **打 tag 并推送**（在仓库根目录）

   ```bash
   # 只发 Server
   git checkout main
   git pull
   git tag server/v0.3.1
   git push origin server/v0.3.1

   # 只发 WPF
   git tag wpf/v0.3.0
   git push origin wpf/v0.3.0

   # 两端都发：两条都推即可（两个独立 Release）
   ```

3. **看 Actions**  
   - 对应 workflow 变绿  
   - 自动创建 Release，资产已挂上  

4. **下载与部署**  
   - Server：Releases 里找 **Server x.y.z** → 下 `rksonos-server-aarch64-linux-musl` → 按「板上安装」  
   - WPF：找 **WPF x.y.z** → 下 `RkSonos.Client.exe` → Windows 双击运行  

### 手动试构建（不发 Release）

Actions 页打开对应 workflow → **Run workflow**（`workflow_dispatch`）。  
仅上传 artifact，**不会**因手动运行而创建 Release；正式发版仍以推 tag 为准。

### 清理（慎用）

`cleanup.yml` 可手动删光 Releases / tags / workflow runs，仅维护或翻车时用。

### 本地不经过 CI 时

```bash
# Server（需本机有对应交叉工具链或在板子上 native build）
cd server && cargo build --release

# WPF
cd wpf/RkSonos.Client
dotnet publish -c Release -r win-x64 --self-contained true \
  -p:PublishSingleFile=true -o publish
```

---

## Roadmap

- **P0** ✅ local + NAS play via mpg123  
- **P1** ✅ REST server + aarch64 CI  
- **P2** ✅ library scan, queue, pause/resume, mode, persistence  
- **P3** 🔧 WPF 客户端；mDNS / Kotlin 待补  
- **P4** DLNA, Docker / deb packaging  

## License

MIT
