package com.example.wiring

import org.springframework.stereotype.Service

class Order(val id: String)

interface OrderStore {
    fun save(order: Order): Order
}

interface Auditable {
    fun audit(): String
}

open class BaseUseCase

class AuditTrail {
    fun record(id: String) {}
}

@Service
class JdbcOrderStore : OrderStore {
    override fun save(order: Order): Order {
        return order
    }
}

// Singleton implementation via object declaration.
object InMemoryStore : OrderStore {
    val fallback: AuditTrail = AuditTrail()

    override fun save(order: Order): Order = order
}

// Interface implementation by delegation.
class DelegatingStore(private val impl: OrderStore) : OrderStore by impl

@Service
class OrderService(
    private val store: OrderStore,
    val tracer: Tracer?,
    private val clock: java.time.Clock,
    val tags: List<String>,
    logger: Logger,
) : BaseUseCase(), Auditable {
    val audit = AuditTrail()

    fun place(order: Order, note: String?): Order {
        val trail = AuditTrail()
        trail.record(order.id)
        audit.record(order.id)
        return store.save(order)
    }

    override fun audit(): String = "orders"
}
