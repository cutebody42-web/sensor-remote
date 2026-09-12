Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "x64.nsh"
!include "WinVer.nsh"
!ifndef PACKAGE
  !error "Pass /DPACKAGE=absolute-package-directory"
!endif
!ifndef OUTPUT
  !error "Pass /DOUTPUT=absolute-installer-path"
!endif
!ifndef VERSION
  !error "Pass /DVERSION=the-packaged-version"
!endif
Name "SENSOR Remote Access"
OutFile "${OUTPUT}"
InstallDir "$LOCALAPPDATA\Programs\SENSOR Remote"
InstallDirRegKey HKCU "Software\SENSOR Technology\Remote Install" "Path"
RequestExecutionLevel user
SetCompressor /SOLID lzma
BrandingText "SENSOR TECHNOLOGY · Attended remote support"
VIProductVersion "${VERSION}.0"
VIAddVersionKey /LANG=1033 "ProductName" "SENSOR Remote Access"
VIAddVersionKey /LANG=1033 "CompanyName" "SENSOR TECHNOLOGY"
VIAddVersionKey /LANG=1033 "FileDescription" "SENSOR per-user setup (unsigned build)"
VIAddVersionKey /LANG=1033 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=1033 "LegalCopyright" "SENSOR TECHNOLOGY"
!define MUI_ABORTWARNING
!ifdef UNIFIED_CANDIDATE
!define MUI_WELCOMEPAGE_TEXT "One SENSOR application for Windows 7 SP1 / 10 / 11 x64 compatibility testing.$\r$\n$\r$\nThe same EXE selects graphics internally. Windows 7/10 runtime acceptance has NOT been completed. Win7 requires a compatible OpenGL driver and Media Foundation.$\r$\n$\r$\nBoth computers must be online and the owner must authorize access. Unsigned engineering candidate; no login-screen/UAC service or firewall changes."
!else
!define MUI_WELCOMEPAGE_TEXT "Install SENSOR for the current Windows user.$\r$\n$\r$\nBoth computers need SENSOR running and Internet access. The remote owner must approve the session.$\r$\n$\r$\nThis build is unsigned and does not provide unattended, login-screen or UAC control. No firewall changes or background service are installed."
!endif
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "${PACKAGE}\LICENSE"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\SENSOR-Remote.exe"
!define MUI_FINISHPAGE_RUN_NOTCHECKED
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Function .onInit
  ${IfNot} ${RunningX64}
    MessageBox MB_ICONSTOP "SENSOR requires 64-bit Windows."
    Abort
  ${EndIf}
!ifdef UNIFIED_CANDIDATE
  ${If} ${IsWin7}
    ${IfNot} ${AtLeastServicePack} 1
      MessageBox MB_ICONSTOP "Windows 7 requires Service Pack 1."
      Abort
    ${EndIf}
    ; No silent deployment of an unverified Win7 candidate.
    IfSilent sensor_win7_declined 0
    MessageBox MB_YESNO|MB_ICONEXCLAMATION|MB_DEFBUTTON2 "This unified SENSOR candidate has NOT been tested on Windows 7. Continue with compatibility testing only?" IDYES sensor_os_accepted
    sensor_win7_declined:
      Abort
  ${ElseIfNot} ${AtLeastWin10}
    MessageBox MB_ICONSTOP "This candidate targets Windows 7 SP1 and Windows 10/11 x64 only."
    Abort
  ${EndIf}
  sensor_os_accepted:
!else
  ${IfNot} ${AtLeastWin10}
    MessageBox MB_ICONSTOP "SENSOR requires Windows 10 or later."
    Abort
  ${EndIf}
!endif
  SetShellVarContext current
  SetRegView 64
FunctionEnd

Section "SENSOR Remote" Main
  SetOutPath "$INSTDIR"
  SetOverwrite on
  ClearErrors
  File "${PACKAGE}\SENSOR-Remote.exe"
  File "${PACKAGE}\SENSOR-CLI.exe"
  File "${PACKAGE}\sensor-network.json"
  File "${PACKAGE}\LICENSE"
  File "${PACKAGE}\README.md"
  File "${PACKAGE}\SOURCE_REVISION.txt"
  File "${PACKAGE}\SHA256SUMS.txt"
  File "${PACKAGE}\DEPENDENCIES.json"
  File "${PACKAGE}\sbom.cdx.json"
!ifdef UNIFIED_CANDIDATE
  File "${PACKAGE}\WINDOWS-BUILD.json"
  File "${PACKAGE}\WINDOWS-IMPORTS.json"
  File "${PACKAGE}\CLI-WINDOWS-IMPORTS.json"
!endif
  ; Exact install/uninstall entries are generated from the built package.
  !include "payload-install.nsh"
  ${If} ${Errors}
    MessageBox MB_ICONSTOP "Installation could not finish. Close SENSOR and retry."
    Abort
  ${EndIf}
  SetOutPath "$INSTDIR"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  WriteRegStr HKCU "Software\SENSOR Technology\Remote Install" "Path" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\SENSORRemote" "DisplayName" "SENSOR Remote Access"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\SENSORRemote" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\SENSORRemote" "Publisher" "SENSOR TECHNOLOGY"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\SENSORRemote" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\SENSORRemote" "DisplayIcon" "$INSTDIR\SENSOR-Remote.exe,0"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\SENSORRemote" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\SENSORRemote" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\SENSORRemote" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\SENSORRemote" "NoRepair" 1
  CreateDirectory "$SMPROGRAMS\SENSOR Remote"
  CreateShortcut "$SMPROGRAMS\SENSOR Remote\SENSOR Remote.lnk" "$INSTDIR\SENSOR-Remote.exe"
  CreateShortcut "$SMPROGRAMS\SENSOR Remote\Uninstall.lnk" "$INSTDIR\Uninstall.exe"
SectionEnd

Section "Uninstall"
  SetShellVarContext current
  SetRegView 64
  ; Never recursively delete INSTDIR: unrelated user files must survive.
  ClearErrors
  Delete "$INSTDIR\SENSOR-Remote.exe"
  ${If} ${Errors}
    MessageBox MB_ICONSTOP "Close SENSOR before uninstalling. Nothing else has been removed."
    SetErrorLevel 1
    Abort
  ${EndIf}
  ; Remove only the startup value belonging to this exact installation.
  ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "SENSORRemote"
  ${If} $0 == '"$INSTDIR\SENSOR-Remote.exe"'
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "SENSORRemote"
  ${EndIf}
  Delete "$INSTDIR\SENSOR-CLI.exe"
  Delete "$INSTDIR\sensor-network.json"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\SOURCE_REVISION.txt"
  Delete "$INSTDIR\SHA256SUMS.txt"
  Delete "$INSTDIR\DEPENDENCIES.json"
  Delete "$INSTDIR\sbom.cdx.json"
!ifdef UNIFIED_CANDIDATE
  Delete "$INSTDIR\WINDOWS-BUILD.json"
  Delete "$INSTDIR\WINDOWS-IMPORTS.json"
  Delete "$INSTDIR\CLI-WINDOWS-IMPORTS.json"
!endif
  !include "payload-uninstall.nsh"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\SENSOR Remote\SENSOR Remote.lnk"
  Delete "$SMPROGRAMS\SENSOR Remote\Uninstall.lnk"
  RMDir "$SMPROGRAMS\SENSOR Remote"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\SENSORRemote"
  DeleteRegKey HKCU "Software\SENSOR Technology\Remote Install"
  ; Identity, received files, audit and contacts under the user profile remain.
SectionEnd
