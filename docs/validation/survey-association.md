# Receipt-based point-association validation

This increment implements the normalized association boundary described by
plan §§7.1–7.4, §7.20, §10.12 and the point-position → observation association
requirement in §10.16. It is a pure transition in `kyberia-survey`; it does not
call a clock, platform API, storage layer or network.

The focused acceptance suite is `crates/survey/tests/association.rs` and has
eight passing tests. It covers:

* known in-window receipt association, including the exact point-start
  boundary;
* API-window association when receipt monotonic time is unknown;
* rejection of outside and point-boundary-crossing ambiguous windows;
* pause/resume behavior and rejection of receipts from the pause gap;
* wrong session, source and reported coordinate frame;
* duplicate observation IDs across strict and associated planes and malformed
  quality evidence;
* future association schema and missing-wire-field rejection;
* exact V1 temporal-uncertainty shape and association collection resource
  bounds;
* deterministic JSON replay;
* preservation of unknown capture time, dwell, scan cache age and pose,
  including retained `ClockUncertain` quality;
* proof that receipt associations do not advance strict point metric/dwell or
  active-time completion.

Commands run in the isolated worktree:

```sh
cargo fmt --all
cargo test -p kyberia-survey --locked --offline
cargo check -p kyberia-survey --locked --offline
```

The survey package suite passed 31 tests and one ignored release benchmark:
eight association tests, 17 strict point tests and six snapshot migration
tests. Runtime CoreWLAN capture, host transport, project-store persistence and
UI evidence-drawer wiring remain integration work. This increment does not
claim that receipt timing estimates when the radio actually heard a beacon.
