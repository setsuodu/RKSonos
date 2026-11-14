# RKSonos HTTP API

Base URL：`http://<host>:8080`（`BIND` 可改端口）。

## 推荐调用顺序

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

## 播放控制

| Method | Path | Body | Description |
|--------|------|------|-------------|
| GET | `/health` | — | liveness，返回纯文本 `ok` |
| GET | `/api/v1/status` | — | state / path / volume / queue / mode |
| POST | `/api/v1/play` | `{"path":"/mnt/nas/a.mp3"}` | 单曲播放（队列=1） |
| POST | `/api/v1/pause` | — | 暂停 |
| POST | `/api/v1/resume` | — | 继续（不从头） |
| POST | `/api/v1/stop` | — | 停止 |
| POST | `/api/v1/volume` | `{"volume":80}` | 0–100 |
| POST | `/api/v1/next` | — | 下一首（受 mode 影响） |
| POST | `/api/v1/prev` | — | 上一首 |
| POST | `/api/v1/mode` | `{"mode":"repeat_all"}` | `normal` / `repeat_one` / `repeat_all` / `shuffle` |

## 队列

| Method | Path | Body | Description |
|--------|------|------|-------------|
| GET | `/api/v1/queue` | — | 当前队列 + index |
| POST | `/api/v1/queue` | `{"paths":["..."],"start":0}` | 整表替换并播放 |
| POST | `/api/v1/queue/add` | `{"paths":["..."]}` | 追加 |
| POST | `/api/v1/queue/remove` | `{"index":3}` | 按索引删除 |
| POST | `/api/v1/queue/clear` | — | 清空 |

## 曲库

| Method | Path | Body | Description |
|--------|------|------|-------------|
| POST | `/api/v1/library/scan` | — | 全量扫描 `MUSIC_ROOT` |
| GET | `/api/v1/library/status` | — | 扫描状态 / 曲目数 |
| GET | `/api/v1/library?q=&limit=&offset=` | — | 列表 / 搜索 |
| POST | `/api/v1/library/play` | `{"path":"..."}` 或 `{"id":42}` | **整库进队列并从指定曲开始播** |

## 板上 curl 示例

```bash
curl -s http://127.0.0.1:8080/health
curl -s -X POST http://127.0.0.1:8080/api/v1/library/scan | jq
curl -s -X POST http://127.0.0.1:8080/api/v1/library/play \
  -H 'Content-Type: application/json' \
  -d '{"path":"/mnt/nas/hero_yoshiki.mp3"}' | jq
curl -s -X POST http://127.0.0.1:8080/api/v1/mode \
  -H 'Content-Type: application/json' \
  -d '{"mode":"repeat_all"}' | jq
curl -s -X POST http://127.0.0.1:8080/api/v1/next | jq
curl -s -X POST http://127.0.0.1:8080/api/v1/pause | jq
curl -s -X POST http://127.0.0.1:8080/api/v1/resume | jq
```
