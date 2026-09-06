@echo off
setlocal
if "%SENSOR_SERVER%"=="" (
  set /p "SENSOR_SERVER=Render HTTPS service URL (for example https://sensor-rendezvous.onrender.com): "
)
if "%SENSOR_SERVER%"=="" (
  echo A Render service URL is required.
  exit /b 2
)
set "SENSOR_MODE=RENDER_TEST"
echo Starting native SENSOR Remote with Internet WSS transport...
start "SENSOR Remote Access" "%~dp0SENSOR-Remote.exe"
endlocal
