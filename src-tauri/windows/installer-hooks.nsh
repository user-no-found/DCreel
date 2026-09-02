!macro NSIS_HOOK_PREUNINSTALL
  ; 覆盖升级时保留 Explorer 集成；只有真正卸载才移除注册项。
  ${If} $UpdateMode <> 1
    ExecWait '"$INSTDIR\dcreel.exe" --unregister-shell-integration'
  ${EndIf}
!macroend
