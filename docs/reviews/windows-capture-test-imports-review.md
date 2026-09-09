# Windows capture test import correction

Disposition: APPROVED. Author: root. Independent reviewer: Russell.
Static review confirms platform guards match the uses; no findings.

Hosted run 34334120439 at source 1711f13, Windows check 102409381715,
reports unused imports in observation-pipeline tests.rs lines 4 and 42.
The affected collector and Duration/Instant imports are now guarded by
cfg(unix), matching every test/helper that uses them. This changes no
production code and suppresses no warning globally.

Local validation: package library tests pass 34, zero failures, one ignored;
all-target Clippy with warnings denied and formatting pass. Log:
.tools/windows-test-imports-regression.log.

A real x86_64-pc-windows-gnu Clippy attempt stopped in the SQLite C build
because x86_64-w64-mingw32-gcc is unavailable, before checking this crate.
.tools/windows-cross-clippy.log preserves that limitation. The installed Rust
target alone is insufficient; no Windows execution success is claimed.
Hosted Windows validation remains required after independent review.
