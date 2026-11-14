# RKSonos WPF Client (P3)

## 行为

- **启动页 = 找设备**：`DiscoveryWindow`
  - mDNS 浏览 `_rksonos._tcp.local.`
  - 无结果时扫描本机网段 `:8080/health`
  - 列表为空且未手动连接成功时 **不打开** `MainWindow`
- 选中设备或手动连接且 `GET /health == ok` 后进入播放页
- 播放页可「切换设备」回到找设备页

## 构建

需要 .NET 10 Windows Desktop：

```bash
cd wpf/RkSonos.Client
dotnet restore
dotnet run -c Release
```

依赖：NuGet `Zeroconf`（mDNS 浏览）。

## 相关文档

- [docs/mdns.md](../docs/mdns.md)
- [docs/api.md](../docs/api.md)
