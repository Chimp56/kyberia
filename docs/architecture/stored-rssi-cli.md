# Stored RSSI analysis CLI

The `kyberia analyze-stored-rssi <bundle> <request.json>
<new-output-directory>` command is a read-only composition boundary around
`kyberia-stored-analysis`. It opens a committed `Bundle` in read-only mode,
passes its canonical observation and survey-store verification through the
reviewed stored-analysis workflow, and emits the workflow's exact canonical
artifact as `analysis.json` together with a small JSON report on stdout.

The request is closed and versioned as
`kyberia.stored-rssi-analysis-request/1`. It carries the project identity and
exact committed project revision, floor/frame, target BSSID, bounded
observation and survey snapshot IDs, optional session/source/adapter scopes,
calibration policy, typed grid geometry, and typed spatial method settings.
IDs and revisions use their canonical domain encodings; revisions are decimal
strings so large unsigned values cannot pass through a lossy JSON number.
Unknown fields, unsupported versions, malformed JSON, nesting deeper than 16,
requests over 1 MiB, and excessive observation or snapshot IDs fail before the
bundle is opened.

The caller selects a spatial method, but cannot supply an arbitrary metric
definition or metric artifact. The CLI derives the matching canonical
registry definition (`wifi.rssi/1`, `wifi.rssi.nearest/1`, or
`wifi.rssi.idw/1`), hashes its canonical bytes, and binds that artifact to the
stored-analysis request. Point values remain measured evidence only when the
inward selector accepts the stored observation and provenance; rejected or
synthetic evidence remains an explicit unknown cell with rejection counts.

The report contains the project revision, canonical output artifact reference,
selected and rejected counts, cell class/known/unknown totals, and unknown
reasons. It deliberately omits observation ID lists from the human-facing
report. The canonical artifact is still provenance-bearing and may contain
observation, source, and location identifiers, so the report includes a
privacy warning and consumers must review the artifact before sharing it.

The destination must be new. Publication creates the directory, writes the
complete artifact to `.analysis.json.pending`, fsyncs that file, atomically
creates `analysis.json` with a same-filesystem hard link, and fsyncs the
directory. A hard link gives final publication create-new/no-clobber semantics;
an existing directory, final file, or pending file fails without overwriting
prior bytes. The pending link is intentionally retained as an exact recovery
copy under the repository's no-delete policy. A process interruption before
the final link can therefore leave a pending file and no final artifact; the
CLI does not remove it, so an operator can diagnose or manually retire it.
The report is printed only after successful publication.

The request reader opens a no-follow, nonblocking Unix file handle, checks that
the handle is a regular file and within the byte budget, then reads from that
handle. This rejects FIFOs, devices, and symlink substitution without waiting
for a producer. The command returns an explicit unsupported-platform error on
non-Unix hosts until an equivalent no-follow, nonblocking regular-file adapter
is provided; it does not risk opening an unbounded named pipe there.

On Unix, the command installs the maintained `signal-hook` 0.4.4 flag adapter
before dispatch. Its signal handler only sets an atomic flag; the storage and
numerical layers poll the existing `Cancellation` port at their documented
boundaries. The CLI emits an `analysis_started` stderr lifecycle event after
registration so an orchestrator can establish readiness without a synthetic
delay. The upstream API documents this flag-polling model and its safe
deferred handling pattern in the
[signal-hook 0.4.4 reference](https://docs.rs/signal-hook/0.4.4/signal_hook/).

Cancellation observed before final publication returns exit code 2 and a
structured stderr error with `code: "cancelled"`; no final artifact is
published, though a pending artifact may remain after work has begun. The
final hard-link is the publication commit point. If SIGINT wins immediately
after that link, the CLI retains the complete artifact, prints a report with
`publication_status: "published_after_cancellation"` and
`cancelled_after_commit: true`, and exits nonzero rather than pretending that
the durable artifact was rolled back. Non-Unix hosts have no signal capability
in this command and report unsupported request acquisition before work begins.

The command does not persist an analysis result back into the project bundle,
choose a survey, invent a pose, reinterpret receipt timing, or perform a live
capture. Durable derived-artifact indexing, interactive cancellation, and
operator-facing privacy review remain follow-up integration work.
