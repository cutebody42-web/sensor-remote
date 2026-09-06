@echo off
setlocal
if "%SENSOR_SERVER%"=="" (
  set "SENSOR_SERVER=https://sensor-rendezvous-production.up.railway.app"
)
if "%SENSOR_SERVER%"=="" (
  echo An Internet rendezvous service URL is required.
  exit /b 2
)
set "SENSOR_MODE=RENDER_TEST"
echo Starting native SENSOR Remote with Internet WSS transport...
echo Service: %SENSOR_SERVER%
start "SENSOR Remote Access" "%~dp0SENSOR-Remote.exe"
endlocal
