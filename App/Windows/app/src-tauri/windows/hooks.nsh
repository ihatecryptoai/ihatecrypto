; The engine runs in an AppContainer (src/broker.rs), and Windows keeps a container off 127.0.0.1, where Ollama and
; LM Studio answer. Only an administrator may let one reach this PC's own servers: the per-machine installer does it
; for the engine's container, by its SID — the one Windows derives from its name, app.ihatecrypto.windows.engine
; (broker.rs's test checks they match) — and the uninstaller takes it back. Connections from the browser to the engine
; still can't come in: the window listens for it.
!define ENGINE_CONTAINER_SID "S-1-15-2-1119508096-4101952096-861444967-376880786-2949364012-3957978147-3634731383"

!macro IHC_LOOPBACK what
  ; A 32-bit installer sees SysWOW64 as $SYSDIR; Sysnative is the real System32 where it has one.
  ${If} ${FileExists} "$WINDIR\Sysnative\CheckNetIsolation.exe"
    nsExec::Exec '"$WINDIR\Sysnative\CheckNetIsolation.exe" LoopbackExempt ${what} -p=${ENGINE_CONTAINER_SID}'
  ${Else}
    nsExec::Exec '"$SYSDIR\CheckNetIsolation.exe" LoopbackExempt ${what} -p=${ENGINE_CONTAINER_SID}'
  ${EndIf}
  Pop $0
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro IHC_LOOPBACK "-a"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro IHC_LOOPBACK "-d"
!macroend
