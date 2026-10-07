# Allowed metadata differences

Only these exact changes are permitted by the parity test. All other names,
descriptions, and schema fields match the extracted upstream fixtures.

```json
[
  {
    "tool": "search",
    "pointer": "/description",
    "upstream": "Search workspace files via ripgrep. Defaults to regex mode and preserves existing pattern/path/timeout behavior. Optional glob filters, literal mode, ignore-case mode, context lines, and match limit are supported. Per-call timeout defaults to 60s and is capped at 600s.",
    "rust": "Search workspace files via ripgrep. Defaults to regex mode and preserves existing pattern/path/timeout behavior. Optional glob filters, literal mode, ignore-case mode, context lines, and match limit are supported. Per-call timeout defaults to 60s and is capped at 600s. The host resolves the ripgrep binary once; configuration files are always disabled.",
    "reason": "Host-owned binary/environment and explicit output policy; overview decisions 8-10 and deviation table."
  },
  {
    "tool": "search",
    "pointer": "/parameters/properties/limit/minimum",
    "upstream": 1,
    "rust": 0,
    "reason": "Zero selects default limit, as specified in 04-search; explicit schema deviation."
  },
  {
    "tool": "shell",
    "pointer": "/description",
    "upstream": "Run a command with host-configured shell policy in the agent's workspace cwd. Captures stdout, stderr, exit code, and duration. Per-call timeout defaults to 60s and is capped at 600s. Stdout/stderr use host-configured per-stream caps (default 256 KiB); oversize output sets truncated=true.",
    "rust": "Run a command with host-configured shell policy in the agent's workspace cwd. Captures stdout, stderr, exit code, and duration. Per-call timeout defaults to 60s and is capped at 600s. Stdout/stderr use host-configured per-stream caps (explicitly supplied by the host); oversize output sets truncated=true.",
    "reason": "Host-owned binary/environment and explicit output policy; overview decisions 8-10 and deviation table."
  }
]
```
