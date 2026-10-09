# crabber-tools

Workspace coding tools for Crabber, mirroring eino-tools: `file_read`, `file_write`,
`file_edit`, `file_list`, `search`, `shell`, `glob`, `apply_patch`, `url_fetch`,
and `tracker_write`. One catalog extension mounts a
host-chosen subset with bounded work, capability-rooted file access, and process
group cleanup on cancellation. Unix only; Rust 2024, toolchain 1.99.0.

Depend on `crabber-tools-catalog` at an immutable git revision. This workspace
pins Crabber at `883189465397b2fd326a6ef358c8a2fbd39fbec8`, matching
crabber-extensions `8c611687723a79944226fa61501be64e4e81a74f`. All three must use
one Crabber revision. These repositories are public; CI rewrites SSH dependency
URLs to HTTPS and optionally uses `CRABBER_READ_TOKEN` when configured. Enable the `CONSUMER_PROBE=true` repository variable for the
standalone consumer CI job.

## Quick start

The application needs `crabber`, `crabber-tools-catalog`, `tokio`, and `tempfile`.
The host explicitly admits the workspace and chooses every process policy.

```rust
use std::{sync::Arc, time::Duration};
use crabber::{Agent, AgentConfig, FakeProvider, PermissionDecision, Selection,
              StaticPolicy, StreamDelta, extension::Scope};
use crabber_tools_catalog::prelude::*;

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let directory = tempfile::tempdir()?;
let root = WorkspaceRoot::open(directory.path())?;
let tools = StandardTools::new(Options {
    root: root.clone(),
    enabled: EnabledSet::only([ToolId::FileRead, ToolId::Search, ToolId::Shell]),
    restrict_to_enabled: false,
    shell: ShellPolicy {
        shell_binary: "/bin/sh".into(),
        startup_mode: StartupMode::NonLogin,
        env: EnvPolicy::minimal_allowlist(),
        run_as: None,
        output_cap_bytes: 65536,
    },
    search: SearchPolicy::resolve_rg_from_path(EnvPolicy::minimal_allowlist())?,
    url_fetch: None,
    tracker: None,
    limits: Limits { max_in_flight: 4, max_blocking_wait: Duration::from_secs(2) },
})?;
let mut config = AgentConfig::new(Selection {
    provider_id: "fake".into(), model_id: "scripted".into(),
});
config.directory = root.path().to_string_lossy().into_owned();
let agent = Agent::builder().memory()
    .config(config)
    .provider(Arc::new(FakeProvider::scripted(vec![vec![
        StreamDelta::TextDelta("ready".into()), StreamDelta::Completed,
    ]])))
    .policy(Arc::new(StaticPolicy::new(PermissionDecision::Allow)))
    .extension(Arc::new(tools), Scope::Global).build()?;
agent.prompt(None, "Ready?").await?.done().await?;
agent.close_extensions().await?;
# Ok(())
# }
```

Run the full credential-free example with `cargo run -p mount-standard --locked -- --check`.
It mounts standard tools beside a host extension and host tool, demonstrates a
missing allowlisted tool and a permission-denied shell call, prints tool events,
and checks persisted results.

## Crates

| Crate | Responsibility |
| --- | --- |
| crabber-tools-core | Envelopes, capabilities, bounds, locks, process policy and identities |
| crabber-tools-fileops | Reads, atomic writes, anchored edits and directory listings |
| crabber-tools-search | Bounded streaming ripgrep JSON search; requires rg >=14 |
| crabber-tools-shell | Explicit login/non-login shell policy, bounded output and timeouts |
| crabber-tools-glob | Sorted capability-confined path discovery |
| crabber-tools-applypatch | Preflighted multi-file patches with atomic per-file commits |
| crabber-tools-urlfetch | Policy-constrained HTTPS and workspace-confined file text |
| crabber-tools-trackerwrite | Explicit bn-backed tracker mutations with bounded lifecycle |
| crabber-tools-catalog | Deterministic metadata, allowlists, Extension and host prelude |

## Host responsibilities

Hosts choose the workspace root, permissions, environment, and uid. Fileops run
as the host uid; `run_as` applies only to spawned processes. Mount
crabber-extensions `command_guard` for command analysis and `tool_result_redactor`
for secret redaction. Shell execution is not a sandbox. `required_permissions`
is advisory; StaticPolicy matches tool names. Prefer `restrict_to_enabled: false`
when mounting other tools, because restrictions apply to the entire run.

Use one global root per agent, or Scope::Session for a shared agent. Multiple
roots mounted globally collide on names. The persisted directory must match the
admitted absolute root; AgentConfig's default `.` is rejected. Reopen the root
handle after deleting and recreating a directory. Hard links and detached
processes remain host concerns. Reads are unordered against in-flight writers.
Started blocking writes may complete after interruption, always atomically.

See [the tool contract](docs/tool-contract.md) for exact schemas, envelopes,
limits, identity inputs, accepted search path races, and deliberate deviations.
Golden upstream fixtures and their allowlist live in [fixtures/eino-tools](fixtures/eino-tools).

## Verification

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --locked
cargo run -p mount-standard --locked -- --check
```

Tests use scripted providers, temporary workspaces, `sh`, and `rg` without
credentials or network. CI runs Linux and macOS and a standalone consumer graph
with crabber-extensions' command guard and a host tool. The consumer template
substitutes the candidate SHA in both manifest and lockfile before `--locked`.

The first deliverable for request `crabber-r-5fxy` is tagged `v0.1.0` at
`3abb69425ed2d3beda740b0896cd0c59e90bdc81`. The second deliverable adds the four
crates above with the owner-approved URL policy and bn backend.

`EnabledSet::all()` includes all ten tools. Enabling `UrlFetch` requires
`url_fetch: Some(UrlFetchPolicy::default())`; that policy denies private and
special-purpose destinations by default and has an empty host allowlist.
Enabling `TrackerWrite` requires `tracker: Some(TrackerPolicy { .. })` with an
absolute bn executable, explicit hub/project/actor, allowed workflow states and
replacement environment. The host supplies these values from its configuration;
there is no inherited tracker namespace. Keep the policy `None` when the tool is
disabled. Missing required policies fail construction instead of omitting tools.
Any supplied policy is validated, even for a disabled tool.

The example retains a first-deliverable subset and leaves both optional policies
`None`; it requires no tracker installation or network. The all-ten-tools catalog
runtime test uses a synthetic bn backend and a confined file URL. Tracker writes
mutate the hub and are not retry-safe, but do not take the workspace writer lock.
All ten tools share mount capacity. Consumer `flows-guest` evidence and the owner's
request-scope decision remain required before resolving the parent request.

To materialize the standalone consumer outside this workspace, copy both the
template and its canonical harness, then replace `CRABBER_TOOLS_REV` in the
scratch manifest and lockfile with the published commit SHA:

```sh
probe_dir=$(mktemp -d)
cp -R ci/consumer-probe/. "$probe_dir/"
cp examples/mount-standard/src/probe.rs "$probe_dir/src/probe.rs"
```

CI performs these same copies before its locked graph check and probe run.
