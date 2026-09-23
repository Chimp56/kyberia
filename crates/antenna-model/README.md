# Antenna pattern v1

`kyberia-antenna-model` defines a bounded, open JSON importer and deterministic
gain evaluator for one tabulated representation: a frequency-indexed full
sphere grid. The crate owns the contract and no vendor catalog.

## Wire semantics

The closed `schema` value is `kyberia.antenna-pattern/1`. The portable
structural schema is in `schema/antenna-pattern-v1.schema.json`; accepted data
must also pass `ValidatedAntenna::parse_json`, which checks semantic
relationships that JSON Schema cannot express.

Units and conventions are explicit in field names and discriminator values:

- `frequency_hz` is an unsigned integer in hertz.
- `azimuth_degrees` and `elevation_degrees` are degrees. Azimuth starts at 0
  on local +X and increases toward +Y; elevation increases toward +Z. The
  right-handed frame is X-forward, Y-left, Z-up.
- `*_gain_dbi` values are dBi. `uncertainty_db` and normalization tolerance
  are dB. Efficiency and its uncertainty are fractions in `[0, 1]`.
- `mount_orientation_local_to_world` is a unit WXYZ quaternion. Evaluation
  rotates the world direction into the antenna-local frame.
- Polarization basis is either linear H/V or circular right/left. Each sample
  stores the matched (co-polar) plane and may store the orthogonal
  (cross-polar) plane. The cross-polar plane must be present everywhere or
  absent everywhere.
- `sample_order` is fixed to `elevation_major_azimuth_minor`: for `A`
  azimuths, flattened sample index is
  `elevation_index * A + azimuth_index`.
- Spatial and frequency interpolation occur in linear power. Frequency
  requests outside the tabulated range return an unsupported-frequency error;
  they are never extrapolated silently.

Each frequency's declared nominal gain must agree with the highest co-polar
sample within the stated gain uncertainty plus the declared normalization
tolerance. Both poles must be azimuth-invariant. Axes and frequency entries
must be strictly increasing, with azimuth starting at 0 and elevation
including both -90 and +90 degrees.

## Provenance and bounds

The source record carries a URI, an SPDX license expression, and a lowercase
SHA-256 for the source artifact. The importer validates their syntax and
length. It does not fetch a URI, prove license rights, or verify that the
checksum matches an external file; the caller retains responsibility for
those checks. The canonical content identity instead hashes the validated,
canonicalized JSON, including its provenance record.

Import is bounded to 8 MiB of JSON, nesting depth 32, 256 frequencies, 720
azimuth samples, 361 elevation samples, one million total grid samples, 2,048
ASCII bytes per text field, and a default 24-million-step validation budget.
Unknown fields, unknown algorithm/version values, non-finite or out-of-range
values, malformed metadata, invalid dimensions, and ambiguous sample planes
are rejected before evaluation.

## Deliberate boundary

V1 does not include spherical harmonics, two-cut reconstruction,
manufacturer-specific file formats, polarization mismatch loss, a catalog of
vendor data, visual pattern validation, or a Sionna adapter. It is a reusable
canonical input/evaluation contract for those later integrations. The tests
use synthetic samples and do not redistribute manufacturer patterns.
