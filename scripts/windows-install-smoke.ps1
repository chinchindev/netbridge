[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$Installer,
    [Parameter(Mandatory=$true)][string]$Version,
    [ValidatePattern('^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$')][string]$BaselineVersion='0.5.0'
)
$ErrorActionPreference='Stop'
$OutputEncoding=[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
if($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_OS -ne 'Windows'){
    throw 'Installer smoke tests may only run on an isolated GitHub Windows runner.'
}
if([version]$BaselineVersion -ge [version]($Version.Split('-')[0])){throw 'Baseline must be older than the candidate'}
$taskRoot=(Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$taskWorkspace=[IO.Path]::GetFullPath($env:GITHUB_WORKSPACE)
if($taskRoot.TrimEnd('\') -ne $taskWorkspace.TrimEnd('\')){throw 'Unexpected workspace'}
$taskWork=Join-Path $taskRoot '.qa/windows-install-smoke'
$taskInstall=Join-Path $taskWork 'installed'
$taskProfile=[Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
if([IO.Path]::GetFullPath($env:LOCALAPPDATA).TrimEnd('\') -ne $taskProfile.TrimEnd('\')){throw 'Installer and application must use the same real runner profile'}
$taskData=Join-Path $taskProfile 'NetBridge'
if(Test-Path -LiteralPath $taskData){throw 'Runner already has NetBridge user data; refusing to overwrite it'}
New-Item -ItemType Directory -Force -Path $taskWork | Out-Null
$taskInstaller=(Resolve-Path -LiteralPath $Installer).Path
$taskOld=Join-Path $taskWork 'previous'
New-Item -ItemType Directory -Force -Path $taskOld | Out-Null
$taskProxyBefore=Get-ItemProperty 'HKCU:/Software/Microsoft/Windows/CurrentVersion/Internet Settings' | Select-Object ProxyEnable,ProxyServer,AutoConfigURL
$taskReport=[ordered]@{version=$Version;baselineVersion=$BaselineVersion;runner=$env:RUNNER_OS;checks=@();offlineRuntimeBootstrap='not tested: runner may already have WebView2';nativeFilePicker='not automated';recoveryUiContent='not inspected: only native startup and preserved file are checked';installer=$taskInstaller}
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class NetBridgeSmokeWindow {
 public delegate bool EnumProc(IntPtr window,IntPtr value);
 [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc callback,IntPtr value);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window,out uint pid);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr window,StringBuilder text,int count);
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr window,uint message,IntPtr w,IntPtr l);
 public static IntPtr Find(uint target){IntPtr found=IntPtr.Zero;EnumWindows((h,v)=>{uint p;GetWindowThreadProcessId(h,out p);var title=new StringBuilder(200);GetWindowText(h,title,200);if(p==target&&title.ToString()=="NetBridge"){found=h;return false;}return true;},IntPtr.Zero);return found;}
}
'@
function Install-NsisSmoke([string]$Path){
    $process=Start-Process -FilePath $Path -ArgumentList @('/S',('/D='+$taskInstall)) -WindowStyle Hidden -PassThru
    if(-not $process.WaitForExit(300000)){throw 'NSIS install timed out'}
    if($process.ExitCode -notin @(0,3010)){throw "NSIS install failed: $($process.ExitCode)"}
}
function Run-NativeWindow([string]$Exe,[string]$Mode){
    $info=[Diagnostics.ProcessStartInfo]::new()
    $info.FileName=$Exe;$info.Arguments=$Mode;$info.UseShellExecute=$false;$info.CreateNoWindow=$true
    $process=[Diagnostics.Process]::Start($info)
    $watch=[Diagnostics.Stopwatch]::StartNew();$window=[IntPtr]::Zero
    while($watch.Elapsed.TotalSeconds -lt 45){
        if($process.HasExited){throw "Native app exited before opening a window: $($process.ExitCode)"}
        $window=[NetBridgeSmokeWindow]::Find($process.Id)
        if($window -ne [IntPtr]::Zero){break}
        Start-Sleep -Milliseconds 200
    }
    if($window -eq [IntPtr]::Zero){throw 'Native window did not open'}
    # Retry a graceful close while startup snapshot reads finish; never kill a user process.
    $close=[Diagnostics.Stopwatch]::StartNew()
    while(-not $process.HasExited -and $close.Elapsed.TotalSeconds -lt 15){
        [NetBridgeSmokeWindow]::PostMessage($window,0x0010,[IntPtr]::Zero,[IntPtr]::Zero)|Out-Null
        $null=$process.WaitForExit(250)
    }
    if(-not $process.HasExited){throw 'Native app did not close normally'}
    if($process.ExitCode -ne 0){throw "Native app exit code: $($process.ExitCode)"}
}
try{
    & gh release download "v$BaselineVersion" --repo $env:GITHUB_REPOSITORY --pattern "NetBridge_${BaselineVersion}_x64-setup.exe" --dir $taskOld
    if($LASTEXITCODE){throw 'Could not download baseline installer'}
    Install-NsisSmoke (Join-Path $taskOld "NetBridge_${BaselineVersion}_x64-setup.exe")
    $taskExe=Join-Path $taskInstall 'netbridge-desktop.exe'
    if(-not (Test-Path -LiteralPath $taskExe)){throw 'Installed baseline EXE missing'}
    if((Get-Item $taskExe).VersionInfo.ProductVersion -cne $BaselineVersion){throw 'Baseline file version mismatch'}
    Run-NativeWindow $taskExe '--preview';$taskReport.checks+='baseline install and native preview'
    New-Item -ItemType Directory -Force -Path $taskData | Out-Null
    $taskConfig=Join-Path $taskData 'config.json';$taskPrefs=Join-Path $taskData 'toolbox-preferences.json'
    $config=@{port=17890;lan_id='';usb_id='';adb_path='auto';rules=@(@{pattern='preserved.example.test';route='system'});check_targets=@()}
    $config | ConvertTo-Json -Depth 5 | Set-Content $taskConfig -Encoding utf8
    @{unit='MB/s'} | ConvertTo-Json | Set-Content $taskPrefs -Encoding utf8
    $configHash=(Get-FileHash $taskConfig).Hash;$prefsHash=(Get-FileHash $taskPrefs).Hash
    Install-NsisSmoke $taskInstaller
    if((Get-Item $taskExe).VersionInfo.ProductVersion -cne $Version){throw 'Upgraded file version mismatch'}
    if((Get-FileHash $taskConfig).Hash -ne $configHash -or (Get-FileHash $taskPrefs).Hash -ne $prefsHash){throw 'Upgrade changed user settings'}
    Run-NativeWindow $taskExe '--preview';$taskReport.checks+='upgrade and native preview; settings preserved'
    Copy-Item $taskConfig ($taskConfig+'.bak')
    [IO.File]::WriteAllText($taskConfig,'{unfinished',[Text.UTF8Encoding]::new($false))
    Run-NativeWindow $taskExe ''
    if([IO.File]::ReadAllText($taskConfig) -ne '{unfinished'){throw 'Corrupt config was silently overwritten'}
    $taskReport.checks+='native window opens with corrupt config; damaged file preserved'
    $taskProxyAfter=Get-ItemProperty 'HKCU:/Software/Microsoft/Windows/CurrentVersion/Internet Settings' | Select-Object ProxyEnable,ProxyServer,AutoConfigURL
    if(($taskProxyBefore|ConvertTo-Json -Compress) -cne ($taskProxyAfter|ConvertTo-Json -Compress)){throw 'Smoke test changed Windows proxy'}
    $taskReport.checks+='Windows proxy unchanged';$taskReport.passed=$true
}catch{$taskReport.passed=$false;$taskReport.error=$_.Exception.Message;throw}
finally{
    $taskReport | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $taskWork 'result.json') -Encoding utf8
    $taskReport | ConvertTo-Json -Depth 6 | Write-Output
}
