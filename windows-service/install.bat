@echo off
setlocal

set SERVICE_NAME=RkvmService
set BASE_PATH=C:\ProgramData\rkvm
set SERVICE_PATH=%BASE_PATH%\rkvm-service.exe
set SOURCE_PATH=%~dp0

if not exist "%SOURCE_PATH%rkvm-service.exe" set SOURCE_PATH=%~dp0..\target\release\

fltmc >nul 2>&1
if errorlevel 1 (
    echo This installer must be run as Administrator.
    exit /b 1
)

if not exist "%SOURCE_PATH%rkvm-service.exe" (
    echo rkvm-service.exe was not found.
    exit /b 1
)
if not exist "%SOURCE_PATH%rkvm-client.exe" (
    echo rkvm-client.exe was not found.
    exit /b 1
)

sc.exe query "%SERVICE_NAME%" >nul 2>&1
if not errorlevel 1 (
    echo Stopping existing service...
    sc.exe stop "%SERVICE_NAME%" >nul 2>&1
    timeout /t 2 /nobreak >nul
    sc.exe delete "%SERVICE_NAME%" >nul
)

echo Copying release
if not exist "%BASE_PATH%" mkdir "%BASE_PATH%"
copy /y "%SOURCE_PATH%rkvm-service.exe" "%BASE_PATH%\" >nul
copy /y "%SOURCE_PATH%rkvm-client.exe" "%BASE_PATH%\" >nul

if exist "%~dp0client.toml" copy /y "%~dp0client.toml" "%BASE_PATH%\client.toml" >nul
if exist "%~dp0certificate.pem" copy /y "%~dp0certificate.pem" "%BASE_PATH%\certificate.pem" >nul

if not exist "%BASE_PATH%\client.toml" (
    echo Missing client.toml. Place it beside install.bat and run the installer again.
    exit /b 1
)
if not exist "%BASE_PATH%\certificate.pem" (
    echo Missing certificate.pem. Place it beside install.bat and run the installer again.
    exit /b 1
)

echo Installing service...
sc.exe create "%SERVICE_NAME%" binPath= "\"%SERVICE_PATH%\"" start= auto obj= LocalSystem
if errorlevel 1 exit /b %errorlevel%
sc.exe config "%SERVICE_NAME%" depend= RpcSs >nul
sc.exe sidtype "%SERVICE_NAME%" unrestricted >nul
sc.exe failure "%SERVICE_NAME%" reset= 86400 actions= restart/1000/restart/1500/restart/10000 >nul
sc.exe failureflag "%SERVICE_NAME%" 1 >nul

echo Starting service...
sc.exe start "%SERVICE_NAME%"

echo Done.
