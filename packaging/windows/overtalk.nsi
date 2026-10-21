; Per-user Overtalk installer. packaging/windows/build.py supplies the defines:
; APP_NAME, REGISTRY_NAME, VERSION, VERSION_NUMERIC, STAGE (the staged folder),
; FILES (generated install/uninstall file lists), OUTFILE, PUBLISHER, HOMEPAGE
; and, when signing, SIGN_COMMAND.
;
; Installs to %LOCALAPPDATA%\Programs\<APP_NAME> without administrator rights,
; adds a Start menu shortcut and an uninstaller. Meetings, settings and models
; live in %APPDATA%\com.judeam.sonefold and are never touched by either.

Unicode true
ManifestDPIAware true
SetCompressor /SOLID lzma
RequestExecutionLevel user

!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "${FILES}"

!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${REGISTRY_NAME}"
!define UNINSTALLER "Uninstall ${APP_NAME}.exe"

!ifdef SIGN_COMMAND
  !uninstfinalize '${SIGN_COMMAND}'
!endif

Name "${APP_NAME}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\${APP_NAME}"
InstallDirRegKey HKCU "${UNINSTALL_KEY}" "InstallLocation"
BrandingText "${APP_NAME} ${VERSION}"

VIProductVersion "${VERSION_NUMERIC}"
VIAddVersionKey "ProductName" "${APP_NAME}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "${APP_NAME} installer"
VIAddVersionKey "CompanyName" "${PUBLISHER}"
VIAddVersionKey "LegalCopyright" "${PUBLISHER}. MIT licence."

!define MUI_ICON "${STAGE}\overtalk.ico"
!define MUI_UNICON "${STAGE}\overtalk.ico"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\overtalk.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Open ${APP_NAME}"

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Function .onInit
  SetShellVarContext current
FunctionEnd

Function un.onInit
  SetShellVarContext current
FunctionEnd

Section "Install"
  ; Never spread files into a folder that holds something else: the uninstaller
  ; removes only what it installed, and the sandbox grant below covers the folder.
  ${DirState} "$INSTDIR" $0
  ${If} $0 == 1
  ${AndIfNot} ${FileExists} "$INSTDIR\overtalk.exe"
    MessageBox MB_ICONSTOP|MB_OK "$INSTDIR is not empty. Choose an empty folder or the folder of an earlier ${APP_NAME}." /SD IDOK
    Abort
  ${EndIf}
  CreateDirectory "$INSTDIR"
  ; Local models and media run in AppContainers (see docs/PLATFORMS.md). Like
  ; Program Files, the program folder is readable by them; it holds no user data.
  nsExec::ExecToLog '"$SYSDIR\icacls.exe" "$INSTDIR" /grant *S-1-15-2-1:(OI)(CI)(RX) /Q'
  Pop $0
  ${If} $0 != 0
    DetailPrint "Could not grant application containers read access ($0); isolated workers grant it per launch."
  ${EndIf}

  !insertmacro OVERTALK_INSTALL_FILES
  WriteUninstaller "$INSTDIR\${UNINSTALLER}"
  CreateShortcut "$SMPROGRAMS\${APP_NAME}.lnk" "$INSTDIR\overtalk.exe" "" "$INSTDIR\overtalk.ico" 0

  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${APP_NAME}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\overtalk.ico"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "URLInfoAbout" "${HOMEPAGE}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\${UNINSTALLER}"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\${UNINSTALLER}" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "EstimatedSize" "$0"
SectionEnd

Section "Uninstall"
  !insertmacro OVERTALK_UNINSTALL_FILES
  Delete "$INSTDIR\${UNINSTALLER}"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${APP_NAME}.lnk"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
  ; The app's "Open Overtalk when you sign in" setting.
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${REGISTRY_NAME}"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "${REGISTRY_NAME}"
SectionEnd
