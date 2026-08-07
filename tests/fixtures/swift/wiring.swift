import Foundation

protocol Store {
    func save(_ record: String)
}

protocol Cache: Store {
    func evict(key: String)
}

class DbStore: Store {
    func save(_ record: String) {
        print(record)
    }
}

struct MemoryStore: Store {
    func save(_ record: String) {}
}

class Service {
    let store: Store
    var label: String

    init(store: Store) {
        self.store = store
        self.label = "svc"
    }

    func persist(record: String) {
        self.store.save(record)
        store.save(record)
    }

    func rebuild() {
        let fallback = DbStore()
        let annotated: Store = MemoryStore()
        fallback.save("a")
        annotated.save("b")
    }

    func send(to target: Store, count: Int) {
        target.save(label)
    }
}
