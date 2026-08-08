import Foundation

public struct Reading: Equatable {
    public let altitude: Double
    public let battery: Double

    public var isLow: Bool { battery < 0.2 }

    public func scaled(by factor: Double) -> Reading {
        Reading(altitude: altitude * factor, battery: battery)
    }
}

public protocol TelemetrySource {
    func latest() -> Reading?
    func history(limit: Int) -> [Reading]
}

public final class Cockpit {
    private var readings: [Reading] = []
    private let source: TelemetrySource

    public init(source: TelemetrySource) {
        self.source = source
    }

    public func refresh() -> Reading? {
        guard let reading = source.latest() else { return nil }
        readings.append(reading)
        return reading
    }

    public func averageAltitude() -> Double {
        guard !readings.isEmpty else { return 0 }
        return readings.reduce(0) { $0 + $1.altitude } / Double(readings.count)
    }

    public static func makeDefault(source: TelemetrySource) -> Cockpit {
        Cockpit(source: source)
    }
}

extension Cockpit: CustomStringConvertible {
    public var description: String { "Cockpit(\(readings.count) readings)" }
}

func formatAltitude(_ meters: Double) -> String {
    String(format: "%.1f m", meters)
}
