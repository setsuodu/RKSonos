# Mounts API (gio smb://, multi-root)

板子默认 **无曲库路径**。客户端通过 API 下发 SMB 挂载并持久化到 `data/config.json`。

依赖（板上）：
```bash
apt install -y gvfs-backends gvfs-fuse libglib2.0-bin
# 确保 gio 可用：which gio
```

## 端点

| Method | Path | Body | 说明 |
|--------|------|------|------|
| GET | `/api/v1/mounts` | — | 已配置挂载列表（不含密码） |
| GET | `/api/v1/mounts/active` | — | 当前 gio 已挂载的 SMB |
| POST | `/api/v1/mounts` | 见下 | 添加并挂载，写入 config |
| DELETE | `/api/v1/mounts/:id` | — | 卸载并删除配置 |
| POST | `/api/v1/mounts/:id/remount` | — | 按已存凭据重新挂载 |

### POST /api/v1/mounts

```json
{
  "uri": "smb://192.168.1.8/music",
  "label": "群晖音乐",
  "id": "nas1",
  "username": "user",
  "password": "secret",
  "mount_now": true
}
```

- `uri` 必填，支持 `smb://user@host/share` 或 `smb://host/share` + username
- `password` 会写入 `config.json`（仅内网板子场景）；响应中永不回传
- 挂载成功后 `mount_point` 为 GVFS 路径，例如 `/run/user/0/gvfs/smb-share:...`

## 推荐流程

```bash
# 1. 加挂载（可多次，多目录）
curl -s -X POST http://板子:8080/api/v1/mounts \
  -H 'Content-Type: application/json' \
  -d '{"uri":"smb://192.168.1.8/music","username":"u","password":"p","label":"NAS"}'

# 2. 扫库（扫所有已挂载 root）
curl -s -X POST http://板子:8080/api/v1/library/scan

# 3. 播放
curl -s -X POST http://板子:8080/api/v1/library/play \
  -H 'Content-Type: application/json' \
  -d '{"path":"/run/user/0/gvfs/.../song.mp3"}'
```

## 持久化

| 文件 | 内容 |
|------|------|
| `STATE_FILE` 默认 `data/state.json` | 队列 / 音量 / 模式 |
| `CONFIG_FILE` 默认 `data/config.json` | mounts 列表（含可选密码） |

启动时自动对 `enabled` 的 mount 执行 `gio mount` 重挂。

## 环境变量（仅硬件/进程）

| Var | Default | 说明 |
|-----|---------|------|
| AUDIO_DEVICE | plughw:2,0 | ALSA |
| BIND | 0.0.0.0:8080 | 监听 |
| STATE_FILE | data/state.json | 播放状态 |
| CONFIG_FILE | data/config.json | 挂载配置 |
| RUST_LOG | info | 日志 |

**不再依赖 `MUSIC_ROOT`。**
