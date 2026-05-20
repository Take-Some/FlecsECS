@echo off
setlocal EnableExtensions EnableDelayedExpansion
cd /d "%~dp0"

echo.
echo [%~nx0] NewEngine FlecsECS plugin build profile
 echo 1^) release
 echo 2^) dev
 echo 3^) test
 echo 4^) bench
 echo.
set /p choice=Select profile [1-4]: 

if "%choice%"=="1" goto release
if "%choice%"=="2" goto dev
if "%choice%"=="3" goto test
if "%choice%"=="4" goto bench

echo Invalid choice: %choice%
exit /b 2

:release
set NEWENGINE_BUILD_PROFILE=release
set NEWENGINE_PLUGIN_PROFILE=release
if exist buildRelease.bat (
  call buildRelease.bat
) else (
  cargo build --release
)
exit /b %ERRORLEVEL%

:dev
set NEWENGINE_BUILD_PROFILE=dev
set NEWENGINE_PLUGIN_PROFILE=dev
if exist buildDev.bat (
  call buildDev.bat
) else (
  cargo build
)
exit /b %ERRORLEVEL%

:test
set NEWENGINE_BUILD_PROFILE=test
set NEWENGINE_PLUGIN_PROFILE=test
cargo test --workspace
exit /b %ERRORLEVEL%

:bench
set NEWENGINE_BUILD_PROFILE=bench
set NEWENGINE_PLUGIN_PROFILE=bench
cargo bench --workspace
exit /b %ERRORLEVEL%
