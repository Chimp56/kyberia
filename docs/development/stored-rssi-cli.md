# Analyze stored RSSI

This Unix CLI command computes a numerical RSSI artifact from observations and
survey snapshots already committed to a Kyberia bundle. A newly created empty
bundle has no survey evidence to analyze. Capture selection and request creation
currently require the application APIs; the desktop survey workflow remains open.

```sh
cargo run -p kyberia-cli --locked --offline -- analyze-stored-rssi \
  /path/to/project.rfatlas /path/to/request.json /path/to/new-analysis
```

The output directory must not exist. The request must be a regular file, not a
symlink or pipe. Use the project's actual identities and committed revision;
invented identifiers cannot select evidence. The
[request fixture builder](../../apps/cli/tests/stored_analysis.rs) shows the complete
wire shape in `request_json`, and the closed
[request types](../../apps/cli/src/stored_analysis.rs) define admission.

| Request fields | Meaning |
| --- | --- |
| `schema` | `kyberia.stored-rssi-analysis-request/1` |
| `project_id`, `project_revision` | Canonical project ID and exact revision as a decimal string |
| `floor_id`, `frame_id` | Scope shared by the observations, snapshots and grid |
| `target_bssid` | Six integer octets, not a colon-separated string |
| `observation_ids`, `snapshots` | Existing canonical observation IDs and snapshot/floor ID pairs |
| `session_scope`, `source_scope`, `adapter_scope` | Optional canonical scope filters; `null` leaves that filter unset |
| `allow_uncalibrated` | Explicit permission to admit evidence without radio calibration |
| `method` | `{"kind":"point_value"}`, `{"kind":"nearest"}`, or `{"kind":"idw","power":2.0}` |
| `support_radius_m`, `minimum_locations`, `maximum_neighbors` | Spatial evidence support and neighbor bounds |
| `extrapolation` | `{"kind":"disabled"}` or `{"kind":"within_radius","radius_m":5.0}` |
| `grid` | Meter origins `origin_x_m`/`origin_y_m`, `resolution_m`, integer offsets and width/height |

Keep extrapolation disabled when you want unsupported space to remain unknown.
Changing to nearest or IDW selects the matching canonical RSSI metric; it does
not turn synthetic or rejected evidence into measured observations.

On success, stdout contains a JSON summary with known/unknown counts, cell
classes and rejection counts. `analysis.json` contains the canonical artifact
for numerical inspection. Unknown cells are not zero RSSI. The artifact may
include source and location identifiers; inspect it before sharing.

Ctrl+C requests cooperative cancellation. Before publication, cancellation
returns exit code 2 with `code: "cancelled"`; a pending file can remain. If
publication already occurred, the report explicitly identifies the committed
result. A `publication_durability` error with `committed: true` means the final
file exists but the directory sync failed. Preserve those bytes for diagnosis.
Retry with a new destination instead of overwriting a previous run. The retained
`.analysis.json.pending` link is intentional and is never automatically deleted.

See the [architecture contract](../architecture/stored-rssi-cli.md) for limits,
publication semantics and platform support, and the
[validation record](../validation/stored-rssi-cli.md) for test scope.
