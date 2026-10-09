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

`tracker_write.json` was extracted on 2026-10-09 from the same immutable Go
revision using `trackerwrite.ToolInfo()` and `info.ToJSONSchema()` in the
throwaway extractor. Its parity test compares independently implemented Rust
metadata with the fixture; production does not load the fixture.
