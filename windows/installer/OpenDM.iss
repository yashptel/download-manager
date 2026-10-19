; OpenDM for Windows, installed for the current user: no administrator
; rights, %LOCALAPPDATA%\Programs\OpenDM, a Start menu entry. The app is
; self-contained (.NET and the Windows App SDK ship with it), so this only
; copies files. Built by scripts\package-windows.ps1, which passes the version
; and folders below.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef Arch
  #define Arch "x64"
#endif
#ifndef SourceDir
  #define SourceDir "..\..\build\windows\OpenDM-" + Arch
#endif
#ifndef OutputDir
  #define OutputDir "..\..\build\windows"
#endif

[Setup]
AppId={{6B0F5C9E-2D4A-4E7B-9C1F-0A8D3E5B7C21}
AppName=OpenDM
AppVersion={#AppVersion}
AppPublisher=OpenDM
AppPublisherURL=https://github.com/yashptel/download-manager
AppSupportURL=https://github.com/yashptel/download-manager/issues
DefaultDirName={autopf}\OpenDM
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
#if Arch == "arm64"
ArchitecturesAllowed=arm64
ArchitecturesInstallIn64BitMode=arm64
#else
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
#endif
MinVersion=10.0.17763
OutputDir={#OutputDir}
OutputBaseFilename=OpenDM-{#AppVersion}-{#Arch}-setup
SetupIconFile=..\OpenDM\Assets\OpenDM.ico
UninstallDisplayIcon={app}\OpenDM.exe
UninstallDisplayName=OpenDM
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
; OpenDM keeps running in the notification area when its window closes, so
; setup stops it itself (see [Code]) instead of asking it to close.
CloseApplications=no
#ifdef Sign
; The "opendm" sign tool is defined by package-windows.ps1 when a certificate
; is configured; Inno signs setup and its uninstaller with it.
SignTool=opendm
#endif

[Tasks]
Name: desktopicon; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\OpenDM"; Filename: "{app}\OpenDM.exe"
Name: "{autodesktop}\OpenDM"; Filename: "{app}\OpenDM.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\OpenDM.exe"; Description: "{cm:LaunchProgram,OpenDM}"; Flags: nowait postinstall skipifsilent

[Code]
const
  RunKey = 'Software\Microsoft\Windows\CurrentVersion\Run';
  ApprovedKey = 'Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run';

{ Stops OpenDM and its engine if they run from this installation, so their
  files can be replaced or removed. Downloads continue from where they were
  when OpenDM starts again. OpenDM goes first: it restarts an engine that
  stops before it does. }
function StopCommand(const Names: String): String;
begin
  Result := 'Get-Process ' + Names + ' -ErrorAction SilentlyContinue | ' +
    'Where-Object { $_.Path -like ''' + ExpandConstant('{app}') + '\*'' } | ' +
    'ForEach-Object { $_.Kill(); $_.WaitForExit(10000) | Out-Null }; ';
end;

procedure StopOpenDM();
var
  ResultCode: Integer;
begin
  Exec(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
    '-NoProfile -NonInteractive -Command "' + StopCommand('OpenDM') + StopCommand('aria2-next, opendm-host') + '"',
    '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  StopOpenDM();
  Result := '';
end;

function InitializeUninstall(): Boolean;
begin
  StopOpenDM();
  Result := True;
end;

{ The app writes these itself: the sign-in entry from Settings and the
  browser extension's host registrations. Only entries that point at this
  installation or its data folder go; the history, settings and saved logins
  stay for a reinstall. }
procedure RemoveIfPointsInto(const Key, Name, Folder: String);
var
  Value: String;
begin
  if RegQueryStringValue(HKCU, Key, Name, Value) and (Pos(Lowercase(Folder), Lowercase(Value)) > 0) then
  begin
    if Name = '' then
      RegDeleteKeyIncludingSubkeys(HKCU, Key)
    else
      RegDeleteValue(HKCU, Key, Name);
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Data: String;
begin
  if CurUninstallStep <> usPostUninstall then Exit;
  RemoveIfPointsInto(RunKey, 'OpenDM', ExpandConstant('{app}'));
  if not RegValueExists(HKCU, RunKey, 'OpenDM') then
    RegDeleteValue(HKCU, ApprovedKey, 'OpenDM');
  Data := ExpandConstant('{localappdata}\OpenDM\browser');
  RemoveIfPointsInto('Software\Google\Chrome\NativeMessagingHosts\org.opendm.host', '', Data);
  RemoveIfPointsInto('Software\Chromium\NativeMessagingHosts\org.opendm.host', '', Data);
  RemoveIfPointsInto('Software\Microsoft\Edge\NativeMessagingHosts\org.opendm.host', '', Data);
  RemoveIfPointsInto('Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\org.opendm.host', '', Data);
  RemoveIfPointsInto('Software\Vivaldi\NativeMessagingHosts\org.opendm.host', '', Data);
  RemoveIfPointsInto('Software\Mozilla\NativeMessagingHosts\org.opendm.host', '', Data);
end;

