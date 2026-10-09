# Golden metadata provenance

Source: eino-tools at `c0975d6764b2536faf639efe516a79de6154a55d`.

Extracted in a temporary archive of that revision with a throwaway Go program:
`go run ./cmd/crabber-fixtures <fixture-directory>`. The program called
`fileops.ReadToolInfo`, `WriteToolInfo`, `EditToolInfo`, `ListToolInfo`,
`search.ToolInfo`, and `shell.ToolInfo`, then `info.ToJSONSchema()`, serializing
`name`, `info.Desc` as `description`, and the schema as `parameters`.
The extractor was not committed. These are upstream fixtures, not Rust output.

The second-deliverable `glob.json` was independently extracted at the same
revision using a throwaway `cmd/crabber-fixtures` Go program calling
`glob.ToolInfo()` and `info.ToJSONSchema()` (2026-10-09).

`apply_patch.json` was extracted at the same pinned revision on 2026-10-09 by
calling `applypatch.ToolInfo()` and `info.ToJSONSchema()` in the throwaway Go
extractor. The original Go test corpus is copied byte-for-byte into
`crates/applypatch/tests/fixtures/upstream_applypatch_test.go`; `go-cases.json`
extracts its twelve structured patch strings and their setup/expected categories.
Rust runtime tests run every extracted case. Additional Rust cases cover malformed
grammar, overlapping ambiguity, content bounds, cancellation, and commit failures.

`url_fetch.json` was separately extracted from an archive of the same immutable
revision with `urlfetch.ToolInfo()` and `info.ToJSONSchema()` in a temporary Go
program. It is not generated from Rust metadata. Its unchanged upstream prose
mentions Go redirect defaults; the Rust runtime uses the stricter manual policy
documented in `docs/tool-contract.md`.

`tracker_write.json` was extracted on 2026-10-09 from the same immutable Go
revision using `trackerwrite.ToolInfo()` and `info.ToJSONSchema()` in the
throwaway extractor. Its parity test compares independently implemented Rust
metadata with the fixture; production does not load the fixture.
