Add-Type -AssemblyName System.Drawing
$bmp = New-Object System.Drawing.Bitmap 600, 400
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.Clear([System.Drawing.Color]::White)
$brush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::Navy)
$font = New-Object System.Drawing.Font('Arial', 22, [System.Drawing.FontStyle]::Bold)
$g.DrawString('Rusty-Print Test Page', $font, $brush, 40, 100)
$subFont = New-Object System.Drawing.Font('Arial', 14)
$subBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::DarkSlateGray)
$g.DrawString('Wireless Bridge Print Job Verification', $subFont, $subBrush, 40, 160)
$bmp.Save('test_page.png', [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose()
$bmp.Dispose()
Write-Output "Generated test_page.png successfully"
