set shell := ["powershell", "-NoLogo", "-Command"]

build:
    cargo build --release

build-linux:
    cargo zigbuild --release --target x86_64-unknown-linux-musl

wsl-build:
    powershell -NoLogo -ExecutionPolicy Bypass -File .\scripts\wsl-build.ps1

wsl-run *ARGS:
    powershell -NoLogo -ExecutionPolicy Bypass -File .\scripts\wsl-run.ps1 {{ARGS}}

fmt:
    cargo fmt --check

check:
    cargo check

clippy:
    cargo clippy --all-targets --all-features -- -D warnings

test:
    cargo test --all-targets --all-features

verify: fmt check clippy test

snapshots:
    cargo run --quiet --bin ui-snapshots --

# Live llama.cpp integration tests. NOT part of `just test` (the tests are
# #[ignore]d). Starts a local llama.cpp server, runs them, then stops it.
# See scripts/llama-integration.ps1 for env overrides.
integration:
    powershell -NoLogo -ExecutionPolicy Bypass -File .\scripts\llama-integration.ps1
