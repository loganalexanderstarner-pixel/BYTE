@echo off
REM Run a command on the Windows dev box with a deterministic environment.
REM
REM   scripts\windows-dev.bat <tag> <command...>
REM
REM Why this exists, rather than invoking commands directly over SSH:
REM
REM  1. PATH. An SSH session's inherited PATH is inconsistent between sessions
REM     on this machine -- one resolved perl and cargo, the next found neither,
REM     which cost several rounds of debugging a missing-tool error that was
REM     really an environment error. Absolute paths remove the variable.
REM
REM  2. Logs. Two background builds writing to the same log file produced stale
REM     output that looked like a failure already fixed. <tag> namespaces them.
REM
REM  3. Quoting. Every call otherwise re-solves bash -> ssh -> PowerShell -> cmd
REM     quoting, which is its own source of mistakes.
REM
REM A GUI application still cannot be started this way: an SSH session is
REM session 0 with no desktop, so WebView2 fails with "Invalid window handle".
REM Use schtasks /IT to reach the interactive session.

setlocal
set PATH=C:\Strawberry\c\bin;C:\Strawberry\perl\site\bin;C:\Strawberry\perl\bin;C:\Users\%USERNAME%\.cargo\bin;C:\Program Files\nodejs;C:\Program Files\CMake\bin;C:\Program Files\Git\cmd;C:\Program Files\Git\bin;C:\Windows\System32;C:\Windows;C:\Windows\System32\Wbem;%PATH%

if "%~1"=="" (
  echo usage: windows-dev.bat ^<tag^> ^<command...^>
  exit /b 2
)
set TAG=%~1
shift

set LOGDIR=C:\dev\logs
if not exist "%LOGDIR%" mkdir "%LOGDIR%"
for /f "tokens=1-4 delims=:." %%a in ("%TIME%") do set STAMP=%%a%%b%%c
set LOG=%LOGDIR%\%TAG%-%STAMP%

cd /d C:\dev\BYTE

REM Rebuild the command line from the remaining arguments.
set CMD=
:loop
if "%~1"=="" goto run
set CMD=%CMD% %1
shift
goto loop

:run
echo [windows-dev] %CMD%
call %CMD% > "%LOG%.out" 2> "%LOG%.err"
set RC=%ERRORLEVEL%
echo [windows-dev] exit %RC%  logs: %LOG%.out / %LOG%.err
exit /b %RC%
