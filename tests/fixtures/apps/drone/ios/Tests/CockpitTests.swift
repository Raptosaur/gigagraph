import XCTest

@testable import Cockpit

final class CockpitTests: XCTestCase {

    private var cockpit: Cockpit!

    override func setUp() {
        super.setUp()
        cockpit = Cockpit(source: StubSource())
    }

    override func tearDown() {
        cockpit = nil
        super.tearDown()
    }

    func testRefreshAppendsAReading() {
        XCTAssertNotNil(cockpit.refresh())
    }

    func testAverageAltitudeOfEmptyCockpitIsZero() {
        XCTAssertEqual(Cockpit(source: EmptySource()).averageAltitude(), 0)
    }

    func testScaledMultipliesAltitude() throws {
        let reading = Reading(altitude: 10, battery: 1)
        XCTAssertEqual(reading.scaled(by: 2).altitude, 20)
    }

    private func makeReading() -> Reading {
        Reading(altitude: 1, battery: 1)
    }
}

struct StubSource: TelemetrySource {
    func latest() -> Reading? { Reading(altitude: 5, battery: 0.5) }
    func history(limit: Int) -> [Reading] { [] }
}

struct EmptySource: TelemetrySource {
    func latest() -> Reading? { nil }
    func history(limit: Int) -> [Reading] { [] }
}
