; Gopu's NSIS hooks (release-v1 ticket 03).

; The finish page: Gopu lives in the tray, so no desktop shortcut unless the user ticks it.
!define MUI_FINISHPAGE_SHOWREADME_NOTCHECKED

; WebView2 too old for Gopu (older than MINIMUMWEBVIEW2VERSION, the WebView2 SDK's target) even after Tauri's update
; step: that step runs Edge's updater, and does nothing when the updater is registered but missing (Windows Sandbox,
; some managed PCs). Then fetch Microsoft's WebView2 installer, which brings its own updater and can install for this
; user alone, in its own window with progress (a silent install looked like a hang). On failure, say how to fix it
; instead of letting Gopu fail to start.
!macro NSIS_HOOK_POSTINSTALL
  ${If} $UpdateMode <> 1
    ReadRegStr $R2 HKLM "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\${WEBVIEW2APPGUID}" "pv"
    ReadRegStr $R3 HKCU "SOFTWARE\Microsoft\EdgeUpdate\Clients\${WEBVIEW2APPGUID}" "pv"
    ${VersionCompare} "$R3" "$R2" $R4
    ${If} $R4 = 1
      StrCpy $R2 $R3 ; the per-user copy is newer
    ${EndIf}
    ${VersionCompare} "${MINIMUMWEBVIEW2VERSION}" "$R2" $R4
    ${If} $R4 = 1
      DetailPrint "$(webview2Downloading)" ; WebView2 $R2 is too old
      Delete "$TEMP\MicrosoftEdgeWebview2Setup.exe"
      NSISdl::download "https://go.microsoft.com/fwlink/p/?LinkId=2124703" "$TEMP\MicrosoftEdgeWebview2Setup.exe"
      Pop $0
      StrCpy $1 1
      ${If} $0 == "success"
        DetailPrint "$(installingWebview2)"
        ExecWait '"$TEMP\MicrosoftEdgeWebview2Setup.exe" /install' $1 ; not /silent: Microsoft's window shows progress
      ${EndIf}
      ${If} $1 <> 0
        MessageBox MB_ICONEXCLAMATION "Gopu needs Microsoft Edge WebView2 ${MINIMUMWEBVIEW2VERSION} or newer, and it couldn't be installed. Get it from https://developer.microsoft.com/microsoft-edge/webview2 and start Gopu again."
      ${EndIf}
    ${EndIf}
  ${EndIf}
!macroend

; Uninstall with "Delete the application data" ticked: everything Gopu stored, not just the webview's folders.
; Memory, logs and the access log (%LOCALAPPDATA%\Gopu, which is also the install folder), settings (%APPDATA%\Gopu)
; and the provider keys in Credential Manager (Gopu:<provider>, see settings::key_name).
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    Delete "$LOCALAPPDATA\Gopu\memory.db"
    Delete "$LOCALAPPDATA\Gopu\memory.db-wal"
    Delete "$LOCALAPPDATA\Gopu\memory.db-shm"
    Delete "$LOCALAPPDATA\Gopu\last-tap.txt"
    Delete "$LOCALAPPDATA\Gopu\mcp-access.log"
    RMDir "$LOCALAPPDATA\Gopu"
    RMDir /r "$APPDATA\Gopu"
    nsExec::Exec 'cmdkey /delete:Gopu:groq'
    nsExec::Exec 'cmdkey /delete:Gopu:openai'
    nsExec::Exec 'cmdkey /delete:Gopu:anthropic'
    nsExec::Exec 'cmdkey /delete:Gopu:openrouter'
    nsExec::Exec 'cmdkey /delete:Gopu:ollama'
    nsExec::Exec 'cmdkey /delete:Gopu:custom'
  ${EndIf}
!macroend
