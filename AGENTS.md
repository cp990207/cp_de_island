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
