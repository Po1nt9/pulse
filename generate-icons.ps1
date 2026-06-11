Add-Type -AssemblyName System.Drawing

function New-PulseIcon {
    param([string]$path, [int]$size)
    $bmp = New-Object System.Drawing.Bitmap($size, $size)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = 'AntiAlias'
    $g.Clear([System.Drawing.Color]::Transparent)
    $brush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(88, 166, 255))
    $margin = [Math]::Max(1, [int]($size * 0.08))
    $g.FillEllipse($brush, $margin, $margin, $size - $margin * 2, $size - $margin * 2)
    $innerBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(63, 185, 80))
    $innerMargin = [int]($size * 0.3)
    $g.FillEllipse($innerBrush, $innerMargin, $innerMargin, $size - $innerMargin * 2, $size - $innerMargin * 2)
    $g.Dispose()
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    Write-Host "Created $path"
}

$iconDir = 'C:\Users\33993\.qoderworkcn\workspace\mq9dca2eetej1k3e\pulse\src-tauri\icons'
New-PulseIcon -path "$iconDir\32x32.png" -size 32
New-PulseIcon -path "$iconDir\128x128.png" -size 128
New-PulseIcon -path "$iconDir\128x128@2x.png" -size 256
New-PulseIcon -path "$iconDir\icon.png" -size 512

# Create ICO file (combine multiple sizes)
$sizes = @(16, 24, 32, 48, 64, 128, 256)
$icoBmps = @()
foreach ($s in $sizes) {
    $bmp = New-Object System.Drawing.Bitmap($s, $s)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = 'AntiAlias'
    $g.Clear([System.Drawing.Color]::Transparent)
    $brush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(88, 166, 255))
    $margin = [Math]::Max(1, [int]($s * 0.08))
    $g.FillEllipse($brush, $margin, $margin, $s - $margin * 2, $s - $margin * 2)
    $innerBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(63, 185, 80))
    $innerMargin = [int]($s * 0.3)
    $g.FillEllipse($innerBrush, $innerMargin, $innerMargin, $s - $innerMargin * 2, $s - $innerMargin * 2)
    $g.Dispose()
    $icoBmps += $bmp
}
$icoBmps[0].Save("$iconDir\icon.ico", [System.Drawing.Imaging.ImageFormat]::Icon)
foreach ($bmp in $icoBmps) { $bmp.Dispose() }

# Copy icon.png as icns placeholder (Tauri build will handle conversion on macOS)
Copy-Item "$iconDir\icon.png" "$iconDir\icon.icns" -Force

Write-Host "All icons generated successfully"
