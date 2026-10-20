; Inno Setup script for the Windows installer.
; Built by .github/workflows/release.yml:
;   iscc /DAppVersion=0.1.0 /DNumericVersion=0.1.0 /DSourceExe=path\to\simple-git-gui.exe /DOutputDir=dist packaging\windows\simple-git.iss
; Installs for the current user by default (no administrator rights needed);
; the first page offers to install for every user instead.

#ifndef AppVersion
  #define AppVersion "0.0.0-dev"
#endif
#ifndef NumericVersion
  #define NumericVersion "0.0.0"
#endif
#ifndef SourceExe
  #define SourceExe "..\..\target\release\simple-git-gui.exe"
#endif
#ifndef OutputDir
  #define OutputDir "..\..\dist"
#endif

#define SystemEnvKey "SYSTEM\CurrentControlSet\Control\Session Manager\Environment"

[Setup]
; Never change AppId: it lets newer versions upgrade the installed one.
AppId={{191C0865-EB11-46FC-B1E7-6B395DFF9081}
AppName=Simple Git
AppVersion={#AppVersion}
AppVerName=Simple Git {#AppVersion}
VersionInfoVersion={#NumericVersion}
AppPublisher=Simple Git contributors
AppPublisherURL=https://github.com/thewenneur/simple-git-gui
AppSupportURL=https://github.com/thewenneur/simple-git-gui/issues
DefaultDirName={autopf}\Simple Git
DefaultGroupName=Simple Git
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#OutputDir}
OutputBaseFilename=simple-git-gui-{#AppVersion}-windows-x86_64-setup
SetupIconFile=..\..\assets\icon\simple-git.ico
UninstallDisplayIcon={app}\simple-git-gui.exe
UninstallDisplayName=Simple Git
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
; Closes a running Simple Git before replacing it.
CloseApplications=yes
; Tells open terminals and Explorer that the PATH changed.
ChangesEnvironment=yes

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "fr"; MessagesFile: "compiler:Languages\French.isl"

[CustomMessages]
en.GitMissing=Simple Git needs Git, which was not found on this computer.%n%nYou can install it now from https://git-scm.com (the default options are fine) and then start Simple Git. Open the Git download page now?
en.AddToPath=Add Simple Git to the PATH (open a folder from a terminal with: simple-git-gui .)
fr.AddToPath=Ajouter Simple Git au PATH (ouvrir un dossier depuis un terminal avec : simple-git-gui .)
fr.GitMissing=Simple Git a besoin de Git, qui n'a pas été trouvé sur cet ordinateur.%n%nVous pouvez l'installer depuis https://git-scm.com (les options par défaut conviennent) puis lancer Simple Git. Ouvrir la page de téléchargement de Git maintenant ?

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "addtopath"; Description: "{cm:AddToPath}"

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "simple-git-gui.exe"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Simple Git"; Filename: "{app}\simple-git-gui.exe"
Name: "{autodesktop}\Simple Git"; Filename: "{app}\simple-git-gui.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\simple-git-gui.exe"; Description: "{cm:LaunchProgram,Simple Git}"; Flags: nowait postinstall skipifsilent

[Registry]
; The user's PATH for a per-user install, the system PATH for every user.
Root: HKCU; Subkey: "Environment"; ValueType: expandsz; ValueName: "Path"; ValueData: "{code:PathWithApp}"; Tasks: addtopath; Check: not IsAdminInstallMode and PathMissingApp
Root: HKLM; Subkey: "{#SystemEnvKey}"; ValueType: expandsz; ValueName: "Path"; ValueData: "{code:PathWithApp}"; Tasks: addtopath; Check: IsAdminInstallMode and PathMissingApp

[Code]
{ PATH handling: the folder is added once, and removed on uninstall. }
function PathRoot: Integer;
begin
  if IsAdminInstallMode then
    Result := HKLM
  else
    Result := HKCU;
end;

function PathKey: String;
begin
  if IsAdminInstallMode then
    Result := '{#SystemEnvKey}'
  else
    Result := 'Environment';
end;

function CurrentPath: String;
begin
  if not RegQueryStringValue(PathRoot, PathKey, 'Path', Result) then
    Result := '';
end;

function PathMissingApp: Boolean;
begin
  Result := Pos(';' + Uppercase(ExpandConstant('{app}')) + ';', ';' + Uppercase(CurrentPath) + ';') = 0;
end;

function PathWithApp(Param: String): String;
begin
  Result := CurrentPath;
  if (Result <> '') and (Result[Length(Result)] <> ';') then
    Result := Result + ';';
  Result := Result + ExpandConstant('{app}');
end;

procedure RemoveAppFromPath;
var
  Path: String;
  P: Integer;
  App: String;
begin
  App := ';' + Uppercase(ExpandConstant('{app}')) + ';';
  Path := ';' + CurrentPath + ';';
  P := Pos(App, Uppercase(Path));
  if P = 0 then
    exit;
  Delete(Path, P, Length(App) - 1);
  { Drop the separators added around it above. }
  Path := Copy(Path, 2, Length(Path) - 2);
  RegWriteExpandStringValue(PathRoot, PathKey, 'Path', Path);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    RemoveAppFromPath;
end;

{ Git for Windows records its folder in the registry; a Git found in the PATH
  (another distribution) is fine too. }
function GitInstalled: Boolean;
var
  Dir: String;
begin
  Result := RegQueryStringValue(HKLM, 'SOFTWARE\GitForWindows', 'InstallPath', Dir)
    or RegQueryStringValue(HKCU, 'SOFTWARE\GitForWindows', 'InstallPath', Dir)
    or (FileSearch('git.exe', GetEnv('PATH')) <> '');
end;

{ Simple Git does nothing without Git: say so once the files are in place. }
procedure CurStepChanged(CurStep: TSetupStep);
var
  ErrorCode: Integer;
begin
  if (CurStep = ssPostInstall) and not WizardSilent and not GitInstalled then
    if MsgBox(CustomMessage('GitMissing'), mbConfirmation, MB_YESNO) = IDYES then
      ShellExec('open', 'https://git-scm.com/downloads/win', '', '', SW_SHOWNORMAL, ewNoWait, ErrorCode);
end;
