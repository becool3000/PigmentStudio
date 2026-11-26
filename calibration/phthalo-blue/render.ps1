param(
    [Parameter(Mandatory = $true)]
    [string]$OutputDirectory,
    [string]$Studio = (Join-Path $PSScriptRoot '../../target/release/pigment-studio-ink.exe')
)

$ErrorActionPreference = 'Stop'
$executable = (Resolve-Path -LiteralPath $Studio).Path
$destination = (New-Item -ItemType Directory -Path $OutputDirectory -Force).FullName
$recording = Join-Path $PSScriptRoot 'recording.json'
$baseline = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'baseline.json') -Raw | ConvertFrom-Json
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('pigment-phthalo-' + [Guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Path $scratch

try {
    $litRecording = Join-Path $scratch 'lit.json'
    $lit = Get-Content -LiteralPath $recording -Raw | ConvertFrom-Json
    $lit.lighting.enabled = $true
    [IO.File]::WriteAllText($litRecording, ($lit | ConvertTo-Json -Depth 12))

    $unlitPng = Join-Path $destination 'unlit.png'
    $litPng = Join-Path $destination 'lit.png'
    $errorLog = Join-Path $scratch 'renderer-errors.txt'
    foreach ($job in @(
        @{ recording = $recording; output = $unlitPng }
        @{ recording = $litRecording; output = $litPng }
    )) {
        $arguments = @('--render', ('"{0}"' -f $job.recording), '--out', ('"{0}"' -f $job.output), '--mode', 'color')
        $process = Start-Process -FilePath $executable -ArgumentList $arguments `
            -WindowStyle Hidden -Wait -PassThru -RedirectStandardError $errorLog
        if ($process.ExitCode -ne 0) {
            $detail = Get-Content -LiteralPath $errorLog -Raw
            throw "Pigment Studio render failed with exit code $($process.ExitCode). $detail"
        }
    }

    Add-Type -AssemblyName System.Drawing
    $unlitImage = [Drawing.Bitmap]::new($unlitPng)
    try {
        $litImage = [Drawing.Bitmap]::new($litPng)
        try {
            $baseline.swatches | ForEach-Object {
                $unlit = $unlitImage.GetPixel($_.x, $_.y)
                $lit = $litImage.GetPixel($_.x, $_.y)
                [pscustomobject]@{
                    group = $_.group; swatch = $_.label; x = $_.x; y = $_.y
                    unlit_srgb = '#{0:X2}{1:X2}{2:X2}' -f $unlit.R, $unlit.G, $unlit.B
                    lit_srgb = '#{0:X2}{1:X2}{2:X2}' -f $lit.R, $lit.G, $lit.B
                }
            } | Export-Csv -LiteralPath (Join-Path $destination 'measurements.csv') -NoTypeInformation -Encoding ASCII
        } finally {
            $litImage.Dispose()
        }
    } finally {
        $unlitImage.Dispose()
    }
} finally {
    Remove-Item -LiteralPath (Join-Path $scratch 'lit.json'), (Join-Path $scratch 'renderer-errors.txt') -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $scratch -ErrorAction SilentlyContinue
}
