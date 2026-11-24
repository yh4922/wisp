# Wisp 本机构建环境（Windows 开发机专用，CI 不使用）
# 用法：. .\script\wisp-env.ps1   然后 cargo build / cargo check
# 作用：
#   1. 使用 rust-toolchain.toml 要求的 Rust 1.98.1（本机由 vfox 安装，没有 rustup）
#   2. 加载 VS 的 vcvars64 环境，并把 VS 自带的 cmake / ninja 加入 PATH（tree-sitter wasm 依赖需要）
#   3. msvc_spectre_libs（pet 依赖）在未安装「Spectre 缓解库」组件时会 panic；
#      这里伪造一个带空 lib\spectre\x64 的 VC 目录绕过检查。安装该 VS 组件后可删除此段。

$rustRoot = if ($env:WISP_RUST_ROOT) { $env:WISP_RUST_ROOT } else { 'D:\Libs\vfox\rust\v-1.98.1\rust-1.98.1' }
$vs = if ($env:WISP_VS_ROOT) { $env:WISP_VS_ROOT } else {
  (& "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe" -products * -prerelease -latest -property installationPath)
}
if (-not $vs) { $vs = 'C:\Program Files\Microsoft Visual Studio\18\Insiders' }

cmd /c "`"$vs\VC\Auxiliary\Build\vcvars64.bat`" >nul && set" | ForEach-Object {
  if ($_ -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($Matches[1], $Matches[2]) }
}

$msvcVer = (Get-ChildItem "$vs\VC\Tools\MSVC" -Directory | Sort-Object Name -Descending | Select-Object -First 1).Name
$msvcBin = "$vs\VC\Tools\MSVC\$msvcVer\bin\Hostx64\x64"
$pathPrefix = @("$rustRoot\rustc\bin")

if (-not (Test-Path "$vs\VC\Tools\MSVC\$msvcVer\lib\spectre\x64")) {
  $fake = Join-Path $env:TEMP 'wisp_fake_vc'
  if (-not (Test-Path "$fake\bin\Hostx64\x64")) {
    New-Item -ItemType Directory -Force "$fake\bin\Hostx64" | Out-Null
    New-Item -ItemType Junction -Path "$fake\bin\Hostx64\x64" -Target $msvcBin | Out-Null
  }
  New-Item -ItemType Directory -Force "$fake\lib\spectre\x64" | Out-Null
  $pathPrefix += "$fake\bin\Hostx64\x64"
}

$pathPrefix += "$vs\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin"
$pathPrefix += "$vs\Common7\IDE\CommonExtensions\Microsoft\CMake\Ninja"
$env:PATH = ($pathPrefix -join ';') + ';' + $env:PATH
$env:RUSTUP_TOOLCHAIN = $null
# 增量编译缓存在本项目可涨到 80GB+，本机磁盘有限，默认关闭（需要时手动设回 1）
if (-not $env:WISP_INCREMENTAL) { $env:CARGO_INCREMENTAL = '0' }

Write-Host "Wisp env: $(rustc --version) | MSVC $msvcVer"
