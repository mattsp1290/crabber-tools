# Working in crabber-tools

This is a Rust 2024 workspace. Use the toolchain in rust-toolchain.toml.
Before committing, run:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --locked
cargo run -p mount-standard --locked -- --check
```

CI also checks for forbidden language constructs and runs the standalone
consumer-probe job. Search tests require ripgrep >=14 on PATH; shell tests need sh.
Use only public Crabber APIs and the immutable git revision in Cargo.toml.
Never commit a Crabber path override or a patch section. Keep its pin aligned
with crabber-extensions; coordinate pin bumps in one consumer cycle.

Hosts own workspace admission, permission policy, environment policy, process
uid, command_guard analysis and tool_result_redactor redaction. File operations
run as the host uid; run_as applies only to child processes.
Tests use scripted providers and synthetic fixtures without credentials or network.
Keep .agents/ and reviews/ ignored and local. Track issues with bn in the
crabber-tools project; record the implementing commit when closing an issue.
