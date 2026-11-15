# RKSonos Android Client (MVP)

对齐 WPF 客户端的最小可用版：

- **发现页**：局域网 `:8080/health` 扫描 + 手动地址（空输入 + 灰斜 placeholder）
- **播放页**：正在播放 / 进度 seek / 模式·上一首·播放·下一首 / 音量
- **音乐库**：点曲目 → 整表进队列并从该曲开始
- **播放列表**：当前曲高亮，显示「第 N / M 首」
- **音响设置**：改地址、切换设备；关于含服务器版本（若 status 有）

## 要求

- Android Studio Ladybug+ / AGP 8.7
- minSdk 26，JDK 17
- 手机与音箱同一局域网；允许 HTTP 明文（已配置）

## 构建

```bash
cd android
# Android Studio 打开本目录，或：
./gradlew :app:assembleDebug
```

安装：

```bash
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

## 说明

- 未做 mDNS（Android 上 NSD 不稳定且权限复杂）；主路径与 WPF 优化后一致：TCP 探活 + `/health`
- 未做挂载管理 / 多语言；后续可补
