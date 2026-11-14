# mDNS / DNS-SD

P3 设备发现约定。

## 服务类型

| 项 | 值 |
|----|-----|
| Service type | `_rksonos._tcp.local.` |
| 传输 | TCP |
| 默认端口 | 与 HTTP API 相同（`BIND`，默认 `8080`） |
| TXT | `path=/` · `proto=http` · `ver=<server version>` |

实例名默认取主机名（`/etc/hostname` 或 `HOSTNAME`），可用环境变量覆盖：

| Env | 含义 |
|-----|------|
| `MDNS=0` / `false` / `off` | 关闭广播 |
| `MDNS_NAME` | 实例显示名（及 host label 来源） |

## Server

启动时用 `mdns-sd` 注册服务，进程存活期间持续应答查询。  
实现见 `server/src/mdns.rs`。

验证（Linux 同网段）：

```bash
avahi-browse -r _rksonos._tcp
# 或
dns-sd -B _rksonos._tcp
```

## 客户端

WPF 启动页 `DiscoveryWindow`：

1. 先 mDNS 浏览 `_rksonos._tcp.local.`
2. 若无结果，再对本机网段做 `/health` 探测（fallback）
3. **未选中/未连上设备前不打开播放页**（`MainWindow`）
4. 支持手动输入 `http://IP:8080`

## 防火墙

需允许：

- UDP `5353`（mDNS）
- TCP `8080`（或你的 `BIND` 端口）
