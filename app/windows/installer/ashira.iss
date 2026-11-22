; Ashira App installer for Windows (Inno Setup 6).
; Per-user install, no admin prompt: the App updates itself by running a newer copy of this
; installer silently, which must not stop for UAC.
;
; Built by the release workflow:
;   iscc /DAppVersion=0.2.11 /DSourceDir=<flutter Release folder> /DOutputName=<file stem> ashira.iss

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\..\build\windows\x64\runner\Release"
#endif
#ifndef OutputName
  #define OutputName "ashira-app-setup"
#endif

[Setup]
; Fixed for every release, so a newer installer upgrades the same install.
AppId={{6F2D4B7A-3C1E-4E8B-9A5D-2B7C1F4E8A90}
AppName=Ashira
AppVersion={#AppVersion}
AppVerName=Ashira {#AppVersion}
AppPublisher=Ashira
AppPublisherURL=https://github.com/Night-stars-1/Ashira
AppSupportURL=https://github.com/Night-stars-1/Ashira/issues
AppUpdatesURL=https://github.com/Night-stars-1/Ashira/releases
DefaultDirName={localappdata}\Programs\Ashira
DefaultGroupName=Ashira
DisableProgramGroupPage=yes
DisableDirPage=auto
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=Output
OutputBaseFilename={#OutputName}
SetupIconFile=..\runner\resources\app_icon.ico
UninstallDisplayIcon={app}\ashira_app.exe
UninstallDisplayName=Ashira
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
; Closes a running Ashira before replacing its files (the updater has already quit).
CloseApplications=yes
RestartApplications=no

[Languages]
; Inno Setup ships no Simplified Chinese; this one is the community translation from
; issrc is-6_7_3 Files/Languages/Unofficial, kept beside this script.
Name: "chinesesimplified"; MessagesFile: "ChineseSimplified.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[InstallDelete]
; A build's files change between releases; drop the old Flutter data so none linger.
Type: filesandordirs; Name: "{app}\data"

[Icons]
Name: "{group}\Ashira"; Filename: "{app}\ashira_app.exe"
Name: "{group}\{cm:UninstallProgram,Ashira}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\Ashira"; Filename: "{app}\ashira_app.exe"; Tasks: desktopicon

[Run]
; After an interactive install, offer to start it. A silent update (/SILENT) starts it too,
; so the App comes back on its own once it has replaced itself.
Filename: "{app}\ashira_app.exe"; Description: "{cm:LaunchProgram,Ashira}"; Flags: nowait postinstall skipifsilent
Filename: "{app}\ashira_app.exe"; Flags: nowait; Check: WizardSilent
