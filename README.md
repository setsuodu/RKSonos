# RKSonos

RK3566 WiFi speaker (Sonos-like) monorepo.

| Component | Stack | 目录 |
|-----------|--------|------|
| Server | Rust + Axum + mpg123 / mpv | `server/` |
| Windows | WPF (.NET) | `wpf/` |
| App (later) | Kotlin | — |

## 文档

| 文档 | 说明 |
|------|------|
| [docs/api.md](docs/api.md) | HTTP API 全表与推荐调用顺序 |
| [docs/mdns.md](docs/mdns.md) | mDNS 服务类型与客户端发现 |
| [docs/roadmap.md](docs/roadmap.md) | 分阶段计划 |
| [wpf/README.md](wpf/README.md) | WPF 客户端说明 |

## Hardware (current)

- RK3566 / Ubuntu Server 20.04 (vendor image)
- 2G RAM / 16G ROM
- USB speaker → ALSA `plughw:2,0`
- NAS via NFS: `/mnt/nas` → `192.168.1.8:/volume1/music`

> Vendor kernel `5.10.226` has `CONFIG_CIFS` disabled. Use NFS for now.

## 板上安装

### 1. 系统依赖

```bash
apt update
apt install -y mpg123 nfs-common curl
apt install -y mpv      # FLAC / m4a / ogg 等
apt install -y socat   # mpv 暂停控制（可选）
```

```bash
aplay -l
# USB 扬声器 card 2 → AUDIO_DEVICE=plughw:2,0
```

### 2. 挂载 NAS

```bash
mkdir -p /mnt/nas
mount -t nfs -o vers=3 192.168.1.8:/volume1/music /mnt/nas
```

`/etc/fstab`：

```
192.168.1.8:/volume1/music  /mnt/nas  nfs  vers=3,defaults,_netdev  0  0
```

### 3. 安装 server 二进制

从 [Releases](../../releases) 的 **Server** 资产下载 `rksonos-server-aarch64-linux-musl`：

```bash
scp rksonos-server-aarch64-linux-musl root@板子IP:/root/rksonos-server
ssh root@板子IP 'chmod +x /root/rksonos-server && mkdir -p /root/data'
```

### 4. 启动

```bash
AUDIO_DEVICE=plughw:2,0 \
MUSIC_ROOT=/mnt/nas \
STATE_FILE=/root/data/state.json \
BIND=0.0.0.0:8080 \
./rksonos-server
```

默认开启 mDNS（`_rksonos._tcp`）。关闭：`MDNS=0`。自定义实例名：`MDNS_NAME=客厅音箱`。

### 5. 验收

```bash
curl -s http://127.0.0.1:8080/health   # ok
# 完整 API 见 docs/api.md
```

## Env

| Var | Default | Meaning |
|-----|---------|---------|
| `AUDIO_DEVICE` | `plughw:2,0` | ALSA device |
| `BIND` | `0.0.0.0:8080` | listen address |
| `MUSIC_ROOT` | `/mnt/nas` | library root for scan |
| `STATE_FILE` | `data/state.json` | queue / volume / mode persistence |
| `RUST_LOG` | `info` | log level |
| `MDNS` | on | `0` / `false` / `off` 关闭广播 |
| `MDNS_NAME` | hostname / `rksonos` | mDNS 实例名 |

## WPF 客户端

1. 启动进入 **找设备页**（mDNS + 局域网 `/health` 探测）
2. **未发现且未手动连上之前，不进入播放页**
3. 选中设备或手动填 `http://IP:8080` 且 health=ok 后打开播放页

```bash
cd wpf/RkSonos.Client
dotnet restore
dotnet run -c Release
```

发行包：见下方 Tag SOP 的 `wpf/v*` Release。

## 发版 Tag SOP

不同平台 **不同 tag 前缀** → 独立 workflow / Release（对齐 FeiQ2026）。

| 平台 | Tag | Workflow | 资产 |
|------|-----|----------|------|
| Server | `server/vX.Y.Z` | `server-package.yml` | `rksonos-server-aarch64-linux-musl` |
| WPF | `wpf/vX.Y.Z` | `wpf-publish.yml` | `RkSonos.Client.exe` |
| Android（预留） | `android/vX.Y.Z` | `android-package.yml` | APK |

```bash
git tag server/v0.3.2 && git push origin server/v0.3.2
git tag wpf/v0.3.2    && git push origin wpf/v0.3.2
```

不要再推裸 `v*`。手动 **Run workflow** 只出 artifact，不建 Release。

## Roadmap

- **P0–P2** ✅ 见 [docs/roadmap.md](docs/roadmap.md)
- **P3** ✅ mDNS + WPF 发现/播放；Kotlin 待做
- **P4** DLNA / Docker / deb

## License

MIT
