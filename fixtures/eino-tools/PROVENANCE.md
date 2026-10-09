# Golden metadata provenance

Source: eino-tools at `c0975d6764b2536faf639efe516a79de6154a55d`.

Extracted in a temporary archive of that revision with a throwaway Go program:
`go run ./cmd/crabber-fixtures <fixture-directory>`. The program called
`fileops.ReadToolInfo`, `WriteToolInfo`, `EditToolInfo`, `ListToolInfo`,
`search.ToolInfo`, and `shell.ToolInfo`, then `info.ToJSONSchema()`, serializing
`name`, `info.Desc` as `description`, and the schema as `parameters`.
The extractor was not committed. These are upstream fixtures, not Rust output.

`url_fetch.json` was separately extracted from an archive of the same immutable
revision with `urlfetch.ToolInfo()` and `info.ToJSONSchema()` in a temporary Go
program. It is not generated from Rust metadata. Its unchanged upstream prose
mentions Go redirect defaults; the Rust runtime uses the stricter manual policy
documented in `docs/tool-contract.md`.
