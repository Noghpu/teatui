param(
    [string]$WslProjectPath = ""
)

$ErrorActionPreference = "Stop"

$projectEntries = @(
    ".gitignore",
    "AGENTS.md",
    "Cargo.lock",
    "Cargo.toml",
    "CLAUDE.md",
    "docs",
    "justfile",
    "scripts",
    "src",
    "tests"
)

$existingEntries = @(
    $projectEntries | Where-Object { Test-Path -LiteralPath $_ }
)

if ($existingEntries.Count -eq 0) {
    throw "No project files found to archive."
}

$archivePath = Join-Path ([System.IO.Path]::GetTempPath()) "teatui-wsl-build-$PID.tar"
$scriptPath = Join-Path ([System.IO.Path]::GetTempPath()) "teatui-wsl-build-$PID.sh"
$destArg = if ([string]::IsNullOrWhiteSpace($WslProjectPath)) { "__TEATUI_DEFAULT__" } else { $WslProjectPath }

try {
    if (Test-Path -LiteralPath $archivePath) {
        Remove-Item -LiteralPath $archivePath -Force
    }
    if (Test-Path -LiteralPath $scriptPath) {
        Remove-Item -LiteralPath $scriptPath -Force
    }

    tar.exe -cf $archivePath @existingEntries
    if ($LASTEXITCODE -ne 0) {
        throw "tar.exe failed while creating $archivePath"
    }

    $archiveWslPath = (wsl.exe --exec wslpath -a $archivePath).Trim()
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($archiveWslPath)) {
        throw "wslpath failed for $archivePath"
    }

    $script = @'
set -euo pipefail
archive="$1"
dest_spec="${2:-}"
if [ -z "$dest_spec" ] || [ "$dest_spec" = "__TEATUI_DEFAULT__" ]; then
    dest="$HOME/projects/teatui-rs/teatui"
else
    case "$dest_spec" in
        "~") dest="$HOME" ;;
        "~/"*) dest="$HOME/${dest_spec#~/}" ;;
        *) dest="$dest_spec" ;;
    esac
fi
case "$dest" in
    "$HOME/projects/"*) ;;
    *) echo "Refusing to replace destination outside $HOME/projects: $dest" >&2; exit 2 ;;
esac
mkdir -p -- "$dest"
cd "$dest"
for entry in .gitignore AGENTS.md Cargo.lock Cargo.toml CLAUDE.md docs justfile scripts src tests; do
    rm -rf -- "$entry"
done
tar -xf "$archive" -C "$dest"
if [ -f "$HOME/.cargo/env" ]; then
    . "$HOME/.cargo/env"
fi
cargo build
'@

    Set-Content -LiteralPath $scriptPath -Value $script -NoNewline -Encoding ascii
    $scriptWslPath = (wsl.exe --exec wslpath -a $scriptPath).Trim()
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($scriptWslPath)) {
        throw "wslpath failed for $scriptPath"
    }

    wsl.exe --exec bash $scriptWslPath $archiveWslPath $destArg
    exit $LASTEXITCODE
}
finally {
    if (Test-Path -LiteralPath $archivePath) {
        Remove-Item -LiteralPath $archivePath -Force
    }
    if (Test-Path -LiteralPath $scriptPath) {
        Remove-Item -LiteralPath $scriptPath -Force
    }
}
