import Testing

@testable import Cockpit

@Suite("Reading arithmetic")
struct ReadingTests {

    @Test func scalingIsProportional() {
        let reading = Reading(altitude: 4, battery: 1)
        #expect(reading.scaled(by: 0.5).altitude == 2)
    }

    @Test("a battery under 20% is low")
    func lowBatteryThreshold() {
        #expect(Reading(altitude: 0, battery: 0.1).isLow)
    }

    @Test(arguments: [0.0, 1.0])
    func batteryStaysInRange(_ battery: Double) {
        #expect(Reading(altitude: 0, battery: battery).battery == battery)
    }

    func makeReading() -> Reading {
        Reading(altitude: 1, battery: 1)
    }
}
