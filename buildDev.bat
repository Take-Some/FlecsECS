@echo off
setlocal EnableExtensions
call "%~dp0..\build_all_plugins.cmd" FlecsECS dev
exit /b %ERRORLEVEL%
