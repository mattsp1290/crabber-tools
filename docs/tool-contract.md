# Tool contract

A success carries `outcome: "succeeded"` and normally omits `error`. Search partial
successes carry `partial: true` and a sanitized `exec_failed` error; retain their
collected matches. A model-facing failure
carries `outcome: "failed"` and `error: {category, message}`. Shell nonzero exits
remain successful calls; read `exit_code`. Tool timeouts are failed/timeout with
`timed_out: true`; `timed_out` and `rejected` outcomes are reserved for callers.
Errors built by this library never contain absolute host paths, command output,
or file contents. Search diagnostics are sanitized to enforce this rule.

The host admits an absolute existing workspace once, as a capability directory.
Every persisted session directory must be absent or an absolute canonical match;
the default AgentConfig directory `.` fails with `workspace_mismatch`. Reopen
WorkspaceRoot after replacing the directory at the same path. Relative in-root
symlinks work; absolute targets (even in-root) and escaping targets fail with
`path_escape`. Parent traversal is rejected syntactically. Hard links are a host
concern. Filesystem operations run as the host uid and preserve mode on atomic
writes; new files use 0644. Ownership is copied when the host is root or the
desired owner is the host uid; an attempted chown failure returns `io`. Other
unprivileged replacements keep the new file’s host ownership. A crash may leave a `.crabber-tools-tmp-*`
sibling, visible to file_list. Writes sync their temp file before rename.

Cancellation produces no model-facing result: Crabber drops the executor future.
A pre-cancelled token fails before any effect. A started blocking file operation
may finish after interruption; its capacity and writer guards remain owned by the
closure. Panics yield failed/unknown and release guards. Process guards kill the
whole process group on future drop. Timeouts allow five seconds for reaping;
leader exit allows five seconds for inherited pipes. Detached descendants or
children that close their pipes are not tracked; hosts own their lifecycle.

Mutating tools (write, edit, shell) share a process-global lock keyed by canonical
root. Read, list and search take capacity only and are unordered against writes.
One deadline bounds the combined lock and capacity wait; expiry is `unavailable`.
Sorted directory listings use an iterative traversal with at most 5000 pending
names across the entire tree and a constant number of open directories; enumeration
must inspect directory names to establish ordering, even after the output cap.

Registering only enabled tools is the per-node allowlist. `restrict_to_enabled`
is run-wide and denies other extensions and host tools; use false for mixed
mounts. Mount one root per agent or use Scope::Session on a shared agent; two
global roots collide on tool names. `required_permissions` is advisory metadata,
not enforced by StaticPolicy: hosts enforce permission rules by name or supply
their own policy. Hosts also own command_guard analysis and output redaction.

Process policy explicitly chooses Inherit, Replace, or Allowlist and optional
run_as uid/gid. Minimal allowlist retains PATH, HOME, LANG and TERM. Supply PATH
for shell command lookup. Login shells may source host startup files. Shell and
search binaries are absolute and validated once; replacing binaries later is a
host concern. Search always disables ripgrep config and clears RIPGREP_CONFIG_PATH.
Search validates paths through the capability, then rg resolves them again:
symlink replacement between these steps is an accepted TOCTOU window. rg does
not follow symlink directories by default.

Schema identity hashes version, name, description and recursively sorted schema
keys. Extension identity includes version, enabled ids and schema hashes in
canonical order, policies (including environment and uid), bounds, root path and
restriction mode. Runtime resources and hooks are not serializable identities.

Metadata differences are enumerated in
[ALLOWED-DIFFS](../fixtures/eino-tools/ALLOWED-DIFFS.md). Cancellation, capability
symlink rules, sanitized errors, explicit process policies, mutating-only locks,
ownership preservation, fsync, and pipe grace are deliberate behavior differences
from eino-tools. Search `limit: 0` selects 200 and is reflected in its schema.

## file_read

Id: `standard.file-read`. retry_safe: `true`. Permissions: `workspace.fs.read`.

Read a workspace-relative UTF-8 text file. Plain {path} calls return the leading content prefix capped at 256 KiB. Supplying offset and/or limit returns a line-window with raw and numbered content. Returns structured errors including path_escape, not_found, is_directory, binary, validation, and io.

```json
{
  "additionalProperties": false,
  "properties": {
    "limit": {
      "description": "Optional number of lines for a line-windowed read. Default 2000 when offset or limit is present; cap is 5000.",
      "maximum": 5000,
      "minimum": 1,
      "type": "integer"
    },
    "offset": {
      "description": "Optional 1-based starting line for a line-windowed read. When omitted, legacy prefix reads are preserved unless limit is present.",
      "minimum": 1,
      "type": "integer"
    },
    "path": {
      "description": "Workspace-relative path of the file to read.",
      "minLength": 1,
      "type": "string"
    }
  },
  "required": [
    "path"
  ],
  "type": "object"
}
```

Success keys: outcome, path, content, content_bytes (empty content omitted). Window mode adds numbered_content, line_start, line_end, total_lines, next_offset, line_truncated, truncation_reason and truncated when nonzero/true.

Limits: Prefix 256 KiB; default window 2000, maximum 5000 lines; line cap 16 KiB. NUL/invalid UTF-8 is binary. Offset past EOF is validation.

Failure categories: validation, path_escape, not_found, is_directory, not_directory, io, unknown, workspace_mismatch, unavailable, binary, too_large.

## file_write

Id: `standard.file-write`. retry_safe: `false`. Permissions: `workspace.fs.write`.

Write (create or overwrite) a workspace-relative file. Content is capped at 256 KiB. Optional create_dirs=true mkdir -p's the parent chain. Returns a structured error envelope on path_escape, not_found (missing parent), too_large, or io.

```json
{
  "additionalProperties": false,
  "properties": {
    "content": {
      "description": "File content. May be empty (truncates the target to 0 bytes).",
      "type": "string"
    },
    "create_dirs": {
      "description": "If true, mkdir -p the parent chain. Default false.",
      "type": "boolean"
    },
    "path": {
      "description": "Workspace-relative target file.",
      "minLength": 1,
      "type": "string"
    }
  },
  "required": [
    "path",
    "content"
  ],
  "type": "object"
}
```

Success keys: outcome, path, bytes_written, created.

Limits: Input 256 KiB. Atomic sibling rename, optional parent creation, preserved permissions/ownership.

Failure categories: validation, path_escape, not_found, is_directory, not_directory, io, unknown, workspace_mismatch, unavailable, too_large.

## file_edit

Id: `standard.file-edit`. retry_safe: `false`. Permissions: `workspace.fs.write`.

Edit a workspace-relative file in place by anchored substring replacement. The anchor MUST appear exactly once in the file. Returns structured error envelopes on path_escape, not_found, is_directory, anchor_not_found, anchor_ambiguous, too_large, or io.

```json
{
  "additionalProperties": false,
  "properties": {
    "anchor": {
      "description": "Literal substring to find. Must match exactly once. No regex.",
      "minLength": 1,
      "type": "string"
    },
    "path": {
      "description": "Workspace-relative file to edit. Must exist.",
      "minLength": 1,
      "type": "string"
    },
    "replacement": {
      "description": "Substitute for the anchor. May be empty.",
      "type": "string"
    }
  },
  "required": [
    "path",
    "anchor",
    "replacement"
  ],
  "type": "object"
}
```

Success keys: outcome, path, bytes_written, anchor_occurrences (including zero on failure).

Limits: Input/output 256 KiB; anchor must occur exactly once (non-overlapping). Empty replacement deletes the anchor.

Failure categories: validation, path_escape, not_found, is_directory, not_directory, io, unknown, workspace_mismatch, unavailable, binary, too_large, anchor_not_found, anchor_ambiguous.

## file_list

Id: `standard.file-list`. retry_safe: `true`. Permissions: `workspace.fs.read`.

List directory entries under a workspace-relative path (empty or "." lists the workspace root). Output is sorted and capped at 5000 entries; oversize results set truncated=true.

```json
{
  "additionalProperties": false,
  "properties": {
    "path": {
      "description": "Workspace-relative directory. Use '.' for the workspace root. Omit the field entirely to default to '.'.",
      "minLength": 1,
      "type": "string"
    },
    "recursive": {
      "description": "If true, walk descendants. Default false (immediate children only).",
      "type": "boolean"
    }
  },
  "type": "object"
}
```

Success keys: outcome, path, entries [{path, is_dir}]; truncated only when capped.

Limits: 5000 entries; sorted pre-order traversal, includes VCS and hidden directories, does not descend symlink directories.

Failure categories: validation, path_escape, not_found, is_directory, not_directory, io, unknown, workspace_mismatch, unavailable.

## search

Id: `standard.search`. retry_safe: `true`. Permissions: `workspace.fs.read`, `workspace.process.exec`.

Search workspace files via ripgrep. Defaults to regex mode and preserves existing pattern/path/timeout behavior. Optional glob filters, literal mode, ignore-case mode, context lines, and match limit are supported. Per-call timeout defaults to 60s and is capped at 600s. The host resolves the ripgrep binary once; configuration files are always disabled.

```json
{
  "additionalProperties": false,
  "properties": {
    "context": {
      "description": "Number of before and after context lines to return. Default 0; cap is 20.",
      "maximum": 20,
      "minimum": 0,
      "type": "integer"
    },
    "glob": {
      "description": "Optional ripgrep include glob or globs, forwarded as repeated -g filters.",
      "oneOf": [
        {
          "minLength": 1,
          "type": "string"
        },
        {
          "items": {
            "minLength": 1,
            "type": "string"
          },
          "minItems": 1,
          "type": "array"
        }
      ]
    },
    "ignore_case": {
      "description": "If true, search case-insensitively via ripgrep -i. Default false.",
      "type": "boolean"
    },
    "limit": {
      "description": "Maximum matches to return. Default 200; cap is 1000.",
      "maximum": 1000,
      "minimum": 0,
      "type": "integer"
    },
    "literal": {
      "description": "If true, search for pattern as a fixed string via ripgrep -F. Default false.",
      "type": "boolean"
    },
    "path": {
      "description": "Workspace-relative search root. Optional; empty or \".\" searches the entire workspace.",
      "type": "string"
    },
    "pattern": {
      "description": "Regex pattern. Forwarded to ripgrep via -e. Inline flags such as (?i) are supported.",
      "minLength": 1,
      "type": "string"
    },
    "timeout_seconds": {
      "description": "Per-call timeout in seconds. 0 or omitted applies the default (60s). Cap is 600s.",
      "maximum": 600,
      "minimum": 0,
      "type": "integer"
    }
  },
  "required": [
    "pattern"
  ],
  "type": "object"
}
```

Success keys: outcome, matches, match_count, duration_ms; optional truncated, truncation_reason, partial, timed_out, error. Matches carry path, line_number, line, submatches [{text,start,end}], optional line_truncated, before and after.

Limits: 60s default/600s maximum; 200 default/1000 maximum matches; context maximum 20; line cap 4 KiB, serialized matches 256 KiB, JSON record 8 MiB, stderr 4 KiB. Non-UTF-8 paths/lines decode lossily. Submatch offsets index UTF-8 bytes in the returned line; ranges beyond retained text are omitted, and literal mode returns no submatches. Limit or byte truncation succeeds; failed rg after matches yields partial success.

Failure categories: validation, path_escape, not_found, is_directory, not_directory, io, unknown, workspace_mismatch, unavailable, timeout, exec_failed, invalid_pattern.

## shell

Id: `standard.shell`. retry_safe: `false`. Permissions: `workspace.process.exec`, `workspace.fs.write`.

Run a command with host-configured shell policy in the agent's workspace cwd. Captures stdout, stderr, exit code, and duration. Per-call timeout defaults to 60s and is capped at 600s. Stdout/stderr use host-configured per-stream caps (explicitly supplied by the host); oversize output sets truncated=true.

```json
{
  "additionalProperties": false,
  "properties": {
    "cmd": {
      "description": "Shell command body. Executed with host-configured shell policy in the agent's workspace cwd.",
      "minLength": 1,
      "type": "string"
    },
    "timeout_seconds": {
      "description": "Per-call timeout in seconds. 0 or omitted applies the default (60s). Cap is 600s.",
      "maximum": 600,
      "minimum": 0,
      "type": "integer"
    }
  },
  "required": [
    "cmd"
  ],
  "type": "object"
}
```

Success keys: outcome, exit_code, stdout, stderr, duration_ms; optional stdout_truncated, stderr_truncated, timed_out, error.

Limits: 60s default/600s maximum; per-stream cap supplied by host (1 byte..16 MiB); excess drained, UTF-8 incomplete suffix removed and invalid sequences replaced. Signals map to 128+signal.

Failure categories: validation, path_escape, not_found, is_directory, not_directory, io, unknown, workspace_mismatch, unavailable, timeout, exec_failed.

## glob

Catalog id: `standard.glob` (catalog integration follows the second-deliverable crates).
Retry safe: true. Advisory permission: `workspace.fs.read`. Include hidden entries, skip
`.git`, `.hg`, `.svn`, `.jj` directories, and never descend through directory
symlinks encountered during walking. An explicitly supplied search root is
resolved through the workspace capability. Patterns match relative to that
search root; returned paths are relative to the workspace. Glob separators are
literal: `*` and `?` do not cross `/`, while `**` can.

```json
{
  "properties": {
    "pattern": {
      "minLength": 1,
      "description": "Doublestar glob pattern to match against paths under the search root, e.g. \"*.go\" or \"**/*_test.go\".",
      "type": "string"
    },
    "path": {
      "description": "Workspace-relative directory to search. Omit or use \".\" for the workspace root.",
      "type": "string"
    },
    "limit": {
      "maximum": 5000,
      "minimum": 1,
      "description": "Maximum number of paths to return. Default 1000; hard cap 5000.",
      "type": "integer"
    }
  },
  "additionalProperties": false,
  "required": [
    "pattern"
  ],
  "type": "object"
}
```

Success keys: `outcome`, `paths` (`path`, `is_dir`), `count`, `truncated`.
Failures retain empty `paths`, zero `count`, false `truncated`, and `error`.
Categories: `validation`, `path_escape`, `not_found`, `not_directory`, `io`,
`unknown`, `workspace_mismatch`, `unavailable`. Default limit 1000, maximum
5000; zero is rejected. Results sort by rendered workspace path and
`truncated` means another matching entry was omitted. Retained results and
directory batches are bounded independently of tree width. Traversal keeps no
recursive call stack or open directory stack; exhausted/evicted batches rescan
siblings, trading additional directory reads for bounded resources. Cancellation
is checked during every scan and follows the common runtime interruption rule.

Non-UTF-8 names use lossy display paths; distinct raw names remain separate
results even when their displayed paths coincide. Raw bytes break display-sort
ties, and every matched entry counts toward truncation.

Glob patterns are limited to 4096 bytes and at most 16 nested unescaped brace
groups outside character classes. These bounds precede recursive glob parsing.
Compilation uses a fallible GlobSet builder on the blocking worker after capacity
admission; overly complex patterns return `validation` instead of panicking.
