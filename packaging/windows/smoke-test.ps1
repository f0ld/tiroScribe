# Installs the built app on a CI machine, starts it and takes a screenshot.
# usage: smoke-test.ps1 <installer> <output dir>
param([string]$Installer, [string]$Out)
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force $Out | Out-Null

Start-Process $Installer -ArgumentList '/S' -Wait
$dir = Join-Path $env:LOCALAPPDATA 'tiroScribe'
Get-ChildItem $dir | Format-Table Name, Length
foreach ($f in 'tiroscribe.exe', 'ffmpeg.exe', 'ffprobe.exe', 'vulkan-1.dll', 'THIRD-PARTY-LICENSES.txt') {
  if (-not (Test-Path (Join-Path $dir $f))) { throw "missing after install: $f" }
}
$exe = Join-Path $dir 'tiroscribe.exe'
$ffmpeg = Join-Path $dir 'ffmpeg.exe'

# One second of silence as 16 kHz mono WAV, decoded and encoded the way the app does.
$wav = Join-Path $Out 'silence.wav'
$w = [IO.BinaryWriter]::new([IO.File]::Create($wav))
$w.Write([Text.Encoding]::ASCII.GetBytes('RIFF')); $w.Write([int]32036)
$w.Write([Text.Encoding]::ASCII.GetBytes('WAVEfmt ')); $w.Write([int]16)
$w.Write([int16]1); $w.Write([int16]1); $w.Write([int]16000); $w.Write([int]32000)
$w.Write([int16]2); $w.Write([int16]16)
$w.Write([Text.Encoding]::ASCII.GetBytes('data')); $w.Write([int]32000)
$w.Write([byte[]]::new(32000)); $w.Close()
$raw = Join-Path $Out 'decoded.raw'
& $ffmpeg -nostdin -y -loglevel error -i $wav -ac 1 -ar 16000 -f f32le $raw
if ((Get-Item $raw).Length -ne 64000) { throw "ffmpeg decoded the wrong size: $((Get-Item $raw).Length)" }
$ogg = Join-Path $Out 'encoded.ogg'
& $ffmpeg -nostdin -y -loglevel error -i $wav -vn -ac 1 -c:a libopus -b:a 48k $ogg
if ((Get-Item $ogg).Length -eq 0) { throw 'ffmpeg wrote an empty Opus file' }
Write-Host 'ffmpeg works'

# CI has no GPU driver: the check may report none or crash, but must not hang or take the app down.
$probeOut = Join-Path $Out 'gpus.json'
$probe = Start-Process $exe -ArgumentList '--scribe-list-gpus' -PassThru -RedirectStandardOutput $probeOut
if (-not $probe.WaitForExit(60000)) { throw 'GPU check hung' }
Write-Host "GPU check exit code $($probe.ExitCode): $(Get-Content $probeOut -Raw)"

$app = Start-Process $exe -PassThru
Start-Sleep -Seconds 30
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
$bounds = [Windows.Forms.Screen]::PrimaryScreen.Bounds
$bmp = [Drawing.Bitmap]::new($bounds.Width, $bounds.Height)
$g = [Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($bounds.Location, [Drawing.Point]::Empty, $bounds.Size)
$bmp.Save((Join-Path $Out 'screenshot.png'))
if ($app.HasExited) { throw "the app exited with code $($app.ExitCode)" }
Write-Host 'app is running after 30 s'
Stop-Process $app
