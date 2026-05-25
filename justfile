set shell := ["powershell", "-NoLogo", "-Command"]

fmt:
    cargo fmt --check

check:
    cargo check

clippy:
    cargo clippy --all-targets --all-features -- -D warnings

test:
    cargo test --all-targets --all-features

verify: fmt check clippy test
