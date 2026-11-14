# RKSonos WPF Client (P3)

Windows 桌面客户端，对接板上 `rksonos-server` REST API。

## 功能

- 手动填写服务器地址连接（`http://板子IP:8080`）
- 状态轮询（约 2s）：正在播放、状态、队列位置、模式
- 播放控制：播放/暂停/继续、停止、上一首/下一首
- 音量滑条、播放模式（normal / repeat_one / repeat_all / shuffle）
- 曲库：扫库、搜索、列表；双击或「整库从这首播」调用 `library/play`
- 单曲试播（`/api/v1/play`）、加入队列
- 队列：查看、删除选中、清空

> mDNS（`_rksonos._tcp`）需 Server 侧先实现广播；本客户端当前以手动 IP 为主，发现能力可在 P3 后半补。

## 环境

- Windows 10/11
- [.NET 8 SDK](https://dotnet.microsoft.com/download/dotnet/8.0)（含 Windows Desktop workload）

## 构建与运行

```bash
cd clients/wpf/RkSonos.Client
dotnet restore
dotnet build -c Release
dotnet run -c Release
```

或 Visual Studio / Rider 打开 `RkSonos.Client.csproj` 直接 F5。

## 使用顺序（与 Server README 一致）

1. 板上启动 server（`BIND=0.0.0.0:8080`）
2. 本机客户端填 `http://板子IP:8080` → 连接
3. 点「扫库」
4. 选一首 → 「整库从这首播」或双击
5. 需要时改模式 / 下一首 / 音量

## 目录

```
clients/wpf/
  README.md
  RkSonos.Client/
    RkSonos.Client.csproj
    App.xaml / App.xaml.cs
    MainWindow.xaml / MainWindow.xaml.cs
    Models/ApiModels.cs
    Services/RkSonosApi.cs
```

## 说明

- 无第三方 UI 库，纯 WPF + `HttpClient`
- 未 push 到 GitHub；按你的要求仅本地交付 zip
- License：与仓库一致 MIT
