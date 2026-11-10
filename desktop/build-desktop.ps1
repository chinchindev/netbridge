[CmdletBinding()]
param([ValidateSet('frontend','check','test','core-test','clippy','build','package')][string]$Task='build')
$ErrorActionPreference='Stop'
$OutputEncoding=[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$taskRoot=Split-Path -Parent $PSScriptRoot
$taskTools=Join-Path $taskRoot '.build-tools/desktop-v2'
$taskNode=Get-ChildItem -LiteralPath $taskTools -Directory -Filter 'node-v24*-win-x64'|Sort-Object Name -Descending|Select-Object -First 1
$taskLlvm=Join-Path $taskRoot '.build-tools/llvm-mingw-20261104-ucrt-x86_64/bin'
$taskRust=Join-Path $env:USERPROFILE '.rustup/toolchains/1.95-x86_64-pc-windows-msvc/bin'
$taskSdk=Join-Path $taskTools 'msvc-native'
$taskCrt=Get-ChildItem -LiteralPath (Join-Path $taskSdk 'VC/Tools/MSVC') -Directory|Select-Object -First 1
$taskWindowsSdk=Get-ChildItem -LiteralPath (Join-Path $taskSdk 'Windows Kits/10/Lib') -Directory|Select-Object -First 1
if(-not $taskNode -or -not $taskCrt -or -not $taskWindowsSdk){throw 'Local Node/MSVC toolchain is incomplete'}
$env:PATH=$taskNode.FullName+';'+$taskRust+';'+$taskLlvm+';'+$env:PATH
$env:RUSTUP_TOOLCHAIN='1.95-x86_64-pc-windows-msvc'
$env:RUSTC=Join-Path $taskRust 'rustc.exe'
$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER=Join-Path $taskLlvm 'lld-link.exe'
$env:LIB=(Join-Path $taskCrt.FullName 'lib/x64')+';'+(Join-Path $taskWindowsSdk.FullName 'ucrt/x64')+';'+(Join-Path $taskWindowsSdk.FullName 'um/x64')
$taskInclude=Join-Path $taskSdk ('Windows Kits/10/Include/'+$taskWindowsSdk.Name)
$env:INCLUDE=(Join-Path $taskCrt.FullName 'include')+';'+(Join-Path $taskInclude 'ucrt')+';'+(Join-Path $taskInclude 'um')+';'+(Join-Path $taskInclude 'shared')
$env:RUSTFLAGS='-C target-feature=+crt-static'
$env:CC=Join-Path $taskLlvm 'clang-cl.exe'
$env:CXX=$env:CC
$env:AR=Join-Path $taskLlvm 'llvm-lib.exe'
$env:CFLAGS='--target=x86_64-pc-windows-msvc'
$env:CXXFLAGS=$env:CFLAGS
$env:VCINSTALLDIR=Join-Path $taskSdk 'VC'
$env:VCToolsInstallDir=$taskCrt.FullName+'\'
$env:CARGO_TARGET_DIR=Join-Path $PSScriptRoot 'src-tauri/target'
Push-Location $PSScriptRoot
try {
 if($Task -in @('frontend','build','package')){
  & (Join-Path $taskNode.FullName 'npm.cmd') run build
  if($LASTEXITCODE -ne 0){exit $LASTEXITCODE}
  if($Task -eq 'frontend'){exit 0}
 }
 if($Task -eq 'package'){
  & (Join-Path $PSScriptRoot 'node_modules/.bin/tauri.cmd') build --target x86_64-pc-windows-msvc --bundles nsis
 }else{
  $taskManifest=if($Task -eq 'core-test'){Join-Path $taskRoot 'Cargo.toml'}else{Join-Path $PSScriptRoot 'src-tauri/Cargo.toml'}
  $taskVerb=if($Task -eq 'core-test'){'test'}else{$Task}
  $taskCargoArgs=@($taskVerb,'--manifest-path',$taskManifest,'--target','x86_64-pc-windows-msvc')
  if($Task -eq 'build'){$taskCargoArgs+='--release'}
  if($Task -eq 'clippy'){$taskCargoArgs+=@('--all-targets','--','-D','warnings')}
  & (Join-Path $taskRust 'cargo.exe') @taskCargoArgs
 }
 exit $LASTEXITCODE
} finally { Pop-Location }
