/// CoreLocation state is not interchangeable with system service availability.
/// Pure adapter policy; no request is made by merely inspecting this value.
struct PermissionAssessment {
    let servicesEnabled: Bool
    let authorization: String

    var mayScan: Bool { servicesEnabled && authorization == "authorized" }
    var mayRequest: Bool { servicesEnabled && authorization == "not_determined" }
    var reason: String { servicesEnabled ? "location_" + authorization : "location_services_disabled" }
}
