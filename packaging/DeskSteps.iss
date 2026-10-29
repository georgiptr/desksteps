#ifndef AppVersion
  #error AppVersion must be supplied by the packaging command
#endif
[Setup]
AppId={{18992877-6FF4-42D7-BCC8-87CC5BB74B1B}
AppName=DeskSteps
AppVersion={#AppVersion}
AppPublisher=Georgi Petrov
AppPublisherURL=https://github.com/georgiptr/desksteps
AppSupportURL=https://github.com/georgiptr/desksteps/issues
DefaultDirName={localappdata}\Programs\DeskSteps
DefaultGroupName=DeskSteps
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.22000
OutputDir=..\dist
OutputBaseFilename=DeskSteps-{#AppVersion}-windows-x64-setup
SetupIconFile=..\target\release\DeskSteps.ico
UninstallDisplayIcon={app}\desksteps.exe
LicenseFile=..\LICENSE
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
AppMutex=Local\DeskSteps.Tray
CloseApplications=no
RestartApplications=no
DisableProgramGroupPage=yes

[Files]
Source: "..\target\release\desksteps.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\THIRD_PARTY_LICENSES.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\docs\*"; DestDir: "{app}\docs"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{userprograms}\DeskSteps"; Filename: "{app}\desksteps.exe"

; Intentionally no Run section: installation never connects or starts the pad.
; Intentionally no AppData deletion: uninstall preserves walking history.

[Code]
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Command: String;
begin
  if CurUninstallStep = usUninstall then
    if RegQueryStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'DeskSteps', Command) then
      if CompareText(Command, '"' + ExpandConstant('{app}\desksteps.exe') + '"') = 0 then
        RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'DeskSteps');
end;
