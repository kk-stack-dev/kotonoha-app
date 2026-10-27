# Installs or updates Kotonoha on Windows.
#
#   irm https://raw.githubusercontent.com/kk-stack-dev/kotonoha-app/main/install.ps1 | iex
#
# Downloads the latest installer, checks it against the release's SHA256SUMS, runs it silently for the
# current user (no administrator rights needed) and starts Kotonoha.

$ErrorActionPreference = 'Stop'
# The progress bar slows Invoke-WebRequest down considerably on Windows PowerShell 5.1
$ProgressPreference = 'SilentlyContinue'

$repo = 'kk-stack-dev/kotonoha-app'
$base = if ($env:KOTONOHA_BASE) { $env:KOTONOHA_BASE } else { "https://github.com/$repo/releases/latest/download" }
$name = 'Kotonoha-windows-x64-setup.exe'

function Fail($message) {
  Write-Host "`nインストールできませんでした: $message" -ForegroundColor Red
  throw $message
}

if (-not [Environment]::Is64BitOperatingSystem) { Fail '64 ビット版の Windows が必要です' }

$tmp = Join-Path ([IO.Path]::GetTempPath()) ("kotonoha-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
  Write-Host 'Kotonoha をダウンロードしています'
  $setup = Join-Path $tmp $name
  $sums = Join-Path $tmp 'SHA256SUMS'
  try {
    Invoke-WebRequest -UseBasicParsing -Uri "$base/$name" -OutFile $setup
    Invoke-WebRequest -UseBasicParsing -Uri "$base/SHA256SUMS" -OutFile $sums
  } catch {
    Fail 'ダウンロードに失敗しました。インターネットの接続を確かめてください'
  }

  $expected = Get-Content $sums | ForEach-Object { $p = $_ -split '\s+'; if ($p[1] -eq $name) { $p[0] } } | Select-Object -First 1
  $actual = (Get-FileHash -Algorithm SHA256 -Path $setup).Hash.ToLowerInvariant()
  if (-not $expected -or $expected -ne $actual) { Fail 'ダウンロードしたファイルが壊れています。もう一度実行してください' }

  Get-Process -Name 'Kotonoha' -ErrorAction SilentlyContinue | Stop-Process -Force
  Write-Host 'インストールしています'
  $process = Start-Process -FilePath $setup -ArgumentList '/S' -Wait -PassThru
  if ($process.ExitCode -ne 0) { Fail "インストーラーが終了コード $($process.ExitCode) で止まりました" }

  $dir = Join-Path $env:LOCALAPPDATA 'Kotonoha'
  $exe = Get-ChildItem -Path $dir -Filter '*.exe' -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -notmatch '^uninstall' } | Select-Object -First 1
  Write-Host 'インストールしました'
  if ($exe) {
    Start-Process -FilePath $exe.FullName
    Write-Host 'セットアップ画面の案内に沿って進めてください'
  } else {
    Write-Host 'スタートメニューから Kotonoha を起動してください'
  }
} finally {
  Remove-Item -Recurse -Force -Path $tmp -ErrorAction SilentlyContinue
}
