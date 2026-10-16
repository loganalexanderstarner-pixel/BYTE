; Extra steps for BYTE's Windows uninstaller (Tauri installerHooks).
; The stock uninstaller already has a "Delete the application data" checkbox; this adds the one step that must always happen.

!macro NSIS_HOOK_POSTUNINSTALL
  ; BYTE no longer opens when the person signs in. Skipped while an update replaces the old version.
  ${If} $UpdateMode <> 1
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "BYTE"
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "byte"
  ${EndIf}
!macroend
