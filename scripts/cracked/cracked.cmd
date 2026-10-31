@echo off
setlocal DisableDelayedExpansion
if not defined LOCALAPPDATA (
    echo ERROR: LOCALAPPDATA is not set. 1>&2
    exit /b 1
)
rem Always replace inherited stock routing, even inside an existing agent shell.
set "JCODE_HOME=%LOCALAPPDATA%\jcode-cracked\home"
set "JCODE_RUNTIME_DIR=%LOCALAPPDATA%\jcode-cracked\run"
set "JCODE_SOCKET=%JCODE_RUNTIME_DIR%\cracked.sock"
set "JCODE_INSTALL_DIR=%LOCALAPPDATA%\jcode-cracked\bin"
set "JCODE_SERVER_NAME=cracked"
set "JCODE_SERVER_DISPLAY_NAME=cracked"
set "JCODE_SESSION_ID="
set "JCODE_SERVER_PID="
set "JCODE_DEBUG_SOCKET="
rem Stock named-profile selections reference a config that this fork never reads.
rem Select fork profiles explicitly with --provider-profile instead.
set "JCODE_PROVIDER_PROFILE_NAME="
set "JCODE_PROVIDER_PROFILE_ACTIVE="
set "JCODE_NAMED_PROVIDER_PROFILE="
set "JCODE_RELOAD_AUTH_STATUS="
set "CRACKED_EXE=%LOCALAPPDATA%\jcode-cracked\bin\cracked.exe"
if /i "%~1"=="--dry-run" goto dryrun
powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "%~dp0seed.ps1"
if errorlevel 1 exit /b 1
if not exist "%CRACKED_EXE%" (
    echo ERROR: Fork executable not found: "%CRACKED_EXE%" 1>&2
    echo Run powershell -NoProfile -ExecutionPolicy Bypass -File scripts\cracked\build.ps1 1>&2
    exit /b 1
)
rem Explicit socket also prevents a duplicate --socket argument from rerouting silently.
"%CRACKED_EXE%" --no-update --socket "%JCODE_SOCKET%" %*
exit /b %errorlevel%

:dryrun
powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "%~dp0seed.ps1" -WhatIf
if errorlevel 1 exit /b 1
echo JCODE_HOME="%JCODE_HOME%"
echo JCODE_RUNTIME_DIR="%JCODE_RUNTIME_DIR%"
echo JCODE_SOCKET="%JCODE_SOCKET%"
echo JCODE_INSTALL_DIR="%JCODE_INSTALL_DIR%"
echo Would run: "%CRACKED_EXE%" --no-update --socket "%JCODE_SOCKET%"
exit /b 0
