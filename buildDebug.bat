@echo off
setlocal EnableExtensions
call "%~dp0..\build_all_plugins.cmd" FlecsECS debug
exit /b %ERRORLEVEL%
