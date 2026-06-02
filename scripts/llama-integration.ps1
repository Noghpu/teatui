# Starts a local llama.cpp server, runs the (normally-ignored) llama
# integration tests against it, then stops the server.
#
# Zero-config on a machine where llama.cpp-vulkan is installed via scoop;
# override any piece via environment variables:
#   TEATUI_LLAMA_SERVER      llama-server binary    (default: llama-server)
#   TEATUI_LLAMA_MODEL_PATH  path to a .gguf file   (default: first gguf in
#                            scoop\persist\llama.cpp-vulkan\models)
#   TEATUI_LLAMA_PORT        listen port            (default: 8080)
#   TEATUI_LLAMA_MODEL       served model alias     (default: qwen3.5-4b)
$ErrorActionPreference = "Stop"

$server = if ($env:TEATUI_LLAMA_SERVER) { $env:TEATUI_LLAMA_SERVER } else { "llama-server" }
$port = if ($env:TEATUI_LLAMA_PORT) { $env:TEATUI_LLAMA_PORT } else { "8080" }
$alias = if ($env:TEATUI_LLAMA_MODEL) { $env:TEATUI_LLAMA_MODEL } else { "qwen3.5-4b" }

$modelPath = $env:TEATUI_LLAMA_MODEL_PATH
if (-not $modelPath) {
    $modelsDir = Join-Path $env:USERPROFILE "scoop\persist\llama.cpp-vulkan\models"
    if (Test-Path $modelsDir) {
        $gguf = Get-ChildItem $modelsDir -Filter *.gguf | Select-Object -First 1
        if ($gguf) { $modelPath = $gguf.FullName }
    }
}
if (-not $modelPath -or -not (Test-Path $modelPath)) {
    throw "No model found. Set TEATUI_LLAMA_MODEL_PATH to a .gguf file."
}

Write-Host "Starting $server on 127.0.0.1:$port with $modelPath"
# `--reasoning off` disables chain-of-thought so the model returns the
# requested JSON directly in `message.content`. teatui wants a JSON draft,
# not reasoning, and a thinking model otherwise spends its token budget in
# `reasoning_content` leaving `content` empty.
$proc = Start-Process -FilePath $server -PassThru -NoNewWindow -ArgumentList @(
    "-m", $modelPath,
    "--host", "127.0.0.1",
    "--port", $port,
    "--alias", $alias,
    "--reasoning", "off",
    "-c", "4096"
)

try {
    $healthUrl = "http://127.0.0.1:$port/health"
    $ready = $false
    foreach ($i in 1..120) {
        if ($proc.HasExited) { throw "llama-server exited early (code $($proc.ExitCode))." }
        try {
            $resp = Invoke-WebRequest -Uri $healthUrl -TimeoutSec 2 -UseBasicParsing
            if ($resp.StatusCode -eq 200) { $ready = $true; break }
        }
        catch { Start-Sleep -Seconds 1 }
    }
    if (-not $ready) { throw "Server did not become healthy at $healthUrl in time." }
    Write-Host "Server healthy. Running integration tests..."

    $env:TEATUI_LLAMA_URL = "http://127.0.0.1:$port"
    $env:TEATUI_LLAMA_MODEL = $alias
    cargo test --test llama_integration -- --ignored --nocapture
    exit $LASTEXITCODE
}
finally {
    Write-Host "Stopping llama-server (PID $($proc.Id))"
    if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }
}
