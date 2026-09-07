#!/usr/bin/env python3
"""Versioned acceptance catalog; test IDs are individual evidence obligations."""
import json
from pathlib import Path

KISMET = "2d25ad004e9216ac963c4f156e9077331717959c"
SIONNA = "bc0549155c7b782c7614a0ec06a0ac4e32b979ae"


def build():
    gates = []

    def add(identifier, kind, versions, hardware, checks, procedure, refs, pins=None, **extra):
        gates.append(dict(id=identifier, status="NOT_RUN", evidence_kind=kind,
                          required_versions=versions.split(), required_hardware=hardware.split(),
                          checks=checks.split(), procedure=procedure, requirements=refs,
                          pinned_versions=pins or {}, **extra))

    add("kismet-live", "hardware_runtime", "kyberia kismet_source_revision kismet_build adapter os",
        "cpu radio_model radio_driver radio_firmware",
        "version_detect authentication_success authentication_rejection capability_discovery datasource_enumeration local_packets remote_packets timestamp_source timestamp_precision monotonic_unknown clock_uncertainty channel_frequency dwell hopping locking noise_optional per_chain_optional source_uuid normalization aggregate_not_sample raw_reference source_version",
        ["Build external Kismet at the pinned revision on Linux and record build flags/dependency lock.",
         "On an authorized isolated AP, fix channel/power/firmware. Capture a numbered beacon sequence through local then remote datasource.",
         "Run the adapter's live acceptance driver; retain auth rejection/success, capabilities, datasource, time-resolved records, and normalized envelopes.",
         "Compare each numbered frame, source UUID, time precision, frequency/channel, dwell and optional fields against independent capture. Save numeric differences and clock-error bounds.",
         "Copy redacted artifacts beside the result template, calculate hashes, fill each check from evidence, then run runtime_gates.py check."],
        ["16.13/Kismet", "20/Gate H", "Appendix I/Kismet gate"], {"kismet_source_revision": KISMET})
    add("kismet-reliability", "hardware_runtime", "kyberia kismet_source_revision kismet_build adapter os",
        "cpu radio_model radio_driver radio_firmware",
        "disconnect reconnect permission_failure helper_restart server_restart hotplug duplicate_replay dropped_event_telemetry queue_backpressure malformed_payload unknown_version unknown_semantic_field source_error partial_capture_preserved latency_benchmark",
        ["With the kismet-live topology, stop/restart source, server and network independently; revoke source permissions and reconnect.",
         "Replay duplicates/reordered records and inject malformed/unknown-version API records into a test-only proxy.",
         "Run bounded load above queue capacity. Compare accepted/dropped/replayed counts against numbered inputs and record latency distribution and queue limits.",
         "Retain logs and immutable pre/post evidence hashes; every failure must preserve finalized data and report its reason."],
        ["16.13/Kismet", "16.15", "16.16", "Appendix I/Kismet gate"], {"kismet_source_revision": KISMET})
    add("kismet-file-parity", "hardware_runtime", "kyberia kismet_source_revision kismet_build adapter os",
        "cpu radio_model radio_driver radio_firmware",
        "kismetdb_import pcapng_import live_db_pcap_parity timestamps channels source_identity duplicates corruption partial_db read_only_import throughput_benchmark second_revision_upgrade normalized_without_kismet",
        ["Capture the same numbered authorized frames into live adapter output, KismetDB, and PCAPNG; hash all originals.",
         "Import files read-only, compare canonical record fields and retained source provenance across all three routes.",
         "Repeat imports to prove idempotence; test truncated copies and corrupt records; verify original files unchanged.",
         "Repeat against a second deliberately supported revision and record both exact revision IDs; archive upgrade fixtures.",
         "Remove Kismet from the execution path and reopen/replay the canonical project."],
        ["16.13/Kismet", "20/Gate H", "Appendix I/Kismet gate"], {"kismet_source_revision": KISMET}, requires_two_revisions=True)
    add("kismet-distribution", "review", "kyberia kismet_source_revision", "",
        "license_audit separate_process notices source_obligations sbom no_gpl_core no_silent_privilege_install",
        ["Inspect actual distributable file inventory and SBOM, dependency graph, license texts and source obligations.",
         "Record reviewer decision and package hashes; process separation alone is not distribution approval."],
        ["1.3", "15.10", "20/Gate G", "Appendix I/Kismet gate"], {"kismet_source_revision": KISMET})
    common_versions = "kyberia sionna_source_revision sionna_rt mitsuba drjit python backend os worker"
    pins = {"sionna_source_revision": SIONNA, "sionna_rt": "2.0.1"}
    add("sionna-install-cpu", "runtime", common_versions, "cpu",
        "fresh_environment locked_install supported_version cpu_probe upstream_tests desktop_without_worker",
        ["Create a fresh worker environment using its checked-in lockfile; retain installer output and installed-package manifest.",
         "Record CPU/LLVM backend probe, then run pinned upstream tests and the worker CPU suite.",
         "Launch and reopen a desktop project with the worker absent; ordinary survey and P1 paths must remain usable."],
        ["8.15", "16.13/Sionna", "20/Gate I", "Appendix I/TST-008"], pins)
    add("sionna-scenes-cpu", "runtime", common_versions, "cpu",
        "scene_conversion geometry_transform openings multi_floor material_conversion frequency_update thin_wall_warning antenna_axes antenna_rotation path_gain radio_map no_data_mask checksum free_space slab reflection transmission_refraction diffuse_scattering diffraction edge_diffraction numerical_regression wifi_sinr_recomposition",
        ["Compile original canonical scenes through Kyberia's compiler; archive canonical and execution scene hashes.",
         "Run both path and radio-map jobs with explicit seed, precision, feature flags, samples and depth on CPU.",
         "Compare numerical outputs with independently reviewed analytic tolerances. For Monte Carlo maps record confidence intervals and cell averaging geometry.",
         "Toggle each interaction independently; preserve path metadata and source versions. Label thin-slab refraction approximation explicitly.",
         "Keep per-transmitter path gain; change channel/activity in Kyberia and verify final Wi-Fi SINR changes without relabeling Sionna generic SINR."],
        ["8.5", "8.15", "16.5", "16.13/Sionna", "20/Gate I"], pins)
    add("sionna-reliability-cpu", "runtime", common_versions, "cpu",
        "malformed_request cancellation timeout worker_crash oom_isolation restart incomplete_artifact atomic_publish cache_hit geometry_invalidation material_invalidation antenna_invalidation engine_invalidation seed_invalidation checksum_rejection artifact_roundtrip remote_auth",
        ["Use test-only fault injection to cancel during scene compilation and compute, exhaust a bounded worker memory limit, force timeout and kill the worker.",
         "Verify desktop survival, structured failure state and no partial artifact published as complete; restart and rerun original request.",
         "Mutate each hash-affecting input independently, confirm cache miss, then replay unchanged input for cache hit.",
         "Corrupt one artifact byte and test checksum rejection; transfer the same immutable bundle through authenticated remote artifact transport."],
        ["8.15", "8.17", "16.13/Sionna", "16.16"], pins)
    add("sionna-convergence-cpu", "runtime", common_versions, "cpu",
        "fixed_seed_repeatability seed_variance sample_convergence depth_convergence interaction_convergence home_benchmark office_benchmark overhead_benchmark tolerance_justification production_defaults",
        ["Repeat fixed seed/backend jobs then sweep at least three seeds, sample budgets and depths for home and office geometry.",
         "Record raw arrays, percentiles/errors against high-budget reference, confidence intervals, elapsed time, peak memory and worker startup overhead.",
         "Review error versus cost and define versioned per-scene defaults; never invent an accuracy claim from a plausible heatmap."],
        ["8.18", "16.13/Sionna", "20/Gate I", "Appendix I/Sionna gate"], pins)
    add("sionna-cuda", "hardware_runtime", common_versions + " cuda driver", "cpu gpu_model gpu_memory",
        "fresh_gpu_environment cuda_probe upstream_tests canonical_scenes cpu_gpu_tolerance deterministic_seed convergence cancellation timeout device_loss numerical_regression",
        ["On compatible CUDA hardware, create a fresh pinned GPU worker environment and capture model/driver/CUDA/memory metadata.",
         "Run upstream and canonical suites with the CPU gate's same immutable job manifests; compare within separately justified CPU/GPU tolerances.",
         "Exercise cancellation, timeout and GPU device loss; keep this gate separate from CPU acceptance."],
        ["8.16", "16.13/Sionna", "20/Gate I"], pins)
    add("sionna-field-holdout", "measured_field", common_versions + " p1_engine calibration_algorithm dataset", "cpu ap_model ap_firmware client_model sensor_driver reference_instrument",
        "ground_truth_geometry calibration_state train_holdout_separation room_holdout session_holdout identifiability priors convergence sensor_bias p0_comparison p1_comparison p2_comparison measured_comparison uncertainty_calibration no_unsupported_superiority",
        ["Acquire consented measured surveys with fixed AP power/channel and documented geometry/material/orientation uncertainty.",
         "Hash the dataset; freeze room/session holdouts before fitting sensor bias/material/power parameters. Check identifiability and freeze weak parameters.",
         "Compare P0/P1/P2 error, bias and interval coverage on held-out data; report failures and environments where P2 does not improve.",
         "Preserve independent measurement reference and calibration certificate; synthetic inputs cannot pass this gate."],
        ["8.14", "16.13/Sionna", "17/Phase 7", "20/Gate I"], pins)
    for platform in ["windows", "macos", "linux"]:
        add("native-" + platform, "hardware_runtime", "kyberia collector os", "cpu radio_model radio_driver radio_firmware",
            "capability_probe permission_granted permission_denied nearby_scan current_link optional_noise scan_cadence timestamp_precision channel_frequency raw_ie source_provenance disconnect_recovery unsupported_fields no_fake_values",
            ["Build and run the native collector acceptance driver on " + platform + " with a real radio and known authorized AP.",
             "Record OS/build/driver/firmware and consent state. Repeat grant/revoke/deny permission and unplug/reconnect where supported.",
             "Measure scan cadence over at least ten complete cycles; preserve stale/throttle/gap flags and source timestamps.",
             "Compare reported identifiers/frequency/RSSI and optional fields with independent reference; absent metrics remain unknown."],
            ["10.10", "16.7", "20/Gate A", "Appendix D"])
    for platform in ["android", "ios"]:
        add("mobile-" + platform, "hardware_runtime", "kyberia app os spatial_sdk", "device_model radio_firmware",
            "permission_granted permission_denied capability_honesty scan_limits current_link active_tiers pose_covariance anchors drift relocalization clock_uncertainty remote_passive_fusion offline_recovery background_limits battery_benchmark",
            ["Install signed companion on a supported physical " + platform + " device; archive OS/SDK/device capability metadata.",
             "Walk a surveyed route twice with pauses/tracking interruption and surveyed anchors; compare manual path and AR pose against independent ground truth.",
             "Pair a passive sensor and verify clock/pose covariance propagation; repeat disconnected/offline and permission-denied conditions.",
             "Android: measure scan throttle and RTT availability. iOS: confirm no general nearby-scan claim; current-link/active/pose only."],
            ["10.10", "10.11", "17/Phase 6", "Appendix D"])
    add("rf-lab", "measured_field", "kyberia collector active_agent dataset", "reference_instrument calibration_certificate ap_model ap_firmware radio_model radio_driver attenuator",
        "attenuation_sweep sensor_bias bands orientation body_loss saturation weak_signal_floor thermal_drift channel_hop_loss capture_loss retries active_rf_fault active_lan_fault active_wan_fault dns_fault roaming repeatability uncertainty",
        ["Use calibrated reference receiver/attenuator and authorized AP/client with documented firmware/channel/power/traffic.",
         "Sweep attenuation and orientation across bands; test cold/warm conditions and saturation/floor behavior.",
         "Independently inject RF attenuation/contention, LAN bottleneck, WAN shaping, loss and DNS delay in controlled topology.",
         "Compare raw counters, iperf outputs and Kyberia attribution; repeat routes/operators/devices and quantify error distributions."],
        ["16.6", "16.7", "16.8", "16.9", "Appendix I/TST-004"])
    add("spectrum-hardware", "hardware_runtime", "kyberia plugin sdk os", "analyzer_model firmware calibration_certificate antenna cable",
        "capabilities frequency_bins rbw detector dwell calibration_reference frequency_offset clipping power_integration occupancy sweep_pose_sync non_wifi_event known_unknown_classification",
        ["Connect calibrated spectrum hardware and inject a controlled reference tone/noise/burst through appropriate attenuation.",
         "Compare frequency/power/sweep and integrated channel quantities; test overload and out-of-range calibration.",
         "Correlate known event time/location with the survey and inspect raw waterfall; ordinary Wi-Fi scan noise cannot satisfy spectrum checks."],
        ["6.6", "17/Phase 5", "Appendix I/SPE-005"])
    add("cross-platform-determinism", "runtime", "kyberia python platform_one platform_two", "cpu_one cpu_two",
        "fixture_bytes analysis_hash numerical_tolerance independent_platforms",
        ["Run python3 tools/validation/fixtures.py check and hashes on two distinct OS platforms, retaining outputs and environment descriptors.",
         "Run the same canonical-domain analysis test driver on both platforms and compare manifests, values and declared numeric tolerances."],
        ["16.14", "17/Phase 0"])
    add("competitor-behavior", "behavioral", "kyberia competitor_build os dataset", "ap_model ap_firmware radio_model radio_driver",
        "legitimate_license matched_geometry matched_scale matched_settings raw_reference workflow_interpolation convex_hull active_topology auto_location ap_on_stick material_loss optimizer unknown_denominator export_permission",
        ["Use legitimate competitor license on a controlled site; record permission to retain screenshots/numeric exports.",
         "Run experiments from plan.md Appendix F with identical AP/adapter/geometry/scale/route/settings where supported.",
         "Preserve independent raw reference data and exact settings; classify conclusions behavioral with uncertainty, never implementation facts."],
        ["16.12", "Appendix F", "Appendix I/TST-005"])
    add("synthetic-contract", "synthetic_contract", "kyberia python harness", "",
        "deterministic_generator original_sources no_measured_claim numerical_semantics tin_hull fixture_hashes",
        ["Run python3 tools/validation/fixtures.py check.",
         "Run python3 -m unittest discover -s tests -p test_research_harness.py -v.",
         "Run python3 tools/validation/fixtures.py hashes; retain command output and exact source revision."],
        ["16.4", "16.5", "16.13/wifiheatmap", "18.11/OSS-010"])
    return {"schema_version": 1,
            "policy": "Catalog NOT_RUN states are defaults, not environment blocker findings. Results are separate evidence manifests.",
            "gates": gates}


if __name__ == "__main__":
    Path(__file__).with_name("gates.json").write_text(json.dumps(build(), indent=2, sort_keys=True) + "\n")
