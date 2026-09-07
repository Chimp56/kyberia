import CoreLocation
import CoreWLAN
import Darwin
import Foundation

func authorizationName(_ status: CLAuthorizationStatus) -> String {
    switch status {
    case .authorized: return "authorized"
    case .denied: return "denied"
    case .restricted: return "restricted"
    case .notDetermined: return "not_determined"
    default: return "unknown"
    }
}

func channel(_ value: CWChannel?) -> [String: Any] {
    guard let value = value else { return unknown("source_did_not_provide") }
    let bands: [Int: String] = [1: "2.4_ghz", 2: "5_ghz", 3: "6_ghz"]
    let widths: [Int: Int] = [1: 20, 2: 40, 3: 80, 4: 160]
    return known([
        "reported_channel_number": value.channelNumber > 0 ? known(value.channelNumber) : unknown("source_did_not_provide"),
        "band": bands[value.channelBand.rawValue].map { known($0) } ?? unknown("unsupported_source_enum"),
        "width_mhz": widths[value.channelWidth.rawValue].map { known($0) } ?? unknown("unsupported_source_enum"),
        "raw_band_enum": value.channelBand.rawValue,
        "raw_width_enum": value.channelWidth.rawValue,
        "frequency_hz": unknown("source_did_not_provide"),
        "center_frequency_hz": unknown("source_did_not_provide"),
        "puncturing": unknown("source_did_not_provide")
    ])
}

func measuredPower(_ value: Int) -> [String: Any] {
    // Zero is an error/no-association sentinel in CWInterface. Do not paint it
    // as a strong measured signal when an OS result has no usable reading.
    guard (-200 ... -1).contains(value) else { return unknown("invalid_or_unavailable_source_value") }
    return known(value)
}

func networkIdentity(_ value: String?) -> [String: Any] {
    guard let value = value else { return unknown("source_did_not_provide") }
    let octets = value.split(separator: ":", omittingEmptySubsequences: false).compactMap { $0.count == 2 ? UInt8($0, radix: 16) : nil }
    guard value.utf8.count == 17 else { return unknown("invalid_or_unavailable_source_value") }
    guard octets.count == 6, octets[0] & 1 == 0, octets.contains(where: { $0 != 0 }) else {
        return unknown("invalid_or_unavailable_source_value")
    }
    return known(value.lowercased())
}

func source(_ name: String, session: String) -> [String: Any] {
    ["source_id": session + ":" + name, "interface_name": name,
     "identity_scope": "collector_process_and_interface", "physical_radio_id": unknown("source_did_not_provide"),
     "driver_version": unknown("source_did_not_provide"), "firmware_version": unknown("source_did_not_provide"),
     "collector": "kyberia-macos-corewlan", "collector_version": collectorVersion,
     "collector_build": collectorBuild, "source_kind": "native_api",
     "source_api": "CoreWLAN.CWInterface.scanForNetworks", "source_schema": protocolVersion,
     "framework_version": Bundle(for: CWWiFiClient.self).infoDictionary?["CFBundleVersion"] as? String ?? "unknown",
     "os_version": ProcessInfo.processInfo.operatingSystemVersionString]
}

let options: Options
do {
    options = try Options(Array(CommandLine.arguments.dropFirst()))
} catch {
    diagnostic("usage: collector probe|scan|authorize [--interface en0] [--limit 1..4096] [--timeout-seconds 1..60] [--include-identifiers]")
    exit(64)
}

let writer = WireWriter()
let processControl = ProcessControl(writer: writer, timeoutSeconds: options.timeoutSeconds)
writer.emit("hello", ["collector": "kyberia-macos-corewlan", "collector_version": collectorVersion,
                      "evidence_origin": "native_runtime",
                      "collector_build": collectorBuild, "os_version": ProcessInfo.processInfo.operatingSystemVersionString,
                      "command": options.command, "identifier_policy": options.includeIdentifiers ? "explicit_unredacted" : "redacted",
                      "max_observations": options.limit, "max_record_bytes": 16384,
                      "timeout_seconds": options.timeoutSeconds, "clock_epoch": writer.session])

let locationManager = CLLocationManager()

final class AuthorizationDelegate: NSObject, CLLocationManagerDelegate {
    func locationManagerDidChangeAuthorization(_ manager: CLLocationManager) {
        let status = authorizationName(manager.authorizationStatus)
        guard status != "not_determined" else { return }
        let permission = PermissionAssessment(servicesEnabled: CLLocationManager.locationServicesEnabled(), authorization: status)
        writer.finish(permission.mayScan ? "ok" : "permission_required",
                      reason: permission.reason, exitCode: permission.mayScan ? 0 : 77,
                      extra: ["final_authorization": permission.authorization,
                              "final_location_services_enabled": permission.servicesEnabled])
    }
}

let authorizationDelegate = AuthorizationDelegate()
if options.command == "authorize" {
    let status = authorizationName(locationManager.authorizationStatus)
    let permission = PermissionAssessment(servicesEnabled: CLLocationManager.locationServicesEnabled(), authorization: status)
    writer.emit("authorization", ["state": status, "requested_by_operator": true,
                                  "location_services_enabled": permission.servicesEnabled, "prompt_requested": permission.mayRequest])
    if !permission.mayRequest {
        writer.finish(permission.mayScan ? "ok" : "permission_required",
                      reason: permission.reason, exitCode: permission.mayScan ? 0 : 77,
                      extra: ["final_authorization": permission.authorization,
                              "final_location_services_enabled": permission.servicesEnabled])
    }
    locationManager.delegate = authorizationDelegate
    // Only this explicit command requests consent; probe/scan never prompt.
    locationManager.requestWhenInUseAuthorization()
}

func collect() {
    let enabled = CLLocationManager.locationServicesEnabled()
    let authorization = authorizationName(locationManager.authorizationStatus)
    let permission = PermissionAssessment(servicesEnabled: enabled, authorization: authorization)
    let permitted = permission.mayScan
    let client = CWWiFiClient.shared()
    let interfaces = (client.interfaces() ?? []).sorted { ($0.interfaceName ?? "") < ($1.interfaceName ?? "") }
    guard interfaces.count <= 32 else {
        writer.finish("error", reason: "interface_resource_limit_exceeded", exitCode: 70)
    }
    var available: [CWInterface] = []
    var sources: [[String: Any]] = []
    for interface in interfaces {
        guard let name = interface.interfaceName, !name.isEmpty, name.utf8.count <= 64 else { continue }
        if let requested = options.interfaceName, name != requested { continue }
        available.append(interface)
        var descriptor = source(name, session: writer.session)
        descriptor["power_on"] = interface.powerOn()
        let supported = interface.supportedWLANChannels()
        descriptor["supported_channel_count"] = supported.map { known($0.count) } ?? unknown("source_did_not_provide")
        descriptor["reported_band_enums"] = supported.map { Array(Set($0.map { $0.channelBand.rawValue })).sorted() } ?? []
        sources.append(descriptor)
    }
    let scanState = permitted && available.contains(where: { $0.powerOn() }) ? "available" : "unavailable"
    writer.emit("capabilities", [
        "location_services_enabled": enabled, "location_authorization": authorization,
        "sources": sources,
        "nearby_scan": ["state": scanState, "condition": "authorized CoreLocation, available powered interface, successful CoreWLAN scan"],
        "noise_dbm": ["state": "conditional", "condition": "CoreWLAN reports a usable nonzero network noiseMeasurement"],
        "channel_width": ["state": "conditional", "condition": "CoreWLAN reports a recognized width enum"],
        "monitor_frames": unknown("not_supported_by_collector"), "channel_dwell": unknown("source_did_not_provide"),
        "channel_hopping_control": unknown("not_supported_by_collector"), "per_chain_signal": unknown("source_did_not_provide"),
        "raw_payload_policy": "discard", "phy_metadata": unknown("not_implemented_by_collector"),
        "capture_timestamp": unknown("source_did_not_provide"), "position": unknown("not_collected")
    ])
    if options.command == "probe" {
        writer.finish("ok", reason: "capabilities_probed_only", exitCode: 0)
    }
    guard permitted else {
        writer.finish("permission_required", reason: permission.reason, exitCode: 77)
    }
    guard !available.isEmpty else {
        writer.finish("unsupported", reason: options.interfaceName == nil ? "no_wifi_interface" : "requested_interface_unavailable", exitCode: 69)
    }
    var count = 0
    for interface in available {
        guard interface.powerOn() else {
            writer.finish("unavailable", reason: "interface_power_off_or_api_failure", exitCode: 69)
        }
        guard let name = interface.interfaceName else {
            writer.finish("unavailable", reason: "interface_removed", exitCode: 69)
        }
        guard CLLocationManager.locationServicesEnabled(), authorizationName(locationManager.authorizationStatus) == "authorized" else {
            writer.finish("permission_required", reason: "location_authorization_changed", exitCode: 77)
        }
        let started = DispatchTime.now().uptimeNanoseconds
        let scanID = UUID().uuidString.lowercased()
        writer.emit("scan_started", ["scan_id": scanID, "source_id": writer.session + ":" + name,
                                     "api_started_monotonic_ns": String(started), "include_hidden": false])
        do {
            let networks = try interface.scanForNetworks(withSSID: nil, includeHidden: false)
            let ended = DispatchTime.now().uptimeNanoseconds
            guard CLLocationManager.locationServicesEnabled(), authorizationName(locationManager.authorizationStatus) == "authorized" else {
                writer.finish("permission_required", reason: "location_authorization_changed", exitCode: 77)
            }
            // Sorting is an output convenience only; it does not manufacture identity.
            let ordered = networks.sorted {
                if $0.rssiValue != $1.rssiValue { return $0.rssiValue > $1.rssiValue }
                return ($0.bssid ?? "") < ($1.bssid ?? "")
            }
            for network in ordered {
                guard CLLocationManager.locationServicesEnabled(), authorizationName(locationManager.authorizationStatus) == "authorized" else {
                    writer.finish("permission_required", reason: "location_authorization_changed", exitCode: 77)
                }
                if count >= options.limit {
                    writer.finish("partial", reason: "observation_limit_reached", exitCode: 2)
                }
                let bssid: [String: Any]
                let ssid: [String: Any]
                if options.includeIdentifiers {
                    bssid = networkIdentity(network.bssid)
                    if let bytes = network.ssidData, (1...32).contains(bytes.count) {
                        ssid = known(bytes.base64EncodedString())
                    } else {
                        ssid = unknown("source_did_not_provide")
                    }
                } else {
                    bssid = unknown("redacted")
                    ssid = unknown("redacted")
                }
                writer.emit("scan_observation", [
                    "observation_id": UUID().uuidString.lowercased(), "scan_id": scanID,
                    "source": source(name, session: writer.session), "evidence_class": "observed_api_result",
                    "api_window": ["start_monotonic_ns": String(started), "end_monotonic_ns": String(ended)],
                    "bssid": bssid, "ssid_octets_base64": ssid,
                    "channel": channel(network.wlanChannel), "rssi_dbm": measuredPower(network.rssiValue),
                    "noise_dbm": measuredPower(network.noiseMeasurement),
                    "measurement_method": "CoreWLAN.CWNetwork.rssiValue/noiseMeasurement",
                    "calibration": "uncalibrated", "result_age_seconds": unknown("source_did_not_provide"),
                    "dwell_seconds": unknown("source_did_not_provide"), "phy": unknown("not_implemented_by_collector"),
                    "position": unknown("not_collected"), "information_elements": unknown("not_retained_by_collector"),
                    "quality": ["capture_time_unknown", "scan_cache_age_unknown", "uncalibrated", "dwell_unknown"]
                ])
                count += 1
            }
        } catch {
            let native = error as NSError
            // Do not emit localized errors that may embed network identifiers.
            writer.finish("error", reason: "corewlan_scan_failed", exitCode: 70,
                          extra: ["native_error_domain": native.domain, "native_error_code": native.code])
        }
    }
    writer.finish("ok", reason: "scan_results_received", exitCode: 0)
}

if options.command != "authorize" {
    DispatchQueue.global().async { collect() }
}
runCollectorLoop()
