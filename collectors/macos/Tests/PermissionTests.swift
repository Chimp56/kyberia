import Foundation

@main
struct PermissionTests {
    static func main() {
        for status in ["authorized", "denied", "restricted", "not_determined", "unknown"] {
            let disabled = PermissionAssessment(servicesEnabled: false, authorization: status)
            precondition(!disabled.mayScan && !disabled.mayRequest)
            precondition(disabled.reason == "location_services_disabled")
        }
        let fresh = PermissionAssessment(servicesEnabled: true, authorization: "not_determined")
        precondition(!fresh.mayScan && fresh.mayRequest && fresh.reason == "location_not_determined")
        for status in ["denied", "restricted", "unknown"] {
            let denied = PermissionAssessment(servicesEnabled: true, authorization: status)
            precondition(!denied.mayScan && !denied.mayRequest && denied.reason == "location_" + status)
        }
        let permitted = PermissionAssessment(servicesEnabled: true, authorization: "authorized")
        precondition(permitted.mayScan && !permitted.mayRequest)
        print("PASS: ten permission/service combinations")
    }
}
