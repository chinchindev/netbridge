[CmdletBinding()]
param([ValidateSet('build','test','check','clippy','fmt')][string]$Task='build', [switch]$Release,
    [string]$ToolsRoot='')
$ErrorActionPreference='Stop'
$OutputEncoding=[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
if ([string]::IsNullOrWhiteSpace($ToolsRoot)) { $ToolsRoot=Join-Path $PSScriptRoot '.build-tools' }
$taskRustBin=Join-Path $env:USERPROFILE '.rustup\toolchains\1.95-x86_64-pc-windows-gnu\bin'
$taskCompilerBin=[IO.Path]::GetFullPath((Join-Path $ToolsRoot 'llvm-mingw-20261104-ucrt-x86_64\bin'))
if (-not (Test-Path (Join-Path $taskRustBin 'cargo.exe'))) { throw 'Rust 1.95 GNU toolchain is required for this portable build script.' }
if (-not (Test-Path (Join-Path $taskCompilerBin 'x86_64-w64-mingw32-clang.exe'))) { throw 'Verified portable LLVM-MinGW compiler is missing.' }
$env:PATH=$taskRustBin+';'+$taskCompilerBin+';'+$env:PATH
$env:RUSTUP_TOOLCHAIN='1.95-x86_64-pc-windows-gnu'
$env:RUSTC=[IO.Path]::GetFullPath((Join-Path $ToolsRoot 'rust-gnu\bin\rustc.exe'))
if (-not (Test-Path -LiteralPath $env:RUSTC)) { throw 'Portable Rust compiler is missing from ToolsRoot.' }
$env:PATH=(Split-Path -Parent $env:RUSTC)+';'+$env:PATH
$env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=Join-Path $taskCompilerBin 'ld.lld.exe'
$env:RUSTFLAGS='-C linker-flavor=ld.lld -C link-self-contained=yes'
$taskArgs=@($Task,'--manifest-path',(Join-Path $PSScriptRoot 'Cargo.toml'))
if ($Release) { $taskArgs+='--release' }
if ($Task -eq 'clippy') { $taskArgs+=@('--all-targets','--','-D','warnings') }
& (Join-Path $taskRustBin 'cargo.exe') @taskArgs
exit $LASTEXITCODE
