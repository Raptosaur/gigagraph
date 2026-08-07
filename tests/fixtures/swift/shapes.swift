import Combine

protocol Describable {
    func describe() -> String
}

final class Cache {
    private var storage: [String: Int] = [:]

    init() {
        storage.reserveCapacity(16)
    }

    func lookup(_ key: String) -> Int {
        if let hit = storage[key] {
            return hit
        }
        return 0
    }

    func warm(keys: [String]) {
        var index = 0
        repeat {
            store(key: keys[index], value: index)
            index += 1
        } while index < keys.count
    }

    private func store(key: String, value: Int) {
        storage[key] = value
    }
}

func report(on cache: Cache) -> String {
    func heading(_ text: String) -> String {
        return text.uppercased()
    }
    let title = heading("cache report")
    do {
        let payload = try encode(cache)
        return title + String(payload.count)
    } catch {
        return title
    }
}

func encode(_ cache: Cache) throws -> [Int] {
    let flag = cache.lookup("ready") > 0 ? 1 : 0
    return [flag]
}
