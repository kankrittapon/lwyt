; 📜 Inno Setup Script for Lightweight Audio Player (MINIAMP)
; Download Inno Setup Compiler from: https://jrsoftware.org/isdl.php

#define MyAppName "Lightweight Audio Player"
#define MyAppVersion "0.1.0"
#define MyAppPublisher "kankrittapon"
#define MyAppURL "https://github.com/kankrittapon/lwyt"
#define MyAppExeName "lightweight_audio_player.exe"

[Setup]
; NOTE: The value of AppId uniquely identifies this application. Do not use the same AppId value in installers for other applications.
; (To generate a new GUID, click Tools | Generate GUID inside Inno Setup Compiler.)
AppId={{E581373A-56E8-4EF5-8EF0-7612E0AF3E8B}}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
AllowNoIcons=yes
; Save the setup executable in the current directory
OutputDir=.
OutputBaseFilename=LightweightAudioPlayer_Setup
Compression=lzma
SolidCompression=yes
WizardStyle=modern

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "thai"; MessagesFile: "compiler:Languages\Thai.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "target\release\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "README.md"; DestDir: "{app}"; Flags: ignoreversion isreadme

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{group}\{cm:UninstallProgram,{#MyAppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent

[Code]
// Helper function to check if a command executable is available in system PATH
function IsInPath(const ExecutableName: String): Boolean;
var
  Cmd, Output: String;
  ResultCode: Integer;
  TempFile: String;
begin
  TempFile := ExpandConstant('{tmp}\path_check.txt');
  Cmd := 'where ' + ExecutableName + ' > "' + TempFile + '" 2>&1';
  
  // Executing cmd.exe silently to search for executable in PATH
  if Executedos(Cmd, ResultCode) and (ResultCode = 0) then
    Result := True
  else
    Result := False;
end;

// Helper to check if MPV exists at default install location
function MpvExistsAtDefaultPath(): Boolean;
begin
  Result := FileExists('C:\Program Files\MPV Player\mpv.exe') or
            FileExists('C:\Program Files (x86)\MPV Player\mpv.exe');
end;

// Event called after installation has completed successfully
procedure CurStepChanged(CurStep: TSetupStep);
var
  WarningMsg: String;
  HasMpv, HasYtdlp: Boolean;
begin
  if CurStep = ssPostInstall then
  begin
    HasMpv := MpvExistsAtDefaultPath() or IsInPath('mpv.exe') or IsInPath('mpv');
    HasYtdlp := IsInPath('yt-dlp.exe') or IsInPath('yt-dlp');

    WarningMsg := '';

    if not HasMpv then
    begin
      WarningMsg := WarningMsg + 
        '• MPV Player was NOT detected on your system.' + #13#10 +
        '  Please install MPV at "C:\Program Files\MPV Player\mpv.exe" or add it to your System PATH.' + #13#10#10;
    end;

    if not HasYtdlp then
    begin
      WarningMsg := WarningMsg + 
        '• yt-dlp was NOT detected in your System PATH.' + #13#10 +
        '  Please download yt-dlp and add it to your Environment Variables (PATH) to enable YouTube streaming.' + #13#10#10;
    end;

    if WarningMsg <> '' then
    begin
      SuppressibleMsgBox(
        '⚠️ Missing Dependencies Detected!' + #13#10#10 +
        'Lightweight Audio Player requires additional software to work properly:' + #13#10#10 +
        WarningMsg +
        'Please refer to the README.txt file in the install directory for more details.',
        mbWarning, MB_OK, MB_OK
      );
    end;
  end;
end;
