# Generate placeholder app + tray icons.
Add-Type -AssemblyName System.Drawing

$outDir = Join-Path $PSScriptRoot "..\app\src-tauri\icons"
$null = New-Item -ItemType Directory -Force -Path $outDir

function New-AppIcon($size, $path) {
    $bmp = New-Object System.Drawing.Bitmap $size, $size
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = 'AntiAlias'
    $g.Clear([System.Drawing.Color]::Transparent)
    # Background gradient (deep blue → cyan)
    $rect = New-Object System.Drawing.RectangleF 0, 0, $size, $size
    $brush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
        $rect,
        [System.Drawing.Color]::FromArgb(255, 12, 32, 64),
        [System.Drawing.Color]::FromArgb(255, 32, 96, 192),
        45.0
    )
    # Rounded square background
    $r = [Math]::Max(2, [int]($size * 0.18))
    $path2 = New-Object System.Drawing.Drawing2D.GraphicsPath
    $path2.AddArc(0, 0, $r*2, $r*2, 180, 90)
    $path2.AddArc($size - $r*2, 0, $r*2, $r*2, 270, 90)
    $path2.AddArc($size - $r*2, $size - $r*2, $r*2, $r*2, 0, 90)
    $path2.AddArc(0, $size - $r*2, $r*2, $r*2, 90, 90)
    $path2.CloseFigure()
    $g.FillPath($brush, $path2)

    # Sun: yellow circle with rays
    $cx = $size / 2.0
    $cy = $size / 2.0
    $sunR = [int]($size * 0.22)
    $rayR = [int]($size * 0.38)
    $rayInner = [int]($size * 0.30)
    $pen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(255, 255, 220, 80), [Math]::Max(1, $size/64.0))
    for ($i=0; $i -lt 12; $i++) {
        $a = $i * 30.0 * [Math]::PI / 180.0
        $x1 = $cx + [Math]::Cos($a) * $rayInner
        $y1 = $cy + [Math]::Sin($a) * $rayInner
        $x2 = $cx + [Math]::Cos($a) * $rayR
        $y2 = $cy + [Math]::Sin($a) * $rayR
        $g.DrawLine($pen, $x1, $y1, $x2, $y2)
    }
    $sunBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 255, 220, 80))
    $g.FillEllipse($sunBrush, $cx - $sunR, $cy - $sunR, $sunR*2, $sunR*2)

    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
}

function New-TrayIcon($size, $path) {
    $bmp = New-Object System.Drawing.Bitmap $size, $size
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = 'AntiAlias'
    $g.Clear([System.Drawing.Color]::Transparent)
    $cx = $size / 2.0
    $cy = $size / 2.0
    $sunR = [int]($size * 0.22)
    $rayR = [int]($size * 0.45)
    $rayInner = [int]($size * 0.30)
    $color = [System.Drawing.Color]::FromArgb(255, 0, 0, 0)
    $pen = New-Object System.Drawing.Pen($color, [Math]::Max(1, $size/16.0))
    for ($i=0; $i -lt 8; $i++) {
        $a = $i * 45.0 * [Math]::PI / 180.0
        $x1 = $cx + [Math]::Cos($a) * $rayInner
        $y1 = $cy + [Math]::Sin($a) * $rayInner
        $x2 = $cx + [Math]::Cos($a) * $rayR
        $y2 = $cy + [Math]::Sin($a) * $rayR
        $g.DrawLine($pen, $x1, $y1, $x2, $y2)
    }
    $sunBrush = New-Object System.Drawing.SolidBrush($color)
    $g.FillEllipse($sunBrush, $cx - $sunR, $cy - $sunR, $sunR*2, $sunR*2)
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
}

New-AppIcon 32  (Join-Path $outDir "32x32.png")
New-AppIcon 128 (Join-Path $outDir "128x128.png")
New-AppIcon 256 (Join-Path $outDir "128x128@2x.png")
New-AppIcon 512 (Join-Path $outDir "icon.png")
New-TrayIcon 32 (Join-Path $outDir "tray.png")

# Build .ico containing 16/32/48/256
function New-Ico {
    param($pngPaths, $icoPath)
    $sizes = @()
    $datas = @()
    foreach ($p in $pngPaths) {
        $bytes = [System.IO.File]::ReadAllBytes($p)
        $datas += ,$bytes
    }
    $count = $pngPaths.Count
    $offset = 6 + ($count * 16)
    $stream = New-Object System.IO.MemoryStream
    $bw = New-Object System.IO.BinaryWriter $stream
    $bw.Write([uint16]0)
    $bw.Write([uint16]1)
    $bw.Write([uint16]$count)
    foreach ($i in 0..($count-1)) {
        $img = [System.Drawing.Image]::FromFile($pngPaths[$i])
        $w = $img.Width; $h = $img.Height
        $img.Dispose()
        $size = $datas[$i].Length
        $bw.Write([byte]([Math]::Min($w, 255)))
        $bw.Write([byte]([Math]::Min($h, 255)))
        $bw.Write([byte]0)
        $bw.Write([byte]0)
        $bw.Write([uint16]1)
        $bw.Write([uint16]32)
        $bw.Write([uint32]$size)
        $bw.Write([uint32]$offset)
        $offset += $size
    }
    foreach ($d in $datas) { $bw.Write($d) }
    $bw.Flush()
    [System.IO.File]::WriteAllBytes($icoPath, $stream.ToArray())
    $bw.Close()
}

# Build small PNGs for ico
$small = Join-Path $outDir "_tmp"
$null = New-Item -ItemType Directory -Force -Path $small
foreach ($s in 16, 32, 48, 64, 256) {
    New-AppIcon $s (Join-Path $small "$s.png")
}
New-Ico @(
    (Join-Path $small "16.png"),
    (Join-Path $small "32.png"),
    (Join-Path $small "48.png"),
    (Join-Path $small "64.png"),
    (Join-Path $small "256.png")
) (Join-Path $outDir "icon.ico")
Remove-Item -Recurse -Force $small

Write-Host "Icons generated under $outDir"
