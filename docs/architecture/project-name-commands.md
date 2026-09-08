# Canonical project and site names

`ProjectCommand::SetProjectName` and `SetSiteName` change bounded canonical
text through `Project::execute`. They reuse project identity, expected
revision, unique operation ID and logical-time admission. A missing site is
an error; renaming never creates an entity or changes its geometry.

Each receipt records the exact previous and current name and an executable
inverse command. `Project::replay` reconstructs the receipt from the prior
project, rejecting forged prior values or inverse names. Undo and redo are
new commands with new operation IDs and revisions. A same-value command is
still an auditable accepted operation, consistent with existing commands;
it is not silently dropped.

These are additive tagged command/event variants in the existing linear
receipt format. Existing variant encodings and historical fixtures are
unchanged. Older readers reject the new variants rather than pretending to
apply them. This increment does not change the immutable operation-log wire
schema or automatically trust its inverse metadata. The application bridge
must still bind a canonical baseline and validate causal inverse values.

Tests in `crates/domain/tests/project_commands.rs` check actual project/site
names, identity and geometry preservation, deterministic receipt serialization
and replay, undo/redo, tampered receipts, missing targets and stale revisions.
Full operation-log materialization, persistence and product rename controls
remain separate integration requirements.
