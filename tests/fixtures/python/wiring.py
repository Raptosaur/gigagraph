# DI wiring shapes: abstract-ish base, implementation, and a consumer whose
# constructor stores an injected typed dependency. Exercises the type-capture
# query tail in src/lang/python.rs (typed params, annotated/constructed
# locals, self-field joins, hierarchy edges).


class BaseStore:
    def save(self, order):
        raise NotImplementedError


class OrderStore(BaseStore):
    def save(self, order):
        persist(order)


class OrderService:
    def __init__(self, store: OrderStore, logger):
        # `self.store = store` captures the ctor PARAM NAME ("store") in type
        # position at extraction; the graph build joins it against the
        # `__init__` param's declared type (OrderStore).
        self.store = store
        # Constructed field: `Klass()` on the right captures Klass directly.
        # The `^[A-Z]` case guard keeps lowercase helper calls out.
        self.cache = CacheClient()
        # Untyped param join: no declared type to substitute; harmless.
        self.logger = logger

    def place(self, order):
        audit = AuditTrail()
        tag: OrderTag = make_tag(order)
        self.store.save(order)
        audit.record(order)
        return tag


def build_service(store: OrderStore = None):
    if store is None:
        store = OrderStore()
    return OrderService(store, None)
