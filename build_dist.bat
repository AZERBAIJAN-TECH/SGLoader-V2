@echo off
setlocal EnableExtensions


set "ROOT=%~dp0"
cd /d "%ROOT%" || exit /b 1

set "OUT_ZIP=%ROOT%SGLoader-V2.zip"

if exist "%OUT_ZIP%" (
  set "N=1"
  :find_free_zip
  set "OUT_ZIP=%ROOT%SGLoader-V2_%N%.zip"
  if exist "%OUT_ZIP%" (
    set /a N+=1
    goto :find_free_zip
  )
)

set "DIST_ROOT=%ROOT%dist\SGLoader-V2"
set "BIN_DIR=%DIST_ROOT%\bin"
set "DEPS_DIR=%DIST_ROOT%\dependencies"
set "LOADER_DIR=%DEPS_DIR%\loader\win-x64"
set "DOTNET_DIR=%DEPS_DIR%\dotnet"

echo [1/6] Cleaning dist...
if exist "%ROOT%dist" rmdir /s /q "%ROOT%dist"

echo [2/6] Ensuring submodules...
git submodule update --init --recursive third_party\SGLoader-Rewrite
if errorlevel 1 exit /b 1

echo [3/6] Building Rust (release)...
cargo build --release
if errorlevel 1 exit /b 1

echo [4/6] Staging SGLoader-V2.exe...
mkdir "%BIN_DIR%" 1>nul 2>nul
mkdir "%DEPS_DIR%" 1>nul 2>nul
copy /y "%ROOT%target\release\SGLoader-V2.exe" "%DIST_ROOT%\SGLoader-V2.exe" 1>nul
if errorlevel 1 exit /b 1

if exist "%ROOT%target\release\SGLoader_V2.pdb" (
  copy /y "%ROOT%target\release\SGLoader_V2.pdb" "%BIN_DIR%\SGLoader_V2.pdb" 1>nul
)

echo [5/6] Publishing SS14.Loader (self-contained, win-x64)...
mkdir "%LOADER_DIR%" 1>nul 2>nul
dotnet publish "%ROOT%third_party\SGLoader-Rewrite\SS14.Loader\SS14.Loader.csproj" -c Release -r win-x64 --self-contained true -o "%LOADER_DIR%" /nologo
if errorlevel 1 exit /b 1

copy /y "%ROOT%third_party\SGLoader-Rewrite\SS14.Launcher\signing_key" "%LOADER_DIR%\signing_key" 1>nul
if errorlevel 1 exit /b 1

echo [6/6] Downloading .NET runtime and zipping...
set "DOTNET_VER=10.0.0"
powershell -NoProfile -ExecutionPolicy Bypass -Command "$ErrorActionPreference='Stop'; $v='%DOTNET_VER%'; $url=('https://dotnetcli.azureedge.net/dotnet/Runtime/{0}/dotnet-runtime-{0}-win-x64.zip' -f $v); $tmp=Join-Path '%ROOT%dist' 'dotnet-runtime.zip'; Invoke-WebRequest -Uri $url -OutFile $tmp; New-Item -ItemType Directory -Force -Path '%DOTNET_DIR%' | Out-Null; Expand-Archive -Path $tmp -DestinationPath '%DOTNET_DIR%' -Force; Remove-Item $tmp -Force; Compress-Archive -Path '%DIST_ROOT%\*' -DestinationPath '%OUT_ZIP%' -Force"
if errorlevel 1 exit /b 1

echo Done: %OUT_ZIP%
exit /b 0
