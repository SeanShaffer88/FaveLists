; FaveLists — per-user Inno Setup installer (Inno Setup 6)
; Per-user install (no admin) so the app's data folder can live next to the
; exe inside the install directory, per the portable-style storage design.
; Build: ISCC /DAppVersion=0.3.0 installer\FaveLists.iss
; Expects the release exe at ..\target\release\favelists.exe relative to this
; script (the release workflow builds it before invoking ISCC).

#ifndef AppVersion
  #define AppVersion "0.3.0"
#endif

[Setup]
AppId={{F18DD44A-4EA6-450E-B84D-7EEAA2E29638}
AppName=FaveLists
AppVersion={#AppVersion}
AppPublisher=Sean Shaffer
AppPublisherURL=https://github.com/SeanShaffer88/FaveLists
AppSupportURL=https://github.com/SeanShaffer88/FaveLists/issues
DefaultDirName={localappdata}\Programs\FaveLists
DefaultGroupName=FaveLists
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=..\dist
OutputBaseFilename=FaveLists-Setup-v{#AppVersion}-x64
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
UninstallDisplayName=FaveLists {#AppVersion}
; User data (FaveListsData\, created at runtime inside the install dir) is
; deliberately NOT removed on uninstall: Inno only removes files it
; installed, and no [UninstallDelete] entry covers the data folder, so a
; user's lists and cached covers survive uninstall/reinstall.

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked

[Files]
Source: "..\target\release\favelists.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\FaveLists"; Filename: "{app}\favelists.exe"
Name: "{autodesktop}\FaveLists"; Filename: "{app}\favelists.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\favelists.exe"; Description: "Launch FaveLists"; Flags: nowait postinstall skipifsilent
