# Golden metadata provenance

Source: eino-tools at `c0975d6764b2536faf639efe516a79de6154a55d`.

Extracted in a temporary archive of that revision with a throwaway Go program:
`go run ./cmd/crabber-fixtures <fixture-directory>`. The program called
`fileops.ReadToolInfo`, `WriteToolInfo`, `EditToolInfo`, `ListToolInfo`,
`search.ToolInfo`, and `shell.ToolInfo`, then `info.ToJSONSchema()`, serializing
`name`, `info.Desc` as `description`, and the schema as `parameters`.
The extractor was not committed. These are upstream fixtures, not Rust output.

`apply_patch.json` was extracted at the same pinned revision on 2026-10-09 by
calling `applypatch.ToolInfo()` and `info.ToJSONSchema()` in the throwaway Go
extractor. The original Go test corpus is copied byte-for-byte into
`crates/applypatch/tests/fixtures/upstream_applypatch_test.go`; `go-cases.json`
extracts its twelve structured patch strings and their setup/expected categories.
Rust runtime tests run every extracted case. Additional Rust cases cover malformed
grammar, overlapping ambiguity, content bounds, cancellation, and commit failures.
