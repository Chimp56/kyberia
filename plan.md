# RF Atlas

## An Evidence-First Wi-Fi Survey, Analysis, and Design Platform

**Document:** `plan.md`  
**Status:** Research-backed product, architecture, reuse, and implementation blueprint  
**Version:** 0.2  
**Research baseline:** 2026-08-30  
**Open-source architecture audit baseline:** 2026-08-30  
**Working name:** RF Atlas (codename only; perform a trademark search before public use)

---

## Table of contents

- [0. Executive decision](#0-executive-decision)
- [1. Epistemic and clean-room boundaries](#1-epistemic-and-clean-room-boundaries)
- [2. Product thesis](#2-product-thesis)
- [3. Engineering principles](#3-non-negotiable-engineering-principles)
- [4. Competitive research](#4-competitive-research-summary)
  - [4.3 Open-source source-code audit and reuse policy](#43-open-source-source-code-audit-and-reuse-policy)
- [5. TamoGraph behavioral model](#5-tamograph-8x-behavioral-model--inside-out)
- [6. Capability catalog](#6-rf-atlas-capability-catalog)
- [7. Measurement methodology](#7-measurement-methodology)
- [8. Predictive engine](#8-predictive-engine-design)
- [9. Automatic AP planner](#9-automatic-ap-planner)
- [10. System architecture](#10-system-architecture)
  - [10.16 Third-party narrow-waist integrations](#1016-third-party-narrow-waist-integrations)
- [11. Storage and schema](#11-project-storage-and-schema)
- [12. Analysis and rendering](#12-analysis-and-rendering-engine)
- [13. User experience](#13-user-experience)
- [14. Monorepo and code organization](#14-monorepo-and-code-organization)
- [15. Security, privacy, and safety](#15-security-privacy-and-safety)
- [16. Validation and testing](#16-validation-and-test-strategy)
- [17. Delivery roadmap](#17-delivery-roadmap)
- [18. Prioritized backlog](#18-prioritized-engineering-backlog)
- [19. First implementation sequence](#19-first-implementation-sequence)
- [20. Decision gates and ADRs](#20-decision-gates-and-architecture-decision-records)
- [21. Risks](#21-major-risks-and-mitigations)
- [22. Open research questions](#22-open-research-questions)
- [23. Definition of done](#23-definition-of-done)
- [Appendices and source bibliography](#appendices)
  - [Appendix I — Open-source source-code architectural audit](#appendix-i--open-source-source-code-architectural-audit)

## 0. Executive decision

Build a local-first, cross-platform Wi-Fi measurement and planning system that combines:

1. **NetSpot's accessibility** — fast onboarding, approachable live inspection, simple point surveys, clear troubleshooting views, cross-platform project portability, and an interface a homeowner can use without understanding RF theory.
2. **Acrylic Wi-Fi Heatmaps' diagnostic depth** — monitor-mode client visibility, retry-rate and client-density maps, active latency/loss/bandwidth/roaming maps, Wi-Fi quality profiles, raw export, GPS workflows, and strong 2D/3D reporting.
3. **TamoGraph's professional survey and planning model** — passive/active/hybrid survey modes, AP-on-a-stick, ranked AP coverage, multi-floor RF modeling, client capability profiles, requirement compliance, antenna-aware predictive simulation, spectrum integration, capacity-aware automatic AP placement, GPS surveying, and detailed reporting.
4. **Capabilities the three products do not expose rigorously enough** — raw-data provenance, uncertainty maps, reproducible algorithm versions, model calibration from measured residuals, explicit separation of Wi-Fi/LAN/WAN problems, robust multi-objective optimization, distributed sensors, AR/LiDAR-assisted path tracking, open project formats, and a plugin SDK.

The central design rule is:

> **Never confuse a measurement, an estimate, and a prediction.**

Every pixel, recommendation, and compliance score must be able to answer:

- What raw evidence produced this result?
- Which sensor and driver produced the evidence?
- Where and when was it captured?
- How uncertain was the position and RF reading?
- Which transformations and algorithm versions were applied?
- Was the value observed, interpolated, extrapolated, simulated, or inferred?
- What assumptions would change the result?

The product should be useful after the first vertical slice, but the end state is intentionally overbuilt: a Wi-Fi digital-twin and field-measurement platform rather than merely a colored heatmap generator.

### 0.1 Open-source implementation decision

The source-code audit of Deconflict, Kismet, wifiheatmap, and Sionna RT changes the reuse strategy from “borrow good code where convenient” to a deliberate **narrow-waist architecture**. RF Atlas owns the canonical truth; specialized upstream projects are allowed only behind explicit versioned boundaries.

| Repository | Primary disposition | RF Atlas role | Hard boundary |
|---|---|---|---|
| **Sionna RT** | **ADOPT** | High-fidelity path/radio-map engine | Runs only inside an isolated optional Python/Mitsuba/Dr.Jit worker; Sionna objects never become RF Atlas domain objects |
| **Kismet** | **INTEGRATE** | Linux monitor-mode, remote capture, packet, BLE/Zigbee/SDR, KismetDB/PCAPNG evidence source | External authenticated process/API/file producer normalized into immutable RF Atlas observations |
| **Deconflict** | **CONTRIBUTE / REFERENCE-ONLY** | Interchange partner, UX/baseline planner reference, upstream collaboration target | No runtime dependency and no use of its heuristic RF/throughput/optimizer as RF Atlas truth |
| **wifiheatmap** | **REFERENCE-ONLY** | Clean-room behavior oracle for point surveys and simple TIN interpolation | No code reuse/runtime dependency; independent fixtures reproduce only documented behavior |

The resulting architectural rule is:

> **External projects may produce evidence or propagation results, but no external project owns RF Atlas truth.**

RF Atlas therefore reimplements the canonical site/geometry/identity/observation/active-test/metric/requirements/plan/provenance/report model, Wi-Fi PHY/MAC/capacity semantics, spatial statistics, uncertainty, optimization, survey workflows, history, reporting, privacy, and cross-platform UX. Hardware capture and electromagnetic ray tracing are the two major areas where direct reuse is strategically preferable to rebuilding mature specialized systems.

The audit is pinned to exact repository revisions and is reproduced in full in Appendix I so every subsystem decision remains traceable.

---

## 1. Epistemic and clean-room boundaries

### 1.1 Evidence labels used in this document

| Label | Meaning |
|---|---|
| **[V] Vendor-documented** | Public product page, manual, release note, or official support documentation states the behavior. |
| **[S] Standards-derived** | Defined by an open standard, regulator, or operating-system API contract. |
| **[O] Open implementation** | Observable in an open-source implementation or official developer documentation. |
| **[I] Engineering inference** | A plausible architecture or algorithm inferred from documented behavior; not claimed as the vendor's actual implementation. |
| **[E] Experiment required** | Must be validated through a controlled black-box test, hardware test, or field study. |

### 1.2 What “understand TamoGraph inside and out” means here

TamoGraph is proprietary. Public documentation reveals its workflows, inputs, outputs, controls, supported hardware, visualizations, and many modeling assumptions, but not its source code or every numerical algorithm. This plan therefore contains:

- a comprehensive **behavioral model** of TamoGraph 8.x based on its official manual and release notes;
- a **clean-room inferred architecture** that could produce the documented behavior;
- explicit labels where interpolation, AP localization, propagation, optimization, caching, or GPU implementation details remain undisclosed;
- a future black-box validation protocol using legitimate trial or purchased licenses;
- no binary disassembly, decompilation, copied assets, copied project formats, or circumvention of licensing controls.

This distinction matters. The goal is to build a better product from public behavior, RF theory, standards, and original engineering—not to clone proprietary code.

### 1.3 Intellectual-property constraints

- Do not reproduce competitor branding, screen layouts, icons, report templates, wording, antenna databases, project formats, or proprietary material libraries.
- Do not redistribute vendor AP/antenna pattern data unless the manufacturer license permits it.
- Build importers only for documented/open formats or with user-supplied files.
- Keep a source-and-license ledger for every dataset, antenna pattern, OUI database, map tile, material preset, and third-party library.
- Prefer official manufacturer radiation-pattern files and user imports over scraping competitor libraries.
- Keep reverse engineering limited to lawful interoperability and documented file/API behavior after legal review.
- Treat GPL applications such as Kismet and wifiheatmap as explicit architectural boundaries rather than casually copying implementation code. Kismet integration is process/API/file based; wifiheatmap is clean-room reference only.
- Track Sionna RT, Mitsuba, Dr.Jit, Kismet, and every transitive native dependency in the release SBOM and source/license ledger.
- Pin audited upstream revisions in fixtures and integration tests; production upgrades require compatibility and numerical-regression review.
- Never deserialize foreign project/domain objects directly into canonical RF Atlas state. All external data crosses a validating adapter contract with source/version provenance.

---

## 2. Product thesis

### 2.1 The actual problem

A person trying to fix Wi-Fi typically has four different questions that current products blur together:

1. **What RF networks and transmitters are present?**
2. **How does my client experience the network while moving through physical space?**
3. **Why is performance poor: RF, contention, roaming, LAN, router, DNS, ISP, or destination?**
4. **Where should APs go, and how should radios be configured, before or after deployment?**

RF Atlas treats these as connected but distinct measurement domains.

### 2.2 Product modes

The same project and evidence model serves three interfaces:

- **Easy mode:** “Walk here, stand still, tap, and follow recommendations.” Designed for home users and small offices.
- **Pro mode:** Complete passive/active/hybrid surveys, requirement profiles, planning, spectrum, reports, AP-on-a-stick, multi-floor work, and team workflows.
- **Lab mode:** Raw frames, radiotap metadata, information elements, calibration diagnostics, algorithm comparison, uncertainty decomposition, custom formulas, plugin development, and reproducible experiments.

Easy mode hides complexity; it must not throw data away. A project started in Easy mode can be opened in Pro or Lab mode without rescanning.

### 2.3 Target users

- Home users diagnosing dead spots and AP placement.
- Enthusiasts, installers, and MSPs.
- Enterprise WLAN engineers.
- Campus, warehouse, hospital, hospitality, venue, and industrial operators.
- RF researchers and developers.
- Hardware vendors validating radios and antennas.
- Eventually, adjacent-radio users mapping BLE, Zigbee/Thread, private LTE/5G, or proprietary ISM-band systems through plugins.

### 2.4 Explicit non-goals

- No deauthentication, jamming, credential attacks, unauthorized packet injection, or offensive Wi-Fi automation.
- No claim that RSSI alone predicts user experience.
- No claim that a laptop adapter measures absolute spectrum power like a calibrated spectrum analyzer.
- No hidden cloud requirement.
- No model-generated recommendations without evidence, assumptions, and an explanation.
- No “single quality score” that obscures failed requirements.
- No universal feature parity across operating systems when their APIs cannot provide it.

---

## 3. Non-negotiable engineering principles

1. **Raw-data first.** Store immutable observations before aggregation.
2. **Observed, inferred, interpolated, extrapolated, and simulated values are visibly distinct.**
3. **Uncertainty is a first-class output.** Every map can expose confidence, sample density, recency, spatial uncertainty, temporal variance, and sensor calibration quality.
4. **Capability negotiation replaces platform fiction.** Collectors advertise what they can actually observe.
5. **Wi-Fi, LAN, gateway, WAN, DNS, and Internet destination measurements are separate.**
6. **Reproducibility.** Analysis results reference an immutable input set, configuration, code version, model version, and random seed.
7. **Algorithm replaceability.** Raw surveys survive changes to heatmap interpolation, propagation models, PHY tables, or compliance logic.
8. **Local-first and offline-capable.** Cloud collaboration is optional.
9. **Privacy by default.** Metadata minimization, payload discard, project-scoped pseudonyms, encryption, and explicit raw-capture opt-in.
10. **Calibration over false precision.** Never show more significant digits than measurement quality justifies.
11. **Explainability over magical recommendations.** Show the binding constraint and counterfactual.
12. **No write-only project format.** Provide documented schemas, exports, and migrations.
13. **Professional rigor without professional-only usability.** Guided workflows and expert controls share one evidence model.
14. **Deterministic core, probabilistic extensions.** Baseline analysis is inspectable; ML augments rather than replaces it.
15. **Measure both directions.** AP-to-client downlink and client-to-AP uplink budgets can differ materially.
16. **Design for heterogeneous clients.** A survey laptop is not an iPhone, IoT sensor, barcode scanner, headset, or legacy laptop.
17. **Treat time as a dimension.** A floor plan is not static RF truth; compare shifts, occupancy, firmware, and configuration changes.
18. **RF Atlas owns the canonical model.** Third-party object models are execution or evidence formats, never persisted product truth.
19. **Foreign compute is quarantined.** Optional engines run behind cancellable, resource-limited, versioned process/job boundaries.
20. **License boundaries are architecture.** GPL integrations remain external unless a deliberate licensing decision says otherwise; permissive engines may be adopted only inside bounded components.
21. **Differential testing over accidental coupling.** Reimplement small/simple algorithms independently and compare against open-source oracles instead of importing an unnecessary dependency.

---

## 4. Competitive research summary

### 4.1 What to take from each product

| Product | Best ideas to adopt | What to exceed | What not to copy blindly |
|---|---|---|---|
| **NetSpot** | Low-friction Inspector; table/chart switching; channel-overlap views; point survey onboarding; snapshots; approachable troubleshooting heatmaps; active and iPerf testing; planning preview; multi-floor drawing; mobile companions; cross-platform projects. | More rigorous active-test topology; monitor-mode data; uncertainty; raw evidence; capacity planning; distributed sensors; AP optimizer; model calibration. | Internet speed maps presented as if they were purely Wi-Fi; simplistic channel overlap as a substitute for airtime/interference; platform claims that hide missing noise/scan capabilities. |
| **Acrylic Wi-Fi Heatmaps** | Native vs monitor capture distinction; multiple adapters; continuous/GPS survey; iPerf active tests; latency, loss, roaming, client density, retry rate; quality profiles; 2D/3D maps; editable reports; raw CSV; AP inventory and map comparison. | Current 6 GHz/Wi-Fi 7 semantics throughout; probabilistic location; explicit uncertainty; modern cross-platform architecture; open schemas; predictive engine and automatic placement. | Treating all captures as equally calibrated; over-reliance on a single threshold grade; inferred AP positions shown without uncertainty. |
| **TamoGraph** | Passive/active/predictive/spectrum modes; scan-cycle-aware survey workflow; active+passive dual-adapter mode; AP-on-a-stick; multi-SSID linking; AP ranking; client capability profiles; per-band requirements; multi-floor planning; antenna patterns; AP classes; capacity-aware auto-planning; GPS; polished reporting. | Transparent formulas; reproducible analysis runs; measured/predicted uncertainty; better active diagnostics; model calibration; open project format/API; remote sensors; AR path tracking; robust optimization and failure scenarios. | Assuming vendor presets are universal truth; undisclosed optimization and propagation details; static file-copy teamwork; results without an auditable derivation graph. |

### 4.2 Capability-level comparison

Legend: **✓** documented, **~** partial/platform-dependent, **—** not a core documented capability, **RF Atlas target** describes the intended result.

| Capability | NetSpot | Acrylic | TamoGraph | RF Atlas target |
|---|---:|---:|---:|---|
| Live nearby-network inspector | ✓ | ✓ | ✓ dashboard/AP list | Best-in-class, with raw IE/capability inspection |
| 2.4/5/6 GHz | ✓ | ✓ current product claims Wi-Fi 7 | ✓ | Full, region-aware, Wi-Fi 7-native |
| Passive OS-level scan | ✓ | ✓ | ✓ | All platforms where APIs allow |
| Monitor-mode frame capture | Limited/adapter-dependent | ✓ Windows supported adapters | ✓ supported adapters/drivers | Native Linux/Windows first; isolated helpers |
| Client/station visibility | Limited | ✓ monitor mode | Indirect/packet analysis context | ✓ with privacy controls and confidence |
| Point survey | ✓ | ✓ | ✓ | ✓ with scan-completeness gate |
| Continuous path survey | Survey workflow varies | ✓ | ✓ | ✓ timestamp/pose aware |
| GPS outdoor survey | Limited by platform/workflow | ✓ | ✓ Pro | ✓ with GNSS accuracy and map projection |
| AR/SLAM indoor path | — | — | — | ✓ mobile companion |
| Passive heatmaps | ✓ | ✓ | ✓ | ✓ plus uncertainty/provenance layers |
| Active throughput | ✓ HTTP/TCP/UDP/iPerf3 | ✓ iPerf/file | ✓ TCP/UDP | Multi-target iPerf3 + lightweight probes |
| Latency/loss | Mobile and active workflows | ✓ | ✓ RTT/loss | Gateway/LAN/WAN/destination separated |
| Jitter/variation | Some mobile diagnostics | Limited | UDP-focused fields | ✓ RFC-aligned metrics |
| Roaming map | Secondary-signal focus | ✓ | ✓ associated-AP trace | ✓ plus transition cause/timing/impact |
| Retry-rate map | — | ✓ monitor mode | Not a headline heatmap | ✓ by frame class/direction/client/AP |
| Client-density map | — | ✓ monitor mode | Capacity model rather than measured map | ✓ observed and estimated |
| Spectrum analyzer integration | — | ✓ | ✓ | Generic plugin layer + Wi-Spy/SDR adapters |
| Predictive RF model | ✓ | Planning-oriented | ✓ Pro, extensive | Multi-tier, calibrated, uncertainty-aware |
| Multi-floor propagation | ✓ planning | 3D visualization; less extensive documented planning | ✓ | True 3D geometry and floor coupling |
| Antenna pattern library/import | ✓ | Limited planning details | ✓ extensive | Open pattern format + validated imports |
| AP-on-a-stick | ✓ snapshots | Can compare scans | ✓ explicit split workflow | ✓ experiment-aware candidate analysis |
| Automatic AP placement | Planning assistance | Planning | ✓ coverage/capacity | Robust multi-objective optimizer |
| Automatic channel/power plan | Partial | Planning | ✓ reconfiguration/planner | Joint placement/channel/width/power/MLO |
| Requirement profiles | Troubleshooting thresholds | ✓ quality profiles | ✓ requirements/compliance | Versioned policy-as-data with evidence |
| Difference/before-after maps | ✓ snapshots | ✓ scan comparison | Survey selection/merging | Time-series, causal annotations, change alerts |
| Team collection | Project merge | Scan/location combination | `.SSTRACK` export/import | Offline merge + live distributed sessions |
| Open raw project schema | No | CSV report export | No public project schema | Yes |
| Re-run analysis with new algorithm | Not transparently | Not transparently | Background recalculation exists | Core design requirement |
| Confidence/uncertainty map | — | Confidence-like propagation controls | Extrapolation controls/warnings | Mandatory companion to every estimate |
| Remote fixed sensors | — | — | — | First-class |
| Extensible radio/plugin SDK | — | — | — | WASM/native SDK |

### 4.3 Open-source source-code audit and reuse policy

The competitive audit explains **what** the product should do. The source-code audit explains **what we should actually reuse**. Those are different questions. A feature can be excellent prior art while its implementation is still the wrong dependency boundary.

#### Repository-level decision

| Project | Useful strength | Source-level conclusion |
|---|---|---|
| **Deconflict** | Small modern web planner, floor editing, channel coloring, AP placement UX, simple fast RF preview | Keep as reference/interchange target. Its inverse-quartic/radius propagation, raster wall attenuation, simple contention estimate, graph coloring, and Lloyd/PSO placement are useful baselines but not the RF Atlas scientific core. Contribute generic reproducibility/interchange improvements upstream where useful. |
| **Kismet** | Mature capture-helper ecosystem, channel control, remote capture, packet pipeline, device tracking, KismetDB, REST/WebSocket, BLE/Zigbee/SDR sources | Integrate externally. Reuse the expensive hardware-facing acquisition system without importing its global service/device model or GPL implementation into RF Atlas core. |
| **wifiheatmap** | Minimal “stand here → click map → collect scan/iperf → interpolate” workflow | Reference only. Reproduce TIN/convex-hull and survey behavior independently as test oracles; do not revive the Qt/GPLv2 codebase. |
| **Sionna RT** | Hardware-accelerated radio path and radio-map solvers, materials, antennas, differentiability, tests | Adopt directly inside an isolated worker. Do not build a competing high-fidelity ray tracer unless measurements later prove Sionna fundamentally unsuitable. |

#### Narrow waist

```text
                                      RF ATLAS
┌──────────────────────────────────────────────────────────────────────────────┐
│ canonical project/site/geometry/identity/observation/metric/requirements     │
│ survey workflows | active diagnostics | spatial analysis | fast RF solver   │
│ Wi-Fi PHY/MAC/capacity | optimization | history | reports | privacy         │
└───────────────────────────────┬──────────────────────────────────────────────┘
                                │ versioned ports; no foreign domain objects
                 ┌──────────────┼────────────────┬────────────────────┐
                 │              │                │                    │
                 ▼              ▼                ▼                    ▼
        Native collectors   Kismet adapter   Sionna job client   Deconflict bridge
        Win/macOS/Linux     REST/WS/KismetDB local/remote worker open interchange
        Android/iOS agents  PCAPNG + sensors Python/Mitsuba/DrJit no runtime dep
                                │
                                ▼
                         wifiheatmap oracle
                      tests/fixtures only; no runtime
```

#### Disposition meanings

- **ADOPT:** direct upstream dependency in a deliberately bounded component.
- **INTEGRATE:** external process/API/protocol/database/file producer.
- **CONTRIBUTE:** generic upstream improvement is the primary home; RF Atlas does not depend on acceptance.
- **REIMPLEMENT:** RF Atlas owns an independent implementation because the semantics, schema, licensing, or product requirements demand it.
- **REFERENCE-ONLY:** use as prior art, behavioral oracle, fixture source, or conceptual comparison without runtime/code dependency.

The complete 80-row repository-module matrix and 172-row RF Atlas subsystem matrix are retained in Appendix I and are authoritative when a summary elsewhere in this plan appears ambiguous.

---

## 5. TamoGraph 8.x behavioral model — “inside out”

This section reconstructs how the product behaves from its documented interfaces. It is the primary clean-room reference for the professional feature set.

### 5.1 Product decomposition [V]

TamoGraph presents four major evidence modes:

1. **Passive survey:** listen for Wi-Fi transmissions and build RF/coverage views.
2. **Active survey:** associate to a target network and measure actual link/transport behavior.
3. **Predictive survey:** create a virtual environment and compute expected RF behavior without a radio.
4. **Spectrum analysis:** ingest non-protocol RF energy from supported spectrum hardware.

Passive and active can run together with two adapters. Passive and spectrum can run together. Active and spectrum are not a sensible simultaneous pairing on a single nearby setup because the active client itself contributes RF energy and can contaminate the spectrum measurement.

### 5.2 Project hierarchy [V]

The conceptual hierarchy is:

```text
Project
├── Floor plans / site maps
│   ├── calibration and coordinate frame
│   ├── environment / extrapolation settings
│   ├── predictive geometry and APs
│   └── surveys
│       ├── mode and metric configuration
│       ├── paths / points
│       ├── passive observations
│       ├── active observations
│       └── spectrum observations
├── AP inventory / grouping / aliases
├── client capability profiles
├── requirements profiles
├── material and antenna libraries
├── visualization settings
└── report configuration
```

Surveys can be selected and combined only when their modes and available metrics are compatible. This implies that TamoGraph's analysis layer operates over a selected set of typed survey datasets rather than one permanently merged raster.

### 5.3 Floor-plan ingestion and coordinate calibration [V]

Documented inputs include common raster formats, vector formats, PDF, and CAD formats such as DWG/DXF. Indoor maps are calibrated by identifying two points with a known physical distance. GPS/georeferenced workflows require multiple non-collinear control points. Predictive projects add floor height, slab/roof material, openings, aligned floor references, and editable geometry.

**Inferred implementation [I]:**

- Imported pages are normalized into an internal scene coordinate system.
- A 2D affine/similarity transform maps image pixels to project meters.
- GPS projects add a projected geospatial coordinate reference system rather than performing calculations directly in latitude/longitude.
- Multi-floor plans use a building coordinate system with per-floor transforms and elevation.

RF Atlas should make these transforms explicit objects rather than hidden settings.

### 5.4 Capture adapters and channel scanner [V]

For passive work, TamoGraph requires compatible hardware and special monitor-capable drivers. Active surveys can use a normal associated Wi-Fi adapter. The scanner exposes channels and per-channel dwell/scan intervals, with a documented default around a quarter second per channel in some workflows. A complete scan cycle depends on the selected channel set and adapter channel-switch behavior.

TamoGraph supports per-adapter RSSI correction. This is important because two chipsets can report different RSSI for the same field strength.

**Inferred pipeline [I]:**

```text
adapter driver
  -> frame/scan event acquisition
  -> timestamp + channel attribution
  -> 802.11 management-frame/IE parser
  -> AP/BSSID registry update
  -> per-cycle observation accumulator
  -> survey-position association
  -> immutable survey samples
```

### 5.5 Survey state machine [V/I]

Likely states:

```text
Idle
  -> Preflight
  -> ScannerWarmup
  -> AwaitingFirstPosition
  -> CapturingPoint | CapturingSegment | CapturingGPS
  -> Paused
  -> Finalizing
  -> Complete
```

Preflight must verify selected adapters, channel set, scan-cycle duration, active server reachability, target SSID/BSSID, map calibration, and metric availability.

### 5.6 Point-by-point surveys [V]

At each clicked position, TamoGraph waits long enough to complete the required scan coverage; its manual describes collecting complete cycles rather than accepting an arbitrary instant. The operator can walk any path and gets stronger spatial truth at the cost of speed.

**Important design lesson:** a point is not complete merely because the user clicked. Completion is a predicate over channel coverage, dwell time, minimum observations, active-test completion, and pose stability.

RF Atlas should show a point-quality ring with independent completion bars:

- channel sweep completeness;
- target AP observations;
- temporal stability;
- active test completion;
- spectrum sweep completeness;
- pose/orientation stability;
- minimum sample count.

### 5.7 Continuous surveys [V]

The operator clicks at the beginning of a path segment and at turns/endpoints while walking at a steady pace. Measurements captured between clicks are positioned along the straight segment, effectively using time progression and assumed uniform motion.

**Inferred position calculation [I]:**

For segment anchors `(x0, t0)` and `(x1, t1)`, a measurement at `t` receives:

```text
u = clamp((t - t0) / (t1 - t0), 0, 1)
position = x0 + u * (x1 - x0)
```

This is useful but fragile when the user pauses, changes speed, curves around furniture, or records a late click. RF Atlas should retain the original anchors and timestamps, expose the assumption, and allow path correction after collection. AR/IMU or visual odometry should replace uniform-motion interpolation when available.

### 5.8 GPS surveys [V]

GPS mode automatically associates observations with position from NMEA or operating-system location sources. The system must preserve horizontal accuracy, altitude quality, fix type, dilution, age, and coordinate reference; a location without uncertainty is incomplete evidence.

### 5.9 Passive analysis layers [V]

Documented passive visualizations include:

- signal level;
- noise and SNR where hardware supplies noise;
- signal-to-interference ratio;
- primary/secondary/tertiary AP coverage;
- AP count and redundant coverage;
- expected PHY rate;
- frame/PHY format;
- channel width;
- per-band/channel coverage;
- requirement compliance.

AP selection, survey selection, band selection, SSID/BSSID grouping, minimum signal thresholds, and extrapolation settings change the layer.

### 5.10 AP grouping and Multi-SSID handling [V]

Multiple virtual BSSIDs can belong to one physical AP. TamoGraph links Multi-SSID identities so a physical transmitter does not falsely count as several independent interferers or redundant APs. It supports relinking after collection.

RF Atlas must generalize this to an **identity graph**:

```text
Physical device
  ├── radio 2.4 GHz
  │   ├── BSSID A / SSID employee
  │   └── BSSID B / SSID guest
  ├── radio 5 GHz
  │   └── BSSID C
  └── MLD identity (Wi-Fi 7, when discoverable)
      ├── link 1
      └── link 2
```

Grouping evidence may include Multiple BSSID elements, transmitted/non-transmitted BSSID profiles, vendor patterns, shared capabilities, colocated radio reports, MLO elements, user declarations, and controller imports. Every inferred edge needs confidence and an explanation.

### 5.11 AP ranking [V]

TamoGraph can evaluate the best, second-best, and third-best AP at each location. This is more useful than simple AP count because roaming and resilience depend on the quality of alternatives, not merely detectability.

RF Atlas should implement arbitrary rank `k`, plus:

- best candidate by RSSI;
- best by predicted bidirectional SNR;
- best by estimated airtime/capacity;
- best by policy-eligible AP;
- best alternative after failure of the primary;
- margin between rank 1 and rank 2;
- hysteresis-sensitive roam opportunity.

### 5.12 AP automatic location [V/I]

TamoGraph estimates AP positions from survey evidence and can use those positions only for icons or as supplemental information in visualizations. It warns that calculated locations are estimates.

The exact algorithm is undisclosed. Plausible implementations include weighted centroids, path-loss fitting, nonlinear least squares, or a proprietary combination.

RF Atlas should implement multiple named solvers and compare them:

1. weighted RSSI centroid;
2. robust log-distance nonlinear least squares;
3. Bayesian position/Tx-power posterior;
4. ray/model-assisted localization;
5. user-anchored/manual position.

The UI should render a probability ellipse/region, not a deceptively precise pin.

### 5.13 AP-on-a-stick [V]

TamoGraph supports surveying one temporarily mounted physical AP at several candidate positions and splitting observations associated with its repeated BSSID into virtual AP identities. This lets a designer empirically evaluate placements before purchasing/installing all APs.

RF Atlas should treat AP-on-a-stick as an experiment:

- candidate placement ID;
- exact mount position, height, orientation, antenna, channel, power, firmware;
- timestamped run;
- environment/occupancy notes;
- calibration status;
- evidence equivalence checks across candidate runs;
- side-by-side and combined hypothetical deployment;
- automatic recommendation with uncertainty and constraints.

### 5.14 Interference model [V/I]

TamoGraph exposes SIR and adjacent/co-channel effects, incorporates network utilization assumptions, handles channel bonding, and prevents multiple SSIDs from one physical AP from being counted as independent radios.

Its exact spectral overlap and airtime formulation is not public. RF Atlas should avoid a binary “overlap” heuristic and instead integrate interference coupling over channel spectra, then combine powers in the linear domain.

### 5.15 Expected PHY model [V]

TamoGraph estimates expected PHY rate using signal-level mappings and a selected client capability profile: supported standard, channel width, and spatial stream count. This explicitly acknowledges that the surveying adapter may outperform or underperform real clients.

RF Atlas must go further by distinguishing:

- AP capability;
- client capability;
- common negotiated capability;
- downlink and uplink SNR;
- MCS/NSS/GI/RU/channel-width feasibility;
- interference/airtime effect;
- expected PHY versus expected goodput;
- confidence in the chipset-specific sensitivity curve.

### 5.16 Active surveys [V]

TamoGraph can associate by SSID with roaming or lock to a BSSID. Basic tests can focus on reachability/RTT; advanced tests use a throughput-test server and support TCP/UDP, upstream/downstream, IPv4/IPv6, and traffic/QoS profiles. Active visualizations include actual PHY rate, TCP/UDP throughput, UDP loss, RTT, associated AP, and requirements.

**Key limitation to avoid:** one active destination cannot by itself distinguish Wi-Fi impairment from a wired uplink, router, WAN, or remote server bottleneck.

### 5.17 Hybrid active + passive [V]

Using separate adapters allows passive channel scanning while another adapter remains associated and performs active tests. This avoids the fundamental conflict between channel hopping and maintaining an active association.

RF Atlas should support additional topologies:

- one associated adapter + one monitor adapter;
- associated adapter + remote passive sensor;
- one tri-band monitor adapter per band;
- multiple fixed sensors + mobile active client;
- active mobile client + Ethernet-connected reference endpoint;
- simultaneous client profiles using phone/laptop/IoT agents.

### 5.18 Spectrum integration [V]

TamoGraph can ingest supported spectrum analyzers and show current, maximum/hold-like, and waterfall/history views. Spectrum samples can be correlated to the survey path and included in reports. Multiple analyzer models can be combined in some releases to improve band coverage or sweep performance.

RF Atlas should store spectrum data as calibrated frequency-time-power tiles with explicit:

- center/start/stop frequency;
- bin width and resolution bandwidth;
- window function;
- detector type;
- dwell/sweep time;
- max/average/min statistics;
- calibration and antenna factors;
- saturation/clipping flags;
- sensor position and orientation.

### 5.19 Predictive environment [V]

TamoGraph's virtual model includes:

- walls, partitions, polygonal obstructions, and attenuation zones;
- separate attenuation values by band and reflection percentage;
- floors, roofs, openings, and multi-floor alignment;
- AP positions, heights, antenna orientation, tilt, and patterns;
- radio standard, channel, width, power, spatial streams, guard interval, and other PHY parameters;
- 6 GHz AP classes and power spectral density behavior;
- client capability and application templates;
- optional higher-quality propagation effects such as reflections/Fresnel-related modeling;
- CPU/GPU quality settings and background calculation.

### 5.20 Predictive propagation implementation [I]

Public behavior is compatible with a tiered solver:

1. direct path/free-space or log-distance loss;
2. transmission losses through intersected objects;
3. floor penetration and vertical geometry;
4. antenna gain in the ray direction;
5. optional reflected/diffracted paths;
6. PHY mapping and coverage rasterization;
7. GPU evaluation of many transmitter-cell combinations.

The exact TamoGraph solver, number of reflections, diffraction model, material coefficients, field combination, spatial grid, and GPU kernels are undisclosed. RF Atlas will expose its own stack openly: original P0/P1 empirical methods plus the pinned Sionna RT P2/P3 execution engine, all validated against measurements.

### 5.21 Automatic AP placement [V/I]

TamoGraph documents automatic planning for coverage and capacity, required-coverage areas, client/application templates, channel plans, transmit-power choices, disabling unnecessary radios, precision settings, minimum AP/RSSI constraints, reconfiguration of existing APs, and—since version 8.4—a per-band maximum clients-per-AP control.

The exact optimizer is undisclosed. Its historical GPU acceleration and large speed improvements suggest extensive parallel scoring of candidate layouts; the outer search may be greedy, heuristic, local-search, evolutionary, integer, or hybrid.

RF Atlas should use an explicit hybrid optimizer rather than attempting to imitate an unknown method:

- geometry-aware candidate generation;
- GPU propagation precomputation/surrogates;
- CP-SAT or mixed-integer decisions for placement/radio/channel/power;
- min-cost-flow/client-assignment subproblems;
- local search for continuous positions or antenna angles;
- robust evaluation under model uncertainty and AP failure.

### 5.22 Requirements and compliance [V]

TamoGraph supports configurable thresholds such as minimum signal, SNR, SIR, AP count, PHY rate, frame format, channel width, active throughput, and maximum RTT. Profiles can require a percentage of the area to pass. Version 8.4 adds a direct requirements-compliance percentage view.

RF Atlas should treat requirements as versioned policy data, not UI state. A compliance result contains:

- policy/profile version;
- target client/application/population;
- eligible area mask;
- metric source and freshness;
- pass/fail/unknown area;
- confidence-weighted and strict compliance;
- exact failing constraints;
- worst locations and recommended remediation;
- whether evidence was measured or predicted.

### 5.23 Reports and teamwork [V]

TamoGraph reports can select maps, surveys, APs, bands, ranks, paths, AP lists, comments, virtual objects, and photos; formats include PDF, ODT/word-processor-oriented output, HTML/MHT, and geospatial exports. Team collection is primarily project-copy and survey-track import/export.

RF Atlas should preserve offline file exchange but add operation-based merging and an optional live coordinator. A report must be a reproducible build artifact generated from an analysis manifest, not a manually assembled screenshot collection.

### 5.24 Likely internal compute graph [I]

A plausible clean-room model is:

```text
Project state
  ├── geometry version
  ├── survey-selection version
  ├── AP/grouping version
  ├── client/profile version
  ├── requirement version
  └── visualization configuration
          │
          v
Normalized evidence queries
          │
          v
Metric fields / propagation fields
          │
          v
Spatial interpolation or simulation raster
          │
          v
Threshold/compliance transform
          │
          v
Color/contour renderer
          │
          v
UI, export, report
```

The documented background recalculation, GPU settings, layer controls, and cached visual behavior strongly imply dependency tracking and raster caching, but exact implementation is unknown.

### 5.25 What must be black-box tested [E]

- Scan-cycle completion criteria in point mode.
- Exact temporal-to-spatial assignment in continuous mode.
- How RSSI samples are averaged or filtered.
- Interpolation kernel and sample influence radius.
- Extrapolation boundary construction.
- AP-location solver and use of wall geometry.
- SIR formula and adjacent-channel coupling.
- Expected-PHY lookup tables.
- Predictive path-loss/material/reflection math.
- Auto-planner objective ordering and search behavior.
- Treatment of multi-BSSID/MLO identities.
- Per-floor propagation and openings.
- Requirement percentage denominator and treatment of unknown cells.
- Cache invalidation and deterministic reproducibility.

Each test belongs in the competitor-validation harness described later; no production behavior should depend on guessing these answers.

---

## 6. RF Atlas capability catalog

The feature IDs below become roadmap, issue, API, test, and documentation anchors. A capability is not complete until its evidence path, uncertainty behavior, export representation, and automated tests exist.

### 6.1 Inspector and live RF browser

#### INS-001 — Nearby network table

Show one row per BSSID/link with expandable physical-device, radio, ESS, and MLD groupings.

Required fields when available:

- SSID, hidden/non-transmitted status, BSSID, MLD/link identity;
- band, primary channel, center frequencies, width, puncturing pattern;
- RSSI, noise, SNR, min/median/max, variance, sample count, age;
- beacon interval, DTIM, last seen, first seen;
- 802.11 generation and advertised PHY capabilities;
- MCS/NSS/channel-width/guard-interval capabilities;
- security suites, PMF capability/requirement, transition modes;
- country/regulatory information, 6 GHz power class, PSC/non-PSC;
- BSS load/channel utilization and station count when advertised;
- vendor/OUI with source version;
- current association status, link rate, MCS/NSS, BSSID, roaming state;
- sensor/adapter and observation method.

Never display unsupported fields as zero. Use `unknown`, `not advertised`, `not observable`, and `not applicable` distinctly.

#### INS-002 — Live signal timeline

- Multi-select APs/networks.
- RSSI, noise, SNR, current PHY, retry rate, channel utilization, latency, loss, and roam events on synchronized timelines.
- Retain configurable history from seconds to days.
- Cursor inspection and event annotations.
- Show adapter channel at every sample to expose hopping gaps.
- Compare raw samples, robust aggregate, and smoothed line.
- Make smoothing optional and label its window/filter.

#### INS-003 — Channel views

- 2.4/5/6 GHz channel occupancy chart.
- Primary and bonded channel spans, 80+80 where relevant, and 320 MHz channels.
- Wi-Fi 7 puncturing visualization.
- Per-channel AP count, estimated airtime, measured energy occupancy, observed frame airtime, and active associated clients.
- Adjacent-channel coupling calculated from spectral masks rather than only overlapping trapezoids.
- DFS state/events and 6 GHz PSC marking.
- Time-slider to reveal intermittent interference.

#### INS-004 — Network comparison

Pin APs/networks and compare:

- RSSI/SNR stability;
- security/capability differences;
- channel/width use;
- BSS load and observed airtime;
- expected client compatibility;
- historical change;
- likely physical-device grouping.

#### INS-005 — Information-element explorer

Lab-mode decoded 802.11 beacon/probe/association information elements with:

- raw bytes;
- parsed tree;
- standards reference;
- malformed/contradictory element warnings;
- diff between APs or before/after firmware/configuration changes.

#### INS-006 — Guided channel recommendation

Recommendations must be scenario-specific and explain:

- affected radios and clients;
- co-channel versus adjacent-channel cost;
- available non-overlapping widths;
- DFS availability/risk;
- 6 GHz client support;
- neighboring utilization and energy occupancy;
- cost of widening/narrowing channels;
- MLO interactions;
- confidence and missing evidence.

The recommendation engine must not reduce Wi-Fi planning to “pick the channel with the fewest SSIDs.”

#### INS-007 — Current connection diagnostician

Continuously show a layered path:

```text
client radio
  -> associated AP/link
  -> WLAN/VLAN
  -> default gateway
  -> local test server
  -> DNS resolver
  -> Internet control targets
  -> user-selected application endpoint
```

Attribute degradations by comparing metrics at each boundary.

### 6.2 Project, site, and map management

#### MAP-001 — Project hierarchy

```text
Workspace
└── Project
    ├── Sites
    │   ├── Buildings
    │   │   ├── Floors
    │   │   └── Outdoor areas
    │   └── Coordinate systems
    ├── Infrastructure inventory
    ├── Survey sessions
    ├── Predictive scenarios
    ├── Requirement policies
    ├── Analysis runs
    └── Reports
```

Support one home through multi-campus projects without imposing enterprise complexity on small projects.

#### MAP-002 — Plan import

Initial formats:

- PNG, JPEG, TIFF, WebP;
- PDF page rasterization/vector extraction;
- SVG;
- GeoJSON and GeoPackage geometry;
- DXF where a reliable permissive library is available.

Later:

- DWG through a separately licensed adapter;
- IFC/BIM;
- GeoTIFF;
- indoor map standards and vendor controller exports.

The import wizard previews layers, units, scale, rotation, page, color/contrast, and vector simplification.

#### MAP-003 — Calibration and georeferencing

- Two-point known-distance calibration.
- Multi-point least-squares calibration for maps with distortion.
- Area/dimension calibration.
- North/orientation control.
- GPS control points with residual error display.
- Select a local projected CRS for outdoor work.
- Store the complete transform and residuals.
- Prevent accidental scale changes after surveys; require a versioned migration.

#### MAP-004 — Floor-plan editor

Draw/edit:

- rooms and coverage zones;
- walls, doors, windows, glass, shelving/racks, elevators, shafts, columns;
- attenuation zones and crowds;
- floor slabs, roofs, ceilings, raised floors;
- stairs, atria, holes/openings;
- cable pathways, closets, switches, PoE budgets, mounting restrictions;
- no-AP, preferred-AP, no-cable, hazardous, privacy, and aesthetic zones.

Snapping, constraints, dimensions, multi-select, copy/paste, layers, undo/redo, and topology validation are mandatory.

#### MAP-005 — Multi-floor building model

- Align floors with shared anchors.
- Store floor elevation, clear height, slab composition, ceiling plenum, and roof.
- Display stacked 2D and true 3D views.
- Model openings and zero/low attenuation paths explicitly.
- Allow propagation from measured APs on adjacent floors.
- Compare vertical coverage and excessive leakage.

#### MAP-006 — LiDAR/room-scan import

- Apple RoomPlan/USDZ import where permitted.
- ARKit/ARCore mesh/anchor import.
- Manual cleanup workflow.
- Confidence per wall/plane/door/opening.
- Merge multiple rooms/scans using user anchors.
- Preserve scan coordinate transforms and source files.

#### MAP-007 — Outdoor mapping

- Offline map packages and user-provided tiles.
- GPS track overlay.
- Satellite/map imagery with license-aware caching.
- KML/KMZ, GeoJSON, and GeoPackage export.
- Terrain/elevation support in later predictive phases.

#### MAP-008 — Photos, notes, and annotations

Attach geolocated/floor-positioned:

- photos of APs, ceilings, obstructions, and closets;
- notes and voice notes;
- cable/port labels;
- installation instructions;
- environmental events such as doors open/closed or crowd occupancy.

### 6.3 Survey acquisition modes

#### SUR-001 — Point survey

A user selects a position and the system captures until the selected quality gate passes. The point stores a capture window, not one scalar.

Quality-gate dimensions:

- selected-channel completion;
- target network seen enough times;
- active tests completed;
- spectrum sweep completed;
- pose stable;
- minimum duration/sample count;
- variance below optional threshold;
- clock and location quality acceptable.

#### SUR-002 — Manual continuous-path survey

- Click/tap start, turns, pauses, and stop.
- Distribute samples by monotonic timestamp only when no better pose exists.
- Detect implausible speed/turns.
- Allow editing path anchors after survey and recompute positions without altering raw timestamps.
- Show unobserved channel gaps along each segment.

#### SUR-003 — AR/SLAM path survey

Mobile companion estimates continuous 6-DoF motion using camera and IMU, anchored to floor-plan control points.

- Record pose plus covariance/tracking quality.
- Allow QR/AprilTag/manual anchors to correct drift.
- Relocalize after interruption.
- Do not pretend centimeter accuracy when tracking is degraded.
- Correlate desktop/remote passive sensors with the mobile pose stream.

#### SUR-004 — GPS survey

- NMEA serial/Bluetooth/USB and OS location sources.
- Store fix, accuracy, satellite/DOP metadata where available.
- Configurable minimum accuracy.
- Automatic pause on poor fix.
- Support vehicle, walking, and high-resolution static modes.

#### SUR-005 — Snapshot survey

A short time-bounded capture at a fixed location for:

- before/after configuration comparisons;
- time-of-day comparisons;
- interference investigation;
- AP-on-a-stick candidates;
- calibration runs.

#### SUR-006 — Guided route

- Generate a route from map geometry and desired sample spacing.
- Route around obstacles.
- Flag unsurveyed or low-confidence cells.
- Re-route after skipped areas.
- Optimize route for point mode or continuous mode separately.

#### SUR-007 — Team survey

- Partition a floor into assignments.
- Device/adapter calibration compatibility check.
- Offline collection and operation-based merge.
- Optional live coordinator showing coverage and missing regions.
- Conflict handling for map edits, AP aliases, and survey metadata.
- Cross-sensor bias estimation in overlap zones.

#### SUR-008 — Remote/fixed sensor survey

A lightweight Linux sensor can stream or spool:

- monitor-mode frames/metadata;
- spectrum data;
- GPS/time;
- environmental telemetry;
- health and calibration.

Fixed sensors enable temporal RF maps, distributed triangulation, and correlation with a moving active client.

#### SUR-009 — Robotic survey (later)

- ROS 2 bridge.
- Pose and covariance ingestion.
- Route/waypoint API.
- Safety remains with the robot stack.
- Useful for warehouses, campuses, and repeatable nightly surveys.

#### SUR-010 — AP-on-a-stick experiment

Dedicated workflow described in Section 5.13, including candidate equivalence checks and hypothetical combined deployment.

### 6.4 Passive survey metrics and heatmaps

Each map can be filtered by project area, time, sensor, network, BSSID, physical AP, radio/link, band, channel, client profile, and evidence method.

#### Core coverage layers

- **PAS-001:** RSSI / received signal level.
- **PAS-002:** noise floor, only when actually measured.
- **PAS-003:** SNR.
- **PAS-004:** co-channel interference power.
- **PAS-005:** adjacent-channel interference power.
- **PAS-006:** SIR.
- **PAS-007:** SINR.
- **PAS-008:** primary/best AP coverage.
- **PAS-009:** secondary, tertiary, and arbitrary rank coverage.
- **PAS-010:** rank-1/rank-2 margin.
- **PAS-011:** AP count above a configurable threshold.
- **PAS-012:** physical-radio count after multi-BSSID deduplication.
- **PAS-013:** band coverage and dual/tri-band eligibility.
- **PAS-014:** channel/width coverage.
- **PAS-015:** coverage holes and edge leakage.

#### Capacity and protocol layers

- **PAS-020:** advertised channel utilization/BSS load.
- **PAS-021:** observed frame airtime utilization.
- **PAS-022:** estimated total medium occupancy.
- **PAS-023:** station/client density.
- **PAS-024:** retry rate overall and by AP/client/direction/frame class.
- **PAS-025:** PHY generation/frame format.
- **PAS-026:** expected MCS/NSS/channel width.
- **PAS-027:** expected PHY rate.
- **PAS-028:** expected single-client goodput.
- **PAS-029:** estimated airtime cost per application profile.
- **PAS-030:** legacy-client/protection overhead exposure.
- **PAS-031:** beacon/probe management overhead by SSID count.
- **PAS-032:** OFDMA/MU-MIMO capability compatibility, clearly labeled capability rather than guaranteed operation.
- **PAS-033:** Wi-Fi 7 MLO eligibility and per-link coverage.
- **PAS-034:** puncturing-aware usable spectrum.

#### Roaming and resilience layers

- **PAS-040:** redundant coverage by eligible APs.
- **PAS-041:** failure-of-one-AP coverage.
- **PAS-042:** roam candidate availability.
- **PAS-043:** overlap corridor quality.
- **PAS-044:** sticky-client risk based on profile/hysteresis assumptions.
- **PAS-045:** 802.11k/v/r/FT capability and consistency.
- **PAS-046:** security/PMF consistency across an ESS.

#### Data-quality layers

- **PAS-050:** raw sample density.
- **PAS-051:** effective sample density after temporal/spatial autocorrelation.
- **PAS-052:** age/recency.
- **PAS-053:** adapter calibration uncertainty.
- **PAS-054:** pose/location uncertainty.
- **PAS-055:** temporal variance.
- **PAS-056:** interpolation uncertainty.
- **PAS-057:** extrapolated/unsupported area.
- **PAS-058:** channel-sweep completeness.
- **PAS-059:** sensor disagreement/bias.

### 6.5 Active survey metrics and diagnostics

#### ACT-001 — Multi-tier latency

At every survey window, measure separately when permitted:

- AP/local link health from OS telemetry;
- default gateway RTT/loss;
- wired LAN reference server RTT/loss;
- DNS resolver RTT and success;
- one or more Internet control targets;
- application endpoint RTT;
- optional one-way delay with synchronized agents.

Render individual maps and an attribution view.

#### ACT-002 — Throughput

- TCP upload/download/bidirectional.
- UDP offered load, received rate, loss, reordering, jitter.
- QUIC/HTTP application-like transfers.
- Single and parallel stream profiles.
- Fixed-duration and fixed-byte tests.
- LAN reference and Internet target separated.
- Detect wired server/uplink bottlenecks.
- Record test-induced channel load.

#### ACT-003 — Link state

Capture when APIs expose it:

- associated BSSID/MLD/link;
- channel/width;
- PHY rate;
- MCS/NSS/GI;
- RSSI/SNR/noise;
- transmit retries/failures;
- power-save state;
- interface counters;
- DHCP/default route/VLAN identity where observable.

#### ACT-004 — Roaming

Record a structured roam event:

```text
old AP/link
new AP/link
trigger/initiator if observable
authentication method
start/end timestamps
scan duration
association/authentication duration
IP continuity
packet-loss burst
RTT/throughput impact
application disruption
candidate context
```

Maps show roam locations, transition paths, repeated ping-pong, failure, and user-impact severity.

#### ACT-005 — Packet loss and delay variation

- ICMP, UDP echo, STAMP/TWAMP-style probes where agents support them.
- Consecutive-loss burst distribution, not only mean percentage.
- RTT distribution: median, p90, p95, p99, max.
- Jitter/packet-delay variation with method stated.
- Reordering and duplication.

#### ACT-006 — Bufferbloat/load interaction

At selected points, run controlled load while measuring low-rate probes to gateway, LAN server, and Internet target. Report baseline versus loaded latency, direction, throughput, and likely bottleneck.

#### ACT-007 — DNS/application probes

Optional profiles:

- DNS lookup latency/failure/cache state;
- TCP connect and TLS handshake;
- HTTP time-to-first-byte and transfer;
- Web browsing burst;
- VoIP-like RTP probe;
- interactive gaming-like small UDP flow;
- video sustained/adaptive flow;
- IoT low-rate reliability.

These are diagnostics, not substitutes for passive RF evidence.

#### ACT-008 — QoS/WMM validation

- DSCP-marked test classes.
- Validate preservation across WLAN/LAN/WAN where user controls the network.
- Compare latency/loss under contention.
- Map unexpected remarking or queue behavior.
- Never assert actual over-the-air access category unless observable.

#### ACT-009 — Multi-client capacity test

Coordinate multiple authorized agents to assess:

- aggregate and per-client throughput;
- fairness;
- latency under load;
- airtime starvation;
- roaming under concurrent traffic;
- band steering and MLO behavior.

### 6.6 Spectrum analysis

#### SPE-001 — Device abstraction

Support:

- vendor-specific analyzers through separately licensed SDK plugins;
- SoapySDR-compatible devices;
- imported spectrum traces;
- remote spectrum sensors.

A Wi-Fi NIC's reported noise/channel utilization is not labeled as spectrum data.

#### SPE-002 — Live spectrum views

- Current power spectral density.
- Average, minimum, peak/max hold.
- Waterfall/spectrogram.
- Per-channel integrated power.
- Occupancy above threshold.
- Time-domain burst/event view where hardware supports it.
- Mark Wi-Fi channels, center frequencies, widths, and surveyed APs.

#### SPE-003 — Survey-correlated spectrum

- Associate frequency-time samples with position/pose.
- Point-mode completeness gate based on full sweep count.
- Continuous-mode sweep/path quality.
- Spectrum heatmaps per frequency range or classified source.
- Retrieve historical window around any path point.

#### SPE-004 — Interferer classification

Initial deterministic signatures with confidence:

- wideband/continuous;
- narrowband carrier;
- frequency hopper;
- periodic burst;
- microwave-like 2.4 GHz broadband emission;
- Bluetooth-like hopping;
- Zigbee/802.15.4-like channel activity;
- video transmitter/proprietary emitter patterns.

ML classifiers may be added only with labeled datasets and an “unknown” class. Classification never replaces the raw waterfall.

#### SPE-005 — Calibration

- Device calibration profile.
- Reference-level correction.
- antenna/cable gain/loss.
- frequency offset.
- clipping and dynamic-range checks.
- periodic validation reminders.

### 6.7 Predictive RF planning

#### PRE-001 — Fast empirical mode

Interactive preview while moving APs:

- free-space/log-distance baseline;
- line-of-sight obstruction intersections;
- frequency-dependent wall/floor losses;
- attenuation zones;
- antenna directional gain;
- per-band EIRP/PSD constraints;
- multi-floor direct paths.

Target: update within an interactive frame budget for ordinary homes/offices.

#### PRE-002 — Detailed high-fidelity mode

Implemented primarily through the isolated Sionna RT worker rather than a second RF Atlas ray tracer:

- multiple reflections;
- transmission/refraction through layered materials;
- edge diffraction;
- optional diffuse scattering;
- coherent path/CIR/CFR artifacts where justified;
- three-dimensional antenna pattern and polarization;
- planar or mesh radio maps;
- path-gain/RSS fields consumed by RF Atlas Wi-Fi semantics;
- terrain/outdoor scene support later.

RF Atlas P0/P1 may retain cheap Fresnel/edge penalties for responsiveness, but P2/P3 interaction physics is delegated to the adopted worker.

#### PRE-003 — Standards/model presets

Use current ITU indoor propagation/material guidance and documented channel-model references as starting points, but store presets as versioned, editable data. “Office,” “warehouse,” or “home” is an initial prior, not truth.

#### PRE-004 — Client profiles

A profile contains:

- supported bands/standards;
- channel widths;
- spatial streams;
- MLO class and link combinations;
- receiver sensitivity or SNR-to-PER/MCS curves;
- transmit power/EIRP behavior;
- antenna gain/orientation/body-loss assumptions;
- roaming thresholds/hysteresis;
- application requirements;
- source and confidence.

Ship generic conservative profiles and allow calibrated real-device profiles.

#### PRE-005 — AP/radio models

- chassis with one or more radios;
- standards/bands;
- conducted power range and increments;
- antenna connector/pattern;
- antenna height/orientation/tilt/polarization;
- chain/spatial-stream capability;
- supported channels/widths/puncturing;
- 6 GHz regulatory class;
- PoE and wired uplink limits;
- cost and install metadata;
- firmware/controller feature constraints.

#### PRE-006 — Antenna library

Use an open JSON schema supporting:

- frequency-specific 3D gain samples or spherical harmonics;
- azimuth/elevation cuts;
- polarization;
- nominal gain and efficiency;
- coordinate convention;
- interpolation method;
- source/license/checksum;
- mount orientation.

Import common manufacturer formats where lawful. Validate normalization and coordinate axes visually.

#### PRE-007 — Hybrid measured/predicted model

Prediction is calibrated with field data:

```text
measured RSSI = physical-model prediction + spatial residual + sensor bias + noise
```

Fit material/path-loss parameters only when the dataset makes them identifiable. Otherwise retain priors and show broad uncertainty. Use cross-validation and hold-out regions to prevent a model that merely memorizes survey points.

#### PRE-008 — Prediction outputs

All passive-like RF layers plus:

- predicted uplink and downlink separately;
- path contribution/debug view;
- dominant walls/floors/material losses;
- line-of-sight/reflection/diffraction path visualization;
- material sensitivity;
- model uncertainty;
- calibration residual;
- “what changed” difference map between scenarios.

### 6.8 Automatic planning and optimization

#### OPT-001 — Candidate generation

Generate AP candidates from:

- ceiling/wall geometry;
- user-marked preferred or allowed regions;
- existing cable drops and closets;
- grid/Poisson samples;
- room centroids and corridor skeletons;
- AP-on-a-stick candidate points;
- existing AP positions.

Prune positions that violate mounting, height, cable, power, safety, aesthetic, or exclusion constraints.

#### OPT-002 — Coverage optimization

Meet per-area requirements for selected client profiles, bands, AP ranks, and failure cases while minimizing weighted cost.

#### OPT-003 — Capacity optimization

Use spatial client-density and application demand profiles to estimate airtime, not merely Mbps. Include:

- protocol overhead;
- expected PHY distribution;
- retransmission margin;
- management overhead;
- contention efficiency;
- uplink/downlink mix;
- per-band client limits;
- wired uplink and AP processing limits.

#### OPT-004 — Joint radio plan

Choose jointly:

- AP placement/model;
- radio enable/disable;
- channel and channel width;
- puncturing where supported;
- transmit power;
- antenna orientation/tilt;
- MLO links;
- client association for capacity evaluation.

#### OPT-005 — Robustness

Evaluate:

- failure of each single AP/radio/uplink;
- model parameter uncertainty;
- occupancy scenarios;
- doors open/closed;
- furniture/crowd attenuation;
- neighboring network changes;
- client capability mix;
- minimum and worst-percentile performance.

#### OPT-006 — Explainable alternatives

Return a Pareto set, not one mysterious answer:

- lowest equipment cost;
- fewest APs;
- highest reliability;
- highest capacity;
- easiest cabling;
- lowest interference/leakage;
- balanced recommendation.

For each, show binding constraints and consequences.

### 6.9 Requirements, quality, and troubleshooting

#### REQ-001 — Policy-as-data

Requirements are declarative, versioned, shareable, and testable. A policy can target floors, zones, applications, device classes, bands, SSIDs, and times.

Example:

```yaml
name: voice-and-video-v1
scope:
  areas: [occupied]
  client_profiles: [phone-2x2, laptop-2x2]
requirements:
  - metric: predicted_bidirectional_rssi_dbm
    operator: ">="
    value: -67
    required_area_percent: 95
  - metric: secondary_eligible_ap_rssi_dbm
    operator: ">="
    value: -72
    required_area_percent: 90
  - metric: active_gateway_rtt_p95_ms
    operator: "<="
    value: 20
    required_area_percent: 95
  - metric: active_packet_loss_percent
    operator: "<="
    value: 1
    required_area_percent: 99
unknown_policy: fail
```

Thresholds above are illustrative, not universal recommendations.

#### REQ-002 — Compliance views

- Per-requirement pass/fail/unknown map.
- Overall strict intersection.
- Area percentage and weighted occupancy percentage.
- Confidence-aware percentage.
- Worst cells and causes.
- Measured versus predicted evidence split.
- Before/after delta.

#### REQ-003 — Quality profiles

Ship editable examples for:

- basic web/office;
- voice;
- video conferencing;
- gaming/interactive;
- high-density venue;
- warehouse handheld/scanner;
- IoT reliability;
- location services;
- guest Internet;
- home balanced/low-cost.

Profiles must cite assumptions and never masquerade as standards mandates.

#### REQ-004 — Root-cause engine

Use a transparent rule/graph system first. Example:

```text
low LAN throughput
AND low expected PHY
AND weak bidirectional SNR
=> likely RF coverage/client capability

low Internet throughput
AND normal LAN throughput
=> likely WAN/ISP/remote path

high retries
AND strong RSSI
AND high channel occupancy
=> likely contention/interference, not weak signal

roam interruption
AND healthy secondary coverage
AND long auth transition
=> investigate security/roaming configuration
```

Rank hypotheses by evidence and list disconfirming tests. An LLM may phrase the report but cannot invent metrics or alter deterministic conclusions.

### 6.10 Comparison, history, and reporting

#### CMP-001 — Snapshots and difference maps

Compare:

- before/after AP move or channel change;
- firmware versions;
- business hours versus empty building;
- doors open/closed;
- old/new hardware;
- measured versus predicted;
- two client profiles;
- two sensors/adapters;
- two interpolation/model algorithms.

#### CMP-002 — Temporal RF observatory

With fixed sensors:

- hourly/daily/weekly channel utilization;
- neighboring AP changes;
- noise/interference events;
- configuration drift;
- AP uptime and signal drift;
- anomaly detection with raw-event links.

#### REP-001 — Report builder

Blocks:

- executive summary;
- methodology and limitations;
- site/floor metadata;
- survey quality;
- infrastructure inventory;
- requirement results;
- selected maps and difference maps;
- AP/radio/channel plan;
- install instructions;
- active-test topology;
- raw-data appendix;
- source/calibration/algorithm manifest;
- photos/notes.

Exports:

- HTML package;
- PDF;
- editable DOCX/ODT where a maintainable renderer exists;
- CSV/JSON/Arrow/Parquet;
- GeoJSON/GeoPackage/KML/KMZ;
- PNG/SVG/GeoTIFF map layers;
- PCAPNG for explicitly retained frames;
- signed analysis manifest.

#### REP-002 — Reproducible report build

A report references immutable analysis-run IDs. Rebuilding the same report with the same binary/model assets must produce semantically identical values; non-deterministic rendering metadata is excluded from the content hash.

### 6.11 Optional multi-radio expansion

The core schema should not hard-code Wi-Fi, but Wi-Fi remains the first product.

Later plugins:

- BLE advertisement inspector and RSSI maps;
- BLE GATT explorer for authorized devices;
- over-the-air BLE sniffer ingestion;
- Zigbee/Thread channel and packet survey;
- 433/915 MHz sensors through SDR;
- private LTE/5G planning;
- UWB ranging maps.

Do not delay Wi-Fi delivery to build these.

---

## 7. Measurement methodology

### 7.1 Three evidence planes

RF Atlas stores three immutable/logically separated planes:

1. **Raw plane** — scan results, frames, radiotap, active-test packets/summaries, spectrum bins, poses, GPS fixes, system counters.
2. **Normalized fact plane** — decoded AP/radio/client observations, active-test intervals, calibrated signal values, spatially assigned samples, identity edges.
3. **Derived plane** — aggregates, interpolation models, prediction rasters, compliance, recommendations, reports.

A derived result never becomes a raw fact.

### 7.2 Coordinate frames

Every spatial item references a coordinate frame:

- image pixel coordinates;
- floor-local meters;
- building-local 3D coordinates;
- geospatial CRS;
- AR session coordinates;
- sensor-local pose.

Transforms form a graph. Each transform has:

- source and target frame;
- matrix/model type;
- control points;
- residual error/covariance;
- valid time interval;
- version and provenance.

### 7.3 Time model

Store both:

- monotonic capture time for ordering/durations;
- wall-clock UTC for correlation.

Each sensor session records estimated clock offset and uncertainty. Distributed high-precision work can use NTP/PTP/GNSS-derived synchronization, but the dataset must still preserve uncertainty rather than assuming perfect clocks.

### 7.4 Sensor capability contract

A collector returns a machine-readable capability document, for example:

```json
{
  "collector": "linux-nl80211-monitor",
  "version": "0.1.0",
  "bands": ["2.4", "5", "6"],
  "modes": ["monitor", "managed"],
  "can_hop_channels": true,
  "can_capture_radiotap": true,
  "reports_noise_dbm": "per-frame-optional",
  "reports_fcs": true,
  "reports_phy": ["ht", "vht", "he", "eht-partial"],
  "simultaneous_managed_monitor": "driver-dependent",
  "raw_payload_policy": "discard-by-default"
}
```

The UI and analysis engine use capabilities, not operating-system names, to decide what is possible.

### 7.5 Adapter calibration

At minimum, calibration estimates an additive RSSI correction per adapter and band. Advanced calibration can include channel/frequency, gain state, temperature, orientation, and antenna.

Recommended workflow:

1. Place reference AP and devices in fixed geometry.
2. Lock channel/width/power.
3. Collect repeated observations at multiple known attenuation levels or distances.
4. Compare adapters against a chosen reference or calibrated instrument.
5. Fit offset and, only if supported, scale/nonlinearity.
6. Record residual distribution and valid ranges.
7. Recheck after driver/OS/firmware changes.

Never silently apply a correction outside its calibrated range.

### 7.6 Device orientation and body effects

Record pose/orientation when possible. Guided surveys should suggest a consistent carrying position or deliberately sample multiple orientations. A phone held against a body and a laptop on a cart can differ. Client profiles can include body-loss/orientation distributions for planning.

### 7.7 Channel sweep scheduling

The scheduler balances:

- dwell per channel;
- complete cycle duration;
- target-band/channel priorities;
- beacon intervals;
- passive-only channels;
- DFS behavior;
- 6 GHz PSC discovery;
- active association constraints;
- multiple adapter allocation.

Each observation records the current tuned channel and dwell interval. Each spatial sample stores channel completeness. A missing AP on a channel not visited is not evidence of absence.

Potential scheduler modes:

- uniform round robin;
- weighted round robin;
- target-network priority;
- band-dedicated adapters;
- adaptive dwell based on observed density/activity;
- full-spectrum audit followed by focused survey.

Adaptive scheduling must not bias results invisibly; preserve the schedule and provide normalized coverage metrics.

### 7.8 Frame parsing

Parse management/control/data metadata sufficient for:

- beacon/probe identity and capabilities;
- channel/width/puncturing;
- security and PMF;
- BSS load;
- Multiple BSSID and MLO relationships;
- retry flag and sequence control;
- transmitter/receiver/BSSID roles;
- PHY/rate metadata from radiotap or platform equivalent;
- airtime estimation;
- roaming-relevant management frames when captured lawfully.

Payloads are discarded by default before persistence. Raw-frame retention is an explicit per-session policy.

### 7.9 Signal aggregation

Store every reported RSSI with method and sensor. Provide multiple explicit aggregators:

- median dBm for robust typical signal;
- trimmed mean dBm;
- linear-power mean converted back to dBm;
- percentile range;
- exponentially weighted live display;
- robust state-space estimate for time series.

Do not use one hidden average everywhere. Spatial interpolation uses the configured aggregate and records it.

### 7.10 Fundamental RF calculations

Convert dBm to milliwatts before summing powers:

```text
P_mW = 10^(P_dBm / 10)
P_dBm = 10 log10(P_mW)
```

When signal and noise are independently measured in dBm:

```text
SNR_dB = signal_dBm - noise_dBm
```

For desired signal `S`, aggregate interference `I`, and noise `N` in linear power:

```text
SIR_dB  = 10 log10(S / I)
SINR_dB = 10 log10(S / (I + N))
```

If noise is not measured, do not manufacture SNR by inserting an unlabeled universal noise floor. A configured expected noise prior can be used only in predicted layers and must be labeled.

### 7.11 Channel-overlap and interference coupling

For each interferer, calculate a coupling coefficient `c_ij` between desired and interfering channel spectra/receiver filters. Then:

```text
I_effective = sum_j(c_ij * P_j * utilization_j)
```

Inputs may include:

- spectral overlap/mask approximation;
- center frequency and width;
- puncturing;
- measured or assumed utilization;
- physical-radio deduplication;
- same-BSS coordination where relevant;
- spatial received power.

Offer a simple explanatory view, but keep the underlying method open and testable.

### 7.12 Airtime and utilization

Maintain separate metrics:

- advertised BSS Load channel utilization;
- adapter CCA busy time if exposed;
- observed frame airtime from captured frames;
- spectrum energy occupancy;
- inferred unobserved airtime;
- scenario utilization assumption in prediction.

Observed frame airtime estimates PHY preamble plus payload duration from decoded rate/MCS metadata. Capture loss means it is a lower bound unless corrected; label it accordingly.

### 7.13 Retry rate

For frames where retry metadata is observable:

```text
retry_rate = retry_marked_frames / eligible_data_frames
```

Break down by transmitter, receiver, BSSID, direction, access category, PHY, location, and frame size where sample size permits. Duplicate capture across multiple sensors must be deduplicated before aggregation.

### 7.14 Client density

Possible evidence tiers:

1. AP-advertised station count — aggregate and potentially stale.
2. Unique observed transmitter identities — privacy-sensitive and affected by randomization/capture loss.
3. Controller/API client inventory — authoritative for owned infrastructure.
4. Application/user-supplied density model — predictive planning.

Never merge these into a single unlabeled “client count.”

### 7.15 Expected PHY and goodput

A baseline expected-PHY pipeline:

```text
RSSI / predicted received power
  -> noise + interference model
  -> SINR distribution
  -> common AP/client PHY capabilities
  -> PER/BLER target and SNR-to-MCS curve
  -> feasible MCS/NSS/GI/BW/RU
  -> PHY rate distribution
  -> MAC efficiency / contention model
  -> expected goodput distribution
```

Use chipset/client-specific empirical curves when available; otherwise use clearly labeled generic curves. A single hard RSSI-to-rate table is an approximation.

### 7.16 Bidirectional link budget

Calculate both:

```text
AP -> client downlink
client -> AP uplink
```

Different transmit powers, antenna gains, noise figures, and receiver sensitivities can make uplink the limiting direction. Coverage compliance should usually use the weaker direction for two-way applications.

### 7.17 Active-test topology and attribution

Recommended endpoints:

- `E0`: local interface telemetry, no packets;
- `E1`: default gateway;
- `E2`: Ethernet LAN test agent near the gateway/core;
- `E3`: optional remote site/VPN endpoint;
- `E4`: stable Internet control endpoint(s);
- `E5`: application endpoint.

Example inference:

```text
E1 and E2 degraded, E4 degraded
    -> Wi-Fi/client/AP/LAN edge likely
E1 and E2 healthy, E4 degraded
    -> WAN/ISP/Internet path likely
E1 healthy, E2 degraded
    -> LAN routing/server/path issue
only E5 degraded
    -> application/destination path likely
```

These are hypotheses with confidence, not absolute diagnoses.

### 7.18 Active test scheduling

Profiles define:

- duration and packet/byte budget;
- warm-up;
- protocol/direction;
- offered load;
- parallelism;
- DSCP;
- endpoints;
- retry/backoff;
- acceptable network impact.

Point surveys can run thorough tests. Continuous surveys use lightweight probes and shorter throughput bursts to avoid smearing one long test across many locations.

### 7.19 Roaming methodology

Roam detection combines:

- association BSSID/link change;
- OS wireless events;
- management frames if monitor capture exists;
- packet path disruption;
- current signal/candidate context;
- IP route/address continuity.

The spatial event is an interval/path segment with uncertainty, not necessarily one point. Separate AP steering, client decisions, link switching within MLO, full reassociation, and IP-layer mobility where observable.

### 7.20 Spatial sample representation

A spatial sample is a capture window:

```text
sample_id
session_id
start_time / end_time
position distribution / covariance
orientation distribution
channel coverage
sensor set
raw observation references
active test references
spectrum references
quality vector
operator annotations
```

Derived point metrics are views over this object.

### 7.21 Interpolation strategy

Implement multiple algorithms behind one interface:

1. nearest neighbor — debugging/baseline;
2. inverse-distance weighting;
3. barrier-aware IDW using wall/path cost;
4. Delaunay/TIN linear interpolation;
5. radial basis functions;
6. ordinary/universal kriging;
7. Gaussian process with uncertainty;
8. physical-model prior plus learned residual;
9. graph/geodesic interpolation inside building geometry.

No algorithm is always best. Select through spatial cross-validation and metric-specific scoring.

### 7.22 Cross-validation

Use blocked spatial cross-validation rather than random point splits that leak nearby correlated samples.

Metrics:

- MAE/RMSE in dB or metric units;
- bias;
- percentile error;
- calibration of prediction intervals;
- boundary/dead-zone classification accuracy;
- stability across survey passes.

The project can choose a default algorithm per metric/area, but the choice and validation results are stored.

### 7.23 Barrier-aware distance

Euclidean distance through a concrete wall should not equal unobstructed hallway distance. Define a cost/path metric that includes:

- geometric distance;
- wall/floor intersections;
- attenuation-zone traversal;
- floor transitions/openings;
- optional learned material penalties.

Use it for interpolation and neighbor selection, not only full prediction.

### 7.24 Extrapolation

Outside the supported sample region:

- render transparent/hatched by default;
- calculate a distance-to-evidence map;
- require explicit opt-in to extrapolate;
- show uncertainty growth;
- never include unsupported cells in compliance as passing;
- allow predictive model support to extend coverage only when labeled as hybrid prediction.

### 7.25 Uncertainty decomposition

For each estimate, track as many components as practical:

- sensor measurement variance;
- calibration uncertainty;
- temporal/environmental variance;
- position/pose uncertainty;
- identity/grouping uncertainty;
- channel-coverage/capture uncertainty;
- interpolation/model uncertainty;
- parameter uncertainty;
- scenario/client-profile uncertainty.

Display total uncertainty and allow expert inspection of components. Avoid adding unrelated errors naively; use probabilistic propagation or scenario bounds appropriate to the model.

### 7.26 AP localization

Recommended robust model:

```text
r_i = P0 - 10*n*log10(d_i/d0) - wall_loss_i - floor_loss_i + error_i
```

Unknowns may include AP position, reference power `P0`, and path-loss exponent `n`. Use Huber loss/RANSAC or Bayesian inference, floor constraints, and building geometry. Report posterior/confidence region and sensitivity to assumptions.

AP positions imported from the user's controller or physically measured outrank inferred locations.

### 7.27 Change detection

Compare aligned surveys using:

- per-location paired residuals;
- uncertainty-aware significance;
- AP identity/config change;
- temporal baseline;
- occupancy/environment annotations.

Do not flag a 2 dB difference as meaningful when sensor/temporal variation is larger.

### 7.28 Survey quality scorecard

Do not collapse quality to one score by default. Show a vector:

- spatial coverage;
- sample spacing;
- channel completeness;
- target-network observations;
- repeatability;
- pose quality;
- calibration status;
- active endpoint health;
- sensor consistency;
- data age;
- interpolation validation.

A summary grade may exist only with the failed dimensions visible.

---

## 8. Predictive engine design

### 8.1 Why one propagation model is not enough

The engine exposes accuracy/cost tiers rather than a single opaque “quality” slider. The audit establishes a deliberate split: **RF Atlas owns fast deterministic planning models; Sionna RT owns the high-fidelity electromagnetic ray/path calculation.**

| Tier | Intended use | Primary implementation | Expected behavior |
|---|---|---|---|
| **P0 Instant** | Dragging APs and rough home planning | RF Atlas log-distance + vector line/material intersections | Interactive preview; conservative; deterministic; no claim of multipath fidelity |
| **P1 Fast** | Normal floor planning and optimizer coefficient generation | RF Atlas multi-wall/floor empirical model | Fast full-floor maps; calibrated material priors; CPU reference + optional `wgpu` acceleration |
| **P2 Detailed** | Professional design verification | **Sionna RT RadioMapSolver/PathSolver worker** | 3D materials/antennas; reflection/refraction/diffraction options; per-transmitter path gain/RSS; path diagnostics |
| **P3 Research** | Sensitivity, CIR/CFR/path studies, inverse calibration | **Sionna RT in explicitly configured research jobs** | Full solver parameters and seeds recorded; potentially expensive; outputs retained as artifacts |
| **P4 Hybrid** | Post-deployment digital twin | RF Atlas measured-residual/calibration model around P1/P2 predictions | Best local fit where measurements exist; uncertainty grows away from evidence |

Results from different tiers never silently share a cache key or quality label. P2/P3 are optional: ordinary desktop surveying and fast planning remain functional without Python, Mitsuba, Dr.Jit, CUDA, or Sionna installed.

### 8.2 Scene representation

Represent the building as a 3D scene, even when editing in 2D:

- floor/ceiling planes;
- extruded walls and columns;
- doors/windows/openings;
- layered materials;
- attenuation volumes;
- AP and client positions/orientations;
- coverage and demand volumes;
- optional furniture/crowd zones;
- outdoor terrain later.

Use robust computational geometry with spatial indices/BVH structures. Geometry validity checks must catch self-intersections, zero-thickness artifacts, gaps, duplicate surfaces, reversed normals, and misaligned floors.

### 8.3 Material model

A material record contains frequency-dependent properties, uncertainty, and source:

```yaml
id: material/concrete-generic-v1
name: Generic reinforced concrete
frequency_response:
  - range_ghz: [2.4, 2.5]
    transmission_loss_db_per_crossing: {mean: 12, stddev: 4}
    reflection_coefficient: {mean: 0.45, stddev: 0.15}
  - range_ghz: [5.1, 5.9]
    transmission_loss_db_per_crossing: {mean: 18, stddev: 5}
    reflection_coefficient: {mean: 0.55, stddev: 0.15}
  - range_ghz: [5.925, 7.125]
    transmission_loss_db_per_crossing: {mean: 21, stddev: 6}
    reflection_coefficient: {mean: 0.58, stddev: 0.16}
source: generic_prior
confidence: low
```

Numbers above are placeholders for schema illustration, not shipped defaults. Real presets require source review and field validation.

Support:

- per-crossing loss;
- loss per meter for volumes;
- complex permittivity/conductivity for advanced solvers;
- roughness/scattering parameters;
- layered assemblies;
- anisotropic or angle-dependent behavior later;
- posterior calibration per site.

### 8.4 Instant/empirical path loss

A baseline model:

```text
PL(d, f) = PL(d0, f)
         + 10*n(f, environment)*log10(d/d0)
         + sum_k L_material_k(f, incidence, thickness)
         + sum_m L_floor_m(f)
         + L_zone(f, path_length)
         + L_misc
```

Received power:

```text
Pr = conducted_power
   + transmit_antenna_gain(direction, f)
   - cable_loss
   - PL
   + receive_antenna_gain(arrival_direction, f)
```

For 6 GHz, model PSD/EIRP/channel-width rules and AP class explicitly; do not scale power using a generic rule hidden in UI code.

### 8.5 Advanced path solver — adopt Sionna RT

Do **not** build a second research-grade RF ray tracer as the default plan. The source audit found Sionna RT already provides the expensive, specialized machinery we need: scene-backed ray tracing, antenna arrays/patterns, radio materials, candidate generation, image-method solving, electromagnetic field calculation, path coefficients/delays/AoA/AoD/Doppler, and radio maps.

RF Atlas adopts Sionna RT inside `rfatlas-sionna-worker`. The worker supports at least:

- `validate_scene`;
- `compute_paths`;
- `compute_radio_map`;
- `convergence_sweep`;
- `calibration_step`;
- `debug_render`.

A path result retained by RF Atlas includes:

```text
job/scene/solver version and hash
transmitter / receiver
frequency / bandwidth
sequence of interactions
path length and delay
angle of departure/arrival
complex path coefficient
Doppler where requested
solver sampling/depth/feature parameters
seed and backend
artifact checksum/provenance
```

The RF Atlas scene compiler deterministically projects canonical vector/BIM geometry, openings, floor slabs, material identities, transmitter/receiver definitions, and tabulated antenna patterns into one or more Sionna/Mitsuba execution scenes. Sionna scene objects are disposable execution representations, never canonical project state.

For click-to-debug workflows, return the dominant paths and interaction/loss breakdown from the worker. For optimizer loops, prefer cached P1 coefficients for search and use P2 Sionna verification on shortlisted plans unless the problem size justifies batch Sionna coefficient generation.

### 8.6 Multipath combination

RF Atlas distinguishes planning-scale power fields from research-scale coherent fields:

- **Planning power mode:** consume per-transmitter path gain/RSS from Sionna or RF Atlas P1 and aggregate in linear power with explicit spatial/frequency averaging. This is the professional default.
- **Coherent field mode:** request/retain complex Sionna path coefficients or CIR/CFR artifacts. Use only for research and calibration because centimeter-scale geometry/frequency/phase errors can dominate fine fading.

The product never paints coherent small-scale fading as precise building truth when the geometry or material model cannot support it. Every derived layer records whether its propagation evidence came from RF Atlas P0/P1, Sionna P2/P3, or a hybrid calibrated model.

### 8.7 Fresnel and diffraction

P0/P1 may use a documented inexpensive Fresnel/edge-obstruction approximation only when it materially improves fast planning. P2/P3 diffraction/reflection/refraction should come from Sionna RT rather than a parallel in-house ray/path implementation.

Expose every Sionna interaction option and approximation in the job manifest, and validate it against canonical scenes plus measured holdouts. If a later field study shows a specific Sionna limitation, add the smallest independently testable RF Atlas correction or upstream contribution before considering a new solver.

### 8.8 Antenna orientation

Antenna gain lookup requires a defined coordinate convention. For every AP/client:

1. transform world direction into antenna-local coordinates;
2. evaluate gain at frequency, azimuth, and elevation;
3. account for polarization mismatch when modeled;
4. apply mount/ceiling/wall orientation;
5. record interpolation and missing-data behavior.

Provide a 3D viewer and canonical-axis validation tests to prevent the common 90°/axis-flip error.

### 8.9 Client uplink model

The planner must model client EIRP and antenna/body loss separately from AP downlink. A low-power handset or scanner can fail uplink while still hearing a high-power AP. The default coverage layer should offer:

- downlink RSSI/SNR;
- uplink RSSI/SNR at AP;
- bidirectional minimum;
- asymmetry margin.

### 8.10 PHY/PER model

Use per-client or generic curves mapping SINR and PHY configuration to packet error probability. Expected goodput should integrate over a distribution rather than choose a single rate with certainty.

Potential model inputs:

- standard and band;
- MCS, NSS, channel width, GI;
- RU allocation for OFDMA scenarios;
- coding/modulation;
- packet length;
- interference dynamics;
- retry limits;
- aggregation size;
- chipset-specific sensitivity.

Initial releases can use empirical lookup curves; later releases can consume lab measurements.

### 8.11 Wi-Fi 7 support

Model Wi-Fi 7 as more than a “be” label:

- 320 MHz channelization in permitted regions/bands;
- puncturing patterns and effective bandwidth;
- MLO with per-link frequency, power, coverage, and contention;
- STR/NSTR/eMLSR-like client constraints where profile data supports them;
- multi-RU/OFDMA capability as a scenario parameter;
- EHT MCS/NSS and frame capabilities;
- link-selection and traffic-splitting policies;
- MLD versus per-link identity in measurements.

Because implementation behavior varies by client/AP/controller, predictions must be profile/scenario based.

### 8.12 Capacity model

Capacity is an airtime allocation problem. For client `i` associated to radio `r`:

```text
required_airtime_i ≈ offered_goodput_i / effective_goodput_when_scheduled_i
```

Then add:

- management/beacon airtime;
- contention/coordination overhead;
- retries and rate variance;
- uplink/downlink split;
- protocol/application overhead;
- safety margin;
- neighboring BSS airtime.

A radio is feasible only if total planned airtime remains below a configured operating target, not merely below 100%.

Model traffic distributions and concurrency, not every registered device transmitting at peak simultaneously unless that is the scenario.

### 8.13 Client association model

Support policies:

- strongest eligible signal;
- controller-like band steering;
- capacity-aware association;
- sticky client with roam hysteresis;
- nearest AP;
- manually pinned populations;
- probabilistic association.

Capacity results depend on association, so association policy is part of the scenario and analysis hash.

### 8.14 Calibration from measured data

Calibration is RF Atlas-owned even when Sionna supplies differentiable propagation. Stages:

1. **Sensor bias check:** estimate adapter/source offsets using overlapping captures.
2. **Fast-model fit:** path-loss exponent/reference loss and empirical material corrections for P1.
3. **Sionna material fit:** where identifiable, fit constrained material conductivity/permittivity/thickness/scattering priors using Sionna differentiability or outer-loop optimization.
4. **AP parameter fit:** Tx power, orientation, or installation-loss corrections only with strong priors/evidence.
5. **Spatial residual model:** fit low-frequency residual field on top of physical predictions.
6. **Blocked hold-out validation:** score on excluded rooms/areas/sessions, not the fitting points.
7. **Posterior/scenario uncertainty:** preserve parameter covariance or bounded scenario ensembles.

Sionna’s demonstrated gradient-based material calibration is an enabling mechanism, not permission to fit everything. AP power, wall loss, antenna gain, path-loss exponent, and sensor bias can trade off. The calibration layer must detect weak identifiability, freeze parameters, enforce physical priors, retain the calibration dataset hash, and reject models that improve training error without held-out improvement.

### 8.15 Sionna RT worker boundary

Sionna RT is a **direct dependency of an isolated optional worker**, not a generic “maybe later” interchange engine. The desktop/core never imports Sionna Python objects.

#### Request contract

A versioned `PropagationRequest` references canonical immutable artifacts rather than embedding mutable UI state:

```text
request_id / schema_version
scene_geometry_hash
material_catalog/version + project overrides
transmitters and receiver grid/query set
frequency / bandwidth / antenna configuration
solver kind: paths | planar-radio-map | mesh-radio-map
LOS/reflection/refraction/diffraction/scattering flags
max depth / samples / convergence parameters
seed
backend/resource limits
requested outputs
```

#### Result contract

`PropagationResult` contains only engine-independent data plus an opaque raw-artifact reference:

```text
request/input hashes
Sionna/Mitsuba/Dr.Jit/Python/backend versions
completion/cancellation/failure state
per-transmitter path gain or RSS tiles
optional Paths/CIR/CFR/taps/Doppler artifacts
Monte Carlo/convergence diagnostics
worker logs/debug render references
checksums and timings
```

RF Atlas then computes Wi-Fi channel coupling, spectral overlap, measured activity, thermal/receiver assumptions, CCA/airtime, association, PHY/PER, capacity, requirements, and optimization outside Sionna. **Never display raw Sionna multi-transmitter SINR as final Wi-Fi SINR**, because Sionna does not know Wi-Fi channel separation, puncturing, CSMA/CA, traffic activity, association policy, or MLO behavior.

#### Execution modes

- local venv/packaged worker over local socket or stdio;
- local container for reproducibility/debugging;
- remote authenticated worker;
- optional Slurm/HPC backend for batch calibration or dense research jobs.

Every mode implements the same capability handshake, cancellation, artifact, and provenance contract.

### 8.16 GPU compute

There are two distinct GPU domains and they must not be conflated:

1. **RF Atlas `wgpu` compute** for P0/P1 rasterization, vector obstruction kernels, interpolation, tile algebra, candidate scoring, and Monte Carlo uncertainty where cross-platform GPU portability matters. Maintain CPU reference implementations and differential tolerances.
2. **Sionna/Mitsuba/Dr.Jit compute** inside `rfatlas-sionna-worker` for P2/P3 radio propagation. Backend/build versions are part of every result manifest; CUDA/LLVM/native dependency conflicts remain quarantined from the desktop.

Do not rewrite Sionna kernels in `wgpu` merely for language purity. Only migrate a high-fidelity operation if profiling and validation demonstrate a concrete product need that cannot be solved upstream or through the worker boundary.

### 8.17 Predictive cache

Cache keys include:

- scene geometry/material version;
- AP/radio model and placement;
- client profile;
- frequency/channel/width/power;
- solver/tier/version/configuration;
- grid/voxel resolution;
- random seed/scenario;
- calibration model.

Use tiled storage so moving one AP invalidates only dependent transmitter tiles/aggregations rather than every project artifact.

### 8.18 Prediction uncertainty

Provide at least:

- deterministic nominal map;
- conservative/optimistic bounds;
- parameter Monte Carlo percentiles;
- spatial residual uncertainty;
- distance-to-measured-calibration support;
- sensitivity ranking by material/AP/client parameter.

The planner can optimize against a selected percentile rather than nominal mean.

---

## 9. Automatic AP planner

### 9.1 Inputs

- Valid candidate areas/points and mounting constraints.
- AP/radio/antenna catalog.
- Existing APs and immutable infrastructure.
- Floor geometry and materials.
- Regulatory domain.
- Coverage zones and requirements.
- Client density/distribution profiles.
- Application demand and concurrency scenarios.
- Wired drops, switch/PoE/uplink constraints, cable cost.
- Equipment/license/install costs.
- Neighboring network evidence/scenarios.
- Resilience/failure requirements.
- Aesthetic/security/leakage constraints.
- Optimization budget/precision.

### 9.2 Decision variables

Illustrative discrete model:

```text
x[p,a]       = AP model a placed at candidate p
r[p,a,b]     = band/radio b enabled
c[p,a,b,k]   = channel option k selected
w[p,a,b,j]   = width option j selected
q[p,a,b,l]   = power level l selected
y[z,i,p,a,b] = client class i in demand cell z assigned to radio
```

Continuous refinements can optimize exact coordinates, height, azimuth, downtilt, or power after a discrete warm start.

### 9.3 Hard constraints

- At most one AP/model per candidate unless explicitly allowed.
- Channel, width, power, and AP-class legality by region.
- Mounting and exclusion geometry.
- Wired drop, cable-length, switch port, and PoE budgets.
- Radio capability compatibility.
- Client eligibility by band/standard/security.
- Minimum bidirectional coverage per required area.
- Required secondary/tertiary coverage or failure resilience.
- Radio airtime/client-count limits.
- AP/wired uplink aggregate limits.
- User-pinned APs/configurations.

### 9.4 Objectives

Use lexicographic or weighted multi-objective optimization with explicit policy. Candidate objectives:

1. minimize infeasible/failed requirement area;
2. minimize worst-case failure under uncertainty;
3. minimize equipment + cable + install cost;
4. minimize AP count;
5. minimize co-/adjacent-channel interference;
6. minimize excessive RF leakage;
7. minimize cable distance and installation complexity;
8. maximize capacity headroom;
9. maximize roam margin/redundancy;
10. minimize power and unnecessary 2.4 GHz radios.

Do not hide the objective ordering. Allow users to inspect tradeoffs.

### 9.5 Solver architecture

A practical hybrid:

```text
geometry/candidate generator
       |
       v
GPU/CPU propagation cache for candidate radios
       |
       v
surrogate coverage/capacity coefficients
       |
       v
CP-SAT discrete placement/channel/power model
       |
       v
client association / min-cost-flow evaluation
       |
       v
full nonlinear simulation and robustness checks
       |
       v
local repair/search
       |
       v
Pareto alternatives + explanations
```

Why hybrid:

- Full ray tracing inside every combinatorial solver node is too expensive.
- Pure greedy placement can miss joint channel/capacity tradeoffs.
- Pure integer linear coefficients can be inaccurate because interference and association are nonlinear.
- Iterative solve-evaluate-cut/repair gives tractable rigor.

### 9.6 Coverage coefficients

Precompute received-power distributions from each candidate radio to each evaluation cell. Interference and SINR remain scenario-dependent, but basic coverage and path loss are reusable.

### 9.7 Client assignment

For a fixed radio plan, assign demand to radios with a min-cost flow or constrained optimization where cost reflects airtime, signal margin, roaming policy, and load. Reject associations that fail client/security/band requirements.

Iterate between radio plan and assignment because load affects feasibility.

### 9.8 Channel planning

Model:

- primary/secondary channel spans;
- width and puncturing;
- co-channel reuse;
- adjacent-channel coupling;
- DFS constraints/events;
- external neighbor occupancy;
- per-floor spatial reuse;
- MLO link interactions;
- radio colocation/self-interference constraints if known.

A graph-coloring heuristic can warm-start CP-SAT; the final score uses spatially weighted interference rather than only graph edges.

### 9.9 Power planning

More power is not always better. Optimize bidirectional coverage, cell boundaries, roaming overlap, interference, and regulatory constraints. Include discrete hardware/controller power steps and antenna gain.

### 9.10 AP count and capacity

Support both:

- maximum clients per AP/radio/band;
- airtime-based capacity.

Client-count limits are operational safeguards, not a substitute for airtime modeling. TamoGraph 8.4's per-band maximum-client control is worth adopting, but RF Atlas should explain when airtime binds first.

### 9.11 Robust planning

Use scenario sets or sampled uncertainty:

- material loss percentiles;
- AP power/calibration error;
- client orientation/body loss;
- crowd density;
- neighboring utilization;
- failed AP/uplink;
- shifted furniture/doors;
- changed demand.

A robust plan passes requirements across a specified fraction or worst-case subset of scenarios. Show cost of robustness.

### 9.12 Explainability

For every recommended AP:

- areas/clients it uniquely serves;
- capacity contribution;
- redundancy contribution;
- constraints forcing the placement;
- what fails if removed;
- sensitivity to position/power/channel;
- cabling/cost impact.

For rejected user proposals, show the exact violations and smallest repair.

### 9.13 Incremental replanning

Support:

- preserve existing AP positions, replan channels/power;
- allow moving only selected APs;
- add minimum new APs;
- constrain changes to maintenance window;
- compare current, minimal-change, and greenfield plans;
- convert predicted plan to post-install validation checklist.

### 9.14 AP-on-a-stick planner integration

Use empirical candidate maps as high-confidence propagation coefficients. The optimizer can combine measured candidate placements into a hypothetical multi-AP layout while accounting for channel/interference/capacity. Warn that separate-time surveys may differ in occupancy/noise.

### 9.15 Planner acceptance tests

- Known small instances with provable optimum.
- Monotonicity checks: relaxing a constraint cannot worsen feasibility.
- Regulatory invalid configurations never emitted.
- Removing demand cannot require more capacity, barring secondary objectives that are explicitly different.
- Candidate permutations do not change deterministic result.
- Full evaluator verifies every returned plan.
- Infeasibility produces a minimal/near-minimal explanation set.

---

## 10. System architecture

### 10.1 Architectural style

Use a modular monolith for the desktop product plus isolated privileged collectors and optional remote services. Do not begin with microservices.

Core boundaries follow ports-and-adapters/hexagonal architecture:

```text
UI / CLI / API
      |
Application use cases and jobs
      |
Domain model and deterministic analysis contracts
      |
Ports
  ├── collectors
  ├── project store
  ├── geometry engine
  ├── compute backend
  ├── active test agents
  ├── report renderer
  └── plugin runtime
      |
Platform and third-party adapters
```

### 10.2 Recommended technology decisions

| Area | Recommendation | Reason |
|---|---|---|
| Core language | Rust | Memory safety, native performance, cross-platform, good systems/networking fit |
| Desktop shell | Tauri 2 | Small native shell, Rust backend, web UI, mobile/plugin pathway |
| UI | TypeScript + React | Mature complex-app ecosystem and testability |
| 2D map canvas | WebGL/WebGPU layer using MapLibre/deck.gl-like patterns or custom renderer | Large tiled rasters, vectors, paths, selection |
| 3D | Three.js initially; native `wgpu` compute | Fast product iteration plus shared GPU kernels |
| GPU compute | `wgpu` | D3D12/Metal/Vulkan/WebGPU portability |
| Metadata store | SQLite, optional SQLCipher | Portable project, transactions, migrations |
| Dense observations | Apache Arrow/Parquet chunks | Columnar scans and efficient analytics |
| Raw packets | PCAPNG sidecars/chunks | Interoperability, multi-interface metadata |
| Geospatial export | GeoPackage/GeoJSON/KML/KMZ | Open GIS interoperability |
| Remote protocol | QUIC with authenticated protobuf/Arrow frames | Multiplexing, reconnect, encryption, streaming |
| Plugin ABI | WASM Component Model/WIT for safe plugins; native plugins for hardware SDKs | Isolation and language interoperability |
| Server option | Rust service + Postgres/PostGIS/object storage | Multi-user/large deployments only |
| Build | Cargo workspace + pnpm | Reproducible monorepo |

These are defaults, not irreversible commitments; capture drivers and geometry libraries require prototypes before final ADRs.

### 10.3 Process model

Desktop installation can contain:

```text
rf-atlas-desktop              unprivileged UI/application
rf-atlas-capture-helper       privileged minimal native helper where required
rf-atlas-active-agent         optional LAN endpoint
rf-atlas-plugin-host          sandbox/process boundary for native plugins
rf-atlas-worker               RF Atlas background CPU/wgpu jobs
rfatlas-kismet-adapter        external Kismet normalization bridge
rfatlas-sionna-worker         optional Python/Mitsuba/Dr.Jit propagation worker
```

External process rules:

- **Kismet remains a separately executable GPL application.** RF Atlas consumes authenticated REST/WebSocket responses, read-only KismetDB, and PCAPNG through an adapter.
- **Sionna is adopted only in the Sionna worker environment.** Python/native dependencies cannot leak into the desktop process.
- **Deconflict has no runtime process by default.** A file/interchange bridge may import/export a documented neutral planning schema.
- **wifiheatmap never appears in production runtime.** It exists only as clean-room behavioral fixtures/oracles.

The native capture helper exposes a narrow authenticated local IPC API and cannot edit projects, render UI, access cloud credentials, or execute arbitrary plugins. Kismet and Sionna adapters similarly return normalized events/artifacts rather than direct access to canonical stores.

### 10.4 Component map

```text
┌──────────────────────────────── Desktop UI ───────────────────────────────┐
│ Inspector | Survey | Analyze | Plan | Compare | Report | Lab             │
└───────────────────────────────┬───────────────────────────────────────────┘
                                │ commands/events
┌───────────────────────────────v───────────────────────────────────────────┐
│ Application layer                                                        │
│ session coordinator | project service | job graph | undo/redo | exports  │
└──────────────┬────────────────┬────────────────┬──────────────────────────┘
               │                │                │
┌──────────────v──────┐ ┌───────v────────┐ ┌─────v─────────────────────────┐
│ Acquisition domain  │ │ Analysis domain │ │ Planning/prediction domain   │
│ sensors, samples,    │ │ metrics, fields,│ │ canonical scene, fast RF,    │
│ clocks, paths        │ │ uncertainty     │ │ Wi-Fi PHY/capacity/optimizer │
└──────────────┬──────┘ └───────┬────────┘ └──────────┬────────────────────┘
               │                │                     │
┌──────────────v────────────────v─────────────────────v────────────────────┐
│ Canonical stores: SQLite | Parquet | PCAPNG | media | immutable artifacts │
└──────────────┬────────────────────────────────────────────────────────────┘
               │ versioned ports / adapters; foreign objects stop here
      ┌────────┼──────────────────────┬──────────────────────┐
      │        │                      │                      │
      ▼        ▼                      ▼                      ▼
 Native    Kismet adapter        Sionna job client      Deconflict bridge
collectors REST/WS/KismetDB      request/result/artifact neutral plan I/O
Win/mac/   PCAPNG + sensors      local/remote worker    no runtime dep
Linux/     external GPL process  adopted Apache engine
mobile

Tests only: wifiheatmap clean-room TIN/point-survey behavior oracle
```

**Architectural invariant:** an adapter may know both the foreign schema and the RF Atlas schema; no other layer may know the foreign schema. This is enforced with crate/package dependency tests.

### 10.5 Domain boundaries

- **Project:** identity, migrations, assets, versions, collaboration.
- **Spatial:** coordinate frames, geometry, floor maps, pose.
- **Radio identity:** devices, radios, BSSIDs, ESSs, MLD links, grouping evidence.
- **Acquisition:** sensors, capabilities, sessions, channel schedules, raw events.
- **Active measurement:** agents, endpoints, probes, throughput sessions.
- **Spectrum:** analyzers, sweeps, bins, classification.
- **Analysis:** metric definitions, aggregation, interpolation, uncertainty, layers.
- **Prediction:** scenes, materials, AP/client models, solvers, calibration.
- **Optimization:** constraints, demand, candidates, solutions, explanations.
- **Requirements:** policy, evaluation, compliance.
- **Reporting:** report specification, rendering, manifest.

No UI component calculates RF metrics independently.

### 10.6 Command/query separation

Commands mutate project state through explicit use cases:

```text
StartSurvey
RecordPathAnchor
CompleteSurveyPoint
LinkBssids
SetApPosition
ImportFloorPlan
RunAnalysis
CreatePredictiveScenario
OptimizePlan
BuildReport
```

Queries read versioned views. Long calculations are jobs with immutable input manifests and cancellable progress.

### 10.7 Event and operation log

Store user/project operations separately from high-volume sensor data:

```text
operation_id
actor_id/device_id
logical timestamp
base version
operation type
payload
inverse/undo metadata
```

Benefits:

- undo/redo;
- offline merge;
- audit history;
- reproducible project state;
- conflict resolution.

Do not event-source every packet; high-volume observations use append-only chunks referenced by operations/manifests.

### 10.8 Analysis job graph

A generic job specification:

```json
{
  "job_type": "spatial_metric_layer",
  "job_version": 3,
  "inputs": {
    "survey_ids": ["..."],
    "identity_graph_version": "...",
    "geometry_version": "...",
    "metric_definition": "sinr/v2",
    "client_profile": "phone-2x2/v1"
  },
  "algorithm": {
    "name": "barrier_gp",
    "version": "0.4.1",
    "parameters": {"...": "..."},
    "seed": 42
  },
  "output_grid": {"resolution_m": 0.5, "area_mask": "occupied"}
}
```

Hash canonical job specs and immutable inputs for cache lookup.

### 10.9 Collector interface

Conceptual Rust API:

```rust
pub trait Collector: Send {
    fn descriptor(&self) -> CollectorDescriptor;
    fn capabilities(&self) -> CapabilitySet;
    fn configure(&mut self, config: CollectorConfig) -> Result<()>;
    fn start(&mut self, sink: EventSink) -> Result<SessionHandle>;
    fn set_channel_plan(&mut self, plan: ChannelPlan) -> Result<()>;
    fn health(&self) -> HealthSnapshot;
    fn stop(&mut self) -> Result<()>;
}
```

Events are typed and forward-compatible:

```text
ScanObservation
RawFrameMetadata
RawFrameChunkReference
InterfaceState
ChannelState
GPSFix
Pose
SpectrumSweep
ClockSync
HealthEvent
```

### 10.10 Platform collectors

#### Windows

- Native Wi-Fi APIs for nearby BSS/network data and associated-interface telemetry.
- Respect modern precise-location consent requirements.
- Npcap or a dedicated supported driver path for raw monitor/radiotap capture.
- Treat monitor capability as adapter/driver-specific; it can disconnect the managed interface.
- Separate adapter for hybrid surveys is the reliable model.

#### Linux

- `nl80211` for interface/scan/channel management.
- monitor interfaces and radiotap via packet capture.
- capability probing for simultaneous managed+monitor operation; do not assume it.
- best first-class platform for remote sensors and raw capture.

#### macOS

- CoreWLAN for available network scans and associated-interface values where authorized.
- Native capture/monitor behavior is OS/hardware constrained and may change.
- External supported adapters or a remote Linux sensor provide reliable professional passive capture.
- Never depend on undocumented command-line tools as the primary API.

#### Android

- `WifiManager` scan results subject to permissions and scan throttling.
- active/current-network metrics and Wi-Fi RTT where supported.
- ARCore pose/depth for survey path.
- use a foreground survey session and communicate capability/degraded scan cadence clearly.

#### iOS/iPadOS

- No general-purpose nearby Wi-Fi scanning API for ordinary apps.
- Use authorized current-network metadata, Network framework active tests, RoomPlan/ARKit pose/floor capture, and remote passive sensors.
- The iPhone companion can be an excellent active client/path tracker while a Linux/Windows/macOS collector records passive RF.

### 10.11 Mobile-desktop coordination

Example high-fidelity home workflow:

```text
iPhone
  - RoomPlan/ARKit path and room geometry
  - current connection metadata
  - gateway/LAN/Internet active probes
        |
        | local encrypted session
        v
Laptop or small Linux sensor
  - full passive channel hopping / monitor capture
  - optional spectrum analyzer
        |
        v
Desktop project
  - fuses pose and RF data using clock offset/uncertainty
```

### 10.12 Remote sensor protocol and Kismet-first deployment

The first professional Linux remote sensor should use **Kismet as the capture engine** rather than immediately reimplementing its hardware/datasource ecosystem. `rfatlas-kismet-adapter` owns the mapping.

#### Kismet integration surfaces

- authenticated REST queries for device/source/state snapshots;
- authenticated WebSocket/event streams where appropriate;
- read-only KismetDB import/tailing for durable historical evidence;
- PCAPNG for frame-level replay/reference;
- capture-source and channel-state telemetry;
- BLE/Zigbee/SDR/other Kismet-supported sources as optional evidence planes.

Kismet device aggregates are enrichment, **not survey samples**. Survey truth comes from time-resolved frame/source observations joined to RF Atlas pose, clock, channel-dwell, calibration, and session context. The adapter must preserve raw identifiers/fields needed for later reinterpretation and record Kismet build/schema/source versions.

#### Canonical normalized envelope

All Kismet/native sensor events become an `ObservationEnvelope` containing at minimum:

```text
observation_id / session_id / source_id
wall-clock and monotonic timestamp + uncertainty
pose/frame reference + covariance when applicable
radio/BSSID/MLD identity evidence
frequency/channel/width and capture dwell context
signal/noise/radiotap/source metadata
raw-frame or raw-record content hash/reference
parser/adapter version
quality/drop/throttle/disconnect flags
privacy/redaction state
```

#### Native RF Atlas remote protocol

A purpose-built native remote protocol may still be added when RF Atlas needs lower-overhead mobile pose fusion, non-Kismet sources, coordinator semantics, or an install footprint Kismet cannot provide. Its requirements remain:

- mutual authentication and encryption;
- capability/identity handshake;
- clock offset/uncertainty;
- sequence numbers/chunk hashes;
- local spool, backpressure, resume;
- metadata/raw retention policy;
- authorized remote configuration;
- health/storage/channel telemetry.

Do not implement a Kismet clone merely to achieve architectural purity.

### 10.13 Plugin system

Plugin categories:

- collector;
- spectrum device;
- frame/IE decoder;
- import/export;
- interpolation algorithm;
- propagation solver;
- AP/antenna catalog provider;
- requirement pack;
- report block;
- controller integration;
- recommendation rule.

WASM plugins get capability-scoped host APIs and no arbitrary filesystem/network access by default. Hardware SDK plugins may require native processes and stricter review/signing.

### 10.14 Controller integrations

Later adapters can import authorized data from UniFi, Aruba, Cisco, Ruckus, Mist, OpenWrt, hostapd, and others:

- AP positions/inventory;
- radio/channel/power configuration;
- client association/history;
- retries/utilization;
- neighbor reports;
- events and firmware.

Controller data is a separate evidence source with timestamps and trust level. Do not overwrite field observations.

### 10.15 Offline-first collaboration

- A project is fully usable as a local bundle.
- Project operations have stable IDs.
- Observation chunks are content-addressed.
- Merge combines operations and chunks; conflicts are explicit.
- Optional server coordinates locks/presence/jobs but is not required to open data.
- Large raw captures can remain external with verified references.

### 10.16 Third-party narrow-waist integrations

#### `rfatlas-kismet-adapter`

Responsibilities:

- authenticate and capability-negotiate with Kismet;
- map source UUID/channel/hopping/health state;
- ingest time-resolved packet/radiotap and selected device metadata;
- correlate Kismet timestamps with RF Atlas clock models;
- emit canonical `ObservationEnvelope` records;
- preserve a raw-field map or content-addressed source reference for forensic reinterpretation;
- surface drops, queue pressure, disconnects, channel dwell, and source limitations;
- support deterministic replay from KismetDB/PCAPNG fixtures.

It must **not** expose Kismet tracker elements or device objects to the domain layer, write canonical project records behind application commands, or make Kismet mandatory for basic surveying.

#### `rfatlas-sionna-worker`

Responsibilities:

- capability/version handshake;
- deterministic canonical-scene compilation validation;
- `PropagationRequest` execution;
- resource limits and cancellation;
- content-addressed cache/artifact store;
- local CPU/CUDA and optional remote/HPC execution;
- return engine-neutral radio-map/path results with full backend provenance;
- run upstream Sionna tests plus RF Atlas acceptance scenes in its build pipeline.

It must **not** perform Wi-Fi channel assignment, final SINR, PHY/goodput, requirements, association, capacity, or optimization policy.

#### Deconflict bridge

Maintain a neutral, versioned planning interchange schema for geometry/AP/radio/channel constraints that can be exported/imported without making either project depend on the other. Upstream contributions should target generic reproducibility, objective breakdowns, weighted conflict edges, field provenance, and deterministic seeds when maintainers agree.

#### wifiheatmap oracle

Keep synthetic fixtures that independently reproduce:

- point-position → observation association;
- “best selected BSSID” scalar semantics;
- Delaunay triangulation + barycentric interpolation;
- no silent extrapolation outside the convex hull;
- simple active throughput/retransmit capture behavior.

These are regression oracles, not production dependencies.

---

## 11. Project storage and schema

### 11.1 Project bundle

Suggested extension: `.rfatlas`.

Logical bundle:

```text
manifest.json
project.sqlite
observations/
  <content-hash>.parquet
captures/
  <content-hash>.pcapng.zst
spectrum/
  <content-hash>.parquet
assets/
  floorplans/
  photos/
  antennas/
cache/
  <analysis-hash>/...
reports/
  <report-id>/...
signatures/
```

A directory form is best during development; a ZIP-like packaged form can be used for transport. Cache and reports can be omitted and rebuilt.

### 11.2 Manifest

Contains:

- schema version;
- project ID/name;
- created/updated times;
- required feature versions;
- chunk hashes/sizes/media types;
- encryption metadata;
- external references;
- optional signatures;
- migration history.

### 11.3 Core entities

#### Spatial

- `Site`
- `Building`
- `Floor`
- `MapAsset`
- `CoordinateFrame`
- `CoordinateTransform`
- `GeometryLayer`
- `Material`
- `AreaMask`
- `Annotation`
- `Photo`

#### Infrastructure

- `PhysicalDevice`
- `Radio`
- `BssIdentity`
- `EssIdentity`
- `MldIdentity`
- `IdentityEdge`
- `ApInstallation`
- `Switch`
- `Port`
- `CablePath`
- `AntennaModel`
- `ApModel`

#### Measurement

- `SensorDevice`
- `Adapter`
- `AdapterCalibration`
- `CollectorCapability`
- `SurveySession`
- `SurveySegment`
- `SpatialSample`
- `PoseSample`
- `GpsFix`
- `ChannelSchedule`
- `FrameObservation`
- `ScanObservation`
- `SpectrumSweep`
- `ActiveEndpoint`
- `ActiveTestRun`
- `ActiveInterval`
- `RoamEvent`
- `ClockModel`

#### Analysis/planning

- `ClientProfile`
- `ApplicationProfile`
- `RequirementPolicy`
- `MetricDefinition`
- `AnalysisRun`
- `DerivedLayer`
- `PredictiveScenario`
- `CalibrationModel`
- `DemandScenario`
- `OptimizationRun`
- `PlanAlternative`
- `ComplianceResult`
- `ReportSpec`
- `ReportBuild`

### 11.4 Identity graph

Do not make BSSID the primary AP identity. Store evidence-weighted relations:

```text
node A --[same_physical_device, confidence=.96, evidence=multiple_bssid_ie]--> node B
node C --[same_mld, confidence=1.0, evidence=eht_mlo_element]--> node D
node E --[user_declared_colocated, confidence=1.0]--> device X
```

Analysis runs pin an identity graph version, ensuring later relinking does not silently change old reports.

### 11.5 Observation schema principles

Every observation includes:

- globally unique ID;
- sensor and adapter IDs;
- collector/driver/OS versions;
- monotonic and UTC timestamps;
- channel/frequency state;
- raw value and units;
- calibration reference;
- raw source reference if retained;
- parse/method version;
- quality flags;
- location association made later, not destructively embedded only once.

### 11.6 Spatial sample linkage

A raw observation can contribute fractionally/probabilistically to spatial samples when position is uncertain or when a continuous path is edited. Preserve the assignment version.

### 11.7 Derived layer schema

A layer contains:

```text
layer_id
metric_definition_id/version
analysis_run_id
spatial extent and CRS
resolution/tile scheme
value data
uncertainty data
support/distance-to-evidence data
provenance class per cell
valid/unknown mask
units
color/render suggestions (not semantics)
summary statistics
```

Values and uncertainty should remain numerical, not only rendered pixels.

### 11.8 Metric registry

A metric definition states:

- identifier/version;
- semantic description;
- units and valid range;
- required evidence/capabilities;
- aggregation method;
- spatial method;
- filters/grouping;
- uncertainty method;
- compatibility rules;
- visualization defaults;
- compliance direction (`higher_is_better`, `lower_is_better`, categorical).

### 11.9 Data migrations

- Forward-only schema migrations with backups.
- Never mutate raw chunk bytes during ordinary migrations; add normalized views or new chunks.
- Migration tests against fixture projects from every supported version.
- Human-readable migration report.
- Ability to open read-only if a feature is unknown/newer.

### 11.10 Encryption

- Optional whole-project encryption using a modern audited construction.
- Separate content keys per project; envelope encryption for team sharing.
- OS keychain integration.
- Raw packet and MAC-sensitive chunks can have stricter keys/retention than ordinary heatmaps.
- Export warns when de-pseudonymized identifiers are included.

### 11.11 Open export guarantees

Even if the primary bundle evolves, guarantee stable exports for:

- normalized observations in Parquet/Arrow and CSV subsets;
- geometry in GeoPackage/GeoJSON;
- raw captures in PCAPNG;
- analysis manifests and requirement policies in JSON/YAML;
- maps as numerical GeoTIFF/tiles plus visual PNG/SVG;
- antenna patterns in documented JSON.

---

## 12. Analysis and rendering engine

### 12.1 Tiled spatial computation

Use a tiled grid/pyramid for large sites:

- independent compute tiles with halo for interpolation;
- multi-resolution previews;
- CPU/GPU scheduling;
- content-addressed tile cache;
- viewport-priority jobs;
- deterministic merge at boundaries.

### 12.2 Numerical layer versus presentation layer

The numerical raster contains values, masks, support, and uncertainty. The renderer applies:

- color scale;
- threshold bands;
- contours;
- opacity;
- hill/3D extrusion;
- AP/path overlays;
- provenance hatching;
- labels/legend.

Changing colors must not trigger metric recomputation.

### 12.3 Color and accessibility

- Perceptually ordered scales for continuous metrics.
- Diverging scales for differences.
- Categorical scales for AP association/bands.
- Color-blind-safe defaults.
- Numeric hover and contours.
- User palettes for branding, but warn against misleading non-monotonic scales.
- Print/PDF preview.

### 12.4 2D and 3D views

2D is primary for field work. 3D supports:

- stacked floors;
- signal surfaces/extrusions;
- volumetric slices;
- AP antennas/orientation;
- propagation paths;
- spectrum versus time/frequency;
- building cutaways.

Do not use 3D merely for spectacle; every view needs an analytical question.

### 12.5 Layer algebra

Allow derived expressions with dimensional checking:

```text
min(downlink_rssi, uplink_rssi)
rank(ap_rssi, 2)
active_lan_throughput - predicted_goodput
survey_A - survey_B
compliance(policy_v3)
```

Custom expressions run in a safe, deterministic DSL and become versioned metric definitions.

### 12.6 Selection semantics

Selections are explicit dimensions:

- surveys/time windows;
- sensors/adapters;
- AP/ESS/radio/link identities;
- band/channel;
- client profile;
- metric method;
- floor/zone;
- evidence type.

The UI displays active selection chips in every map/report to prevent accidental interpretation of the wrong subset.

### 12.7 Unknown semantics

Distinguish:

- not measured;
- not supported by sensor;
- filtered out;
- below detection threshold;
- failed test;
- no association;
- invalid geometry;
- solver failure;
- outside evidence support.

These states are not numerically zero and have separate map patterns.

### 12.8 Recommendations engine

Start as deterministic rules over analysis facts. Rule output:

```text
hypothesis
supporting evidence
contradicting evidence
confidence
recommended next test
action candidates
expected effect and tradeoffs
```

An optional LLM can summarize but receives only structured facts and must cite metric/layer IDs in generated text.

---

## 13. User experience

### 13.1 Top-level navigation

```text
Home | Inspector | Survey | Analyze | Plan | Compare | Report | Lab
```

- **Home:** recent projects, sensors, active agents, guided tasks.
- **Inspector:** immediate nearby/current-network view without a project.
- **Survey:** map and field acquisition.
- **Analyze:** heatmaps, metrics, requirements, root cause.
- **Plan:** geometry, predictive APs, optimizer.
- **Compare:** snapshots/scenarios/time.
- **Report:** reproducible report builder.
- **Lab:** raw evidence, algorithms, calibration, plugin tools.

### 13.2 First-run home workflow

1. Launch Inspector immediately; do not force project creation.
2. Ask the practical goal: dead spot, AP placement, slow room, roaming, new deployment, interference, or full professional audit.
3. Run capability check and explain what the current device can/cannot measure.
4. Offer floor-plan import, quick draw, or mobile room scan.
5. Recommend survey topology: laptop only, second adapter, phone + sensor, or spectrum device.
6. Generate a route/checklist.
7. Show results as “What we observed / What it likely means / What to test or change.”

### 13.3 Capability honesty panel

Example:

```text
This Mac can:
✓ scan nearby 2.4/5/6 GHz networks
✓ run active tests on the connected network
✓ provide current RSSI/noise on supported interfaces
~ capture passive frames with limitations
✗ remain connected while hopping all channels with one radio

For full passive + active surveying:
Add a supported external adapter or pair a Linux sensor.
```

### 13.4 Survey HUD

Show only field-critical information:

- current position/path and tracking quality;
- target networks/APs;
- current band/channel sweep;
- point/segment completion rings;
- active endpoint health;
- adapter/sensor health;
- live signal/latency summary;
- pause/undo/mark-event/photo;
- voice/haptic cues.

Audio cues can announce “point complete,” “tracking degraded,” “target AP missing,” or “scan cycle incomplete.”

### 13.5 Heatmap interaction

Click any cell to open an evidence drawer:

- value and uncertainty;
- observed/interpolated/predicted class;
- nearby raw samples;
- selected AP/client profile;
- algorithm/version;
- dominant causes/paths;
- requirement status;
- alternate algorithms/models;
- suggested next measurement to reduce uncertainty.

### 13.6 Troubleshooting narrative

Keep three columns:

| Observed | Interpretation | Next action |
|---|---|---|
| Strong RSSI, high retries, high airtime | Coverage is adequate; contention/interference is more likely | Inspect channel occupancy/spectrum; test narrower channel or different channel |
| Weak bidirectional RSSI and low expected PHY | Coverage/client link budget issue | Reposition/add AP; verify wall model and client profile |
| LAN test healthy, Internet test slow | Wi-Fi likely not the bottleneck | Investigate router/WAN/ISP/destination |

Every row links to supporting maps/samples.

### 13.7 Planning UX

- Drag APs with instant preview.
- Toggle client profiles and demand scenarios.
- Show downlink/uplink/asymmetry.
- “Why here?” explanation for optimized APs.
- Lock existing infrastructure.
- Compare cheapest/balanced/resilient layouts.
- Convert selected plan to install checklist and validation survey.

### 13.8 Expert controls without clutter

Use progressive disclosure:

- default panel with practical settings;
- advanced panel with method/thresholds;
- Lab link with raw mathematical configuration.

Projects remember explicit choices; defaults are versioned and shown in manifests.

### 13.9 Keyboard and large-project UX

- Search/filter APs by SSID/BSSID/alias/vendor/channel.
- Named groups and saved selections.
- Command palette.
- Layer presets.
- Fast multi-select.
- Undo/redo.
- Background jobs with resumable progress.
- Autosave and crash recovery.
- Large AP counters and selection summaries.

### 13.10 Mobile UX

The phone companion focuses on:

- room/floor capture;
- path/pose;
- active tests/current connection;
- photos/notes;
- guided route;
- live pairing with passive sensor;
- quick results.

Do not squeeze the entire desktop prediction/optimizer UI onto a phone.

---

## 14. Monorepo and code organization

### 14.1 Repository layout

```text
rf-atlas/
├── apps/
│   ├── desktop/                  # Tauri + React desktop application
│   ├── cli/                      # Project/survey/analysis/conversion CLI
│   ├── sensor-linux/             # Headless RF Atlas native sensor (later/optional)
│   ├── active-agent/             # LAN/remote active measurement endpoint
│   ├── mobile-ios/               # Swift/SwiftUI RoomPlan/ARKit companion
│   ├── mobile-android/           # Kotlin/Compose ARCore/Wi-Fi companion
│   └── coordinator/              # Optional team/server deployment
├── crates/
│   ├── domain/                   # Pure canonical domain types/invariants
│   ├── application/              # Commands, queries, workflows, jobs
│   ├── project-store/            # SQLite/bundle/migrations/content store
│   ├── spatial/                  # Coordinates, transforms, vector geometry, grids
│   ├── radio-model/              # AP/client/antenna/identity canonical model
│   ├── ieee80211/                # Independent IE/frame parser + normalized semantics
│   ├── acquisition/              # Sessions, samples, schedules, quality gates
│   ├── collector-api/            # Stable collector capabilities/events
│   ├── collector-windows/        # Native WLAN adapter
│   ├── collector-npcap/          # Windows monitor adapter where viable
│   ├── collector-linux/          # Native nl80211/radiotap adapter/replay path
│   ├── collector-macos/          # CoreWLAN/BPF adapter
│   ├── adapter-kismet/            # Kismet REST/WS/KismetDB/PCAPNG normalization
│   ├── active-measurement/       # Probe/iperf3/test orchestration
│   ├── spectrum/                 # Spectrum model and analysis
│   ├── interpolation/            # IDW/TIN/RBF/kriging/GP implementations
│   ├── metrics/                  # Metric registry and deterministic transforms
│   ├── uncertainty/              # Error models and uncertainty propagation
│   ├── propagation/              # RF Atlas P0/P1 empirical models and contracts
│   ├── propagation-sionna-client/# Sionna worker request/result/artifact client
│   ├── phy-model/                # Channel coupling/SINR/PER/MCS/goodput
│   ├── capacity/                 # Airtime and client association
│   ├── optimizer/                # AP/channel/width/power optimization
│   ├── requirements/             # Policy evaluation
│   ├── recommendations/          # Transparent root-cause rules
│   ├── rendering/                # Tiled numerical/render interfaces
│   ├── reporting/                # Report AST and exporters
│   ├── plugin-host/              # WASM/native plugin host
│   ├── protocol/                 # Native remote/coordinator protocol
│   └── test-fixtures/            # Shared golden/synthetic datasets
├── workers/
│   └── sionna/                    # Python worker; pinned sionna-rt/Mitsuba/Dr.Jit
│       ├── rfatlas_sionna/
│       ├── requirements.lock
│       ├── tests/
│       └── container/
├── bridges/
│   └── deconflict/               # Neutral planning interchange only
├── packages/
│   ├── ui-components/
│   ├── map-renderer/
│   ├── report-components/
│   ├── schemas/                  # Generated TS/JSON schemas
│   └── docs-site/
├── plugins/
│   ├── examples/
│   ├── spectrum-soapysdr/
│   └── exporters/
├── catalogs/
│   ├── materials/                # RF Atlas identity/prior/provenance; Sionna projection
│   ├── ap-radios/
│   ├── client-profiles/
│   ├── regulatory/
│   ├── requirements/
│   └── antennas/                 # Open/tabulated canonical representation
├── schemas/
│   ├── project/
│   ├── events/
│   ├── observation-envelope/
│   ├── propagation/
│   ├── plan-interchange/
│   ├── plugin-wit/
│   └── report/
├── fixtures/
│   ├── synthetic-scenes/
│   ├── captures/
│   ├── kismet/
│   ├── sionna/
│   ├── wifiheatmap-oracle/        # Clean-room synthetic behavior fixtures
│   ├── survey-projects/
│   └── competitor-blackbox/      # Results/notes, never vendor binaries/data
├── docs/
│   ├── architecture/
│   ├── adr/
│   ├── algorithms/
│   ├── measurement-methods/
│   ├── security/
│   ├── legal-source-ledger/
│   ├── upstream-audits/
│   └── validation/
├── tools/
│   ├── calibration/
│   ├── data-generation/
│   ├── schema-check/
│   └── release/
├── Cargo.toml
├── pnpm-workspace.yaml
├── deny.toml
├── justfile
├── plan.md
└── README.md
```

### 14.2 Dependency rules

Enforce with architecture tests/lints:

```text
UI -> application -> canonical domain
adapters -> ports/domain
metrics -> domain/spatial, never UI
optimizer -> propagation/capacity contracts, never platform collectors
reporting -> immutable analysis results, never live mutable UI state

Kismet schema/classes -> adapter-kismet ONLY
Sionna/Python/Mitsuba/DrJit -> workers/sionna ONLY
Deconflict interchange types -> bridge schema ONLY
wifiheatmap behavior -> fixtures/tests ONLY
```

Forbidden:

- platform collector code imported by domain crates;
- UI code calculating signal/interference/compliance;
- report code querying raw mutable project state without a manifest;
- optimizer mutating project infrastructure directly;
- plugins accessing stores except through scoped host APIs;
- storing a Kismet device/tracker object, Sionna scene/material/device object, or Deconflict project object as canonical state;
- importing Kismet GPL implementation code into a permissively licensed RF Atlas core without a deliberate licensing ADR;
- Python/Sionna native libraries loaded in the desktop address space;
- production code depending on wifiheatmap.

Adapter packages are the **only bilingual layers** allowed to understand both an upstream schema and RF Atlas schema.

### 14.3 Pure core and side-effect shell

Keep formulas, grouping, aggregation, interpolation, requirement evaluation, and optimization inputs as pure/deterministic functions where possible. Isolate:

- clocks;
- randomness;
- OS APIs;
- network I/O;
- filesystem;
- GPU execution;
- database transactions.

Inject seeds/clocks and compare CPU/GPU/reference implementations.

### 14.4 Domain units

Use strong types for:

- dBm versus dB;
- hertz versus channel number;
- meters versus map pixels;
- UTC versus monotonic duration;
- probability versus percentage;
- EIRP versus conducted power versus PSD;
- latency versus throughput;
- azimuth/elevation coordinate conventions.

Avoid bare floating-point parameters at public boundaries.

### 14.5 Error model

Errors are typed:

- capability unavailable;
- permission/consent missing;
- adapter/driver failure;
- incomplete scan;
- active endpoint unreachable;
- invalid geometry;
- insufficient evidence;
- numerical failure;
- project corruption/migration;
- plugin failure;
- policy/configuration error.

User-facing errors include remediation and preserve partial data safely.

### 14.6 Feature flags

Compile-time flags only for platform/hardware dependencies. Product capability must be runtime-detected. Avoid a matrix of subtly different analysis engines by edition.

### 14.7 API stability

- Domain schemas versioned independently from UI.
- Plugin contracts use semantic version/capability negotiation.
- Remote protocol supports additive fields and explicit required features.
- CLI commands designed for automation and JSON output.
- Public library APIs begin only after internal contracts stabilize.

### 14.8 Documentation-as-code

Every metric/algorithm has a document containing:

- definition and units;
- required evidence;
- formula/algorithm;
- assumptions;
- uncertainty behavior;
- fixtures and validation;
- citations;
- version history.

Generate UI help and report methodology text from the same registry where practical.

---

## 15. Security, privacy, and safety

### 15.1 Threat model

Protect against:

- unauthorized access to projects containing network/location data;
- malicious raw frames or malformed floor/CAD/import files;
- compromised remote sensors;
- privileged capture-helper abuse;
- malicious plugins;
- project-bundle path traversal/decompression bombs;
- report injection;
- accidental packet payload retention;
- cloud/coordinator tenant leakage;
- active-test misuse against third-party targets;
- supply-chain compromise.

### 15.2 Least-privilege capture

- Privileged helper does only interface enumeration/configuration/capture.
- Authenticate local IPC with per-install credentials and OS permissions.
- Drop privileges after device open when possible.
- No arbitrary shell commands.
- Strict input/output schemas and rate limits.
- Separate helper updates/signing.
- Clear indicator whenever monitor/raw capture is active.

### 15.3 Data minimization

Defaults:

- retain management/frame metadata needed for analysis;
- discard payload bytes before disk;
- do not retain probe-request SSID history by default;
- pseudonymize third-party client MACs with a project-scoped keyed transform;
- preserve owned infrastructure identities only when user marks/imports them;
- aggregate client density where individual identities are unnecessary;
- configurable retention for raw observations.

### 15.4 Raw capture policy

Raw PCAPNG is opt-in, with:

- purpose notice;
- duration/size limit;
- capture filter;
- payload truncation option;
- encryption requirement recommendation;
- retention deadline;
- export warning;
- legal/organizational authorization attestation for enterprise use.

### 15.5 Active test safety

- Default endpoints are local gateway and user-launched agents.
- Internet tests use conservative rates and explicit opt-in.
- Prevent arbitrary high-rate floods.
- Require authenticated active agents.
- Rate limit, duration limit, and server-side authorization.
- Mark DSCP/QoS tests as potentially disruptive.
- No tests against targets the user is not authorized to measure.

### 15.6 Plugin sandbox

WASM plugins receive declared capabilities such as:

```text
read_normalized_observations
emit_derived_layer
read_project_geometry
network_to_allowlisted_domain
write_export_file
```

Native plugins run out of process with equivalent IPC policy. Signed/trusted status is visible; unsigned plugins require explicit enablement.

### 15.7 Project security

- Authenticated encryption.
- Content hashes and optional signatures.
- Secure temporary files.
- Zip-slip/path traversal checks.
- Resource quotas during import/decompression/rendering.
- Sanitized HTML/report output.
- Sensitive-field export preview.

### 15.8 Remote sensors

- Mutual TLS or equivalent authenticated QUIC.
- Per-sensor scoped credentials and revocation.
- Signed configuration commands.
- Replay protection and sequence validation.
- Local fail-safe if coordinator is lost.
- No remote arbitrary code execution/update without a separate signed mechanism.
- Health and tamper events.

### 15.9 Privacy UX

At project creation, select a profile:

- **Home private:** preserve owned AP identifiers, pseudonymize neighbors/clients.
- **Consultant:** client-owned identities, strict raw-retention controls.
- **Research:** explicit consent and configurable identifiers.
- **Public/outdoor:** aggressive third-party minimization.

Reports default to redacting unrelated neighboring SSIDs/BSSIDs and client identifiers.

### 15.10 Supply chain

Third-party integration policy:

- Pin and SBOM `sionna-rt`, Mitsuba, Dr.Jit, Python runtime, Kismet, and helper/container images.
- Record upstream version/build/backend in every imported observation batch or propagation result.
- Run upstream tests plus RF Atlas contract/differential fixtures before upgrades.
- Treat Kismet/wifiheatmap GPL boundaries as packaging/release-review items, not assumptions that a process boundary automatically resolves every licensing question.
- Never silently download a new Sionna/Kismet version into an existing reproducible project analysis; upgrades create a new engine capability/version.


- Lock dependencies.
- `cargo-deny`/license/advisory checks.
- SBOM for releases.
- Reproducible/signable builds where feasible.
- Signed desktop/sensor binaries and updates.
- Fuzz all packet/file parsers.
- Avoid linking GPL/proprietary components into permissively licensed core unless license strategy explicitly permits it; subprocess interoperability and separately distributed plugins may be required.

### 15.11 Responsible feature boundary

Explicitly exclude:

- deauthentication/disassociation injection;
- credential capture/cracking workflows;
- evil-twin automation;
- covert person tracking from randomized client signals;
- claims of human presence/location without appropriate sensors, consent, and validated uncertainty.

Passive RF inventory and client-density analytics must be framed as network diagnostics, not surveillance.

---

## 16. Validation and test strategy

### 16.1 Test pyramid

1. Pure unit tests for formulas, units, parsers, geometry, policy.
2. Property tests for invariants and transformations.
3. Golden tests for frames, survey fixtures, maps, and reports.
4. CPU/GPU differential tests.
5. Collector contract tests with recorded platform events.
6. Hardware-in-loop tests.
7. Controlled RF lab tests.
8. Repeatable field trials.
9. Cross-product black-box comparison.
10. Long-running reliability and corruption-recovery tests.

### 16.2 Frame/parser testing

- Valid and malformed information elements.
- Truncation, duplicate/conflicting fields, unusual ordering.
- Multiple BSSID and MLO fixtures.
- HT/VHT/HE/EHT capability combinations.
- 2.4/5/6 GHz channels, widths, puncturing.
- Security/RSN/PMF combinations.
- Fuzz with coverage-guided tools.
- Differential checks against Wireshark/TShark for overlapping decoded fields.

### 16.3 Geometry tests

- Calibration transform round trips.
- Multi-floor alignment.
- Wall/path intersection edge cases.
- Holes/atria/doors.
- Coordinate unit conversions.
- Antenna rotation conventions.
- Invalid/self-intersecting import.
- CRS reprojection fixtures.

### 16.4 Interpolation tests

Synthetic fields with known truth:

- radial decay;
- abrupt wall boundary;
- corridor propagation;
- multi-floor source;
- sparse/noisy samples;
- clustered samples;
- location uncertainty.

Validate error and uncertainty calibration under blocked spatial holdout.

### 16.5 Propagation canonical scenes

- Free-space line of sight.
- One wall with known loss.
- Multiple parallel walls.
- Floor slab and opening.
- Single reflection.
- Knife-edge diffraction.
- Directional antenna rotation.
- Uplink/downlink asymmetry.
- Two APs/interference.
- 6 GHz power/PSD/channel-width cases.

CPU reference output becomes a golden fixture; advanced solver comparisons include tolerances.

### 16.6 RF laboratory setup

Progressive lab options:

1. Fixed AP/client geometry in a controlled room.
2. Shield boxes and programmable attenuators for sensitivity/rate curves.
3. Conducted RF paths where hardware permits.
4. Calibrated spectrum analyzer/reference receiver.
5. Anechoic/semi-anechoic access through a partner/university if available.
6. Rotating platform for antenna/device orientation.

Record AP firmware, power, channel, width, traffic, temperature, adapters, drivers, and environment.

### 16.7 Adapter calibration validation

- Repeat same geometry across adapters.
- Cross-band/channel tests.
- Cold/warm/long-run drift.
- Driver and OS version changes.
- Orientation/body effects.
- Detect saturation near AP and weak-signal floor.
- Produce a residual/error report, not merely one offset.

### 16.8 Active measurement tests

- Controlled Ethernet reference with known bottlenecks.
- Introduce RF attenuation, contention, WAN shaping, packet loss, latency, DNS delay independently.
- Verify root-cause attribution identifies the correct layer.
- Compare iperf3 JSON summaries with captured counters.
- Validate UDP loss/jitter and load-induced latency.
- Test roaming with repeatable movement/attenuation.

### 16.9 Survey repeatability protocol

Repeat the same route:

- same operator/device/time;
- different operator;
- different device/adapter;
- different time/occupancy;
- reverse direction;
- point versus continuous;
- AR pose versus manual path.

Quantify spatial bias, variance, and confidence. Use findings to set recommended sample spacing and quality gates.

### 16.10 AP localization validation

- Ground-truth AP positions/height.
- Varied survey geometries and floors.
- Hidden/limited path coverage.
- Unknown Tx power.
- Directional antennas.
- Compare solvers and probability-region calibration.
- Ensure user-known/imported position supersedes inference.

### 16.11 Optimizer validation

- Small exact instances.
- Synthetic buildings with known symmetries.
- Constraint mutation/metamorphic tests.
- Infeasibility explanation tests.
- Robustness scenario validation.
- Full nonlinear rescore.
- Compare greedy, CP-SAT, local search, and hybrid results.
- Ensure repeated deterministic runs match.

### 16.12 Cross-product black-box harness

Use legitimate versions of NetSpot, Acrylic, and TamoGraph on a controlled test site. The purpose is outcome comparison and regression detection, not format/code extraction.

Test matrix:

- same floor plan/scale;
- same AP and adapter when supported;
- same point positions;
- fixed AP power/channel;
- simple open room, one-wall, corridor, multi-floor;
- passive signal map;
- active throughput/RTT/loss;
- AP auto-location;
- interpolation between sparse points;
- extrapolation boundary;
- SIR/overlap behavior;
- expected PHY;
- AP-on-a-stick;
- predictive wall loss;
- automatic placement.

Record screenshots/numeric exports where licenses allow, exact settings, and observations. Mark results `behavioral`, not implementation facts.


### 16.13 Open-source integration runtime gates

The source audit is static. Production adoption/integration requires runtime evidence.

#### Kismet gate

- Run pinned Kismet against representative Linux radios.
- Verify local and remote capture helper lifecycle, channel hopping/locking, source errors/retries, queue/drop telemetry, and restart behavior.
- Compare REST/WebSocket, KismetDB, and PCAPNG records against the same raw capture.
- Verify timestamp/channel/source identity preservation and adapter mapping.
- Confirm the adapter uses time-resolved evidence rather than treating Kismet device aggregates as survey samples.
- Test version mismatch, schema additions, disconnect, partial DB, corrupt frame, and replay idempotence.

#### Sionna gate

- Build/run pinned worker on CPU and available CUDA hardware.
- Run upstream tests and RF Atlas canonical indoor scenes.
- Verify deterministic tolerance for fixed seed/backend and characterize Monte Carlo variance.
- Measure convergence versus samples/depth/features and define production defaults.
- Test scene compilation, material/antenna transforms, multi-floor geometry, cancellation, timeout, OOM/crash recovery, cache correctness, and artifact checksums.
- Compare P2 results against measured holdouts and P1 baseline before claiming superiority.
- Verify final Wi-Fi SINR/capacity changes when RF Atlas channel/activity logic is applied instead of Sionna's generic multi-transmitter SINR.

#### Deconflict gate

- Preserve numerical fixtures for channel coloring, heuristic interference, fast propagation, and placement as comparison baselines.
- Test neutral interchange round trips independently of Deconflict runtime.
- Do not gate RF Atlas releases on upstream contribution acceptance.

#### wifiheatmap gate

- Recreate synthetic point-survey and TIN/convex-hull fixtures independently.
- Verify RF Atlas can intentionally match the oracle baseline, then demonstrate/measure where barrier-aware or uncertainty-aware methods improve it.
- Keep the original GPL code outside production build/runtime paths.

### 16.14 Acceptance targets

Initial targets should be established empirically, then versioned. Candidate gates:

- No raw observation loss on clean shutdown or crash-recovery fixture.
- Deterministic analysis hashes and numerical results within defined CPU/GPU tolerances.
- Point-survey channel completeness accurately represented.
- No unsupported metric displayed as measured.
- Spatial transforms round-trip within map-calibration tolerance.
- Interpolation uncertainty is statistically calibrated on synthetic and field holdouts.
- Active attribution correctly identifies independently injected RF/LAN/WAN faults in the controlled matrix.
- Planner outputs satisfy all hard constraints under full evaluator.
- Reports contain complete evidence/algorithm manifest.

Do not set impressive-looking RF accuracy numbers until the lab has established defensible baselines.

### 16.15 Performance tests

- 10, 100, 1,000, and 10,000 AP observations.
- Multi-hour/multi-day sensor captures.
- Multi-floor sites from small home to stadium/warehouse scale.
- High-resolution tiled maps.
- GPU/CPU fallback.
- Low-memory laptop behavior.
- Project open/migration/recovery.
- Remote sensor disconnect/replay/backpressure.

### 16.16 Reliability tests

- Power loss during capture/write.
- Disk full.
- Adapter unplug/replug.
- Driver reset.
- sleep/wake.
- clock jump.
- network loss.
- sensor duplicate/reordered chunks.
- corrupt bundle/chunk.
- plugin crash/hang.
- GPU device loss.

Partial captures must remain usable and clearly marked incomplete.

---

## 17. Delivery roadmap

The sequencing is designed to prove the hardest scientific and platform assumptions early while delivering a useful tool before the full predictive planner exists.

### Phase 0 — Research harness and architecture proof

**Goal:** establish evidence contracts, platform capabilities, and project persistence before building polished UI.

Deliverables:

- Source/license ledger and clean-room policy.
- Rust workspace and architecture tests.
- Capability schema and collector event schema.
- Windows native scan prototype.
- Kismet integration spike on Linux: capture helper, channel control, REST/WebSocket, KismetDB, PCAPNG, source/drop telemetry.
- Independent Linux nl80211/radiotap replay/parser prototype for non-Kismet operation and differential testing.
- Sionna RT worker spike: pinned CPU/CUDA environment, capability handshake, one canonical scene, radio-map/path result round trip.
- Deconflict neutral interchange schema proof and wifiheatmap clean-room TIN fixture.
- macOS CoreWLAN prototype.
- Android/iOS capability spikes.
- Floor-plan import/calibration prototype.
- Immutable observation storage in SQLite + Parquet.
- PCAPNG import/reference parser path.
- Basic active endpoint and gateway/LAN probes.
- Synthetic survey generator.
- Minimal numerical heatmap renderer.

Exit criteria:

- One project can store a map, calibrated coordinate system, sensors, scan observations, points, and an analysis manifest.
- Same fixture produces deterministic output on two platforms.
- Capability matrix is based on running probes, not assumptions.
- Raw data can be exported without the UI.

### Phase 1 — Useful vertical slice: home dead-spot mapper

**Goal:** outperform free consumer analyzers for the user's original need.

Deliverables:

- Inspector table, signal timeline, and channel views.
- Project/map import and two-point calibration.
- Point and manual continuous surveys.
- Nearby RSSI and current-network active tests.
- Separate gateway, LAN-agent, and Internet latency/throughput.
- RSSI, AP count, band, channel, gateway RTT, LAN throughput, Internet throughput, loss, and data-quality maps.
- Barrier-aware IDW plus nearest-neighbor debug mode.
- Basic snapshots and difference maps.
- Guided “find dead spots / place router” report.
- Windows + macOS desktop; Linux sensor pairing.

Exit criteria:

- A user can launch, import/draw a home plan, walk a route, locate a dead spot, and distinguish weak Wi-Fi from slow Internet.
- Every map cell exposes source samples and whether it is observed/interpolated/extrapolated.
- Survey can be repeated before/after an AP move.

### Phase 2 — Professional passive/active survey

**Goal:** match the core measured-survey strengths of NetSpot Pro, Acrylic, and TamoGraph.

Deliverables:

- Kismet-backed Linux monitor/remote capture as the preferred first professional passive path; supported Windows monitor capture remains a native/helper path.
- Independent RF Atlas 802.11 parser/normalizer and PCAPNG replay so Kismet is enrichment/integration rather than semantic authority.
- Full channel scheduler and completeness metrics.
- Information-element parser/explorer.
- Identity graph for multi-BSSID/physical radios/MLO.
- Noise/SNR where available; SIR/SINR; channel overlap/coupling.
- Retry, airtime, client-density, frame/width, expected-PHY maps.
- iPerf3 integration and active profiles.
- Roam events/maps.
- Dual-adapter hybrid survey.
- Requirements profiles/compliance.
- AP auto-location with uncertainty.
- AP-on-a-stick.
- Survey merge/team offline workflow.
- Reproducible reports and raw exports.

Exit criteria:

- Professional survey can be completed with a supported dual-adapter setup.
- Every metric passes documented fixture/lab tests.
- Requirements report distinguishes unknown from pass.
- Cross-product black-box benchmark is documented.

### Phase 3 — Predictive planning foundation

**Goal:** match core TamoGraph/NetSpot predictive workflows with transparent methods.

Deliverables:

- Floor-plan geometry editor.
- Multi-floor building model.
- Versioned material library.
- AP/radio/client/antenna models.
- Instant and fast empirical propagation.
- Bidirectional link budget.
- Expected PHY/goodput.
- Measured-versus-predicted comparison.
- Global/site calibration and residual maps.
- GPU raster compute for P0/P1.
- Productionized Sionna RT P2 worker, deterministic scene compiler, path-gain/RSS import, and canonical-scene validation.
- Manual scenario comparison and reports.

Exit criteria:

- User can model a multi-floor deployment and immediately preview changes.
- Canonical scenes and field holdouts meet established validation baselines.
- Prediction uncertainty and assumptions are visible.

### Phase 4 — Automatic planner

**Goal:** exceed TamoGraph's planner through transparent, robust multi-objective optimization.

Deliverables:

- Candidate generator and infrastructure constraints.
- Coverage and capacity demand models.
- CP-SAT placement/channel/width/power solver.
- Client-assignment/min-cost-flow evaluation.
- Robust/failure scenarios.
- Existing-network minimal-change planning.
- Pareto alternatives and explanations.
- AP-on-a-stick empirical candidate integration.
- Install bill of materials, cable plan, and validation route.

Exit criteria:

- All hard constraints verified by full evaluator.
- Small instances match known optimum.
- Returned plans explain binding constraints and removal impact.
- Field validation closes the predicted-versus-measured loop.

### Phase 5 — Spectrum and distributed observatory

**Goal:** fuse protocol and raw RF evidence, and support persistent sites.

Deliverables:

- SoapySDR spectrum plugin.
- Vendor spectrum adapters where SDK/licensing permits.
- Current/average/max/waterfall/occupancy views.
- Position-correlated spectrum maps.
- Deterministic interferer signatures and event capture.
- Remote Linux sensor hardened deployment, using Kismet-backed capture where it provides the best hardware/source support.
- Live multi-sensor coordinator.
- Temporal baselines, change detection, and alerts.

Exit criteria:

- Spectrum calibration and sweep metadata are complete.
- A non-Wi-Fi energy event can be traced from temporal alert to waterfall to surveyed location.
- Sensor disconnect/replay works without duplicate/corrupt data.

### Phase 6 — Mobile spatial intelligence

**Goal:** remove manual path friction and enable phone-as-client measurement.

Deliverables:

- iOS RoomPlan/ARKit companion.
- Android ARCore/RTT companion.
- AR path anchors and drift correction.
- Phone active-client profiles/calibration.
- Live fusion with remote passive sensor.
- Guided route, photos, voice notes, haptics.
- Multi-device synchronized surveys.

Exit criteria:

- Pose uncertainty is measured and represented.
- AR path materially improves placement versus uniform-speed interpolation in validation routes.
- iOS limitations are handled through pairing rather than misleading nearby-scan claims.

### Phase 7 — Detailed propagation and digital twin

**Goal:** build the ambitious research-grade layer around an adopted high-fidelity engine rather than maintaining a second ray tracer.

Deliverables:

- Sionna RT P2/P3 path/radio-map worker hardened for local CPU/CUDA and remote/HPC jobs.
- Advanced canonical-to-Sionna scene compilation, antenna/material adapters, and path debugging.
- Inverse material/power/bias calibration with identifiability controls and spatial holdouts.
- Dynamic occupancy/material-state scenarios.
- Model-aided GP/residual fields.
- What-if timeline and configuration imports.
- Multi-AP/controller telemetry fusion.
- Automated post-change validation.
- Upstream Sionna contributions for generic tabulated antenna/material import or other engine-generic gaps where accepted.

Exit criteria:

- Sionna-backed P2 beats P1 on held-out field datasets for defined environments without overfitting; where it does not, the product reports that honestly.
- Path debug view explains meaningful improvements/errors.
- Convergence and backend variance are characterized.
- Uncertainty is calibrated and grows outside measured support.
- No high-fidelity result bypasses RF Atlas Wi-Fi channel/PHY/MAC/capacity semantics.

### Phase 8 — Ecosystem and adjacent radios

**Goal:** make RF Atlas a platform.

Deliverables:

- Stable plugin SDK.
- Public project/metric schemas.
- Controller integrations.
- BLE/Zigbee/Thread modules, initially integrating Kismet-supported evidence sources before building custom capture stacks.
- ROS 2 robotic surveys.
- Optional hosted collaboration.
- Catalog contribution/review system.

Exit criteria:

- Third parties can add a collector, metric, or export without modifying core.
- Plugins are sandboxed/versioned and projects remain portable.

---

## 18. Prioritized engineering backlog

Priority meanings:

- **P0:** architectural or scientific foundation; build early.
- **P1:** required for a credible professional measured-survey product.
- **P2:** predictive/planner differentiation.
- **P3:** advanced/distributed/ecosystem.

### 18.1 Foundations

| ID | Pri | Work item | Acceptance evidence |
|---|---:|---|---|
| FND-001 | P0 | Define domain units and IDs | Compile-time distinction for dB/dBm/Hz/m/pixels/time |
| FND-002 | P0 | Capability schema | Recorded fixtures for each platform/collector |
| FND-003 | P0 | Observation event schema | Forward/backward compatibility tests |
| FND-004 | P0 | Project bundle manifest | Round-trip and corruption tests |
| FND-005 | P0 | SQLite schema/migrations | Upgrade fixtures from every schema version |
| FND-006 | P0 | Parquet chunk writer/reader | Crash-safe append/finalization and hash checks |
| FND-007 | P0 | Content-addressed asset store | Duplicate suppression and integrity verification |
| FND-008 | P0 | Analysis manifest/hash | Deterministic canonicalization tests |
| FND-009 | P0 | Coordinate-frame graph | Calibration and transform property tests |
| FND-010 | P0 | Metric registry | One source for UI help, units, and compute contract |
| FND-011 | P0 | Operation log/undo model | Merge and inverse-operation fixtures |
| FND-012 | P0 | Source/license ledger | CI rejects catalog entries without provenance/license |

### 18.2 Collectors and capture

| ID | Pri | Work item | Acceptance evidence |
|---|---:|---|---|
| COL-001 | P0 | Windows native nearby-BSS collector | SSID/BSSID/channel/RSSI/IE fixture and consent UX |
| COL-002 | P0 | Linux nl80211 scan collector | Reproducible scan events and capability probe |
| COL-003 | P0 | Kismet Linux monitor/remote adapter | Live + KismetDB/PCAPNG fixture parity; timestamps/channel/source/drop facts preserved |
| COL-004 | P0 | macOS CoreWLAN collector | Supported field/capability matrix by OS version |
| COL-005 | P1 | Npcap monitor collector | Supported hardware fixture; graceful unsupported state |
| COL-006 | P1 | Adapter hotplug/recovery | Unplug/replug without losing finalized data |
| COL-007 | P1 | Channel scheduler | Logged schedule, completeness, weighted modes |
| COL-008 | P1 | Privileged helper IPC | Threat-model review and permission tests |
| COL-009 | P1 | Multi-adapter coordinator | Dedicated band and active/passive topologies |
| COL-010 | P1 | Android scanner/current-link collector | Throttling-aware quality and permission state |
| COL-011 | P1 | iOS current-link/active collector | No unsupported nearby scan claims |
| COL-012 | P1 | Kismet remote Linux sensor deployment | Remote capture reconnect/source/channel/drop telemetry and normalized replay |
| COL-013 | P3 | Controller telemetry adapters | Evidence remains separate from field observations |
| COL-014 | P2 | Native Linux raw monitor collector fallback | Implement only if Kismet gate shows a survey-critical gap; differential parity fixtures required |

### 18.3 802.11 semantics and identity

| ID | Pri | Work item | Acceptance evidence |
|---|---:|---|---|
| WIFI-001 | P0 | Beacon/probe IE parser | Fuzzed and differential fixtures |
| WIFI-002 | P0 | Channel/frequency/width model | 2.4/5/6 and regional fixtures |
| WIFI-003 | P1 | RSN/security/PMF parser | Transition and malformed configurations |
| WIFI-004 | P1 | HT/VHT/HE capability parser | Golden capability summaries |
| WIFI-005 | P1 | EHT/Wi-Fi 7 capability parser | 320 MHz/puncturing/MLO fixtures |
| WIFI-006 | P1 | Multiple BSSID identity linking | Physical radio count test |
| WIFI-007 | P1 | MLO/MLD identity graph | Link-to-MLD grouping fixtures |
| WIFI-008 | P1 | User/controller identity overrides | Versioned precedence and undo |
| WIFI-009 | P1 | OUI catalog update pipeline | Signed/versioned source and offline database |
| WIFI-010 | P2 | 802.11k/v/r consistency analysis | ESS-wide capability report |
| WIFI-011 | P2 | Management overhead estimator | SSID/beacon interval scenario tests |

### 18.4 Maps and spatial acquisition

| ID | Pri | Work item | Acceptance evidence |
|---|---:|---|---|
| MAPB-001 | P0 | Raster/PDF/SVG plan import | Unit/scale and malicious-file fixtures |
| MAPB-002 | P0 | Two-point calibration | Transform residual/round-trip test |
| MAPB-003 | P0 | Point survey state machine | Completion gates and cancellation recovery |
| MAPB-004 | P0 | Manual continuous paths | Timestamp assignment and editable anchors |
| MAPB-005 | P0 | Survey HUD | Field usability test and no hidden incompleteness |
| MAPB-006 | P1 | Route guidance/coverage holes | Generated route closes selected spacing gaps |
| MAPB-007 | P1 | GPS/NMEA | Accuracy/fix-state and replay tests |
| MAPB-008 | P1 | Multi-floor frames/alignment | Propagation-ready 3D transforms |
| MAPB-009 | P1 | Geometry editor | Valid topology, undo/redo, layers |
| MAPB-010 | P1 | Photos/notes/voice annotations | Positioned asset round trip |
| MAPB-011 | P2 | AP-on-a-stick workflow | Split candidate identity and combined scenario |
| MAPB-012 | P2 | iOS RoomPlan/ARKit import | Alignment and confidence retention |
| MAPB-013 | P2 | Android ARCore pose | Drift/anchor quality validation |
| MAPB-014 | P3 | ROS 2 pose/route adapter | Repeatable autonomous fixture |

### 18.5 Active measurement

| ID | Pri | Work item | Acceptance evidence |
|---|---:|---|---|
| ACTB-001 | P0 | Endpoint topology model | Gateway/LAN/Internet distinctions in schema/UI |
| ACTB-002 | P0 | ICMP/UDP/TCP lightweight probes | Loss/RTT distributions and timeout semantics |
| ACTB-003 | P0 | Authenticated LAN active agent | Rate limits and mutual auth |
| ACTB-004 | P0 | iPerf3 orchestration/JSON ingestion | Upload/download/UDP fixtures |
| ACTB-005 | P1 | Active point profile | Reproducible test sequence and impact metadata |
| ACTB-006 | P1 | Continuous lightweight profile | Spatially bounded measurement intervals |
| ACTB-007 | P1 | DNS/TCP/TLS/HTTP probes | Controlled fault injection |
| ACTB-008 | P1 | Roam event detector | Repeatable AP-transition lab test |
| ACTB-009 | P1 | Jitter/burst-loss analysis | RFC-aligned method metadata |
| ACTB-010 | P2 | Bufferbloat-under-load test | Baseline/loaded attribution fixture |
| ACTB-011 | P2 | DSCP/WMM validation profiles | Marking/queue test in controlled WLAN |
| ACTB-012 | P2 | Multi-client coordinator | Fairness and aggregate/per-client output |
| ACTB-013 | P3 | STAMP/TWAMP agent | Clock/one-way uncertainty represented |

### 18.6 Analysis and heatmaps

| ID | Pri | Work item | Acceptance evidence |
|---|---:|---|---|
| ANA-001 | P0 | Numerical tiled layer format | Value/mask/support/uncertainty separation |
| ANA-002 | P0 | Nearest + IDW interpolation | Synthetic field baseline |
| ANA-003 | P0 | Barrier-aware IDW | Wall/corridor fixture improvement |
| ANA-004 | P0 | Sample density/support maps | No extrapolated cell looks measured |
| ANA-005 | P0 | RSSI/AP count/band/channel maps | Exact point values trace to observations |
| ANA-006 | P0 | Active RTT/loss/throughput maps | Endpoint dimension cannot be omitted |
| ANA-007 | P1 | SNR/noise semantics | Unknown when sensor lacks noise |
| ANA-008 | P1 | SIR/SINR/interference coupling | Linear-power and channel-coupling tests |
| ANA-009 | P1 | Retry/airtime/client maps | Capture-loss/method caveats included |
| ANA-010 | P1 | Ranked AP/redundancy maps | Physical-radio dedup and rank tests |
| ANA-011 | P1 | Expected PHY/goodput | Client-profile and bidirectional tests |
| ANA-012 | P1 | Roaming/association maps | Time/path-linked events |
| ANA-013 | P1 | Difference maps | Uncertainty-aware significance |
| ANA-014 | P1 | Requirement engine | Pass/fail/unknown and area denominator tests |
| ANA-015 | P1 | Root-cause rules | Controlled RF/LAN/WAN fault matrix |
| ANA-016 | P2 | Kriging/GP interpolation | Blocked spatial CV and interval calibration |
| ANA-017 | P2 | Model-prior residual interpolation | Held-out improvement without overfit |
| ANA-018 | P2 | AP localization solvers | Ground-truth confidence-region tests |
| ANA-019 | P3 | Temporal anomaly engine | Event links and false-positive controls |

### 18.7 Predictive model

| ID | Pri | Work item | Acceptance evidence |
|---|---:|---|---|
| PREB-001 | P1 | Material/AP/client/antenna schemas | Version/source/uncertainty required |
| PREB-002 | P1 | Fast direct/multi-wall solver | Canonical geometry fixtures |
| PREB-003 | P1 | Multi-floor direct solver | Slab/opening fixtures |
| PREB-004 | P1 | Antenna transform/gain engine | Axis/rotation golden tests |
| PREB-005 | P1 | Bidirectional link budget | Low-power-client asymmetry case |
| PREB-006 | P1 | PHY/PER/goodput engine | Lab/generic curve fixtures |
| PREB-007 | P2 | GPU solver path | CPU differential tolerance |
| PREB-008 | P2 | Measurement calibration | Identifiability checks and holdout report |
| PREB-009 | P2 | Prediction uncertainty | Scenario percentiles and support map |
| PREB-010 | P2 | Sionna PathSolver integration | LOS/reflection/refraction/diffraction canonical scenes + path artifact contract |
| PREB-011 | P2 | Sionna RadioMapSolver integration | Per-transmitter path-gain/RSS tiles, convergence and measured holdouts |
| PREB-012 | P2 | Wi-Fi 7/MLO scenario model | Link policy/puncturing tests |
| PREB-013 | P2 | Sionna worker productionization | Pinned env, cancellation, cache, crash recovery, CPU/CUDA/remote capability matrix |

### 18.8 Optimizer

| ID | Pri | Work item | Acceptance evidence |
|---|---:|---|---|
| OPTB-001 | P2 | Candidate generator | Geometric/exclusion/cable constraints |
| OPTB-002 | P2 | Coverage coefficient cache | Correct invalidation and reuse |
| OPTB-003 | P2 | CP-SAT placement model | Known optimal small instances |
| OPTB-004 | P2 | Channel/width/power model | Regulatory and interference constraints |
| OPTB-005 | P2 | Client assignment/capacity | Airtime and max-client constraints |
| OPTB-006 | P2 | Full evaluator/repair loop | Every returned plan verified |
| OPTB-007 | P2 | Robust/failure scenarios | N-1 and uncertainty tests |
| OPTB-008 | P2 | Pareto alternatives | Non-dominated and explainable |
| OPTB-009 | P2 | Existing deployment replanning | Lock/minimal-change behavior |
| OPTB-010 | P3 | Continuous position/orientation refinement | Improves full score without violating constraints |

### 18.9 Spectrum

| ID | Pri | Work item | Acceptance evidence |
|---|---:|---|---|
| SPEB-001 | P2 | Spectrum capability/schema | Calibration and sweep metadata required |
| SPEB-002 | P2 | SoapySDR plugin | Recorded sweep replay fixture |
| SPEB-003 | P2 | PSD/waterfall/max/occupancy | Numerical/visual golden tests |
| SPEB-004 | P2 | Position-correlated spectrum | Point/continuous completeness |
| SPEB-005 | P3 | Deterministic interferer signatures | Labeled test traces and unknown class |
| SPEB-006 | P3 | Vendor analyzer plugins | License-separated distribution |

### 18.10 UI, reporting, and ecosystem

| ID | Pri | Work item | Acceptance evidence |
|---|---:|---|---|
| UX-001 | P0 | Inspector launch without project | First useful data immediately |
| UX-002 | P0 | Evidence drawer | Every rendered cell traceable |
| UX-003 | P0 | Selection/context chips | Screenshots always reveal scope |
| UX-004 | P0 | Unknown/provenance visual language | Usability test distinguishes states |
| UX-005 | P1 | Report AST and HTML/PDF | Rebuild from immutable manifest |
| UX-006 | P1 | CSV/Parquet/GeoPackage exports | Round-trip/schema documentation |
| UX-007 | P1 | Easy-mode guided diagnosis | Home workflow usability test |
| UX-008 | P1 | Large-project AP search/groups | 10k AP performance fixture |
| UX-009 | P1 | Accessibility/print palettes | Contrast/color-blind tests |
| UX-010 | P2 | Planner explanations | Binding constraint/removal impact |
| UX-011 | P2 | Mobile live pairing | Clock/pose/RF fusion quality shown |
| UX-012 | P3 | WASM plugin SDK | Third-party sample metric/export |
| UX-013 | P3 | Optional coordinator | Offline-first and tenant isolation tests |

### 18.11 Open-source integrations and upstream contributions

| ID | Pri | Disposition | Work item | Acceptance evidence |
|---|---:|---|---|---|
| OSS-001 | P0 | INTEGRATE | Kismet adapter: REST/WS/source state | Fixture + live-radio normalized envelope parity |
| OSS-002 | P0 | INTEGRATE | KismetDB/PCAPNG replay | Idempotent replay; timestamps/channel/source/drop facts preserved |
| OSS-003 | P1 | REIMPLEMENT | Independent 802.11 parser/normalizer | Differential tests against captures/Kismet plus modern HT/VHT/HE/EHT fixtures |
| OSS-004 | P1 | ADOPT | Sionna worker bootstrap | Pinned upstream tests + RF Atlas canonical scene pass |
| OSS-005 | P1 | REIMPLEMENT | Canonical scene → Sionna compiler | Deterministic scene hash, geometry/material/antenna validation |
| OSS-006 | P1 | REIMPLEMENT | PropagationRequest/Result/artifact contract | CPU/CUDA round trip, cancellation, crash-safe artifacts |
| OSS-007 | P2 | CONTRIBUTE | Deconflict deterministic seeds/objective breakdown/interchange improvements | Upstream PR or documented independent bridge; RF Atlas remains unblocked |
| OSS-008 | P2 | CONTRIBUTE | Kismet generic EHT/MLO/source metadata gaps | Fixture-proven upstream issue/PR; adapter fallback retained |
| OSS-009 | P2 | CONTRIBUTE | Sionna generic tabulated antenna/material import gaps | Upstream issue/PR with tests; local adapter works independently |
| OSS-010 | P0 | REFERENCE-ONLY | wifiheatmap point/TIN oracle | Independent synthetic fixture matches documented behavior with no runtime code reuse |
| OSS-011 | P0 | REIMPLEMENT | Third-party provenance/SBOM ledger | Every imported observation/result records adapter + upstream version/build |
| OSS-012 | P0 | REIMPLEMENT | Architecture dependency lints | CI fails on foreign schema leakage into canonical crates/packages |


---

## 19. First implementation sequence

This is the recommended order for the first fourteen engineering iterations. It is deliberately vertical; each iteration leaves executable evidence.

### Iteration 1 — Project skeleton and invariants

- Cargo/pnpm/Tauri workspace.
- Strong units and IDs.
- Architecture/dependency tests.
- SQLite project and manifest.
- CLI: `new`, `inspect`, `verify`.
- Fixture generator.

### Iteration 2 — First real Wi-Fi evidence

- Implement one native collector on the developer's primary platform and Linux replay fixtures.
- Persist BSSID/channel/RSSI/time/adapter.
- Inspector table.
- Export normalized CSV/Parquet.

### Iteration 3 — Spatial truth

- Import raster floor plan.
- Two-point calibration.
- Coordinate frame graph.
- Manual survey point capture.
- Exact-point RSSI display.

### Iteration 4 — First honest heatmap

- Numerical tile layer.
- nearest and IDW.
- observed/interpolated/extrapolated masks.
- sample support and density map.
- evidence drawer.

### Iteration 5 — Active attribution

- LAN active agent.
- gateway and LAN RTT/loss.
- iPerf3 integration.
- separate LAN/Internet maps.
- controlled bottleneck tests.

### Iteration 6 — Continuous path and quality

- Path anchors/timestamp assignment.
- point/segment completeness.
- editable path and recomputation.
- survey HUD and crash-safe capture.

### Iteration 7 — Cross-platform capability layer

- Add remaining desktop native scanners.
- capability panel.
- permissions/consent UX.
- adapter/OS fixture matrix.

### Iteration 8 — Monitor-mode foundation

- Integrate Kismet on Linux through `rfatlas-kismet-adapter` for capture sources, hopping, radiotap/frame evidence, KismetDB, and PCAPNG.
- Build independent RF Atlas beacon/IE parser and normalized schema for replay/native collectors.
- Channel scheduler/completeness model remains RF Atlas-owned.
- Differential fixtures compare Kismet/native/replay interpretations.
- Capture health includes source drops, channel dwell, reconnect, permissions, and clock quality.

### Iteration 9 — Professional maps

- identity grouping;
- noise/SNR when available;
- physical AP count and ranked coverage;
- SIR/coupling baseline;
- retry/airtime groundwork.

### Iteration 10 — Requirements and reports

- policy schema;
- pass/fail/unknown maps;
- HTML/PDF report from analysis manifest;
- before/after snapshots.

### Iteration 11 — Geometry/prediction proof

- walls/materials/AP/client/antenna canonical schemas;
- fast RF Atlas P1 multi-wall solver;
- instant prediction preview;
- bootstrap pinned `rfatlas-sionna-worker`;
- deterministic canonical scene → Sionna projection for one floor and one multi-floor fixture;
- import per-transmitter path gain/RSS and compare with P1;
- measured-predicted residual view.

### Iteration 12 — Planning proof

- candidate points;
- simple coverage optimizer;
- known-optimum fixture;
- cheapest/balanced alternatives;
- “why here?” explanation.

### Iteration 13 — Kismet integration hardening

- Live/remote radio validation matrix.
- REST/WS/KismetDB/PCAPNG parity/replay tests.
- Drop/disconnect/schema-drift/restart handling.
- BLE/Zigbee/SDR evidence adapter proof without making them core survey requirements.
- Packaging/license notices and external-process lifecycle.

### Iteration 14 — Sionna detailed verification and calibration

- CPU/CUDA capability matrix and convergence sweeps.
- Reflection/refraction/diffraction acceptance scenes.
- Path-debug artifacts and worker crash/cancel/OOM handling.
- Blocked measured holdout comparison versus P1.
- First constrained inverse material/bias calibration experiment.

At this point, reassess real-world usefulness and data quality before investing in advanced ray tracing, spectrum classifiers, or cloud infrastructure.

---

## 20. Decision gates and architecture decision records

### Gate A — Native capture viability

Before committing to full cross-platform parity:

- benchmark actual scan cadence and fields on representative Windows/macOS/Linux devices;
- verify monitor-capable adapters and distribution constraints;
- decide whether Windows requires a dedicated signed driver or Npcap is sufficient;
- define the supported-hardware certification program.

### Gate B — UI renderer

Prototype:

- large floor raster/vector map;
- 100+ numerical layers;
- 10k AP/path overlays;
- tile streaming;
- 3D floors.

Choose between a web mapping stack and a custom GPU renderer based on measured latency/memory, not preference.

### Gate C — Storage split

Benchmark SQLite-only, SQLite + Parquet, and embedded analytical engines against:

- live writes;
- multi-hour monitor capture;
- spatial queries;
- report scans;
- migration/portability.

Default recommendation remains SQLite metadata + Parquet observations.

### Gate D — iPerf integration

Decide between:

- invoking an installed/bundled `iperf3` process;
- using `libiperf` through FFI;
- implementing a purpose-built active protocol.

Start with process orchestration for compatibility; retain a native lightweight probe protocol for continuous surveys.

### Gate E — Geometry kernel

Select libraries only after testing:

- import formats;
- polygon booleans/offsets;
- line/surface intersections;
- invalid geometry repair;
- WASM/desktop compatibility;
- licenses.

### Gate F — Optimizer

Prototype OR-Tools CP-SAT interoperability versus a native Rust solver stack. Correctness, model expressiveness, distribution size, license, and deployment complexity matter more than language purity.

### Gate G — Licensing/business model

Default technical recommendation:

- permissive core (`Apache-2.0 OR MIT`) or MPL-2.0 if reciprocal improvements are desired;
- open project schemas and CLI regardless;
- separately packaged plugins for proprietary hardware SDKs;
- hosted collaboration/support/catalog certification as possible commercial layers.

Decide only after auditing dependencies and intended business model.


### Gate H — Kismet integration boundary

Before declaring Kismet the supported professional Linux capture path:

- validate live radios and remote capture;
- confirm timestamp/channel/source/drop semantics through API, KismetDB, and PCAPNG;
- establish version compatibility policy;
- document packaging/license obligations;
- prove RF Atlas can still open projects and replay normalized evidence without Kismet installed.

If Kismet cannot preserve enough time-resolved survey context, keep it as raw/enrichment capture and use native collector paths for survey-critical events rather than weakening the canonical model.

### Gate I — Sionna RT adoption boundary

Before making P2 a supported product tier:

- pin a worker environment that runs upstream and RF Atlas acceptance tests;
- establish CPU/CUDA performance/convergence envelopes;
- validate geometry/material/antenna transforms;
- verify cancellation/crash/OOM recovery and artifact integrity;
- demonstrate held-out field improvement or clearly document where P1 remains preferable;
- confirm the desktop remains fully functional without the worker.

Do not begin a new in-house high-fidelity ray tracer unless this gate produces a specific, measured, non-upstream-fixable blocker.

### Initial ADR list

- ADR-001: local-first modular monolith.
- ADR-002: immutable raw/normalized/derived planes.
- ADR-003: capability-negotiated collectors.
- ADR-004: strong units and explicit unknowns.
- ADR-005: SQLite + Parquet + PCAPNG project storage.
- ADR-006: Tauri/React desktop and Rust core.
- ADR-007: `wgpu` compute with CPU reference.
- ADR-008: deterministic metric registry and analysis manifests.
- ADR-009: project coordinate-frame graph.
- ADR-010: metadata-only passive capture by default.
- ADR-011: remote sensor protocol, Kismet-first integration, and native-protocol fallback.
- ADR-012: RF Atlas owns canonical truth; foreign schemas terminate at adapters.
- ADR-013: Kismet external-process integration boundary.
- ADR-014: Sionna RT adopted in isolated optional worker.
- ADR-015: Deconflict contribution/interchange-only strategy.
- ADR-016: wifiheatmap clean-room reference-only strategy.
- ADR-012: WASM plugin boundary.
- ADR-013: empirical-first, ray-tracing-later predictive tiers.
- ADR-014: hybrid solver for AP planning.
- ADR-015: policy-as-data requirements.
- ADR-016: transparent rule engine before LLM narration.

---

## 21. Major risks and mitigations

| Risk | Why it matters | Mitigation |
|---|---|---|
| Cross-platform Wi-Fi APIs expose different data | Feature parity claims become misleading | Capability contracts, platform-specific UX, remote sensor pairing |
| Monitor-mode adapter support is fragile | Professional passive surveys depend on it | Certified hardware matrix, isolated helpers, Linux sensor path, replay fixtures |
| RSSI is not calibrated absolute RF power | Heatmaps across devices can disagree | Calibration profiles, overlap bias estimation, uncertainty, no false precision |
| Noise often unavailable/unreliable | SNR maps can be fabricated accidentally | Strict evidence requirements; expected noise only in labeled prediction |
| Continuous survey path error | Mislocated samples produce convincing false maps | Store timestamps/anchors, pose uncertainty, post-edit, AR path option |
| Interpolation looks more certain than evidence | Smooth maps can hide sparse data | support/uncertainty layers, blocked CV, extrapolation hatching |
| Predictive geometry/materials are wrong | Ray tracing can be precisely wrong | fast priors, calibration, sensitivity, held-out validation, editable source data |
| PHY/goodput prediction varies by hardware | Generic tables overpromise | client profiles, empirical curves, distributions, conservative defaults |
| Optimizer exploits model errors | Mathematically optimal plan fails in reality | robust scenarios, full evaluator, post-install validation loop |
| Wi-Fi 7 behavior evolves across clients | MLO/puncturing predictions may be speculative | profile-based capabilities, controller/field evidence, versioned models |
| Active tests disturb the network | Measurement changes what is measured | test-impact metadata, rate budgets, lightweight continuous profile |
| Internet tests confound Wi-Fi | Users get wrong remediation | multi-tier endpoint topology and attribution |
| Raw capture creates privacy/security liability | Sensitive third-party data may be retained | payload discard, pseudonyms, opt-in raw retention, encryption, redaction |
| CAD/BIM import is complex/licensed | Format support can consume project scope | staged formats, adapter boundary, prioritize raster/PDF/SVG/DXF |
| GPU numerical differences | Non-reproducible maps/reports | CPU reference, tolerance tests, algorithm/version manifests |
| Large project performance | Stadium/campus cases can overwhelm desktop | tiled compute, columnar chunks, spatial indices, background jobs |
| Proprietary vendor data licenses | Cannot legally ship copied AP/antenna catalogs | manufacturer/open imports, source ledger, user-supplied models |
| “Overkill” delays useful release | Research platform never becomes usable | vertical roadmap; Phase 1 solves dead spots before advanced solver work |
| ML distracts from fundamentals | Opaque models may look impressive but generalize poorly | deterministic baseline, holdouts, uncertainty, ML only as residual/classifier |
| Fixed-sensor clock/duplicate issues | Distributed observations miscorrelate | monotonic sequence, clock model, content hashes, deduplication |
| Product complexity overwhelms users | Professional capability becomes unusable | Easy/Pro/Lab progressive disclosure and guided workflows |

---

### 21.1 Open-source dependency risks added by the audit

| Risk | Consequence | Mitigation |
|---|---|---|
| Kismet GPL/packaging boundary misunderstood | Distribution/legal friction | External process/API/file integration, notices/SBOM, explicit legal/license review before bundling |
| Kismet schema/device aggregation becomes semantic authority | Lost survey timing/context and lock-in | Canonical ObservationEnvelope, independent parser, raw/replay fixtures, adapter-only foreign schema |
| Sionna Python/native stack breaks desktop installation | Reliability/support burden | Optional isolated worker, pinned environment/container, capability handshake, desktop remains useful without it |
| Sionna Monte Carlo/high-fidelity output is treated as exact | False precision | Convergence sweeps, measured holdouts, prediction uncertainty, P1/P2 comparison, backend/version provenance |
| Upstream API/version drift | Reproducibility break | Pin audited versions, compatibility fixtures, content-addressed results, migration/engine-version policy |
| Deconflict contribution work distracts core roadmap | Schedule slip | Upstream work only when generic and small; no production dependency on acceptance |
| wifiheatmap code accidentally enters production | GPL coupling and old architecture debt | Clean-room synthetic oracle directory; architecture CI forbids production dependency |
| “Reuse” creates overlapping duplicate models | Maintenance complexity | One canonical RF Atlas model; adapters project/normalize at narrow waist |


## 22. Open research questions

### Measurement

- Which consumer adapters provide stable per-frame signal/noise metadata across bands and drivers?
- How should capture loss be estimated well enough to normalize observed airtime and client density?
- What survey point duration gives defensible repeatability per band/channel density?
- Can overlapping fixed sensors estimate hidden airtime more accurately than one mobile sensor?
- How accurately can AP Tx power be inferred without controller access?
- How should MLO per-link active metrics be captured consistently across operating systems?

### Spatial inference

- Which interpolation algorithm wins under realistic wall/corridor geometries and sparse home surveys?
- How should pose covariance be propagated into RF field uncertainty efficiently?
- Can an AR path plus sparse plan anchors outperform manual continuous paths without unacceptable battery use?
- What is the best way to combine physical propagation priors with GP residuals while preserving extrapolation honesty?

### Prediction

- Which material parameters are identifiable from ordinary walk surveys?
- How many reflection/diffraction interactions improve held-out accuracy before geometry uncertainty dominates?
- What spatial/frequency averaging best predicts user experience without modeling unstable small-scale fading?
- How should 6 GHz PSD and regulatory classes be represented across regions and channel widths?
- What generic client profiles remain conservative across common phones/laptops/IoT devices?

### Capacity and optimization

- What simple airtime model provides the best value before packet-level simulation becomes necessary?
- How should OFDMA/MU-MIMO/MLO benefits be modeled without assuming ideal scheduling?
- Which robust objective best balances equipment cost against uncertainty and N-1 resilience?
- Can AP-on-a-stick candidate surveys serve as a surrogate model for the optimizer without double-counting time-varying interference?

### Spectrum

- Which affordable analyzers offer calibrated tri-band data and redistributable SDKs?
- Can generic SDR hardware sweep 5/6 GHz quickly enough for walking surveys?
- How reliable are deterministic versus learned interferer classifiers across hardware and environments?


### Open-source integration

- Which Kismet REST/WebSocket/KismetDB fields preserve enough per-packet/per-source timing and channel context for survey-grade correlation on each supported version?
- Which Kismet EHT/MLO facts are currently complete enough to consume versus worth contributing upstream?
- What default Sionna sample/depth/interaction settings give the best accuracy-per-second for residential, office, warehouse, and multi-floor scenes?
- How stable are Sionna results across CPU/CUDA backends at fixed seed and acceptable numerical tolerance?
- Which canonical antenna/material representations can map losslessly enough into Sionna while remaining engine-independent?
- Which Deconflict interchange/reproducibility improvements are likely to be accepted upstream and useful beyond RF Atlas?
- What is the cleanest licensing/distribution UX when Kismet is user-installed versus optionally bundled by platform/package?

### Product and ecosystem

- Is a fully documented `.rfatlas` directory/bundle sufficient, or should the primary project itself be a GeoPackage extension?
- Which controller integrations provide useful data without creating a maintenance trap?
- What minimum plugin ABI can remain stable while RF/metric models evolve?
- Which portions should be a reusable Rust library versus app-internal until APIs stabilize?

---

## 23. Definition of done

A feature is done only when all applicable items are true:

- Domain semantics and units documented.
- Required collector capabilities declared.
- Unsupported/unknown behavior defined.
- Raw/normalized/derived schema complete.
- Algorithm and assumptions documented.
- Uncertainty/support behavior implemented.
- Deterministic fixtures and property tests pass.
- CPU/GPU or cross-platform parity tested where applicable.
- UI exposes provenance and selection context.
- Export representation documented.
- Report methodology generated.
- Security/privacy review complete.
- Performance envelope measured.
- User workflow tested.
- Source/license ledger updated.
- Migration/backward compatibility considered.
- Failure/crash/recovery behavior tested.
- No canonical project record requires Kismet, Sionna, Deconflict, or wifiheatmap object schemas to interpret it.
- Kismet can be disconnected/replaced and previously normalized surveys remain usable and reproducible.
- Sionna worker can be absent, crashed, moved to another backend, or upgraded without corrupting project state.
- Every P2/P3 propagation artifact identifies exact Sionna/Mitsuba/Dr.Jit/Python/backend versions and input hashes.
- Final Wi-Fi interference, PHY, capacity, requirements, and optimizer results are computed by RF Atlas semantics rather than raw Sionna/Deconflict scores.

A heatmap is not done because it looks plausible.

---

# Appendices

## Appendix A — TamoGraph feature traceability audit

Baseline: public TamoGraph 8 documentation and the current 8.4 release information as of the research date. `[V]` means documented; disposition describes RF Atlas.

### A.1 Product, hardware, and capture

| Documented TamoGraph behavior | Status | RF Atlas disposition |
|---|---:|---|
| Windows and macOS editions | [V] | Desktop on Windows/macOS; Linux desktop/sensor also first-class |
| 802.11 a/b/g/n/ac/ax/be support | [V] | Native semantic model through EHT/Wi-Fi 7, versioned by decoder capability |
| Passive surveys require compatible adapter/driver | [V] | Certified adapter matrix plus Linux remote sensor fallback |
| Active surveys work with ordinary associated adapter | [V] | Same, with explicit endpoint topology |
| Predictive surveys require no Wi-Fi adapter | [V] | Same |
| Multiple adapters and simultaneous passive+active | [V] | Same; generalize to distributed sensors |
| Per-adapter signal correction | [V] | Calibration distributions, not only scalar correction |
| Configurable channels and scan intervals | [V] | Logged channel schedule and completeness per sample |
| 2.4/5/6 GHz and Wi-Fi 7 USB/integrated adapters | [V] | Capability-probed hardware; no blanket adapter claims |
| Predictive-only launch without adapter/admin on Windows | [V] | Unprivileged planner mode by architecture |

Sources: [T1], [T2], [T3], [T4].

### A.2 Projects, plans, and field collection

| Documented TamoGraph behavior | Status | RF Atlas disposition |
|---|---:|---|
| Raster, PDF, vector, DWG/DXF plan inputs | [V] | Staged open importers; licensed DWG adapter if justified |
| Two-point scale calibration | [V] | Same plus multi-point residual calibration |
| GPS control-point calibration | [V] | Same with CRS and covariance |
| Environment type/guess range/extrapolation controls | [V] | Explicit interpolation/prediction method and uncertainty |
| Continuous path survey with clicks at turns | [V] | Same plus editable timestamp assignment and AR pose |
| Point-by-point survey with complete scan cycles | [V] | Same, implemented as multidimensional quality gate |
| GPS automatic survey | [V] | Same with fix quality and pause rules |
| Passive, active, hybrid, and passive+spectrum collection | [V] | Same plus distributed topologies |
| Mix survey modes in a project | [V] | Typed sessions; merge only semantically compatible evidence |
| Pause/resume and path display | [V] | Same; crash-safe partial sessions |
| Voice assistance | [V] | Audio/haptic cues with accessibility settings |
| Floor/map comments and photos | [V] | Positioned annotations, install evidence, and report blocks |
| Autosave/project backups | [V] | Transactional autosave, content hashes, recovery, versioned operations |
| Survey track export/import for team work | [V] | Offline operation/chunk merge plus optional live coordination |

Source: [T2].

### A.3 AP inventory, dashboard, and organization

| Documented TamoGraph behavior | Status | RF Atlas disposition |
|---|---:|---|
| AP list with SSID/BSSID, vendor, channel, signal, security, max rate, MIMO | [V] | Expand into physical-device/radio/BSSID/MLD identity graph |
| Search by name, SSID, or MAC | [V] | Full query/filter/command palette |
| AP aliases, grouping, selected/total counter | [V] | Saved selections/groups and visible scope on every map |
| Multi-SSID detection/relinking | [V] | Evidence-weighted identity graph with versioned overrides |
| Dashboard scan-cycle duration, adapter/band health, RSSI/AP history | [V] | Inspector health timeline and quality diagnostics |
| Active dashboard for AP/channel/PHY/noise/SNR/throughput/loss/RTT | [V] | Same, split by endpoint and evidence method |
| Automatic/manual AP placement on plan | [V] | Multiple localization solvers plus probability region |
| Option to use inferred AP location only as icon or as analysis input | [V] | Provenance toggle and analysis-manifest pin |
| AP rank selection | [V] | Arbitrary rank, margins, eligibility, and failure scenarios |

Sources: [T2], [T3].

### A.4 Passive visualizations

| TamoGraph layer/control | Status | RF Atlas disposition |
|---|---:|---|
| Signal level | [V] | Observed aggregate + interpolation + uncertainty |
| Noise level | [V] | Only when measured; predicted prior separately labeled |
| SNR | [V] | Strict method/evidence requirements |
| Signal-to-interference ratio | [V] | Open linear-power/channel-coupling formula |
| AP coverage areas | [V] | Best/eligible/physical-radio views |
| Number of APs | [V] | BSSID and deduplicated physical-radio variants |
| Secondary/tertiary AP coverage | [V] | Arbitrary rank, margin, roam/failure interpretation |
| Expected PHY rate | [V] | Distribution from client profile, bidirectional SINR, PER curves |
| Frame format | [V] | Non-HT/HT/VHT/HE/EHT and capability details |
| Channel bandwidth | [V] | 20–320 MHz plus puncturing/effective width |
| Per-band/channel views | [V] | Same with region/PSC/DFS/MLO dimensions |
| Requirements overlays | [V] | Versioned policy and strict pass/fail/unknown |
| Survey/AP selection and filtering | [V] | Explicit dimensional selection chips/manifests |
| AP detection/placement/coverage thresholds | [V] | Versioned thresholds and sensitivity analyses |
| SIR utilization assumptions | [V] | Measured/advertised/assumed utilization kept separate |
| Extrapolation control and warning | [V] | Hatching, support distance, uncertainty, compliance unknown |

Source: [T2].

### A.5 Active visualizations and tests

| TamoGraph behavior | Status | RF Atlas disposition |
|---|---:|---|
| SSID roaming or locked BSSID active survey | [V] | Same plus MLO/link policy and explicit transition type |
| Basic reachability/RTT mode | [V] | Gateway/LAN/WAN/application tiers |
| Advanced TCP/UDP throughput test | [V] | iPerf3 plus lightweight native probes |
| Upstream/downstream | [V] | Up/down/bidirectional and test-induced load metadata |
| IPv4/IPv6 | [V] | Same |
| QoS traffic types/DSCP/WMM-oriented profiles | [V] | Controlled DSCP preservation/contention validation |
| Actual PHY rate | [V] | OS/driver-derived with capability/method metadata |
| TCP/UDP throughput | [V] | Distribution and endpoint-specific maps |
| UDP loss | [V] | Burst, reorder, jitter, offered/received load |
| RTT | [V] | median/p95/p99 and separate targets |
| Associated AP/roaming path | [V] | Structured roam events and disruption impact |
| Active requirements | [V] | Policy-as-data and unknown semantics |

Source: [T2].

### A.6 Spectrum analysis

| TamoGraph behavior | Status | RF Atlas disposition |
|---|---:|---|
| Supported Wi-Spy/WiPry-family analyzer integration | [V] | Vendor SDK plugins where legal plus generic SoapySDR |
| Current spectrum | [V] | Calibrated PSD with metadata |
| Maximum/peak view | [V] | max hold plus detector definition |
| Waterfall/history | [V] | Tiled frequency-time store |
| Channel/frequency axes | [V] | Same plus integrated channel power/occupancy |
| Passive+spectrum simultaneous survey | [V] | Same; distributed sensors supported |
| Historical spectrum around survey point | [V] | Exact time-window retrieval with clock uncertainty |
| Multiple analyzer models in some workflows | [V] | Multi-sensor fusion with calibration/band allocation |
| Spectrum in reports/exports | [V] | Numerical exports and reproducible plots |

Sources: [T2], [T5].

### A.7 Predictive modeling

| TamoGraph behavior | Status | RF Atlas disposition |
|---|---:|---|
| Walls/partitions as lines/polygons | [V] | True 3D extrusions and validated topology |
| Custom attenuation by 2.4 and 5/6 GHz | [V] | Frequency-dependent distributions/material physics |
| Reflection percentage | [V] | Documented coefficient/solver semantics |
| Attenuation zones in dB per distance | [V] | Volumetric attenuation with uncertainty |
| Floors, roofs, heights, alignment, holes/openings | [V] | Building coordinate graph and true 3D propagation |
| Duplicate floors and adjacent-floor propagation | [V] | Same plus scenario/version control |
| Mix measured and virtual AP evidence | [V] | Hybrid layers with provenance per cell |
| AP standard/channel/streams/GI/rates/power | [V] | Complete radio scenario and legality validation |
| 6 GHz SP/LPI/VLP and PSD settings | [V] | Region-versioned regulatory policy |
| AP antenna rotation/elevation/tilt/height | [V] | 3D transforms, polarization, and validation viewer |
| Generic/vendor antenna library and editor | [V] | Open schema/imports with source/license checks |
| AP presets combining radio/antenna | [V] | Versioned equipment catalog |
| Client capability templates | [V] | Rich client Tx/Rx/roam/MLO profiles and empirical calibration |
| Application templates/requirements | [V] | Demand distributions and policy profiles |
| Low/medium/good/best quality | [V] | Named solver tiers with explicit algorithms |
| Reflection/Fresnel advanced effects | [V] | Open path records and canonical validation |
| CPU/GPU compute and background precomputation | [V] | `wgpu` plus CPU reference and content-addressed tiles |
| AP power best-practice guidance | [V] | Bidirectional optimizer and asymmetry maps |

Sources: [T2], [T6].

### A.8 Auto-planner

| TamoGraph behavior | Status | RF Atlas disposition |
|---|---:|---|
| Define deployment/required coverage areas | [V] | Coverage/demand masks with weighted occupancy |
| Coverage optimization | [V] | Explicit constraints and robust percentile evaluation |
| Simple/advanced capacity methods | [V] | Airtime model plus client assignment |
| Client/application templates | [V] | Spatial stochastic demand profiles |
| Minimum AP count/RSSI constraints | [V] | General declarative requirements |
| Channel plan and power control | [V] | Joint channel/width/puncturing/power/MLO plan |
| Disable unnecessary 2.4 GHz radios | [V] | Radio-enable variables with coverage/client constraints |
| Standard/high-precision planning | [V] | Search-budget/solver tier with quality metrics |
| Reconfigure existing AP channels/power without moving | [V] | Minimal-change replanning |
| Maximum clients per AP by band in 8.4 | [V] | Same plus airtime and per-radio constraints |
| GPU-accelerated placement history | [V] | Precomputed GPU candidate scoring + explicit outer solver |

Sources: [T2], [T3], [T6].

### A.9 Requirements and reporting

| TamoGraph behavior | Status | RF Atlas disposition |
|---|---:|---|
| Min signal/SNR/SIR/AP count/PHY/frame/width thresholds | [V] | Versioned requirements with client/area scope |
| Active throughput/RTT thresholds | [V] | Endpoint and percentile-specific requirements |
| Required percentage of area | [V] | Strict, weighted, confidence-aware, and unknown percentages |
| Basic/medium/advanced-like profiles | [V] | Editable cited profiles, no universal claim |
| Requirements Compliance percentage in 8.4 | [V] | Per-constraint and overall reproducible result |
| PDF/HTML/ODT/MHT/KMZ-style reports/exports | [V] | HTML/PDF/editable/open numerical/GIS exports |
| Select plans/surveys/APs/bands/ranks/paths/photos/comments | [V] | Report AST over immutable analysis manifests |
| Branding/language options in higher edition | [V] | Theme/localization separated from analytical semantics |

Sources: [T2], [T3].

---

## Appendix B — NetSpot feature traceability audit

Baseline: current public product/help pages as of the research date.

### B.1 Inspector

| NetSpot behavior | RF Atlas disposition |
|---|---|
| Nearby SSID/BSSID/signal/channel/band/security/vendor/type/mode table | Adopt and expand to parsed IE/physical-device/MLO model |
| Signal/noise history graphs | Adopt; retain raw/smoothed method and adapter channel gaps |
| 2.4/5/6 GHz channel overlap graph | Adopt UI clarity; replace simplistic overlap with coupling/airtime evidence underneath |
| Table/chart switching, filters, search, CSV export | Adopt |
| AP aliases/grouping/hidden SSID handling | Adopt with versioned identity graph |
| Real-time network comparison | Adopt and add active/path context |

Sources: [N1], [N2], [N3].

### B.2 Survey and heatmaps

NetSpot documents passive, active, troubleshooting, snapshot, and AP-on-a-stick-related workflows. Its current help inventory includes roughly two dozen visualizations depending on platform/edition/project data.

| NetSpot visualization/workflow | RF Atlas disposition |
|---|---|
| Signal level | Adopt + uncertainty |
| SNR | Adopt with strict noise evidence |
| SIR | Adopt with open coupling/utilization method |
| Secondary signal | Adopt as arbitrary ranked eligible AP |
| Noise | Adopt only where measured |
| AP quantity | Adopt physical/BSSID variants |
| Frequency-band coverage | Adopt |
| PHY mode | Adopt/expand to EHT/MLO |
| Download/upload | Adopt but split LAN/Internet |
| Wireless transmit rate | Adopt with method/OS caveats |
| iPerf3 upload/download | Adopt and extend bidirectional/UDP/profiles |
| Troubleshooting: SNR, low/high signal, high noise, overlap, weak secondary, low speed | Adopt as transparent rule graph, not opaque labels |
| Point-on-map collection | Adopt with scan completeness gate |
| Overlapping sample-radius guidance | Replace fixed visual assumptions with support/quality map |
| Snapshots, merge/resume, before/after | Adopt as versioned sessions/difference maps |
| AP-on-a-stick by merging snapshots | Adopt as explicit experiment |
| Custom heatmap appearance, AP sensitivity, contours/markers | Adopt, separating rendering from metric calculation |
| PDF/CSV reporting | Adopt and broaden open exports/manifests |

Sources: [N4], [N5], [N6], [N7].

### B.3 Planning

| NetSpot behavior | RF Atlas disposition |
|---|---|
| Upload or draw single/multi-floor plan | Adopt |
| Walls, doors, windows, racks/materials | Adopt with validated 3D geometry |
| Floor slabs/thickness/material | Adopt with true multi-floor scene |
| Stair/atrium cutouts and alignment | Adopt |
| AP manufacturer/model selection | Adopt only with licensed/open data |
| Antenna pattern imports in multiple formats | Adopt through open antenna schema/import plugins |
| Multiple routers/APs | Adopt |
| Real-time signal preview | Adopt using P0/P1 solver |
| Planning heatmaps/reports | Adopt with bidirectional/client/uncertainty views |
| Mobile Android planning | Defer full editor; companion capture first |

Sources: [N1], [N8].

### B.4 Mobile/cross-platform

| NetSpot behavior | RF Atlas disposition |
|---|---|
| Windows/macOS/Android projects and desktop portability | Adopt open cross-platform bundle |
| Android nearby inspection/surveys | Adopt within scan throttling/permission limits |
| iOS current-network speed/ping/heatmap/device discovery | Adopt active/current-link role; pair with passive sensor for RF scan |
| Enterprise unlimited project scale | Avoid artificial data limits in core; scale through architecture |

Sources: [N1], [N9], [N10].

---

## Appendix C — Acrylic Wi-Fi Heatmaps feature traceability audit

Baseline: current public product page and detailed official user manual as of the research date.

### C.1 Capture and survey workflow

| Acrylic behavior | RF Atlas disposition |
|---|---|
| Any adapter in native scan mode | Adopt capability-based native collectors |
| Supported monitor-mode adapters on Windows | Adopt through Npcap/validated adapter path |
| Multiple simultaneous adapters | Adopt/generalize to distributed sensors |
| Native mode sees AP/beacon information; monitor sees frames/clients | Make this distinction explicit in UI and schema |
| Channel hopping or fixed channel | Adopt with logged schedule/completeness |
| Normal point, continuous, and GPS modes | Adopt plus AR/SLAM |
| Continuous mode positions intermediate samples along straight route | Adopt only as fallback with retained assumptions |
| iPerf recommended, large-file fallback | Use iPerf3 + native probes; avoid fragile file fallback as primary |
| Gateway ICMP latency | Adopt but add LAN/WAN/application tiers |
| Pause/stop/undo | Adopt with crash recovery |
| Blueprint, satellite, georeferenced/GPS calibration | Adopt with open CRS/transform model |
| Locations/sublocations and repeated scans | Adopt site/building/floor/session hierarchy |

Source: [A1].

### C.2 Passive analysis

| Acrylic layer | RF Atlas disposition |
|---|---|
| RSSI | Adopt + uncertainty |
| AP coverage | Adopt |
| strongest/best channel or AP views | Adopt with eligibility and airtime context |
| advertised data rate | Keep distinct from expected/actual rate |
| client/cell density in monitor mode | Adopt with evidence tier/privacy |
| number of APs | Adopt physical/BSSID variants |
| retry rate in monitor mode | Adopt with denominator/direction/capture caveats |
| channel overlap and co-channel interference | Adopt with open spectral coupling |
| SNR with compatible hardware | Adopt only when measured |
| detailed grid | Adopt evidence drawer/numerical export |
| RF spectrum with compatible analyzer | Adopt plugin architecture |
| AP triangulation/automatic and manual location | Adopt probabilistic solvers |

Source: [A1].

### C.3 Active and quality analysis

| Acrylic behavior | RF Atlas disposition |
|---|---|
| Bandwidth, latency, packet loss, AP roaming maps | Adopt and extend distributions/endpoints/structured events |
| Quality profiles with RSSI, redundant coverage, overlap, co-channel, latency, bandwidth, loss, roaming | Adopt as policy-as-data |
| Red/green maps, compliance percentage, grades | Adopt pass/fail/unknown; avoid hiding dimensions behind grade |
| Default web/VoIP-like profiles | Provide cited editable examples |
| Compare scans/overlays | Adopt uncertainty-aware difference maps |

Source: [A1].

### C.4 Visualization, reporting, and data

| Acrylic behavior | RF Atlas disposition |
|---|---|
| Map, satellite, 2D and 3D views | Adopt with analytical 3D purposes |
| Custom colors/ranges/thresholds/opacity/contours/legend | Adopt with accessible defaults and separated rendering |
| Executive, technical, and complete report types | Adopt through composable report templates |
| Editable DOCX and KMZ | Target editable/open formats where maintainable |
| Raw CSV with point source, GPS accuracy, x/y, AP/security/channel, ping/failure/latency/bandwidth/association/spectrum | Exceed with open normalized Parquet/JSON schema and full provenance |
| AP friendly names/descriptions/inventory | Adopt identity/inventory model |
| OUI database updates | Adopt signed/versioned catalog |
| Plot cache and UI refresh settings | Implement deterministic content-addressed cache and live-view controls |

Sources: [A1], [A2], [A3].

---

## Appendix D — Platform capability strategy

Capabilities can change with OS releases, permissions, hardware, and drivers; runtime probing is authoritative.

| Platform | Nearby scan | Current link | Monitor/raw | Active tests | Spatial tracking | Recommended role |
|---|---|---|---|---|---|---|
| **Windows 10/11** | Native Wi-Fi API; location consent constraints | Good OS telemetry, adapter-dependent details | Npcap/supported drivers and adapters; may disrupt association | Full | External/mobile/GPS | Primary desktop; dual-adapter professional survey |
| **Linux** | nl80211 | Good, driver-dependent | Best open monitor/radiotap path; simultaneous managed+monitor not guaranteed | Full | GPS/external/mobile | Primary remote sensor and Lab platform |
| **macOS** | CoreWLAN subject to permissions/version behavior | Good for associated interface where exposed | Built-in/external behavior constrained; remote sensor recommended for guaranteed professional passive | Full | Pair with iOS/other sensors | Polished desktop/active client/planner |
| **Android** | Available with permissions and scan throttling | Good current-network APIs; device-dependent | Ordinary apps generally not general raw monitor collectors | Full | ARCore, Wi-Fi RTT on supported devices/APs | Mobile survey/path/active client and basic nearby scan |
| **iOS/iPadOS** | No general-purpose nearby network scan for normal apps | Authorized current-network metadata | Not a general monitor collector | Full controlled probes | ARKit/RoomPlan | Excellent pose/room-scan/active client paired with passive sensor |

Sources: [P1]–[P10].

---

## Appendix E — Metric semantics checklist

Before shipping a metric, answer all rows.

| Question | Example failure prevented |
|---|---|
| What exact physical/logical quantity is this? | Calling Internet download “Wi-Fi speed” |
| What are the units? | Mixing dB and dBm |
| Which sensor/API/frame field supplies it? | Inventing noise on an unsupported adapter |
| Is it observed, derived, assumed, or predicted? | Showing expected PHY as actual PHY |
| What is the aggregation window? | Comparing a 100 ms sample with a 30 s test |
| What are the filters/grouping rules? | Counting multiple BSSIDs as redundant APs |
| How are powers combined? | Averaging/summing dBm directly |
| How is position assigned? | Treating a late path click as exact location |
| What uncertainty/support accompanies it? | Smooth confident extrapolation into unsurveyed rooms |
| What client/application profile applies? | Planning for a 4x4 laptop when users have 1x1 phones |
| What endpoint/path does it test? | Blaming Wi-Fi for remote server congestion |
| Can the measurement perturb the network? | Throughput test causing its own latency spike |
| Which algorithm/version produced it? | Old/new report values changing silently |
| How is unknown represented? | Missing data rendered as zero/failure/pass |
| What validation fixture proves it? | Visually plausible but mathematically wrong map |

---

## Appendix F — Research experiments backlog

### F.1 TamoGraph behavioral experiments

1. Open-room point grid to infer spatial interpolation kernel.
2. Sparse convex-hull survey to observe extrapolation boundary.
3. Two samples with controlled values to observe weighting.
4. Unequal point density to test sample weighting/averaging.
5. Continuous segment with deliberately nonuniform walking speed.
6. Continuous path with pause and late turn click.
7. AP auto-location with symmetric/asymmetric point layouts.
8. AP auto-location with known wall between samples.
9. Multi-SSID AP interference dedup test.
10. Adjacent-channel controlled APs at variable power to infer coupling.
11. Expected-PHY outputs versus RSSI/client profile.
12. Material single-wall predictive attenuation test.
13. Reflection-quality toggle in canonical corridor.
14. Multi-floor slab/opening test.
15. Antenna pattern rotation test.
16. Auto-planner symmetry and tie-breaking.
17. Auto-planner minimum clients/capacity constraints.
18. Auto-planner channel/power reconfiguration behavior.
19. Requirements area denominator with unknown/unsurveyed cells.
20. CPU versus GPU output reproducibility.

### F.2 NetSpot behavioral experiments

1. Heatmap influence radius and interpolation near boundaries.
2. Noise/SNR behavior across Windows/macOS adapters.
3. Active Internet versus iPerf layer semantics.
4. Channel overlap calculation for bonded channels.
5. Snapshot/AP-on-a-stick merge rules.
6. Predictive material and multi-floor behavior.
7. Troubleshooting threshold customization and unknown handling.

### F.3 Acrylic behavioral experiments

1. Native versus monitor AP/client observation completeness.
2. Retry-rate denominator and deduplication.
3. Continuous route timing assignment.
4. Channel-overlap `<20 dB`-style influence behavior described in manual.
5. AP triangulation across scans.
6. Quality percentage/grade denominator.
7. iPerf/gateway test timing per point.
8. Raw CSV round trip and calculated-position semantics.

For every experiment, record software build, OS, adapters/drivers, AP hardware/firmware, settings, floor plan, exact route/points, raw independent reference data, screenshots/exports, and conclusion confidence.

---

## Appendix G — Primary source bibliography

Competitor behavior is based primarily on official vendor pages/manuals. Technical implementation references favor official standards, platform documentation, and primary papers.

### TamoGraph / TamoSoft

- **[T1]** [TamoGraph product overview and feature list](https://www.tamos.com/products/wifi-site-survey)
- **[T2]** [TamoGraph Site Survey Help Documentation, Version 8](https://www.tamos.com/htmlhelp/tg/index.htm)
- **[T3]** [TamoSoft news and TamoGraph 8.4 release summary](https://www.tamos.com/about/news)
- **[T4]** [TamoGraph downloads and compatible adapters](https://www.tamos.com/download/main/tg)
- **[T5]** [TamoGraph spectrum analysis overview](https://www.tamos.com/products/wifi-site-survey)
- **[T6]** [TamoGraph 8.0 changes: 6 GHz, PSD, spectrum, GPU auto-placement notes](https://pal.tamos.com/en/knowledgebase/article/whats-new-in-tamograph-version-8-0)
- **[T7]** [TamoGraph 8.4 detailed announcement](https://pal.tamos.com/en/announcements/article/whats-new-in-tamograph-site-survey-8-4)

### NetSpot

- **[N1]** [NetSpot product/features overview](https://www.netspotapp.com/)
- **[N2]** [NetSpot Inspector and feature documentation](https://www.netspotapp.com/features.html)
- **[N3]** [NetSpot terms and metric definitions](https://www.netspotapp.com/help/terms-definitions/)
- **[N4]** [Starting a NetSpot survey](https://www.netspotapp.com/help/how-do-i-start-my-survey/)
- **[N5]** [NetSpot report and heatmap interpretation](https://www.netspotapp.com/help/how-to-understand-netspot-report/)
- **[N6]** [NetSpot heatmap appearance/settings](https://www.netspotapp.com/help/how-can-i-adjust-the-appearance-of-my-heatmaps/)
- **[N7]** [NetSpot editions and platform feature differences](https://www.netspotapp.com/help/what-is-the-difference-between-netspot-free-scanner-reporter-and-pro/)
- **[N8]** [NetSpot predictive survey/planning workflow](https://www.netspotapp.com/help/how-to-perform-predictive-survey-with-netspot/)
- **[N9]** [NetSpot for Android manual](https://www.netspotapp.com/help/netspot-android-manual/)
- **[N10]** [NetSpot for iOS](https://www.netspotapp.com/netspot-for-ios.html)

### Acrylic Wi-Fi Heatmaps

- **[A1]** [Acrylic Wi-Fi Heatmaps official user manual](https://www.acrylicwifi.com/en/wifi-heatmaps/user-manual/)
- **[A2]** [Acrylic Wi-Fi Heatmaps product overview](https://www.acrylicwifi.com/en/wifi-heatmaps/)
- **[A3]** [Acrylic requirements and adapter compatibility](https://www.acrylicwifi.com/en/wifi-heatmaps/requirements-and-compatibility/)
- **[A4]** [Acrylic documentation/tutorials and sample exports](https://www.acrylicwifi.com/en/wifi-heatmaps/documentation-and-tutorials/)

### Operating-system and capture platforms

- **[P1]** [Microsoft: Wi-Fi access and location-consent changes](https://learn.microsoft.com/en-us/windows/win32/nativewifi/wi-fi-access-location-changes)
- **[P2]** [Microsoft Native Wi-Fi API](https://learn.microsoft.com/en-us/windows/win32/nativewifi/native-wifi-reference)
- **[P3]** [Npcap developer guide](https://npcap.com/guide/npcap-devguide.html)
- **[P4]** [Linux Wireless documentation: `iw` and monitor mode](https://wireless.docs.kernel.org/en/latest/en/users/documentation/iw.html)
- **[P5]** [Linux kernel `nl80211` netlink specification](https://www.kernel.org/doc/html/latest/networking/netlink_spec/nl80211.html)
- **[P6]** [Apple CoreWLAN](https://developer.apple.com/documentation/corewlan)
- **[P7]** [Apple NEHotspotNetwork](https://developer.apple.com/documentation/networkextension/nehotspotnetwork)
- **[P8]** [Android Wi-Fi scanning](https://developer.android.com/develop/connectivity/wifi/wifi-scan)
- **[P9]** [Android Wi-Fi RTT](https://developer.android.com/develop/connectivity/wifi/wifi-rtt)
- **[P10]** [Android ARCore motion tracking](https://developers.google.com/ar/develop/fundamentals)
- **[P11]** [Apple RoomPlan](https://developer.apple.com/augmented-reality/roomplan/)
- **[P12]** [Apple ARKit](https://developer.apple.com/augmented-reality/arkit/)
- **[P13]** [Radiotap field documentation](https://www.radiotap.org/)
- **[P14]** [Wireshark User’s Guide](https://www.wireshark.org/docs/wsug_html/)
- **[P15]** [Wireshark Developer’s Guide and extcap](https://www.wireshark.org/docs/wsdg_html/)

### Open capture/sensor architecture

- **[O1]** [Kismet remote capture](https://www.kismetwireless.net/docs/readme/remotecap/remotecap/)
- **[O2]** [Kismet data sources](https://www.kismetwireless.net/docs/readme/datasources/datasources/)
- **[O3]** [KismetDB unified logging](https://www.kismetwireless.net/docs/readme/logging/kismetdb/)

### Active measurement

- **[M1]** [iperf3 official documentation](https://software.es.net/iperf/)
- **[M2]** [RFC 4656: One-Way Active Measurement Protocol](https://www.rfc-editor.org/rfc/rfc4656)
- **[M3]** [RFC 5357: Two-Way Active Measurement Protocol](https://www.rfc-editor.org/rfc/rfc5357)
- **[M4]** [RFC 3393: IP Packet Delay Variation Metric](https://www.rfc-editor.org/rfc/rfc3393)
- **[M5]** [RFC 8762: Simple Two-Way Active Measurement Protocol](https://www.rfc-editor.org/rfc/rfc8762)

### RF propagation and spectrum

- **[R1]** [ITU-R P.1238: propagation data and prediction methods for indoor radio systems](https://www.itu.int/rec/R-REC-P.1238/en)
- **[R2]** [ITU-R P.2040: effects of building materials and structures on radiowave propagation](https://www.itu.int/rec/R-REC-P.2040/en)
- **[R3]** [3GPP TR 38.901 channel model reference](https://www.3gpp.org/dynareport/38901.htm)
- **[R4]** [FCC: opening the 6 GHz band for unlicensed use](https://www.fcc.gov/document/fcc-opens-6-ghz-band-wi-fi-and-other-unlicensed-uses-0)
- **[R5]** [FCC: very-low-power operation across the 6 GHz band](https://www.fcc.gov/document/fcc-opens-entire-6-ghz-band-very-low-power-device-operations)
- **[R6]** [SoapySDR project](https://github.com/pothosware/SoapySDR/wiki)
- **[R7]** [GNU Radio FFT documentation](https://wiki.gnuradio.org/index.php/FFT)
- **[R8]** [NVIDIA Sionna RT documentation](https://nvlabs.github.io/sionna/rt/)
- **[R9]** [NVIDIA Sionna open-source repository](https://github.com/NVlabs/sionna)

### Spatial inference and radio-map research

- **[I1]** [Bravenec et al., “Influence of Measured Radio Map Interpolation on Indoor Positioning Algorithms,” IEEE Sensors Journal, 2023](https://ieeexplore.ieee.org/document/10192546/)
- **[I2]** [Kumar et al., “Gaussian Process Regression for Fingerprinting Based Localization,” 2016](https://ora.ox.ac.uk/objects/uuid%3A3a634995-3754-4e4d-9eee-c11092a01a7a)
- **[I3]** [Sato, “Mitigating the Impact of Location Uncertainty on Radio Map-Based Predictive Rate Selection via Noisy-Input Gaussian Process,” 2025](https://arxiv.org/abs/2509.14710)
- **[I4]** [Lu et al., “Model-Aided Learning for Sparse Received Signal Strength Indicator Radio Map Estimation,” 2026](https://ieeexplore.ieee.org/document/11626128/)
- **[I5]** [Og et al., “Ray-Traced Augmentation for Signal Strength Based Localization,” 2026](https://arxiv.org/abs/2608.23901)
- **[I6]** [“A Tutorial on Learning-Based Radio Map Construction,” 2026](https://arxiv.org/abs/2603.17499)

### Data, optimization, and application architecture

- **[D1]** [Apache Arrow](https://arrow.apache.org/)
- **[D2]** [Apache Parquet](https://parquet.apache.org/)
- **[D3]** [OGC GeoPackage](https://www.ogc.org/standards/geopackage/)
- **[D4]** [Google OR-Tools CP-SAT](https://developers.google.com/optimization/cp/cp_solver)
- **[D5]** [Google OR-Tools min-cost flow](https://developers.google.com/optimization/flow/mincostflow)
- **[D6]** [Tauri 2 documentation](https://v2.tauri.app/)
- **[D7]** [`wgpu` documentation](https://wgpu.rs/)
- **[D8]** [WebAssembly Component Model](https://component-model.bytecodealliance.org/)

---

## Appendix H — Final architectural position

The best product is not “NetSpot, Acrylic, and TamoGraph in one UI.” That would reproduce their accumulated complexity and opaque assumptions.

The stronger design is:

```text
A trustworthy measurement substrate
    + capability-aware collectors
    + explicit spatial/time uncertainty
    + reproducible metric and interpolation engine
    + calibrated multi-tier propagation model
    + robust, explainable AP optimizer
    + approachable guided workflows
    + open project/export/plugin contracts
```

The first release should already solve the practical question:

> “Where does my connection degrade as I move, is Wi-Fi actually the cause, and where should I place or configure my AP?”

The long-term platform should answer the professional question:

> “Given this building, client population, applications, neighboring RF environment, infrastructure constraints, uncertainty, and budget, what deployment is most likely to meet the requirements—and what evidence proves it?”

That is the standard to build toward.

<!-- Reference-style shortcuts used throughout the traceability appendices. -->
[T1]: https://www.tamos.com/products/wifi-site-survey
[T2]: https://www.tamos.com/htmlhelp/tg/index.htm
[T3]: https://www.tamos.com/about/news
[T4]: https://www.tamos.com/download/main/tg
[T5]: https://www.tamos.com/products/wifi-site-survey
[T6]: https://pal.tamos.com/en/knowledgebase/article/whats-new-in-tamograph-version-8-0
[T7]: https://pal.tamos.com/en/announcements/article/whats-new-in-tamograph-site-survey-8-4
[N1]: https://www.netspotapp.com/
[N2]: https://www.netspotapp.com/features.html
[N3]: https://www.netspotapp.com/help/terms-definitions/
[N4]: https://www.netspotapp.com/help/how-do-i-start-my-survey/
[N5]: https://www.netspotapp.com/help/how-to-understand-netspot-report/
[N6]: https://www.netspotapp.com/help/how-can-i-adjust-the-appearance-of-my-heatmaps/
[N7]: https://www.netspotapp.com/help/what-is-the-difference-between-netspot-free-scanner-reporter-and-pro/
[N8]: https://www.netspotapp.com/help/how-to-perform-predictive-survey-with-netspot/
[N9]: https://www.netspotapp.com/help/netspot-android-manual/
[N10]: https://www.netspotapp.com/netspot-for-ios.html
[A1]: https://www.acrylicwifi.com/en/wifi-heatmaps/user-manual/
[A2]: https://www.acrylicwifi.com/en/wifi-heatmaps/
[A3]: https://www.acrylicwifi.com/en/wifi-heatmaps/requirements-and-compatibility/
[A4]: https://www.acrylicwifi.com/en/wifi-heatmaps/documentation-and-tutorials/
[P1]: https://learn.microsoft.com/en-us/windows/win32/nativewifi/wi-fi-access-location-changes
[P2]: https://learn.microsoft.com/en-us/windows/win32/nativewifi/native-wifi-reference
[P3]: https://npcap.com/guide/npcap-devguide.html
[P4]: https://wireless.docs.kernel.org/en/latest/en/users/documentation/iw.html
[P5]: https://www.kernel.org/doc/html/latest/networking/netlink_spec/nl80211.html
[P6]: https://developer.apple.com/documentation/corewlan
[P7]: https://developer.apple.com/documentation/networkextension/nehotspotnetwork
[P8]: https://developer.android.com/develop/connectivity/wifi/wifi-scan
[P9]: https://developer.android.com/develop/connectivity/wifi/wifi-rtt
[P10]: https://developers.google.com/ar/develop/fundamentals
[P11]: https://developer.apple.com/augmented-reality/roomplan/
[P12]: https://developer.apple.com/augmented-reality/arkit/
[P13]: https://www.radiotap.org/
[P14]: https://www.wireshark.org/docs/wsug_html/
[P15]: https://www.wireshark.org/docs/wsdg_html/
[O1]: https://www.kismetwireless.net/docs/readme/remotecap/remotecap/
[O2]: https://www.kismetwireless.net/docs/readme/datasources/datasources/
[O3]: https://www.kismetwireless.net/docs/readme/logging/kismetdb/
[M1]: https://software.es.net/iperf/
[M2]: https://www.rfc-editor.org/rfc/rfc4656
[M3]: https://www.rfc-editor.org/rfc/rfc5357
[M4]: https://www.rfc-editor.org/rfc/rfc3393
[M5]: https://www.rfc-editor.org/rfc/rfc8762
[R1]: https://www.itu.int/rec/R-REC-P.1238/en
[R2]: https://www.itu.int/rec/R-REC-P.2040/en
[R3]: https://www.3gpp.org/dynareport/38901.htm
[R4]: https://www.fcc.gov/document/fcc-opens-6-ghz-band-wi-fi-and-other-unlicensed-uses-0
[R5]: https://www.fcc.gov/document/fcc-opens-entire-6-ghz-band-very-low-power-device-operations
[R6]: https://github.com/pothosware/SoapySDR/wiki
[R7]: https://wiki.gnuradio.org/index.php/FFT
[R8]: https://nvlabs.github.io/sionna/rt/
[R9]: https://github.com/NVlabs/sionna
[I1]: https://ieeexplore.ieee.org/document/10192546/
[I2]: https://ora.ox.ac.uk/objects/uuid%3A3a634995-3754-4e4d-9eee-c11092a01a7a
[I3]: https://arxiv.org/abs/2509.14710
[I4]: https://ieeexplore.ieee.org/document/11626128/
[I5]: https://arxiv.org/abs/2608.23901
[I6]: https://arxiv.org/abs/2603.17499
[D1]: https://arrow.apache.org/
[D2]: https://parquet.apache.org/
[D3]: https://www.ogc.org/standards/geopackage/
[D4]: https://developers.google.com/optimization/cp/cp_solver
[D5]: https://developers.google.com/optimization/flow/mincostflow
[D6]: https://v2.tauri.app/
[D7]: https://wgpu.rs/
[D8]: https://component-model.bytecodealliance.org/

---

## Appendix I — Open-source source-code architectural audit

This appendix is the complete source-code architectural audit incorporated into the plan. Its subsystem-level dispositions are authoritative for reuse boundaries. If a future implementation decision changes a disposition, update both the relevant architecture section and this matrix through an ADR.

### Deconflict, Kismet, wifiheatmap, and Sionna RT

**Audit date:** 2026-08-30  
**Purpose:** decide, subsystem by subsystem, what RF Atlas should **ADOPT**, **INTEGRATE**, **CONTRIBUTE**, **REIMPLEMENT**, or treat as **REFERENCE-ONLY**.

### Executive decision

The audit changes the project from “build an open-source TamoGraph replacement and borrow ideas from adjacent tools” to a clearer **narrow-waist architecture**:

1. **RF Atlas owns the truth.** The canonical site, geometry, identity, observation, active-test, metric, analysis, requirement, plan, provenance, and report models are RF Atlas code and file formats.
2. **Kismet is integrated, never embedded.** It is the preferred first Linux monitor-mode, remote-radio, packet, BLE/Zigbee/SDR, and KismetDB source. RF Atlas communicates through authenticated APIs and files and maps all data into its own immutable evidence model.
3. **Sionna RT is adopted—but quarantined in an optional worker.** RF Atlas directly uses its Apache-2.0 path and radio-map solvers inside a pinned Python/Mitsuba/Dr.Jit environment. The desktop communicates with that worker through a versioned job/artifact boundary.
4. **Deconflict is a collaboration and interoperability target, not the computational foundation.** Its current planner is useful and well organized for its size, but its core propagation, throughput, interference, and optimization models are intentionally heuristic and remain in the browser application rather than a durable RF engine package.
5. **wifiheatmap is prior art and a clean-room behavior oracle only.** Its minimal point-survey flow is useful; its code, data model, platform adapters, interpolation, persistence, maintenance state, and GPLv2 coupling are not a rational foundation.
6. **Most of the valuable product remains ours.** Site-survey acquisition, active diagnostics, spatial statistics, uncertainty, hybrid calibration, Wi-Fi PHY/MAC/capacity, optimizer, requirements, explanations, history, reports, privacy, and cross-platform UX must be reimplemented.

At repository level, the one-line disposition is:

| Repository | Primary disposition | Exact role |
|---|---|---|
| Deconflict | **CONTRIBUTE / REFERENCE-ONLY** | Interchange partner and baseline planner; no runtime dependency |
| Kismet | **INTEGRATE** | External capture/sensor/logging subsystem |
| wifiheatmap | **REFERENCE-ONLY** | Minimal survey/TIN workflow oracle |
| Sionna RT | **ADOPT** | Optional high-fidelity propagation dependency inside an isolated worker |

This is not a failure to reuse code. It is the correct reuse shape. Hardware capture and electromagnetic ray tracing are high-cost specialized domains with mature implementations; our differentiating system is the evidence-preserving layer between measurement, physical prediction, Wi-Fi behavior, optimization, and explanation.

### Disposition vocabulary

Every subsystem in the two matrices below receives exactly one **primary** disposition:

- **ADOPT** — take the upstream package or code as a direct dependency inside a deliberately bounded RF Atlas component. We track its releases, notices, tests, and security updates.
- **INTEGRATE** — treat the upstream product as an external process, sensor, API, protocol, database, or file producer. We do not import its internal domain model into RF Atlas.
- **CONTRIBUTE** — implement the generic improvement upstream as the primary outcome. RF Atlas must not depend on acceptance for its core roadmap.
- **REIMPLEMENT** — build an RF Atlas-owned clean-room subsystem because it is product-defining, semantically incompatible, architecturally coupled, too shallow, stale, or license-sensitive.
- **REFERENCE-ONLY** — study behavior, source layout, algorithms, tests, and failure modes, but ship no dependency and copy no implementation/data. Recreate only independently specified behavior.

A row can mention a secondary upstream PR in its rationale, but the bold label remains its sole primary disposition.

### Audit criteria

The source trees were evaluated on:

- domain-model fit and semantic precision;
- module boundaries and dependency direction;
- process/concurrency/failure isolation;
- extension points and API stability;
- cross-platform and hardware reach;
- determinism, provenance, uncertainty, and reproducibility;
- test and CI structure;
- performance strategy;
- security/privilege model;
- license compatibility and distribution boundary;
- maintenance activity and concentration risk;
- cost of adapting the code versus implementing a stable RF Atlas contract.



### Source index and pinned audit baseline

This is a **static source-code audit**. It inspected the pinned repository trees and representative architectural files through GitHub’s source API. It did not claim that the repositories were locally built, benchmarked, or exercised against hardware. Runtime proofs are listed later as mandatory follow-up gates.

| Key | Repository / pinned revision | Representative inspected files |
|---|---|---|
| D-BASE | `sean-reid/deconflict@8d5dd0a4751550d4aab371fbd3f3b1a542ed5968` (2026-04-16) | `README.md`, monorepo/package manifests, CI workflow, repository tree |
| D-RF | same | `apps/web/src/lib/rf/propagation.ts`, `apps/web/src/lib/workers/optimizer-worker.ts`, `apps/web/src/lib/state/solver.svelte.ts` |
| D-DATA | same | `apps/web/src/lib/canvas/materials.ts`, `apps/web/src/lib/data/ap-models.ts`, vendor catalog files |
| D-CORE | same | `packages/geometry/src/interference-graph.ts`, `packages/channels/src/{overlap,throughput}.ts`, `packages/solver/src/algorithms/dsatur.ts` |
| D-STATE | same | `apps/web/src/lib/state/persistence.svelte.ts`, project/floor/AP/wall state files |
| K-BASE | `kismetwireless/kismet@2d25ad004e9216ac963c4f156e9077331717959c` (2026-08-30) | `LICENSE`, root tree/build workflows |
| K-CAP | same | `capture_framework.h`, `kis_datasource.h`, `datasourcetracker.h`, `kis_external_packet.h` |
| K-PIPE | same | `packetchain.h`, `packet.h`, `phy_80211.h`, `phy_80211_components_v2.h` |
| K-DATA | same | `devicetracker.h`, `entrytracker.h`, `eventbus.h`, `kis_databaselogfile.h` |
| K-API | same | `kis_net_beast_httpd.h`, `plugintracker.h` |
| W-BASE | `weimens/wifiheatmap@4602ce27903f3231567575feff3a1bffd962777b` (2021-08-21) | `README.md`, `CMakeLists.txt`, source/test tree, `LICENSE` |
| W-MODEL | same | `entries/{measurement,measurement_type,bss}.h`, `measurements.cpp`, `measurementcontroller.cpp` |
| W-COLLECT | same | `linuxscan.cpp`, `windowsscan.cpp`, `androidscan.cpp`, `iperf.cpp` |
| W-SPATIAL | same | `heatmap.cpp`, `document.cpp`, tests |
| S-BASE | `NVlabs/sionna-rt@bc0549155c7b782c7614a0ec06a0ac4e32b979ae` (2026-08-11) | `pyproject.toml`, `LICENSE`, `README.md`, package/test tree |
| S-SCENE | same | `src/sionna/rt/scene.py`, radio devices, scene objects, registry |
| S-PATH | same | `path_solvers/path_solver.py`, candidate/image/field solver package, `Paths` tests |
| S-MAP | same | `radio_map_solvers/radio_map_solver.py`, planar/mesh radio map classes, radio-map tests |
| S-MAT | same | `radio_materials/{radio_material_base,radio_material,itu_material}.py`, custom-material developer guide |
| S-ANT | same | `antenna_pattern.py`, antenna arrays, transmitter/receiver classes |

### Decision counts

| Disposition | Repository-module rows | RF Atlas subsystem rows |
|---|---|---|
| ADOPT | 8 | 3 |
| INTEGRATE | 13 | 7 |
| CONTRIBUTE | 11 | 4 |
| REIMPLEMENT | 7 | 154 |
| REFERENCE-ONLY | 41 | 4 |

The predominance of **REIMPLEMENT** in the product matrix is intentional: the reusable projects solve specialized capture or propagation problems, while RF Atlas’s unique value lies in the canonical evidence model and the analytical/decision layers around them.

### Target architecture after the audit

```text
                                      RF ATLAS
┌──────────────────────────────────────────────────────────────────────────────┐
│ Tauri desktop / mobile clients                                              │
│                                                                            │
│  Project + site + identity + observation + metric + requirement schemas     │
│  Survey state machines   Active tests   Spatial analytics   Fast RF solver  │
│  Wi-Fi PHY/MAC/capacity  Optimizer      History/reporting   Policy/security │
└───────────────────────────────┬──────────────────────────────────────────────┘
                                │ versioned ports; no foreign domain objects
                 ┌──────────────┼────────────────┬────────────────────┐
                 │              │                │                    │
                 ▼              ▼                ▼                    ▼
        Native collectors   Kismet adapter   Sionna job client   Deconflict bridge
        Win/macOS/Linux     REST/WS/KismetDB local/remote worker Open plan interchange
        Android/iOS agents  PCAPNG + sensors Python/Mitsuba/DrJit no runtime dependency
                 │              │                │
                 │              ▼                ▼
                 │          Kismet server     sionna-rt
                 │          capture helpers   PathSolver/RadioMapSolver
                 │
                 └──────────────► canonical ObservationEnvelope
```

#### Architectural invariants

- No UI state object is a persisted domain entity.
- No Kismet tracked field, Kismet device key, Sionna Scene object, Deconflict AP object, or wifiheatmap Measurement becomes canonical.
- External payloads are retained by content hash and mapped by versioned adapters.
- Raw evidence, normalized evidence, calibrated evidence, predictions, and recommendations are separate artifact classes.
- Every computed layer records engine/version/parameters/input hashes/seed and uncertainty metadata.
- The fast solver and Sionna solve the same RF Atlas `PropagationRequest` contract but may expose different capabilities and confidence.
- Sionna output used for Wi-Fi planning is path gain/per-transmitter RSS; final interference, PHY, airtime, capacity, association, and roaming are computed by RF Atlas.
- Kismet device aggregates are inventory/enrichment. Spatial survey values come from time-resolved observations correlated with the RF Atlas path.
- GPL code is not linked into a permissively licensed core. Bundling or distributing Kismet still requires a documented license/notice/source-compliance plan and legal review; a process boundary is an architectural strategy, not legal magic.

### Canonical integration contracts

#### ObservationEnvelope

The Kismet adapter, native collectors, mobile agents, spectrum sources, controller adapters, imports, and active-test agents all emit a common envelope with at least:

```text
observation_id             UUIDv7/content identity
session_id / snapshot_id   project acquisition context
collector_id               physical/logical source
source_kind + source_ref   native, Kismet API, KismetDB, PCAPNG, controller, import
captured_wall_time         UTC with source and precision
captured_monotonic_time    source-local monotonic time when available
clock_offset/drift/error   cross-source synchronization evidence
pose                       frame, x/y/z or lat/lon/alt, orientation, covariance
radio identity             network/AP/radio/BSSID/MLD/client relationships
channel                    band, primary, center frequencies, width, puncturing
signal                     RSSI/noise/SNR, units, per-chain values, calibration ID
frame/link fields          type, retry, rate/MCS/NSS, airtime inputs, IE/raw references
active-test fields         endpoint, route tier, protocol, offered load, distribution
quality flags              throttled, hopping, stale, inferred, malformed, saturated
raw source reference       content hash plus adapter/source schema version
```

Adapters may leave fields unknown. They may not fabricate a value to satisfy a common schema.

#### PropagationRequest / PropagationResult

The fast RF Atlas solver and Sionna worker share a conceptual contract:

```text
request:
  canonical scene revision and transform
  material/AP/antenna/client profile revisions
  transmitters and receiver surface/points
  frequency, bandwidth, temperature/noise assumptions
  enabled interactions and accuracy budget
  seed, sample budget, cache policy, requested outputs

result:
  per-transmitter path gain/RSS tiles or point paths
  optional CIR/CFR/AoA/AoD/delay/Doppler artifacts
  convergence/sample metadata and warnings
  engine/package/backend versions
  input/output content hashes
  uncertainty and unsupported-capability declarations
```

RF Atlas then applies channel coupling, activity, PHY/PER, association, airtime, capacity, policy, and optimizer logic.



### Repository architectural findings

#### Deconflict

##### What the code actually is

Deconflict is a young static SvelteKit application with three small reusable TypeScript packages (`solver`, `geometry`, `channels`) and most product behavior in `apps/web`. Its monorepo, TypeScript settings, unit tests, Playwright E2E/visual structure, lint/typecheck/build jobs, and solver benchmarks are a respectable foundation for a small web planner. The CI security audit and benchmark are currently non-blocking, however, and the project has not yet accumulated a long maintenance history.

The most consequential boundary is that **the RF propagation and AP placement optimizer are not durable engine packages**. They live under the web application, consume raster masks and browser-style data structures, and run in a Web Worker. The state system persists JSON and floor-plan PNG data URLs in localStorage and contains a backward-compatibility facade between older aggregate state and newer atoms. That is entirely reasonable for a compact offline web app, but it is exactly the coupling RF Atlas must avoid.

##### Propagation model

The current fast field is pragmatic rather than electromagnetic:

- an inverse-quartic signal-power curve is used for visual calculations;
- another squared linear falloff is used in optimizer scoring;
- a DDA-like raster ray march counts/translates material-mask crossings;
- cross-floor behavior is slab attenuation;
- a log-distance range helper uses fixed PL0/sensitivity/path-loss exponent assumptions;
- polar attenuation can be precomputed over angular bins and cached by quantized AP position.

This is a valuable instant-preview reference. It is not a link budget, deterministic ray solver, or calibrated stochastic model. The existence of different falloff approximations in render and optimizer paths also creates a model-consistency hazard.

##### Interference, channel, and capacity model

The base graph connects APs whose 3D interference spheres overlap. App code filters edges with wall/slab attenuation and solves bands independently with graph coloring. The channel overlap helper is intentionally simple. Throughput starts from hard-coded peak tables, applies a configurable/default MAC efficiency, divides by an effective co-channel count, and caps at ISP speed.

That can answer “show me a plausible starter plan.” It cannot support claims about client goodput, hidden nodes, uplink, CCA, OBSS, partial spectral coupling, retries, OFDMA, MLO, backhaul, device profiles, or robust capacity.

##### Optimizer

The three stages are real and understandable:

1. weighted Lloyd/k-means-style initialization using room density/signal deficit;
2. particle swarm optimization;
3. coordinate descent with exact raster rescoring.

The implementation precomputes wall crossings on a coarse grid, caps samples, and scores weighted average best signal. It uses `Math.random()` without an injected seed. There are no installation hard constraints, client assignments, channel/power/width decisions, uncertainty scenarios, Pareto alternatives, or independent full-model verifier.

##### Data catalogs

The AP catalog is useful as a discovery seed, and vendor files often include broad source comments/FCC IDs. But a field such as “typical indoor range” is not a stable vendor specification. The model omits patterns, mounting, regional variants, effective EIRP, per-radio SKU/firmware, measured uncertainty, and field-level licenses. The material catalog similarly compresses a material to one 5 GHz attenuation-per-meter value and a typical thickness.

**Decision:** collaborate on a small open planning interchange format, deterministic baseline behavior, and provenance. Do not build RF Atlas by incrementally turning this browser app into a capture, survey, physics, and optimization platform.

#### Kismet

##### Architectural core

Kismet has the strongest acquisition architecture in the set. Radio-specific capture helpers run separately, can use IPC or network links, expose probe/list/open/tune/hop/capture/spectrum callbacks, and isolate blocking hardware work. A datasource tracker manages drivers, source discovery, transactions, retries, channel hopping, remote connections, and packet/device ingestion.

Captured data flows through a typed, prioritized packet chain: post-capture, link-layer dissection, decryption, data dissection, classification, tracking, and logging. Packet work can be assigned by related-device hashes to reduce cross-thread locking. Packet components carry L1 signal/noise/frequency/channel/data-rate, per-antenna values, GPS and protocol-specific records. PHY handlers update a shared device tracker with signal, location, packet, frequency, encryption and seen-by evidence.

This is mature, high-value work we should not reproduce before RF Atlas has a compelling unsupported need.

##### Why not embed it

Kismet’s internal flexibility comes with deep coupling:

- pervasive lifetime globals/service lookup;
- runtime tracked-field registration and dynamic serializers;
- PHY handlers and device tracker types coupled to the server lifecycle;
- native plugins receive global-registry access and are ABI-coupled;
- a broad server/UI/WIDS/logging domain far beyond site surveying;
- GPLv2-or-later default licensing for the project unless specific files say otherwise.

A linked library or native plugin would put RF Atlas on Kismet’s internal ABI and license path. The correct boundary is the authenticated HTTP/REST/WebSocket surface, read-only KismetDB import, and PCAPNG evidence. Kismet remains independently operable and debuggable.

##### Integration caveat: aggregation is not a survey sample

Kismet’s device tracker is excellent for “what devices have been seen and what is their accumulated state?” A site survey needs “what did this exact sensor observe, on this channel, at this time and pose, under this dwell/clock/calibration context?” RF Atlas must ingest time-resolved packet/data records or source events and correlate them with its path. Device summaries are enrichment, not the primary radio map input.

##### Upstream boundary

Generic datasource, radio, parser, timestamp, capability, channel, and export improvements belong upstream. RF Atlas-specific project IDs, floor coordinates, active-test topology, spatial statistics, policy, or optimizer concepts do not.

**Decision:** Kismet is the preferred external Linux monitor/remote/multi-radio capture subsystem, with a strict adapter and pinned contract fixtures.

#### wifiheatmap

wifiheatmap is valuable because it demonstrates the smallest coherent open site-survey loop. Its architecture is straightforward: QML UI → `MeasurementController`/task queue → platform scanner or embedded libiperf → flat measurement collection → Delaunay interpolation → ZIP document.

That simplicity also establishes why it should not be extended into RF Atlas:

- a measurement is only a position, BSS, one of four metric types, and scalar;
- selected BSS values are combined by maximum;
- no timestamp, repeat distribution, sensor identity/calibration, pose uncertainty, path, test topology, or provenance is stored;
- interpolation is barycentric inside Delaunay triangles with no barriers, uncertainty, cross-validation or controlled extrapolation;
- iperf mode is a fixed short TCP test and performs unsafe process-wide stdout redirection;
- platform code is selected at compile time and contains obsolete/incomplete Windows/Android behavior;
- the project file is an unversioned-style ZIP of JSON and one image;
- the audited branch has not advanced since 2021 and the code is GPLv2.

**Decision:** preserve its workflow and numerical behavior as clean-room regression cases. Do not fork, link, vendor, or make upstream revival a roadmap dependency.

#### Sionna RT

##### Architectural core

Sionna RT is a genuine radio-propagation engine. Its `Scene` wraps Mitsuba scene geometry/parameters and owns radio objects, materials, transmitters/receivers, arrays, frequency, bandwidth, temperature, and rendering. Radio materials are Mitsuba BSDFs; the package includes frequency-dependent ITU-R P.2040 models and a documented abstract interface for custom sampling/evaluation/traversal. Antenna patterns and polarization models are callable and registry-backed.

`PathSolver` composes candidate generation, image-method geometry, and field calculation to produce coefficients, delays, arrival/departure angles and Doppler. It exposes LOS, specular/diffuse reflection, transmission, diffraction and edge-diffraction controls. `RadioMapSolver` launches sampled rays over a planar or mesh measurement surface and estimates cell-average path gain, RSS and generic SINR.

Its test tree covers candidate generation, image method, radio maps, CIR/CFR/taps, Doppler, rendering and utilities. Contributions require tests, lint, DCO sign-off and Apache-2.0 headers.

##### Important limits for indoor Wi-Fi planning

Sionna is a physical channel engine, not a Wi-Fi planner:

- scene frequency and bandwidth are global per run;
- a scene uses shared TX and RX arrays, which complicates mixed AP/client antenna types;
- radio-map SINR treats all transmitters in the run as interferers and does not know Wi-Fi channels, spectral masks, traffic duty cycle, CCA, association or scheduling;
- path calculations assume thin material slabs without angular deflection through thick volumetric objects;
- the radio-map solver is Monte Carlo and needs convergence/error handling;
- high-fidelity scenes require mesh/material compilation rather than a simple floor-plan mask;
- candidate/path discovery and differentiable field calculation have different gradient constraints; backpropagation requires evaluated loop mode, and calibration must refresh/validate path candidates;
- native Python/Mitsuba/Dr.Jit dependencies are substantial and version-sensitive.

These are not reasons to reject Sionna. They tell us exactly where to put the boundary.

##### Adoption shape

The package belongs in `rfatlas-sionna-worker`, a separately versioned optional component. The worker accepts canonical scene/job projections, emits content-addressed Arrow/Parquet or equivalent arrays plus a manifest, and can run locally on CPU/CUDA or remotely. RF Atlas should generally request per-transmitter path gain/RSS, then perform channel coupling, Wi-Fi PHY/MAC, capacity and policy evaluation itself.

Sionna’s differentiability makes measured calibration compelling. The upstream guide already demonstrates recovering material conductivity through gradient descent. RF Atlas can extend this into a constrained multi-location inverse problem, but it must own priors, sensor bias, train/validation splits, identifiability and uncertainty.

**Decision:** adopt the upstream solvers and material/antenna machinery inside an isolated worker; reimplement all product schemas, meshing/orchestration, Wi-Fi behavior, calibration governance, and UX.


### Repository-module disposition matrix

| Repository | Subsystem | Primary disposition | What it is | Decision rationale |
|---|---|---|---|---|
| Deconflict | Repository/application shell | REFERENCE-ONLY | SvelteKit static web application in a pnpm/Turborepo monorepo. | Useful product and CI reference, but not a durable RF Atlas host: browser-only, localStorage persistence, UI-owned computation, and a domain model too narrow for measured surveys. |
| Deconflict | Svelte UI and workflow components | REFERENCE-ONLY | Planner forms, floor switching, AP placement, wall editing, room labeling, and report interaction. | Borrow interaction ideas only. Adopting the component tree would import the current state model and pixel-mask assumptions into the product shell. |
| Deconflict | Canvas engine, camera, hit testing, pan/zoom, drag/place/select | REFERENCE-ONLY | Purpose-built Canvas 2D interaction layer. | Competent small-app implementation, but RF Atlas needs tiled rendering, measured overlays, uncertainty, 3D, large projects, and stable domain commands rather than component-driven mutations. |
| Deconflict | Floor-plan raster import, OCR, morphology, and boundary detection | REFERENCE-ONLY | Browser image pipeline using Tesseract.js and raster operations. | Good UX reference; production ingestion must preserve source scale, transforms, provenance, vector/PDF geometry, manual correction, and deterministic derived artifacts. |
| Deconflict | Svelte state atoms and compatibility facade | REFERENCE-ONLY | AP, floor-plan, wall, floor, and project state atoms with a backward-compatible facade. | The compatibility layer shows an evolving domain boundary. RF Atlas should use commands/events over a canonical core rather than make Svelte state authoritative. |
| Deconflict | LocalStorage persistence and v2→v3 migration | REFERENCE-ONLY | JSON in localStorage plus per-floor PNG data URLs; single migration function. | Not suitable for large/raw survey data, transactions, checksums, schema contracts, partial recovery, collaboration, or reproducible analyses. |
| Deconflict | @deconflict/geometry primitives | REFERENCE-ONLY | Small TypeScript geometry helpers and an AP-sphere interference graph. | Algorithms are easy to reproduce and too shallow to justify a cross-language dependency; the interference graph is geometric rather than an RF coupling model. |
| Deconflict | @deconflict/channels channel catalog | REFERENCE-ONLY | Band/channel/width/regulatory types and helper tables. | RF Atlas needs authoritative, dated, country-specific rules, DFS/AFC/indoor constraints, puncturing, center-segment validation, and provenance. |
| Deconflict | Channel-overlap calculation | REFERENCE-ONLY | 2.4 GHz rule-of-thumb and interval overlap elsewhere. | Too coarse for spectral masks, partial overlap, puncturing, adjacent-channel rejection, and device-dependent coupling. |
| Deconflict | @deconflict/solver graph-coloring algorithms | REFERENCE-ONLY | Greedy, Welsh–Powell, DSatur, backtracking, validation, worker wrapper. | Good baseline oracle, but simple enough to implement/test natively. Current DSatur minimizes neighbor color counts when no legal color exists rather than weighted RF cost. |
| Deconflict | Interference-graph construction | REFERENCE-ONLY | Edges originate from overlap of 3D AP interference spheres; app later filters with heuristic attenuation. | RF Atlas requires receiver-location coupling, CCA relationships, hidden-node asymmetry, channel masks, traffic, and client distributions. |
| Deconflict | Fast propagation engine | REFERENCE-ONLY | Inverse-quartic or log-distance range plus DDA wall-mask attenuation and slab loss. | Keep as a behavioral baseline only. It is a useful instant preview heuristic, not a calibrated link-budget or deterministic propagation engine. |
| Deconflict | Wall/floor material catalog | REFERENCE-ONLY | Six wall materials with one 5 GHz dB-per-meter value and typical thickness; floor presets. | Do not import as canonical data. Values are not field-level provenance records, do not vary fully by frequency/moisture/construction, and conflate attenuation with geometry. |
| Deconflict | AP model catalog | REFERENCE-ONLY | Hand-curated vendor/model/band, max power, width, streams, and typical indoor range. | Useful discovery list, but typical range is not a manufacturer invariant; no antenna patterns, regulatory variants, firmware/radio SKUs, uncertainty, or per-field source ledger. |
| Deconflict | Throughput model | REFERENCE-ONLY | Hard-coded peak rates, a default 50% MAC efficiency, equalized effective co-channel contender count, and ISP cap. | Insufficient for MCS/PER, RU/OFDMA scheduling, airtime, client capability, uplink, retries, contention domains, hidden nodes, MLO, or load. |
| Deconflict | Channel-plan orchestration | REFERENCE-ONLY | Per-band graph coloring with fixed assignments and heuristic wall/slab filtering. | Use as a regression baseline. RF Atlas needs joint channel/width/power/placement optimization and explicit objective evidence. |
| Deconflict | Three-stage AP optimizer | REFERENCE-ONLY | Density/deficit-weighted Lloyd initialization, PSO, then coordinate descent in a Web Worker. | Objective is weighted best-signal coverage, with capped raster samples and unseeded Math.random. Missing hard constraints, capacity, SINR, channels, power, robustness, Pareto alternatives, and reproducibility. |
| Deconflict | Heatmap rendering | REFERENCE-ONLY | Canvas image rendering over its fast signal field. | Rendering is coupled to the heuristic field. RF Atlas needs numeric tiles, unknown masks, uncertainty, provenance, accessible palettes, layer algebra, and exportable values. |
| Deconflict | PDF reporting | REFERENCE-ONLY | Client-side jsPDF report generation. | Use layout ideas only. Reports must be reproducible from versioned analysis artifacts, templates, policies, sources, and exact parameters. |
| Deconflict | Open project/interchange schema | CONTRIBUTE | No durable interoperable project file is currently the architectural center. | Propose a versioned, unit-safe, source-provenanced planning interchange format. Keep it a narrow common denominator rather than RF Atlas’s full survey schema. |
| Deconflict | Deterministic optimizer controls | CONTRIBUTE | Current worker uses Math.random and heuristic sampling. | Upstream seed injection, stable sampling, objective-component output, and reproducibility tests are broadly useful and independently reviewable. |
| Deconflict | Weighted conflict/channel solver | CONTRIBUTE | Current graph solver treats conflicts mostly as unweighted edges. | Contribute optional edge costs and deterministic tie-breaking without importing RF Atlas’s full PHY/MAC model. |
| Deconflict | Catalog provenance and uncertainty | CONTRIBUTE | Catalog comments cite broad sources but records lack per-field evidence metadata. | Add source URL/reference, retrieval date, jurisdiction/SKU, measured-vs-datasheet status, uncertainty, and license fields before any cross-project reuse. |
| Deconflict | Measured-survey import and prediction residuals | CONTRIBUTE | Project currently centers on predictive planning. | Only pursue after maintainer alignment. A small CSV/JSON point import and residual view can make Deconflict a useful open planner without duplicating RF Atlas acquisition. |
| Kismet | Kismet server as a whole | INTEGRATE | Long-running C++ wireless capture, parsing, tracking, logging, and API server. | Run as a separately installed or explicitly bundled process. Never make its in-memory model or GPL server core an RF Atlas library dependency. |
| Kismet | External capture helper binaries | INTEGRATE | Per-radio helper processes isolate hardware access and communicate over IPC/TCP. | This is high-value, mature hardware support. Let Kismet own adapters, privileges, channel control, and failure recovery for Kismet-backed sensors. |
| Kismet | Datasource probing, open/close, retry, and capability handling | INTEGRATE | Datasource tracker coordinates builders, local helpers, remote sources, channel tuning, and retries. | Consume exposed datasource state/capabilities; do not clone the service-locator implementation. |
| Kismet | Channel hopping and source control | INTEGRATE | Kismet can tune/hop sources and exposes configuration/control through its datasource subsystem. | RF Atlas should request survey policies and record actual dwell schedules; Kismet remains the actuator for its radios. |
| Kismet | External datasource wire protocol | INTEGRATE | Versioned framed protocol with protobuf/optimized/MessagePack generations under the capture framework. | Use transitively through official Kismet components. Avoid copying protocol implementation into permissive core; prefer documented server APIs and fixtures. |
| Kismet | Packet chain | REFERENCE-ONLY | Typed multistage POSTCAP→dissect→decrypt→classify→track→log pipeline with threaded affinity. | Excellent architecture study, but deeply coupled to Globalreg, packet components, PHY handlers, and Kismet lifetime services. |
| Kismet | Packet component and L1 metadata model | REFERENCE-ONLY | Component container includes GPS, RSSI/noise/frequency/channel/data-rate and per-antenna measurements. | Use it to ensure RF Atlas loses no evidence, but define an immutable, versioned canonical ObservationEnvelope rather than copy dynamic C++ types. |
| Kismet | 802.11 frame/IE dissection | INTEGRATE | Kaitai/manual parsers populate dot11 packet/device records, security, country, QBSS, power, HT/VHT, extension tags, and more. | Consume parsed fields and raw PCAP. RF Atlas still needs an independent parser for native collectors, validation, modern EHT/MLO semantics, and provenance. |
| Kismet | Non-Wi-Fi PHY parsers | INTEGRATE | Bluetooth, BLE sniffers, Zigbee, SDR/ADS-B and other source families feed the same server. | Use as optional expansion data sources. Preserve source-specific evidence and do not force all radios into Wi-Fi semantics. |
| Kismet | Device tracker and seen-by aggregation | INTEGRATE | Thread-safe device map, PHY-specific records, signal/location histories, names/tags, and datasource relationships. | Useful enrichment and live inventory; not authoritative for survey samples because aggregation can erase per-observation timing and uncertainty. |
| Kismet | Dynamic tracked-field/serialization registry | REFERENCE-ONLY | Runtime field IDs, builders, serializers, transforms, and object pooling. | Flexible inside Kismet but too dynamic and globally coupled for RF Atlas’s stable public schema. |
| Kismet | Internal event bus | REFERENCE-ONLY | Asynchronous channel subscription inside the server. | Consume externally exposed events where stable; do not link to or mirror the internal bus types. |
| Kismet | HTTP/REST/WebSocket server and authentication | INTEGRATE | Boost.Beast routes, role-authenticated endpoints, JWT/API credentials, WebSocket routes, CORS, static content. | Primary live integration boundary. Add version/capability negotiation and treat every response as source data mapped into RF Atlas types. |
| Kismet | KismetDB SQLite log | INTEGRATE | Unified database records devices, packets, data, sources, alerts, snapshots and JSON/normalized fields. | Primary offline/import boundary. Import through a versioned adapter, retain original database hash, and never mutate the source file. |
| Kismet | PCAPNG streaming/export | INTEGRATE | Packet export is suitable for lossless reprocessing and protocol-tool interoperability. | Use for packet evidence and parser regression; correlate with RF Atlas path/time records instead of treating GPS as the only spatial frame. |
| Kismet | GPS, signal histories, and source metadata | INTEGRATE | Kismet associates packets/devices with GPS and datasource metadata. | Map into evidence with clock/spatial uncertainty. Indoor x/y/z remains owned by RF Atlas. |
| Kismet | Alerts/WIDS | INTEGRATE | Alert tracker and PHY logic generate security/behavior events. | Expose as an optional evidence layer, clearly separate from coverage diagnosis. Do not rebuild WIDS in the first product phases. |
| Kismet | Native shared-object plugin ABI | REFERENCE-ONLY | Plugins receive global registry access and are ABI-coupled to Kismet internals. | Too powerful and unstable for RF Atlas integration; a Kismet upgrade could break a linked plugin. Prefer external APIs. |
| Kismet | External HTTP plugin/proxy mechanisms | REFERENCE-ONLY | Kismet supports external web integration paths. | Evaluate only if the normal API cannot expose required data. Avoid making RF Atlas run inside Kismet’s UI or route lifecycle. |
| Kismet | Kismet web UI | REFERENCE-ONLY | Operational capture/device interface. | Keep available for sensor debugging; RF Atlas needs site/project/survey semantics and should not fork the UI. |
| Kismet | Modern 802.11 metadata completeness | CONTRIBUTE | The inspected parser exposes rich classic/HT/VHT and extension-tag data, but explicit EHT/MLO/puncturing semantics were not established by this audit. | After a fixture-based gap check, contribute generic EHT/MLO/channel-width/puncturing fields and tests upstream rather than maintain a Kismet-only patch. |
| Kismet | Stable observation/export contract | CONTRIBUTE | Current APIs expose Kismet’s rich tracked model and logs rather than an RF-survey-neutral envelope. | Propose additive fields/endpoints only where broadly useful: capture timestamp source, monotonic/UTC mapping, clock uncertainty, dwell schedule, calibration ID, per-chain signal. |
| Kismet | Capture helper reliability and hardware support | CONTRIBUTE | Generic radio bugs and new supported devices belong with Kismet. | Upstream fixes that can be reproduced independently; keep RF Atlas adapter work out of Kismet unless it benefits other clients. |
| Kismet | RF Atlas Kismet adapter | REIMPLEMENT | Version negotiation, API/KismetDB/PCAP mapping, deduplication, health, and project association. | This adapter is product-specific and must be owned/tested in RF Atlas even though the source system is integrated. |
| wifiheatmap | Qt/QML application shell | REFERENCE-ONLY | Qt 5.12/C++17 desktop and Android UI with a flat source layout. | No strategic reuse: stale platform APIs, GPLv2, tight QObject/QML coupling, and a model far below the planned scope. |
| wifiheatmap | Manual point-survey workflow | REFERENCE-ONLY | Open map, select scan/connected/iperf mode, click a position, measure, repeat, select APs, render. | Excellent minimum-workflow reference and usability baseline; reimplement around immutable sample batches, quality feedback, and cancellation. |
| wifiheatmap | BSS and measurement model | REFERENCE-ONLY | A measurement is position + BSS + one of four metric types + scalar value. | Missing timestamps, repeats/distributions, sensor/calibration, test topology, uncertainty, orientation, path, packet evidence, and identity relations. |
| wifiheatmap | MeasurementController/TaskQueue orchestration | REFERENCE-ONLY | Compile-time platform branches dispatch scans and iperf work into mutable collections. | Use only to enumerate failure modes. RF Atlas requires capability-driven collectors, structured cancellation, progress, provenance, and isolation. |
| wifiheatmap | Linux nl80211 privileged helper | REFERENCE-ONLY | pkexec-launched helper with a small line protocol for scan/connected measurements. | Do not reuse. Design a narrow audited helper or use Kismet; authenticate messages, bound resources, and preserve capability evidence. |
| wifiheatmap | Windows WLAN collector | REFERENCE-ONLY | Windows Native WLAN API implementation; channel handling is incomplete and the asynchronous measure path appears defective. | Useful API breadcrumb only. Build and test a modern native collector against a shared contract. |
| wifiheatmap | Android collector | REFERENCE-ONLY | QtAndroid/JNI WifiManager scan implementation from the pre-modern throttling/permission era. | Reimplement natively in Kotlin with current permissions, scan throttling, link-layer stats, foreground/background rules, and capability honesty. |
| wifiheatmap | Embedded libiperf wrapper | REFERENCE-ONLY | Fixed one-second omit and three-second TCP run, with bps/retransmit outputs and process-wide stdout redirection. | Use upstream iperf3 as a separately managed engine or audited library adapter; own test topology, modes, warm-up, metadata, and safety. |
| wifiheatmap | Delaunay/TIN heatmap | REFERENCE-ONLY | Barycentric linear interpolation within the convex hull of point samples. | Keep as a numerical baseline. Reimplement TIN alongside IDW/RBF/kriging/GP, barriers, unknown masks, cross-validation, and uncertainty. |
| wifiheatmap | Selected-BSS aggregation | REFERENCE-ONLY | Multiple selected BSS values are combined by maximum signal/value. | Useful explicit baseline, but RF Atlas must distinguish network, AP, radio, BSSID, MLD, band, redundancy, and metric-specific aggregation. |
| wifiheatmap | ZIP project persistence | REFERENCE-ONLY | ZIP containing data.json and mapimage.png without a durable public schema/migration system. | The container idea is sound; the implementation is not. Build a checksummed, versioned bundle with immutable raw evidence and derived artifacts. |
| wifiheatmap | Unit tests and fixtures | REFERENCE-ONLY | Tests cover models, document, and heatmap behavior. | Translate concepts into clean-room golden cases; do not vendor GPL fixtures/code into a permissive core without an explicit license decision. |
| wifiheatmap | Upstream fork/contribution program | REFERENCE-ONLY | Last audited branch commit is from August 2021. | Do not make delivery depend on revival. File isolated correctness fixes only if the maintainer re-engages; otherwise cite it as prior art and move on. |
| Sionna RT | sionna-rt Python package | ADOPT | Apache-2.0 package, version 2.0.1 at the audited commit, built on Mitsuba 3 and Dr.Jit. | Make it a pinned direct dependency of an optional RF Atlas worker, not of the Rust/Tauri desktop process. |
| Sionna RT | Mitsuba/Dr.Jit native runtime | ADOPT | Hardware-accelerated differentiable scene traversal and vectorized computation; exact versions are pinned upstream. | Adopt transitively only inside the worker environment. Isolate native dependency conflicts and record backend/build versions in every result. |
| Sionna RT | Scene and scene-object runtime | ADOPT | Mitsuba-backed scene with objects, materials, transmitters/receivers, frequency, bandwidth, temperature, arrays, rendering, and parameter traversal. | Use as the worker’s execution scene, not RF Atlas’s canonical project model. |
| Sionna RT | ITU and custom radio-material engine | ADOPT | Frequency-dependent ITU-R material models plus extensible Mitsuba BSDF interface, thickness, scattering and cross-polarization parameters. | Use for high-fidelity runs. Canonical material identity, priors, provenance, uncertainty, and measured calibration remain RF Atlas-owned. |
| Sionna RT | Antenna patterns, polarization, and arrays | ADOPT | Registries and callable field patterns for isotropic, dipole, 3GPP and custom antennas; planar arrays and polarization models. | Use in the worker. Add adapters from open tabulated patterns and handle Sionna’s shared-scene array constraints explicitly. |
| Sionna RT | PathSolver candidate generation/image method/field calculation | ADOPT | Computes paths, coefficients, delays, AoA/AoD and Doppler with LOS, reflection, diffuse scattering, transmission and diffraction options. | Do not reimplement. Pin versions, expose all solver parameters, and validate canonical scenes. |
| Sionna RT | Paths, CIR/CFR/taps/Doppler outputs | ADOPT | Structured channel outputs with an upstream unit-test suite. | Preserve full outputs for advanced analysis; derive Wi-Fi metrics outside Sionna. |
| Sionna RT | RadioMapSolver | ADOPT | Monte Carlo cell-average path gain, RSS, and thermal-noise-plus-transmitter SINR on planar or mesh surfaces. | Use path gain or per-transmitter RSS as primary evidence. RF Atlas must recompute channel-aware interference rather than present Sionna SINR as final Wi-Fi SINR. |
| Sionna RT | Notebook previewer and renderer | REFERENCE-ONLY | pythreejs interactive preview and Mitsuba rendering utilities. | Useful for worker debugging and research notebooks; not the product renderer or floor-plan editor. |
| Sionna RT | Example scenes and upstream tests | REFERENCE-ONLY | Unit tests cover candidates, image method, radio maps, CIR/CFR, Doppler, rendering and utilities. | Run upstream tests in the worker build and use concepts for adapter fixtures, but maintain independent RF Atlas acceptance scenes. |
| Sionna RT | RF Atlas canonical scene/material/AP/antenna schemas | REIMPLEMENT | Sionna objects are execution-engine types with one scene frequency/bandwidth and shared arrays. | Own stable schemas and generate one or more Sionna scenes/jobs as a projection. |
| Sionna RT | Floor-plan/BIM-to-Mitsuba scene compiler | REIMPLEMENT | Sionna expects scene geometry and materials, not a TamoGraph-style editable 2D floor plan. | Build deterministic extrusion/meshing, opening handling, slab geometry, material assignment, coordinate transforms, diagnostics, and scene hashes. |
| Sionna RT | RF Atlas worker/service wrapper | REIMPLEMENT | Process lifecycle, capability handshake, resource limits, job cancellation, caching, RPC, artifacts, and remote execution are outside Sionna. | Own this boundary. Prefer local socket/stdio control plus Arrow IPC/Parquet artifacts; support container/venv and remote Slurm backends. |
| Sionna RT | Per-band/per-channel run orchestration | REIMPLEMENT | Scene frequency and bandwidth are global, and radio-map SINR assumes all transmitters in a run interfere. | Partition/rerun by frequency, channel, bandwidth, antenna group and scenario; combine path gains with RF Atlas spectral/airtime models. |
| Sionna RT | Wi-Fi PHY/MAC/capacity semantics | REIMPLEMENT | Sionna models propagation/channel response, not CSMA/CA, MCS/PER, association, retries, OFDMA scheduling, MLO, roaming or goodput. | Keep deterministic radio physics and Wi-Fi network behavior as separate composable layers. |
| Sionna RT | Inverse calibration orchestration | REIMPLEMENT | Upstream demonstrates trainable material conductivity with evaluated Dr.Jit loops. | Build constrained multi-point calibration, priors, adapter bias, train/validation splits, candidate-path refresh, identifiability checks, and uncertainty around the adopted solver. |
| Sionna RT | Generic material/antenna import extensions | CONTRIBUTE | The package intentionally exposes custom material and antenna extension interfaces. | Contribute broadly useful tabulated pattern/material importers, tests, and documentation rather than carry a private engine patch. |
| Sionna RT | Indoor Wi-Fi examples and radio-map convergence diagnostics | CONTRIBUTE | Current engine is radio-generic; RF Atlas needs disciplined multi-band indoor workflows. | Upstream generic examples for seeded convergence, material calibration, and per-frequency runs; keep CSMA/channel-planning logic out of Sionna. |
| Sionna RT | Per-transmitter antenna-array flexibility | CONTRIBUTE | The audited scene API uses a shared TX array and shared RX array for the scene. | Open an issue/design discussion before coding. If accepted, contribute a generic engine capability; otherwise group RF Atlas jobs by array/pattern. |
### Complete RF Atlas subsystem disposition matrix

Every planned RF Atlas subsystem below has one primary disposition. The `Implementation source` column names the upstream boundary where applicable; it does not make that upstream data model canonical.

| ID | RF Atlas subsystem | Primary disposition | Implementation source | Why |
|---|---|---|---|---|
| FND-001 | Desktop shell and application lifecycle | REIMPLEMENT | RF Atlas | Tauri/Rust product concern; none of the four provides the required cross-platform, privileged-helper-aware shell. |
| FND-002 | Canonical domain entities and value objects | REIMPLEMENT | RF Atlas | Must remain stable across collectors, Sionna versions, Kismet fields, UI frameworks, and file migrations. |
| FND-003 | Units, coordinate frames, transforms, and time model | REIMPLEMENT | RF Atlas | Core evidentiary semantics; external projects use incompatible spatial/time assumptions. |
| FND-004 | Project bundle and manifest | REIMPLEMENT | RF Atlas | Checksummed/versioned bundle for maps, raw observations, analyses, scenes, reports, and source ledger. |
| FND-005 | Schema migration and recovery | REIMPLEMENT | RF Atlas | Requires transactional migrations, validation, backup, forward-compatibility policy, and partial artifact recovery. |
| FND-006 | Commands, queries, operation log, and analysis job graph | REIMPLEMENT | RF Atlas | Needed for reproducibility, undo/redo, collaboration, cancellation, and dependency invalidation. |
| FND-007 | Source/license/provenance ledger | REIMPLEMENT | RF Atlas | Every AP/material/antenna/map/catalog entry needs field-level evidence and redistribution status. |
| FND-008 | Network/AP/radio/BSSID/MLD/client identity graph | REIMPLEMENT | RF Atlas | Kismet can enrich but cannot own project identity or user-confirmed groupings. |
| FND-009 | Metric registry and semantic versioning | REIMPLEMENT | RF Atlas | Prevents RSSI, SINR, throughput, uncertainty, and vendor metrics from silently changing meaning. |
| FND-010 | Open export guarantees | REIMPLEMENT | RF Atlas | CSV/JSON/GeoPackage/Parquet/PCAPNG/report exports must not depend on any vendor or repo object model. |
| MAP-001 | Project/site/building/floor hierarchy | REIMPLEMENT | RF Atlas | Broader than Deconflict and must connect maps, surveys, snapshots, plans, policies, and reports. |
| MAP-002 | Raster/PDF/SVG/DXF plan import | REIMPLEMENT | RF Atlas | Preserve transforms, layers, provenance, and deterministic derivatives; Deconflict is a raster UX reference only. |
| MAP-003 | Scale calibration and georeferencing | REIMPLEMENT | RF Atlas | Needs uncertainty, multiple control points, CRS metadata, and later recalibration without corrupting observations. |
| MAP-004 | Floor-plan and wall/opening editor | REIMPLEMENT | RF Atlas | Vector geometry is canonical; raster masks are derived acceleration artifacts. |
| MAP-005 | Multi-floor 2.5D/3D building model | REIMPLEMENT | RF Atlas | Must represent slabs, voids, floor transforms, ceiling heights, material layers, and vertical paths. |
| MAP-006 | LiDAR/room-scan import | REIMPLEMENT | RF Atlas | Requires platform-specific reconstruction, alignment, confidence, simplification, and user correction. |
| MAP-007 | Outdoor map/GPS support | REIMPLEMENT | RF Atlas | Requires CRS, map licensing/cache policy, GPS accuracy, paths, and large-area tiling. |
| MAP-008 | Photos, notes, annotations, and exclusions | REIMPLEMENT | RF Atlas | Project evidence and planning constraints are first-class domain records. |
| MAP-009 | Canonical material catalog | REIMPLEMENT | RF Atlas + Sionna adapter | Own identity, frequency curves/priors/provenance; project to Sionna materials for high-fidelity runs. |
| MAP-010 | Canonical AP/radio model catalog | REIMPLEMENT | RF Atlas | Separate product, radio, regulatory SKU, firmware, power limits, chains, bands, ports, patterns, and uncertainty. |
| MAP-011 | Canonical antenna pattern library/import | REIMPLEMENT | RF Atlas + Sionna adapter | Own open tabulated representation and license ledger; compile to Sionna callable patterns. |
| MAP-012 | Regulatory/channel rule catalog | REIMPLEMENT | RF Atlas | Dated jurisdiction-specific rules, DFS/AFC, indoor/outdoor, EIRP/PSD, widths, center segments and puncturing. |
| INS-001 | Nearby network table | REIMPLEMENT | Native collectors + Kismet | UI/query semantics are ours; observations may originate from Kismet or native OS APIs. |
| INS-002 | Live signal timeline | REIMPLEMENT | RF Atlas | Needs per-source sampling context, missing-data semantics, bands/BSSIDs, smoothing controls, and calibration. |
| INS-003 | Channel and overlap views | REIMPLEMENT | RF Atlas | Must distinguish advertised occupancy, spectral overlap, observed airtime, CCA busy time, retries, and non-Wi-Fi energy. |
| INS-004 | Network/AP/radio comparison | REIMPLEMENT | RF Atlas | Depends on canonical identity, metric semantics, time windows, and evidence quality. |
| INS-005 | 802.11 information-element explorer | REIMPLEMENT | RF Atlas parser + Kismet enrichment | Need raw bytes, parser version, contradictions, EHT/MLO support, and links to derived capabilities. |
| INS-006 | Guided channel recommendation | REIMPLEMENT | RF Atlas | Recommendation must cite measured airtime/interference, regulatory constraints, clients, uncertainty, and alternative tradeoffs. |
| INS-007 | Current-connection diagnostician | REIMPLEMENT | RF Atlas | Correlates local link state, AP observations, gateway/LAN/Internet tests, roaming and controller evidence. |
| CAP-001 | Collector capability contract | REIMPLEMENT | RF Atlas | A stable contract must express permissions, scan type, monitor mode, bands, timing, signal provenance, channel control and calibration. |
| CAP-002 | Windows/macOS/Linux managed-mode collectors | REIMPLEMENT | RF Atlas | Needed for zero-extra-hardware launch-and-scan UX; Kismet cannot replace all native OS paths. |
| CAP-003 | Linux monitor-mode capture | INTEGRATE | Kismet | Use Kismet first for mature adapters, privilege handling, hopping and frame capture; add native RF Atlas capture only behind a later evidence-based gate. |
| CAP-004 | Kismet live sensor connection | INTEGRATE | Kismet REST/WebSocket | Strict adapter with capability/version negotiation; never expose Kismet JSON directly to UI/domain code. |
| CAP-005 | KismetDB offline import | INTEGRATE | KismetDB | Read-only, version-aware importer retaining source hash and raw fields. |
| CAP-006 | PCAPNG import/export and packet evidence | INTEGRATE | Kismet/standard tooling | Accept Kismet streams/files while preserving an engine-independent PCAPNG evidence path. |
| CAP-007 | Kismet remote sensors | INTEGRATE | Kismet | Fastest route to distributed Linux radio sensors; RF Atlas correlates them with project position/time/calibration. |
| CAP-008 | RF Atlas native remote-sensor protocol | REIMPLEMENT | RF Atlas | Needed for managed-mode/mobile/active-test/path data and stable product semantics not present in Kismet. |
| CAP-009 | Generic channel sweep/hopping scheduler | REIMPLEMENT | RF Atlas | Own policy, fairness, survey intent and measurement-bias accounting; delegate actuation to each collector. |
| CAP-010 | Independent 802.11 parser and normalization | REIMPLEMENT | RF Atlas | Required for native sources, differential testing, modern standards, stable schema and non-Kismet operation. |
| CAP-011 | BLE/Zigbee/SDR/other-radio acquisition | INTEGRATE | Kismet | Optional evidence plane; defer custom radio capture and use Kismet’s mature source ecosystem. |
| CAP-012 | GPS/SLAM/path-to-observation time fusion | REIMPLEMENT | RF Atlas | Project coordinate frames, interpolation, clock offsets and covariance are outside Kismet/wifiheatmap. |
| CAP-013 | Clock synchronization and uncertainty | REIMPLEMENT | RF Atlas | Store wall/monotonic clocks, source, drift, offset estimates and uncertainty for cross-sensor fusion. |
| CAP-014 | Capture health and dropped-data telemetry | REIMPLEMENT | RF Atlas + Kismet mapping | Unify source queue drops, dwell coverage, permission errors, throttling and sensor disconnects. |
| CAP-015 | Adapter calibration profiles | REIMPLEMENT | RF Atlas | Per-device/band/orientation offsets and uncertainty need versioned measurement provenance. |
| CAP-016 | Raw capture retention/redaction policy | REIMPLEMENT | RF Atlas | Project/user policy controls payload retention, MAC hashing, secrets, packet truncation and export. |
| CAP-017 | Generic Kismet metadata and EHT/MLO improvements | CONTRIBUTE | Kismet | Upstream only after fixture-based gap confirmation; keep product mappings in RF Atlas. |
| SUR-001 | Point survey | REIMPLEMENT | RF Atlas; wifiheatmap reference | Immutable sample batches, dwell/progress, repetitions, quality feedback, cancellation and active/passive synchronization. |
| SUR-002 | Manual continuous-path survey | REIMPLEMENT | RF Atlas | Path timestamps and covariance, pace guidance, resampling, gaps, turn/body effects, and replay. |
| SUR-003 | AR/SLAM path survey | REIMPLEMENT | RF Atlas mobile | Platform-specific pose graph, map alignment, relocalization, drift correction and uncertainty. |
| SUR-004 | GPS survey | REIMPLEMENT | RF Atlas | Accuracy filtering, speed/heading, CRS conversion, path smoothing, large-area tiling and pause/resume semantics. |
| SUR-005 | Snapshot survey | REIMPLEMENT | RF Atlas | Repeatable timed evidence bundle for before/after comparison and incident capture. |
| SUR-006 | Guided route | REIMPLEMENT | RF Atlas | Coverage-aware next-point suggestions, missed-area detection, accessibility and route state. |
| SUR-007 | Team survey | REIMPLEMENT | RF Atlas | Assignment, device calibration, conflict resolution, offline merge and provenance. |
| SUR-008 | Remote/fixed sensor survey | REIMPLEMENT | RF Atlas orchestration + Kismet sensors | Project lifecycle, fixed pose/calibration, windows, event triggers and source fusion are ours. |
| SUR-009 | Robotic survey | REIMPLEMENT | RF Atlas | Robot pose/telemetry adapter, safe route constraints, synchronized sampling and replay. |
| SUR-010 | AP-on-a-stick experiment | REIMPLEMENT | RF Atlas | Experimental AP state, temporary placement, candidate comparison, stabilization and result attribution. |
| SUR-011 | Survey quality scorecard | REIMPLEMENT | RF Atlas | Spatial density, temporal coverage, channel dwell, calibration, motion, uncertainty and active-test validity. |
| PAS-001 | RSSI/noise/SNR aggregation | REIMPLEMENT | RF Atlas | Metric-specific robust statistics, distributions, source normalization and minimum sample evidence. |
| PAS-002 | SIR/SINR and channel-overlap analysis | REIMPLEMENT | RF Atlas | Combine spectral coupling, path gain, observed activity and receiver assumptions; do not reuse Deconflict or raw Sionna SINR. |
| PAS-003 | Airtime, CCA busy, and utilization | REIMPLEMENT | RF Atlas; Kismet evidence | Separate captured frame airtime, AP QBSS, adapter counters, CCA and spectrum energy with trust levels. |
| PAS-004 | Retries, frame mix, rates, and protocol health | REIMPLEMENT | RF Atlas; Kismet enrichment | Per-link/direction/frame semantics and capture-bias warnings. |
| PAS-005 | AP/BSSID/radio/MLD grouping | REIMPLEMENT | RF Atlas identity graph | Explicit evidence and user overrides, not name/OUI heuristics alone. |
| PAS-006 | Protocol and client-capability layers | REIMPLEMENT | RF Atlas | Independent parsing/modeling for HT/VHT/HE/EHT, widths, NSS, security, roaming, MLO and limitations. |
| PAS-007 | Roaming and resilience layers | REIMPLEMENT | RF Atlas | Secondary AP coverage, overlap quality, failure scenarios, sticky-client risk and transition evidence. |
| PAS-008 | Data-quality and uncertainty layers | REIMPLEMENT | RF Atlas | Show support distance, density, source disagreement, calibration and confidence—not false continuous certainty. |
| ACT-001 | Multi-tier latency | REIMPLEMENT | RF Atlas | Gateway, LAN target and Internet/application probes with protocol, payload, route and attribution metadata. |
| ACT-002 | TCP/UDP/QUIC throughput | REIMPLEMENT | RF Atlas + upstream iperf3 | Own safe orchestration, server discovery, direction, streams, duration, warm-up, CPU limits and result schema. |
| ACT-003 | Link-state telemetry | REIMPLEMENT | RF Atlas native collectors | Association, PHY rate, MCS/NSS where available, retransmits, power save and route/interface changes. |
| ACT-004 | Roaming active test | REIMPLEMENT | RF Atlas | Timestamp BSSID changes, interruption, packet loss, reassociation/auth stages, triggers and device capability. |
| ACT-005 | Loss, jitter, and delay distributions | REIMPLEMENT | RF Atlas | Preserve distributions and sample timing rather than one scalar. |
| ACT-006 | Bufferbloat/load interaction | REIMPLEMENT | RF Atlas | Coordinated load and latency with LAN-vs-WAN separation and safety limits. |
| ACT-007 | DNS/application probes | REIMPLEMENT | RF Atlas | Pluggable, consented probes with endpoint, cache, TLS and network path context. |
| ACT-008 | QoS/WMM validation | REIMPLEMENT | RF Atlas | Traffic-class generation and capture/controller corroboration. |
| ACT-009 | Multi-client capacity test | REIMPLEMENT | RF Atlas | Coordinated agents, offered load, fairness, scheduling, airtime and endpoint capacity validation. |
| ACT-010 | Test-topology validation | REIMPLEMENT | RF Atlas | Detect Wi-Fi-on-both-ends, slow server NIC/CPU, VPN, ISP bottleneck, route changes and endpoint contention. |
| SPA-001 | TIN/Delaunay interpolation | REIMPLEMENT | RF Atlas; wifiheatmap oracle | Implement independently with exact unknown/convex-hull semantics and numerical tests. |
| SPA-002 | IDW and radial-basis interpolation | REIMPLEMENT | RF Atlas | Metric-aware tuning, spatial indexing, barriers and validation. |
| SPA-003 | Kriging/Gaussian-process models | REIMPLEMENT | RF Atlas | Uncertainty-aware optional methods with scalable approximations and reproducible hyperparameters. |
| SPA-004 | Barrier/interior-aware distance | REIMPLEMENT | RF Atlas | Use vector geometry/topology rather than Deconflict’s raster crossing count. |
| SPA-005 | Extrapolation and unknown masks | REIMPLEMENT | RF Atlas | Explicit support radius/hull/interior rules; never silently paint unsupported areas. |
| SPA-006 | Spatial cross-validation | REIMPLEMENT | RF Atlas | Blocked folds, residual maps, method selection and leakage prevention. |
| SPA-007 | Uncertainty decomposition | REIMPLEMENT | RF Atlas | Sensor, time, path, interpolation, model and scenario components retained separately. |
| SPA-008 | AP localization | REIMPLEMENT | RF Atlas | Robust probabilistic estimate with floor/wall model, multi-sensor bias and confidence region. |
| SPA-009 | Temporal change detection | REIMPLEMENT | RF Atlas | Align snapshots, account for sampling support and distinguish configuration, load and environmental changes. |
| SPA-010 | Tiled computation and layer algebra | REIMPLEMENT | RF Atlas | Numeric layers are immutable artifacts; rendering and formulas operate over tiles with provenance. |
| SPE-001 | Spectrum source adapters | INTEGRATE | Kismet-supported hardware and future vendor SDKs | Use existing source ecosystem; RF Atlas normalizes calibrated sweeps and capabilities. |
| SPE-002 | Live spectrum visualization | REIMPLEMENT | RF Atlas | Waterfall, occupancy, max/average/percentiles, sweep quality and synchronized Wi-Fi context. |
| SPE-003 | Survey-correlated spectrum maps | REIMPLEMENT | RF Atlas | Tie sweeps to path/time/calibration and preserve frequency resolution. |
| SPE-004 | Interferer classification | REIMPLEMENT | RF Atlas | Evidence-ranked classification with unknown state, confidence and raw spectral excerpts. |
| SPE-005 | Spectrum calibration | REIMPLEMENT | RF Atlas | Device response, reference source, frequency-dependent offsets, noise floor and uncertainty. |
| PRE-001 | Fast empirical/multi-wall preview solver | REIMPLEMENT | RF Atlas; Deconflict reference | CPU/GPU-friendly deterministic link budget with vector barriers, antenna gain, frequency and calibration. |
| PRE-002 | 2D/3D scene compiler and meshing | REIMPLEMENT | RF Atlas | Canonical geometry to engine-specific meshes with openings, layers, diagnostics and hashes. |
| PRE-003 | Material execution adapter | REIMPLEMENT | RF Atlas → Sionna | Map canonical priors/curves/material IDs to Sionna ITU/custom BSDFs without making engine types canonical. |
| PRE-004 | Client profiles | REIMPLEMENT | RF Atlas | Receiver sensitivity, bands, NSS, antennas, orientation/body loss, uplink power, standards and roaming behavior. |
| PRE-005 | AP/radio execution profiles | REIMPLEMENT | RF Atlas | Per-radio power/EIRP, pattern, mounting, chains, channels, limits and uncertainties. |
| PRE-006 | Antenna execution adapter | REIMPLEMENT | RF Atlas → Sionna | Compile canonical tabulated/analytic patterns and orientations; group jobs if engine arrays differ. |
| PRE-007 | Sionna worker process and RPC | REIMPLEMENT | RF Atlas | Own lifecycle, environment pinning, capabilities, job control, cancellation, artifacts, resource limits and remote execution. |
| PRE-008 | High-fidelity path solver | ADOPT | Sionna PathSolver | Use upstream LOS/reflection/transmission/diffraction/channel path machinery instead of rebuilding radio ray tracing. |
| PRE-009 | High-fidelity radio-map solver | ADOPT | Sionna RadioMapSolver | Use upstream Monte Carlo path gain/RSS field calculation; treat its generic SINR as intermediate only. |
| PRE-010 | Measured/predicted residual fusion | REIMPLEMENT | RF Atlas | Bias/residual models, spatial validation, confidence and versioned calibration datasets are product differentiation. |
| PRE-011 | Inverse material/power/bias calibration | REIMPLEMENT | RF Atlas around Sionna | Constrained hierarchical fitting, identifiability, refresh cycles and holdout validation. |
| PRE-012 | Prediction/convergence uncertainty | REIMPLEMENT | RF Atlas | Combine Monte Carlo variance, geometry/material priors, sensor residuals and scenario sensitivity. |
| PRE-013 | Prediction cache and job scheduler | REIMPLEMENT | RF Atlas | Content-addressed inputs, tile reuse, invalidation, local/remote resources and cancellation. |
| PRE-014 | Per-band/channel/antenna Sionna orchestration | REIMPLEMENT | RF Atlas | Multiple engine runs and post-combination are required because Sionna scene frequency/bandwidth/arrays are shared. |
| PRE-015 | Generic tabulated antenna/material support | CONTRIBUTE | Sionna RT | Contribute engine-generic importers/tests where upstream accepts them; keep RF Atlas catalog semantics local. |
| PHY-001 | Channel/width/center-frequency model | REIMPLEMENT | RF Atlas | Standards- and jurisdiction-aware, including 320 MHz and puncturing. |
| PHY-002 | Spectral masks and interference coupling | REIMPLEMENT | RF Atlas | Frequency-dependent overlap and receiver rejection, not binary graph edges. |
| PHY-003 | MCS/PER/link adaptation model | REIMPLEMENT | RF Atlas | PHY generation, bandwidth, NSS, GI, coding, SNR/PER curves and implementation margin. |
| PHY-004 | Bidirectional link budget | REIMPLEMENT | RF Atlas | Client uplink power/sensitivity/antenna/body loss must be modeled separately from AP downlink. |
| PHY-005 | Wi-Fi 6/6E/7, OFDMA, MLO and puncturing semantics | REIMPLEMENT | RF Atlas | Network behavior layer is outside all four reusable cores. |
| PHY-006 | CSMA/CA airtime and contention | REIMPLEMENT | RF Atlas | Use measured activity and scenario load; model contention domains, overhead and retries. |
| PHY-007 | Hidden nodes, OBSS and spatial reuse | REIMPLEMENT | RF Atlas | Directional/asymmetric sensing and BSS color/OBSS-PD behavior cannot be reduced to AP sphere overlap. |
| PHY-008 | Client association and load balancing | REIMPLEMENT | RF Atlas | Per-client candidates, policy, sticky behavior, capacity and controller evidence. |
| PHY-009 | Expected goodput and capacity | REIMPLEMENT | RF Atlas | Compose PHY, PER, airtime, scheduling, load, backhaul and active calibration. |
| PHY-010 | Roaming decision/transition model | REIMPLEMENT | RF Atlas | Device profile, thresholds, 802.11k/v/r/MLO, security and measured transition behavior. |
| OPT-001 | Candidate generation | REIMPLEMENT | RF Atlas; Deconflict reference | Geometry, mounting rules, cabling/exclusions, existing APs, uncertainty and user constraints. |
| OPT-002 | Coverage placement | REIMPLEMENT | RF Atlas | Hard coverage/redundancy/uplink requirements with verified propagation coefficients. |
| OPT-003 | Capacity placement | REIMPLEMENT | RF Atlas | Client demand, airtime, association, backhaul and failure cases—not only best RSSI. |
| OPT-004 | Joint channel/width/power/radio plan | REIMPLEMENT | RF Atlas | Coupled mixed discrete/continuous problem with full-model verification. |
| OPT-005 | Multi-floor and installation constraints | REIMPLEMENT | RF Atlas | Mounting, PoE/cable, AP count/cost, zones, aesthetics, existing hardware and regulatory limits. |
| OPT-006 | Robust/scenario planning | REIMPLEMENT | RF Atlas | Material uncertainty, doors/occupancy, AP failure, demand and device-profile scenarios. |
| OPT-007 | Pareto alternatives and explainability | REIMPLEMENT | RF Atlas | Return distinct plans, binding constraints, expected deltas and evidence—not a single opaque optimum. |
| OPT-008 | Deterministic seeds and reproducibility | REIMPLEMENT | RF Atlas | Every stochastic stage and coefficient build is seeded/versioned; also contribute generic seed support to Deconflict. |
| OPT-009 | Incremental replanning | REIMPLEMENT | RF Atlas | Warm starts and localized recomputation after map, AP, client, requirement or measurement changes. |
| OPT-010 | AP-on-a-stick planner integration | REIMPLEMENT | RF Atlas | Use experiment results to update priors and compare candidate sites. |
| OPT-011 | Nonlinear verification and survey validation loop | REIMPLEMENT | RF Atlas | Re-evaluate candidate plans with full propagation/PHY/capacity and close the loop with measured surveys. |
| OPT-012 | Deconflict reproducibility and weighted-solver enhancements | CONTRIBUTE | Deconflict | Small generic upstream improvements; RF Atlas does not depend on acceptance. |
| REQ-001 | Policy-as-data requirements | REIMPLEMENT | RF Atlas | Versioned thresholds, scopes, metric definitions, unknown handling and provenance. |
| REQ-002 | Compliance maps/tables | REIMPLEMENT | RF Atlas | Evaluate exact numeric artifacts with coverage denominator and unknown semantics. |
| REQ-003 | Quality profiles | REIMPLEMENT | RF Atlas | Home/voice/warehouse/high-density/custom profiles are transparent policy bundles. |
| REQ-004 | Root-cause engine | REIMPLEMENT | RF Atlas | Evidence graph across RF, airtime, active tests, topology, clients and controller data. |
| REQ-005 | Recommendation evidence and counterfactuals | REIMPLEMENT | RF Atlas | Every recommendation states observations, assumptions, confidence, predicted effect and validation action. |
| CMP-001 | Snapshots and difference maps | REIMPLEMENT | RF Atlas | Align datasets, preserve support/uncertainty and distinguish raw versus normalized change. |
| CMP-002 | Temporal RF observatory | REIMPLEMENT | RF Atlas | Time-series retention, schedules, events, replay and anomaly/change attribution. |
| REP-001 | Report builder | REIMPLEMENT | RF Atlas | Template/policy-driven sections, evidence tables, maps, recommendations, appendices and redaction. |
| REP-002 | Reproducible report build | REIMPLEMENT | RF Atlas | Manifest pins inputs, software versions, parameters, source hashes and generated artifacts. |
| REP-003 | Deconflict planning interchange | CONTRIBUTE | Deconflict + RF Atlas | Define a narrow open planning schema upstream; implement RF Atlas bridge independently if agreement fails. |
| UX-001 | Desktop information architecture | REIMPLEMENT | RF Atlas | Inspector, Survey, Analyze, Plan, Compare and Report workflows around one project model. |
| UX-002 | 2D numeric tile/canvas renderer | REIMPLEMENT | RF Atlas | Large maps, numeric sampling, layer composition, selection, unknown/uncertainty and GPU acceleration. |
| UX-003 | 3D building/RF view | REIMPLEMENT | RF Atlas | Visualize geometry, floors, APs, rays/volumes and uncertainty; Sionna preview remains a research tool. |
| UX-004 | Mobile survey application | REIMPLEMENT | RF Atlas | Native permissions, scan limitations, SLAM, offline sessions, haptics/audio guidance and desktop pairing. |
| UX-005 | Accessibility and color semantics | REIMPLEMENT | RF Atlas | Colorblind-safe palettes, numeric inspection, patterns, keyboard/screen-reader support and print behavior. |
| UX-006 | Capability honesty panel | REIMPLEMENT | RF Atlas | Show exactly what each OS/adapter can observe, infer or cannot measure. |
| UX-007 | Troubleshooting narrative | REIMPLEMENT | RF Atlas | Evidence-linked, uncertainty-aware explanation rather than generic tips. |
| UX-008 | Expert controls and presets | REIMPLEMENT | RF Atlas | Progressive disclosure with inspectable defaults and reproducible advanced parameters. |
| UX-009 | Large-project interaction/performance | REIMPLEMENT | RF Atlas | Virtualization, tiling, background jobs, cancellation and incremental updates. |
| UX-010 | Deconflict planner UI as behavioral reference | REFERENCE-ONLY | Deconflict | Use screenshots/workflow comparisons; no component inheritance. |
| UX-011 | wifiheatmap point-workflow oracle | REFERENCE-ONLY | wifiheatmap | Retain a clean-room behavioral checklist for the smallest useful survey flow. |
| UX-012 | Kismet operations UI | REFERENCE-ONLY | Kismet | Keep for sensor administration/debugging; do not fork into the product. |
| UX-013 | Sionna notebook preview | REFERENCE-ONLY | Sionna RT | Use for research validation; RF Atlas owns production rendering. |
| EXT-001 | Internal plugin SDK | REIMPLEMENT | RF Atlas | Stable capability-scoped contracts; do not copy Kismet’s global-registry ABI. |
| EXT-002 | Out-of-process connector protocol | REIMPLEMENT | RF Atlas | Versioned, authenticated, resource-bounded and language-neutral for Kismet, Sionna and future tools. |
| EXT-003 | Controller integrations | REIMPLEMENT | RF Atlas | Timestamped read-only evidence adapters with trust level; no silent overwrite of field data. |
| EXT-004 | Offline-first collaboration and merge | REIMPLEMENT | RF Atlas | Operation/event merge with immutable evidence, conflicts and provenance. |
| SEC-001 | Privilege separation | REIMPLEMENT | RF Atlas; Kismet integration | Use Kismet’s packaged helper model for Kismet radios and a separately audited minimal helper for native capture. |
| SEC-002 | Plugin/worker sandbox and resource limits | REIMPLEMENT | RF Atlas | Separate users/processes, file/network grants, CPU/GPU/memory/time limits and explicit trust. |
| SEC-003 | Remote sensor authentication | REIMPLEMENT | RF Atlas | Mutual authentication, enrollment, rotation, least privilege, replay resistance and audit logs. |
| SEC-004 | Project encryption and secret handling | REIMPLEMENT | RF Atlas | At-rest keys, export redaction, credential separation and recoverability. |
| SEC-005 | SBOM, license and dependency policy | REIMPLEMENT | RF Atlas | Enforce GPL process boundaries, Apache notices, catalog redistribution, advisories and signed releases. |
| SEC-006 | Data minimization and privacy UX | REIMPLEMENT | RF Atlas | MAC/SSID/payload controls, retention windows, user warnings and share-safe exports. |
| OPS-001 | Packaging, updater and optional component manager | REIMPLEMENT | RF Atlas | Core app, Kismet connector detection, Sionna worker/venv/container and model/data packs must be independently manageable. |
| OPS-002 | Observability and diagnostics bundle | REIMPLEMENT | RF Atlas | Structured logs, job traces, collector health, source versions and privacy-safe support export. |
| OPS-003 | Performance, caching and cancellation | REIMPLEMENT | RF Atlas | Content-addressed derived artifacts, tile caches, bounded queues, backpressure and cancellation. |
| OPS-004 | Local GPU/CPU and remote HPC/Slurm jobs | REIMPLEMENT | RF Atlas + Sionna worker | Same job contract across local and remote execution with artifact hashes and resource provenance. |
| TST-001 | Unit/property tests | REIMPLEMENT | RF Atlas | Domain invariants, units, geometry, statistics, channel rules and policies. |
| TST-002 | Parser corpus, fuzzing and differential tests | REIMPLEMENT | RF Atlas + Kismet comparison | Raw frame corpus across malformed/classic/HE/EHT cases; compare but do not depend on Kismet interpretations. |
| TST-003 | Propagation canonical scenes | REIMPLEMENT | RF Atlas + Sionna | Analytic free-space, single slab, reflection, diffraction and multi-floor cases with engine/version tolerances. |
| TST-004 | Hardware/RF lab validation | REIMPLEMENT | RF Atlas | Calibrated attenuation, orientation, device bias, hopping, packet loss and active-test topology. |
| TST-005 | Competitor black-box harness | REIMPLEMENT | RF Atlas | Repeatable behavioral comparison against TamoGraph/NetSpot/Acrylic under legitimate licenses. |
| TST-006 | Kismet/Sionna adapter contract fixtures | REIMPLEMENT | RF Atlas | Pinned API responses, KismetDB files, PCAPNG, scene bundles and worker outputs across supported versions. |
| TST-007 | Reproducibility/performance/reliability suite | REIMPLEMENT | RF Atlas | Seeds, hashes, interruption/recovery, large projects, clock drift, sensor loss, worker crash and resource limits. |
| TST-008 | Run Sionna upstream tests in worker CI | ADOPT | Sionna RT test contract | Execute the upstream package suite for pinned environments in addition to independent RF Atlas tests. |

### Required adapter designs

#### `rfatlas-kismet-adapter`

The adapter should have four independently testable inputs:

1. **live control/status** — authenticated server version, datasource inventory/capabilities, channel state, health, and configuration;
2. **live evidence** — supported WebSocket/event/stream endpoints or packet export;
3. **offline evidence** — read-only KismetDB import;
4. **lossless packet evidence** — PCAPNG stream/file import.

Rules:

- negotiate and record Kismet version and field capabilities before ingestion;
- map into `ObservationEnvelope`; preserve unknown source fields in a raw content-addressed sidecar;
- retain datasource UUID, interface, driver, hopping configuration, current channel, signal units and GPS/timestamp provenance;
- deduplicate replayed API/log/PCAP records without discarding independently observed packets;
- separate device aggregate snapshots from packet/time-series observations;
- correlate indoor pose by timestamp in RF Atlas rather than writing RF Atlas coordinates into Kismet’s model;
- treat adapter conversion as deterministic and golden-fixture tested;
- fail closed on unknown schema versions for fields that affect metric semantics, while retaining raw import capability;
- expose Kismet’s operational UI as a diagnostic link, not an embedded product pane;
- package Kismet separately or with full GPL compliance materials. Do not silently install setuid/capability helpers.

#### `rfatlas-sionna-worker`

Recommended boundary:

```text
control plane: versioned protobuf/JSON-RPC over local socket/stdio or mTLS remote channel
bulk plane:    content-addressed scene bundles and Arrow IPC/Parquet/NumPy artifacts
execution:     pinned uv/venv or OCI image; CPU/LLVM and CUDA capability probes
remote:        same immutable job bundle submitted through a Slurm adapter
```

Required job types:

- `validate_scene` — geometry/material/transform diagnostics;
- `path_query` — selected transmitter/receiver path details;
- `radio_map` — path gain or RSS over planar/mesh receiver surface;
- `convergence_sweep` — sample/depth/seed sensitivity;
- `calibration_step` — evaluated-loop loss/gradient for selected parameters;
- `render_debug` — optional research artifact, never canonical product rendering.

Every result manifest records:

- RF Atlas scene/profile revisions and hashes;
- Sionna/Mitsuba/Dr.Jit/Python/backend versions;
- device/driver when relevant;
- frequency/bandwidth/noise/temperature;
- interactions, depth, samples, seed, synthetic-array mode and loop mode;
- start/end/cancellation state and resource usage;
- convergence diagnostics and warnings such as thin-material approximation;
- output shape, units, no-data mask, checksum and numerical precision.

#### Deconflict interoperability

Do not begin with a code fork. Begin with a design issue proposing a small `OpenRFPlan`-style schema containing only:

- coordinate frame and unit;
- floors and transforms;
- vector boundaries/walls/slabs/openings with material references;
- AP/radio placements and basic channel/power settings;
- room/zone demand weights;
- requirements relevant to predictive planning;
- catalog references with provenance;
- extensions map with namespaced ownership.

Do **not** put RF Atlas observations, packet evidence, active tests, uncertainty tiles, operation logs or reports into the shared minimum. RF Atlas can attach those through its own bundle and project a planning view into the interchange format.

Contribution order:

1. schema RFC and import/export fixtures;
2. deterministic seed and stable sampling;
3. objective-component output and weighted conflicts;
4. catalog provenance fields;
5. optional measured point import/residual view, only after architectural agreement.

#### wifiheatmap clean-room oracle

Record, without copying implementation:

- point placement and measurement interaction sequence;
- connected-network versus scan versus iperf modes;
- selected-BSS aggregation behavior;
- Delaunay interpolation inside the sample hull;
- project round-trip expectations;
- cancellation/error and sparse-point behavior.

Use those observations to build an independent baseline fixture. The purpose is compatibility knowledge and regression coverage, not source reuse.



### Upstream contribution portfolio

#### Deconflict — contribute selectively

| Priority | Proposed upstream work | Acceptance boundary |
|---|---|---|
| 1 | Versioned open planning interchange schema and fixtures | Generic planner data only; no RF Atlas runtime dependency |
| 2 | Seeded PRNG, deterministic sampling, objective decomposition | Small, independently testable solver improvement |
| 3 | Weighted conflict costs and deterministic tie-breaking | Preserve current simple mode; add optional weights |
| 4 | Per-field catalog provenance, uncertainty and license metadata | No wholesale copying of vendor data into RF Atlas |
| 5 | Measured point import and prediction-residual layer | Only after maintainer agrees on domain direction |

Avoid upstreaming the entire RF Atlas survey engine, Kismet adapter, Sionna worker, PHY/MAC model or optimizer. That would turn two projects into coupled partial copies.

#### Kismet — contribute generic capture facts

| Priority | Proposed upstream work | Gate |
|---|---|---|
| 1 | Fixture-based audit of HE/EHT/MLO/puncturing and 6 GHz fields | Confirm actual gaps against captures before filing patches |
| 2 | Additive capture timestamp/clock/dwell/calibration metadata where broadly applicable | Must fit Kismet’s source model, not RF Atlas projects |
| 3 | Stable example/fixtures for external live observation consumption and KismetDB versions | Coordinate with maintainers before proposing a new API |
| 4 | New radio/driver and capture-helper reliability fixes | Reproducible independently of RF Atlas |
| 5 | Per-chain signal and source-capability fidelity | Add only when hardware/source can supply it honestly |

Do not propose floor plans, heatmaps, survey policies, AP optimization or active tests as Kismet core features.

#### Sionna RT — contribute generic propagation capabilities

| Priority | Proposed upstream work | Gate |
|---|---|---|
| 1 | Tabulated antenna-pattern importer with polarization/unit tests | Generic radio use, not Wi-Fi-vendor catalog logic |
| 2 | Indoor multi-frequency/radio-map convergence example | Preserve radio-generic framing |
| 3 | Measured material-calibration example with holdout validation | Generic inverse-problem methodology |
| 4 | Discuss per-transmitter/per-receiver array support | Design issue first; likely cross-cutting engine change |
| 5 | Reproducible scene/material conversion examples | Avoid making RF Atlas’s schema an upstream dependency |

Sionna requires tests, lint, Apache headers and DCO-signed commits. Keep RF Atlas’s channel/airtime/capacity layer out of the ray tracer.

#### wifiheatmap — no strategic contribution dependency

A single issue summarizing reproducible correctness problems could be appropriate if the maintainer is active. Do not allocate roadmap-critical features or build an RF Atlas fork on the assumption of revival.



### Major risks introduced or retired by these decisions

| Risk | Effect | Mitigation |
|---|---|---|
| Kismet API/schema drift | Live/import adapters break or change semantics | Pin supported versions, capability probe, golden API/KismetDB/PCAP fixtures, raw-field preservation |
| GPL distribution mistakes | License/compliance exposure | No linking, separate packaging boundary, notices/source offer/SBOM, legal review before bundled distribution |
| Kismet aggregation bias | Incorrect spatial heatmaps | Prefer time-resolved packet/data evidence; record channel dwell and correlate pose by timestamp |
| Sionna dependency weight | Install failures and desktop instability | Isolated optional worker, pinned environment, capability checks, no import in core process |
| Sionna compute cost | Slow interactive planning | Fast RF Atlas solver for iteration; Sionna for verification/calibration/selected tiles; content-addressed cache and remote jobs |
| Sionna generic SINR misuse | False Wi-Fi conclusions | Request path gain/per-TX RSS and recompute spectral/activity/PHY/MAC semantics in RF Atlas |
| Sionna global frequency/arrays | Excess runs or wrong mixed-radio model | Job partitioning/grouping, explicit warnings, cache, upstream design discussion |
| Deconflict model copied as “physics” | False accuracy and future rewrite | Reference-only baseline; independently specified fast solver and validation scenes |
| Deconflict bus factor/young code | Dependency abandonment | No runtime dependency; contributions are optional and narrowly scoped |
| wifiheatmap stale/GPL code | Maintenance and license burden | Reference-only clean-room behavior tests |
| Catalog provenance gaps | Redistribution or accuracy failure | RF Atlas field-level source ledger; do not bulk-import heuristic ranges/material values |
| Canonical model overreach | Adapter impedance and migration pain | Narrow immutable evidence schema, extension fields, explicit unknowns, versioned projections |

### Runtime validation gates still required

The static audit is enough for architecture decisions, but adoption/integration is not complete until these gates pass:

#### Kismet gate

- build/install a pinned release on a supported Linux sensor;
- exercise managed source discovery, monitor-mode hopping, disconnect/retry, remote capture and permission failure;
- collect the same controlled beacons into live API, KismetDB and PCAPNG;
- verify timestamps, RSSI/noise/frequency/channel, per-chain data, GPS/source UUID and dropped-packet telemetry;
- measure live adapter latency/backpressure and offline import throughput;
- test upgrade fixtures across at least two supported Kismet revisions;
- complete distribution/license review.

#### Sionna gate

- build pinned CPU and CUDA worker images/environments;
- run upstream tests and RF Atlas canonical free-space/slab/reflection/diffraction scenes;
- establish deterministic tolerance across seeds/backends;
- benchmark path and radio-map jobs at representative home/office geometry sizes;
- validate material frequency updates and thin-wall limitations;
- prove cancellation, crash recovery, cache hashes and remote artifact round trips;
- compare selected predictions to measured surveys with holdout data.

#### Deconflict gate

- run unit/E2E/visual tests at the pinned commit;
- export numerical outputs for controlled plans and compare render versus optimizer falloff;
- verify channel/regulatory tables and catalog records against primary sources;
- contact maintainer with the interchange RFC before implementing a bridge;
- require deterministic fixture round trips before declaring interoperability.

#### wifiheatmap gate

- build only in an isolated research environment if needed;
- record the point-flow and interpolation behavior with synthetic points;
- do not make product code or release engineering depend on a successful build.



### Revised implementation sequence

1. **Freeze the canonical contracts first.** Implement unit-safe geometry/time/identity/ObservationEnvelope and the source ledger before any adapter.
2. **Build the Kismet offline adapter before live streaming.** A versioned KismetDB/PCAPNG fixture is deterministic and exposes mapping gaps without concurrency noise.
3. **Add Kismet live capability/status and packet/device evidence.** Keep aggregate inventory and time-resolved observations separate.
4. **Build native managed-mode collectors.** This preserves the “launch it on my laptop and walk around” product even when Kismet is absent.
5. **Deliver the point/continuous survey and honest TIN/IDW maps.** Use wifiheatmap only as a behavioral comparison.
6. **Implement the fast vector multi-wall solver and measured residual framework.** Use Deconflict as a baseline oracle, not a code dependency.
7. **Build the Sionna worker proof.** Canonical scene → mesh/material projection → path gain tile → manifest → RF Atlas layer.
8. **Implement Wi-Fi-aware spectral/PHY/MAC/capacity composition.** Never display Sionna generic SINR as the final result.
9. **Add calibration and uncertainty.** Fit sensor bias/Tx power/material groups with spatial holdout validation.
10. **Implement the explainable optimizer.** Use deterministic candidate generation, CP-SAT/flow or equivalent, full-model verification, robust scenarios and Pareto output.
11. **Submit narrow upstream contributions.** Kismet facts, Sionna generic engine capabilities, and Deconflict interoperability/reproducibility—each independently useful.
12. **Add remote/HPC execution and multi-radio expansion.** Keep the same ports/artifacts rather than introducing a second architecture.

### Final recommendation

Use a **federated open-source strategy**, not a fork-first strategy:

- `rfatlas-core`, project schema, survey engine, analytics, Wi-Fi behavior, optimizer and UX: **REIMPLEMENT and own**.
- Kismet server/capture ecosystem: **INTEGRATE**.
- Sionna RT path/material/antenna/radio-map engine: **ADOPT inside an isolated worker**.
- Deconflict: **CONTRIBUTE a narrow interchange/reproducibility layer; otherwise reference**.
- wifiheatmap: **REFERENCE-ONLY**.

This preserves the two genuinely expensive upstream achievements—radio capture and high-fidelity propagation—without inheriting the wrong product model, license boundary, stale platform layer, or heuristic planner semantics.


---

### Plan v0.2 integration note

Version 0.2 incorporates the 2026-08-30 source-code audit of Deconflict, Kismet, wifiheatmap, and Sionna RT. The principal changes are: Kismet-first external capture integration, Sionna RT adoption in an isolated high-fidelity worker, Deconflict contribution/interchange-only status, wifiheatmap reference-only status, and explicit canonical-model/foreign-schema boundaries across architecture, testing, roadmap, backlog, and ADRs.
