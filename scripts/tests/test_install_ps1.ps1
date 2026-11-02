$ErrorActionPreference = 'Stop'
$installer = Join-Path $PSScriptRoot '../../install.ps1'
$tokens = $null; $errors = $null
[void][System.Management.Automation.Language.Parser]::ParseFile($installer, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw "PowerShell syntax errors: $errors" }
$script:calls = @()
function global:wsl.exe {
    $script:calls += ,@($args)
    $global:LASTEXITCODE = 0
}
try {
    & $installer -Version v1.2.3 -Method curl
    if ($script:calls.Count -ne 2 -or $script:calls[1][-2] -ne 'curl' -or $script:calls[1][-1] -ne 'v1.2.3') { throw 'WSL arguments were not forwarded' }
    function global:wsl.exe { $global:LASTEXITCODE = 1 }
    $failed = $false
    try { & $installer } catch { $failed = $_.ToString() -match 'WSL2' }
    if (-not $failed) { throw 'Missing distribution must fail with guidance' }
} finally {
    Remove-Item Function:\wsl.exe -ErrorAction SilentlyContinue
}
