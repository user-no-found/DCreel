# DCreel

DCreel 是一个面向 Windows 的本地桌面整理工具，使用 Rust、Tauri 2、React 和 TypeScript 构建。桌面盒子本身对应 Windows 桌面中的真实文件夹，也可以把桌面之外的已有目录连接成“外部文件夹入口”。

*DCreel*，桌面鱼篓，“鱼”是摸鱼的“鱼”。
没做完的需求文档、永远改不完的表格、不敢点开的会议纪要，暂时不想面对的“不开心”，都藏起来。
游戏图标、视频站快捷方式、闲聊窗口，以及摸鱼时留下的证据，不太方便见光的“快乐”，都藏起来。

## 原生 Desktop Host 架构

Windows 版启动时会直接定位同目录或资源目录中的 `creel-desktop-host.exe`，不再使用实验环境变量，也不存在 WebView 盒子回退路径。`npm run tauri dev` 的前置命令会先构建 Debug 原生组件；安装包构建则会先构建 Release Host 与 Shell DLL。

自动化测试需要与日常布局完全隔离时，可以额外设置 `CREEL_STATE_DIR` 指向专用目录；未设置时仍只使用 Windows 标准应用配置目录。这个变量只改变当前模型状态的读写位置，也不会由设置页写入：

```cmd
set CREEL_STATE_DIR=C:\temp\creel-native-test
src-tauri\target\debug\dcreel.exe --silent
```

主程序启动一个 `creel-desktop-host.exe --ipc-stdio`，并将盒子及设置的完整快照同步给它。协议当前包含：

1. `Hello` / `Ready`：核对协议版本。
2. `Sync` / `Synced`：按 revision 同步全部盒子、全局偏好和统一显隐状态。
3. `GeometryChanged`：原生移动或缩放结束后，将盒子 ID、位置和尺寸回传给主程序持久化。
4. `ImportFiles`：Host 接收 OLE 文件拖入后上报盒子 ID 与路径；Tauri 复用同名避让、跨卷复制后删除和目录自包含检查等安全移动语义。
5. `UserAction`：原生盒子菜单上报新建、重命名、收起、锁定、换色和移除操作，主程序复用现有命令和持久化路径执行。
6. `Notification`：原生文件操作失败时转发到 Tauri 通知通道，不会被当成协议故障而重启 Host。
7. `Shutdown` / `Stopped`：主程序退出时请求宿主干净退出；stdin 意外关闭也会触发退出。

当前 Host 覆盖进程隔离、窗口生命周期、批量状态同步、GDI 渲染、桌面 Z 序、几何交互与持久化、设置同步、真实 Shell 图标与媒体缩略图、鼠标与键盘多选、批量 Explorer 菜单、双向 OLE 文件拖放、滚动浏览，以及盒子完整管理菜单。拖动期间收到 watchdog 的旧状态快照时，Host 会保留实时窗口矩形，避免盒子跳回旧位置；松开鼠标并保存后，后续快照会以新位置为准。

只验证 Host IPC 而不启动 Tauri，可在 Windows CMD 中运行端到端探针。探针会依次创建盒子、真实驱动一次移动和缩放、核对两次 `GeometryChanged`、执行单击/Ctrl 多选、F5、方向键、Escape 和反向拖框，通过键盘菜单键与鼠标分别创建并取消多选 Explorer 菜单，再创建并取消盒子菜单、更新并收起盒子、删除盒子、关闭宿主，并检查每个 revision 的确认事件；探针不会打开、新建、改名或删除项目，结束时会恢复鼠标位置：

```cmd
cargo build --manifest-path src-tauri\Cargo.toml -p creel-desktop-host --bins
src-tauri\target\debug\creel-desktop-host-probe.exe src-tauri\target\debug\creel-desktop-host.exe C:\desktop\Creel
```

## 桌面层实现

DCreel 没有把原生盒子窗口设为 `Progman` 或 `WorkerW` 的子窗口。当前实现采用可交互的独立顶层工具窗口，并执行以下约束：

1. 枚举 `Progman`，找不到时退回第一个 `WorkerW`。
2. 用 `GW_HWNDPREV` 找到宿主正上方的 Z 序位置，再依次插入所有 DCreel 盒子。
3. 使用 `SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE`，只校正层级，不打断正在进行的移动或缩放。
4. Explorer 重启、Win+D 或其他程序改变层级后，看门狗会重新枚举宿主并恢复位置。
5. 盒子几何由 Host 统一回传并持久化，支持负坐标的多显示器布局。

可以在应用运行时检查真实 HWND、窗口矩形、扩展样式和 Z 序：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\inspect-windows.ps1
```

正常结果中，所有盒子应该连续出现在 `Progman` 的前一组位置，扩展样式包含 `0x80`（`WS_EX_TOOLWINDOW`）且不包含 `0x40000`（`WS_EX_APPWINDOW`）。

## 安全语义

DCreel 对磁盘操作采用保守约定：

- “移除盒子”只删除 `state.json` 中的显示条目，永远不删除盒子目录和其中的文件。
- 导入或快速整理遇到同名文件时使用 `name (2).ext`、`name (3).ext` 等新名称，不覆盖现有内容。
- 快速整理只扫描 Windows 用户桌面中的普通文件，忽略文件夹、`desktop.ini` 和点开头的隐藏项。
- 快速整理必须先展示预览，再由用户点击“确认整理”；应用启动时不会自动搬动桌面文件。
- 文件跨磁盘卷移动时会先复制，再在复制成功后删除源文件。

## 开发环境

建议在 Windows 10/11 上准备：

- Node.js 20 或更高版本（当前工程已在 Node.js 24 验证）。
- 可正常链接 Windows MSVC 目标的 Rust stable 和 Cargo（当前机器已验证）。
- WebView2 Runtime（Windows 11 通常已经内置）。

工程没有 C++ 源码，也不调用 C++ 编译器。Win32、COM、Shell Extension 和 Desktop Host 都通过 Rust `windows` crate 直接使用 Windows ABI。

安装依赖并运行浏览器 UI 预览：

```bash
npm install
npm run dev
```

运行完整 Tauri 应用：

```bash
npm run tauri dev
```

只构建可直接运行的生产版 EXE，不生成 NSIS/MSI 安装包：

```bash
npm run build:release
```

生成结果为 `src-tauri/target/release/dcreel.exe`。主程序必须通过这个 Tauri
生产构建命令生成；直接执行 `cargo build --release` 不会设置 Tauri 的生产环境，
可能将 `devUrl` 编进程序并在启动时访问 `localhost`。

在 Windows PowerShell 或 CMD 中构建安装包：

```bash
npm run tauri build
```

生成结果位于 `src-tauri/target/release/bundle/`。`tauri.conf.json` 已启用 NSIS/MSI 等 Tauri 默认桌面目标，并为 WiX 配置中文与英文语言。

## 常用检查

```bash
# TypeScript 类型检查 + Vite 生产构建
npm run build

# Rust 编译检查
cargo check --workspace --manifest-path src-tauri/Cargo.toml

# Rust 单元测试
cargo test --workspace --manifest-path src-tauri/Cargo.toml

# 单独构建原生 Shell DLL 与 Desktop Host
npm run build:native

# 验证 COM DLL 的类工厂、命令对象和可卸载状态
cargo run --manifest-path src-tauri/Cargo.toml -p creel-shell \
  --bin creel-shell-probe -- src-tauri/target/debug/creel_shell.dll

# Windows CMD：验证单进程 Desktop Host 的握手、三次同步与退出
cargo build --manifest-path src-tauri\Cargo.toml -p creel-desktop-host --bins
src-tauri\target\debug\creel-desktop-host-probe.exe ^
  src-tauri\target\debug\creel-desktop-host.exe C:\desktop\Creel
```

如果在 WSL 中开发、Rust 只安装在 Windows，可以调用 Windows 工具链：

```bash
cmd.exe /C "cd /d C:\desktop\Creel && cargo check --manifest-path src-tauri\Cargo.toml"
```

## 图标处理

原始图保留在仓库根目录的 `Creel .png`，脚本不会覆盖它。运行：

```bash
npm run icons
```

脚本会执行两步：

1. 从画布边缘做暗色区域连通填充，只把与外边缘相连的黑色背景变为透明；插画内部的黑色线稿不会按颜色全局删除。
2. 调用 Tauri CLI 生成 Windows ICO、macOS ICNS、通用 PNG、Windows Store、iOS 和 Android 尺寸。

主要输出：

- `src-tauri/icons/creel-icon.png`：1024 × 1024 透明主图。
- `src-tauri/icons/icon.ico`：Windows 应用图标。
- `src-tauri/icons/icon.icns`：macOS 图标。
- `public/creel-icon.png`：前端界面使用的 256 × 256 图标。

## 本地状态位置

Tauri 通过系统应用配置目录保存数据。Windows 默认位置为：

```text
%APPDATA%\com.creel.desktop\
  state.json
```

这里只保存设置和盒子布局，不再保存任何“收纳目录”。`desktop_folder` 指向系统真实桌面中的一级文件夹；`portal` 只记录用户选择的目标路径，不复制外部文件夹内容。设置页不展示配置路径入口。

## 工程结构

```text
src/
  App.tsx              主控制台、盒子管理、快速整理与设置流程
  styles.css           米白纸张与手绘气质的完整视觉样式
  lib/bridge.ts        Tauri 管理命令桥接和隔离的浏览器演示数据
  lib/format.ts        文件大小与扩展名显示工具
  types.ts             前端数据模型

src-tauri/
  src/lib.rs           Tauri 启动、插件、托盘与命令注册
  src/desktop_host.rs  原生宿主进程控制、协议握手、快照同步、超时重启与退出清理
  src/desktop_windows.rs  Desktop Host 状态同步、显隐和桌面层看门狗
  src/directory_watchers.rs  本地目录实时监听和前端事件通知
  src/desktop_context_menu.rs  当前用户级 Windows 桌面右键菜单注册与清理
  src/commands.rs      文件导入/打开/定位、桌面整理、快速新建与配置命令
  src/store.rs         状态持久化、目录读取、首启数据和路径安全规则
  src/models.rs        Rust 序列化数据模型
  capabilities/        Tauri 2 主窗口、事件和文件夹对话框最小权限
  icons/               多平台应用图标
  crates/creel-ipc/    外部命令及 Desktop Host 共享的版本化 JSON Lines 协议
  crates/creel-shell/  纯 Rust cdylib：Explorer IExplorerCommand、类工厂和 COM 探针
  crates/creel-desktop-host/  进程外纯 Rust Win32 桌面渲染宿主、IPC 多窗口模式与探针

scripts/
  make-icons.mjs       原图黑边透明化与图标主资源生成
  inspect-windows.ps1  只读的 DCreel HWND、样式、矩形与 Z 序诊断
```
