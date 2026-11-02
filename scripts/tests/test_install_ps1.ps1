$ErrorActionPreference = 'Stop'
$installer = Join-Path $PSScriptRoot '../../install.ps1'
$tokens = $null; $errors = $null
[void][System.Management.Automation.Language.Parser]::ParseFile($installer, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw "PowerShell syntax errors: $errors" }
$global:tokenimumTestCalls = @()
function global:wsl.exe {
    $global:tokenimumTestCalls += ,@($args)
    $global:LASTEXITCODE = 0
}
try {
    & $installer -Version v1.2.3 -Method curl
    if ($global:tokenimumTestCalls.Count -ne 2) { throw 'Expected probe and install calls' }
    $source = [IO.File]::ReadAllText($installer)
    $names = @('probeScript', 'installScript')
    for ($i = 0; $i -lt 2; $i++) {
        foreach ($argument in $global:tokenimumTestCalls[$i]) {
            if ($argument.Contains('"')) { throw 'WSL argument contains a double quote' }
        }
        $command = $global:tokenimumTestCalls[$i][-1]
        if ($command -notmatch '^printf %s ([A-Za-z0-9+/=]+) \| base64 -d \| bash') { throw 'Unexpected WSL command' }
        $decoded = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($Matches[1]))
        $pattern = '(?s)\$' + $names[$i] + " = @'\r?\n(.*?)\r?\n'@"
        $match = [regex]::Match($source, $pattern)
        if (-not $match.Success -or $decoded -cne $match.Groups[1].Value.Replace("`r`n", "`n")) { throw 'Encoded script did not round-trip' }
    }
    if ($global:tokenimumTestCalls[1][-1] -notmatch 'bash -s -- curl v1\.2\.3$') { throw 'WSL arguments were not forwarded' }
    foreach ($badVersion in @('v1.2.3;touch', 'v1.2', 'latest ', 'LATEST')) {
        $failed = $false
        try { & $installer -Version $badVersion } catch { $failed = $true }
        if (-not $failed) { throw 'Invalid version was accepted' }
    }
    if ($global:tokenimumTestCalls.Count -ne 2) { throw 'Invalid version reached WSL' }
    function global:wsl.exe { $global:LASTEXITCODE = 1 }
    $failed = $false
    try { & $installer } catch { $failed = $_.ToString() -match 'WSL2' }
    if (-not $failed) { throw 'Missing distribution must fail with guidance' }
} finally {
    Remove-Item Function:\wsl.exe -ErrorAction SilentlyContinue
    Remove-Variable tokenimumTestCalls -Scope Global -ErrorAction SilentlyContinue
    $global:LASTEXITCODE = 0
}
