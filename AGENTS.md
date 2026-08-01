# 开发经验记录

## Windows + Rust (windows-gnu) 工具链编译问题

### 1. libshlwapi.a 缺失（链接失败：cannot find -lshlwapi）
rustup 自带的 self-contained 目录不完整，需要从 MinGW 安装目录补充：
```powershell
Copy-Item "C:\mingw64\x86_64-w64-mingw32\lib\libshlwapi.a" -Destination "$env:USERPROFILE\.rustup\toolchains\stable-x86_64-pc-windows-gnu\lib\rustlib\x86_64-pc-windows-gnu\lib\self-contained\libshlwapi.a"
```

### 2. dlltool.exe 找不到
GNU 工具链编译时需要 MinGW 的 binutils，运行前把 MinGW 加入 PATH：
```powershell
$env:PATH = "C:\mingw64\bin;$env:PATH"
cargo build
```

### 3. WebView2Loader.dll 找不到（Dioxus 桌面应用运行时）
编译产物不会自动包含 WebView2Loader.dll，需手动复制到 exe 同目录：
```powershell
Copy-Item "target\debug\build\webview2-com-sys-*\out\x64\WebView2Loader.dll" -Destination "target\debug\WebView2Loader.dll"
```

## 岛（island）动画约定

### 两侧尺寸动画只能用 flex-grow / flex-basis，禁止 flex:1 ↔ width 切换
`.side-left` / `.side-right` 的宽度变化必须始终通过 `flex-grow` + `flex-basis`（数值↔数值可插值）表达。
若在 `flex: 1`（flex 驱动）与 `width: 48px`（长度）之间切换，CSS transition 无法插值，会导致左右互切时一侧瞬缩、一侧瞬开。
折叠侧固定 `flex-grow: 0; flex-basis: 48px`，展开侧 `flex-grow: 1; flex-basis: 0px`；过渡速率统一走 `--motion-grow` + `--spring-gentle`，与圆球↔岛展开一致。
