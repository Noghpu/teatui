set shell := ["powershell", "-NoLogo", "-Command"]

build:
    cargo build --release

build-linux:
    cargo zigbuild --release --target x86_64-unknown-linux-musl

build-windows:
    cargo build --release --target x86_64-pc-windows-msvc

fmt:
    cargo fmt --check

check:
    cargo check

clippy:
    cargo clippy --all-targets --all-features -- -D warnings

test:
    cargo test --all-targets --all-features

verify: fmt check clippy test

# Opt-in live smoke helper.
# Required gate: TEATUI_SMOKE_LIVE=1.
# Typical environment: TEATUI_SMOKE_MODEL, TEATUI_SMOKE_LLAMA_SERVER,
# TEATUI_SMOKE_LLAMA_URL, TEATUI_SMOKE_WORKSPACE pointing at a disposable jj
# repo, and either TEATUI_SMOKE_GITEA_URL or TEATUI_SMOKE_WSL_DISTRO.
smoke-live:
    cargo run --quiet --bin smoke-live --
