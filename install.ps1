param(
    [string]$Version = 'latest',
    [ValidateSet('auto', 'gh', 'curl')][string]$Method = 'auto'
)
$ErrorActionPreference = 'Stop'
if ($Version -cnotmatch '^(latest|v\d+\.\d+\.\d+([-.][A-Za-z0-9.-]+)?)\z') {
    throw 'Invalid version: use latest or a release tag such as v0.0.0.'
}
$Method = $Method.ToLowerInvariant()
function ConvertTo-EncodedScript([string]$Script) {
    $normalized = $Script.Replace("`r`n", "`n").Replace("`r", "`n")
    return [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($normalized))
}
if (-not (Get-Command wsl.exe -ErrorAction SilentlyContinue)) {
    throw 'WSL is missing. Install WSL2 and a Linux distribution with wsl --install, then retry: https://learn.microsoft.com/windows/wsl/install'
}
# Starting a command also verifies that a default distribution is configured.
$probeScript = @'
case "$(uname -r)" in *[Ww][Ss][Ll]2*) exit 0;; *) exit 1;; esac
'@
$probeEncoded = ConvertTo-EncodedScript $probeScript
& wsl.exe --exec bash -c "printf %s $probeEncoded | base64 -d | bash"
if ($LASTEXITCODE -ne 0) {
    throw 'A working default WSL2 Linux distribution is required. Run wsl --install or select one with wsl --set-default <distribution>, then retry.'
}
# Authentication is deliberately performed inside WSL, where the binaries will run.
$installScript = @'
set -euo pipefail
method=$1; version=$2
if [ "$method" = auto ]; then if command -v gh >/dev/null; then method=gh; else method=curl; fi; fi
if [ "$method" = gh ]; then
  gh api -H "Accept: application/vnd.github.raw+json" "repos/datamonsterr/tokenimum/contents/install.sh?ref=main" | bash -s -- --method gh --version "$version"
else
  if [ -n "${GH_TOKEN:-${GITHUB_TOKEN:-}}" ]; then
    curl -fsSL -H "Authorization: Bearer ${GH_TOKEN:-$GITHUB_TOKEN}" -H "Accept: application/vnd.github.raw+json" "https://api.github.com/repos/datamonsterr/tokenimum/contents/install.sh?ref=main" | bash -s -- --method curl --version "$version"
  else
    curl -fsSL "https://raw.githubusercontent.com/datamonsterr/tokenimum/main/install.sh" | bash -s -- --method curl --version "$version"
  fi
fi
'@
$installEncoded = ConvertTo-EncodedScript $installScript
# The command contains no double quotes, including on Windows PowerShell 5.1.
& wsl.exe --exec bash -c "printf %s $installEncoded | base64 -d | bash -s -- $Method $Version"
if ($LASTEXITCODE -ne 0) { throw 'Installation inside WSL failed. Authenticate gh or set GH_TOKEN inside the default WSL distribution, and check the release version.' }
Write-Host 'Run tokenimum and your coding agent inside the same WSL distribution.'
