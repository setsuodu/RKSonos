# RKSonos 开发计划

目标：在 RK3566（Ubuntu Server）上实现 Sonos 风格 WiFi 音箱。  
全程 WiFi，支持局域网 NAS 音乐、后续手机/电脑 App、DLNA 投送。

硬件基线：
- RK3566 / 2G RAM / 16G ROM
- USB 喇叭 → ALSA `plughw:2,0`
- 厂商内核 5.10.226（`CONFIG_CIFS` 未开启，NAS 暂用 NFS）
- 同一套 Server 代码支持：原生二进制 / Docker / 未来 Debian 包

技术栈：
| 端 | 技术 |
|----|------|
| Server | Rust + Axum + mpg123 / mpv |
| Android | Kotlin（后续） |
| Windows | WPF |

---

## P0 — 环境验证（已完成）

**目标**：确认板上能稳定出声，并能播放局域网 NAS 里的文件。

- [x] `aplay -l` 确认 USB 喇叭为 card 2 → `plughw:2,0`
- [x] `mpg123 -a plughw:2,0 /root/hero_yoshiki.mp3` 本地播放成功
- [x] 安装 `nfs-common`；群晖开启 NFSv3
- [x] `mount -t nfs -o vers=3 192.168.1.8:/volume1/music /mnt/nas`
- [x] `mpg123 -a plughw:2,0 /mnt/nas/hero_yoshiki.mp3` NAS 播放成功
- [ ] （可选）`/etc/fstab` 开机自动挂载 NFS
- [ ] （可选）向厂家要开启 `CONFIG_CIFS` 的内核/镜像

**结论**：本地 + NAS 播放通路打通，可进入 Server。

---

## P1 — Server MVP（已完成）

**目标**：板上跑一个最小可远程控制的播放服务，API 可测。

功能：
- REST API：`/health`、`/api/v1/status`、`/api/v1/play`、`/pause`、`/stop`、`/volume`
- 用 `mpg123` 子进程播指定绝对路径（本地或 `/mnt/nas/...`）
- 配置全部走环境变量（`AUDIO_DEVICE`、`BIND`），便于原生 / Docker / deb 同一二进制
- GitHub Actions：交叉编译 aarch64，上传 artifact / Release

---

## P2 — 播放器完善 + 曲库（已完成）

**目标**：从「能播一个文件」变成「能当音箱用」。

- [x] 播放队列（增删、下一首/上一首）
- [x] 循环 / 随机（`normal` / `repeat_one` / `repeat_all` / `shuffle`）
- [x] 真正的 pause/resume（mpg123 `-R` 远程控制）
- [x] 音量写入 mpg123 + 尝试 amixer
- [x] 曲库扫描：`MUSIC_ROOT` 下扫 mp3/flac 等，内存索引
- [x] API：列表、搜索、`library/play`（整库进队列）、队列操作
- [x] 状态持久化（队列、当前索引、音量、模式 → `STATE_FILE`）

交付物：Server 仍为单一二进制；API 文档见 [api.md](./api.md)。

**推荐调用顺序**见 [api.md](./api.md)。

---

## P3 — 发现与客户端

**目标**：手机/电脑能发现设备并控制。

- [x] mDNS / DNS-SD 广播（`_rksonos._tcp`），见 [mdns.md](./mdns.md)
- [x] Windows 客户端（WPF）：启动先发现设备，找不到不进播放页；曲库/队列/遥控
- [ ] Android App（Kotlin）：发现设备、浏览曲库、点播、简单遥控
- [ ] 基础鉴权（可选 token / 仅局域网）

仍不强制 DLNA；先自有协议跑通。

---

## P4 — DLNA / 高级与部署

**目标**：兼容投屏生态，并具备可分发形态。

- DLNA/UPnP MediaRenderer：手机/电脑可「投送到 RKSonos」；投送后源可关
- （可选）MediaServer：把已扫曲库暴露给其他 DLNA 客户端
- Docker 镜像（`--device /dev/snd` + volume 挂 NAS）
- Debian 包（systemd unit、默认 env）
- 多设备/分组播放（若有精力）
- 厂商内核补 CIFS 或用户态 SMB 方案落地

---

## 部署原则（全程）

1. **一套代码**：行为由环境变量 / 挂载决定，不写死路径与设备号  
2. **CI**：按平台 tag 发版（`server/v*`、`wpf/v*`、`android/v*`），产物可直接下载  
3. **先板后云**：所有播放逻辑以板上 ALSA 为准，Docker 仅作打包与未来硬件复用  
4. **P0 不通不写业务**：音频与 NAS 通路必须先绿  

## 当前状态

| 阶段 | 状态 |
|------|------|
| P0 环境验证 | ✅ 完成 |
| P1 Server MVP | ✅ 完成 |
| P2 队列/曲库 | ✅ 完成 |
| P3 发现 + WPF | ✅ mDNS / WPF 完成；Kotlin / 鉴权待做 |
| P4 DLNA/打包 | 未开始 |
