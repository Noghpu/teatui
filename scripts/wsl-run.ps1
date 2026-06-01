$ErrorActionPreference = "Stop"

$WslProjectPath = if ([string]::IsNullOrWhiteSpace($env:TEATUI_WSL_PROJECT_PATH)) {
    ""
} else {
    $env:TEATUI_WSL_PROJECT_PATH
}
$CargoRunArgs = $args
$scriptPath = Join-Path ([System.IO.Path]::GetTempPath()) "teatui-wsl-run-$PID.sh"
$destArg = if ([string]::IsNullOrWhiteSpace($WslProjectPath)) { "__TEATUI_DEFAULT__" } else { $WslProjectPath }

try {
    if (Test-Path -LiteralPath $scriptPath) {
        Remove-Item -LiteralPath $scriptPath -Force
    }

    $script = @'
set -euo pipefail
dest_spec="${1:-}"
shift
if [ -z "$dest_spec" ] || [ "$dest_spec" = "__TEATUI_DEFAULT__" ]; then
    dest="$HOME/projects/teatui-rs/teatui"
else
    case "$dest_spec" in
        "~") dest="$HOME" ;;
        "~/"*) dest="$HOME/${dest_spec#~/}" ;;
        *) dest="$dest_spec" ;;
    esac
fi
cd "$dest"
if [ -f "$HOME/.cargo/env" ]; then
    . "$HOME/.cargo/env"
fi
exec cargo run "$@"
'@

    Set-Content -LiteralPath $scriptPath -Value $script -NoNewline -Encoding ascii
    $scriptWslPath = (wsl.exe --exec wslpath -a $scriptPath).Trim()
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($scriptWslPath)) {
        throw "wslpath failed for $scriptPath"
    }

    wsl.exe --exec bash $scriptWslPath $destArg @CargoRunArgs
    exit $LASTEXITCODE
}
finally {
    if (Test-Path -LiteralPath $scriptPath) {
        Remove-Item -LiteralPath $scriptPath -Force
    }
}
