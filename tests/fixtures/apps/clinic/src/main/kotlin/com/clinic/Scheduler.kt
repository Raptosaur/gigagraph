package com.clinic

import java.time.Duration
import java.time.Instant

data class Slot(val patientId: Long, val at: Instant, val reason: String)

interface Scheduler {
    fun schedule(patientId: Long, reason: String): Slot

    fun cancel(patientId: Long): Boolean

    fun pending(): List<Slot> = emptyList()
}

class InMemoryScheduler(private val clock: () -> Instant = Instant::now) : Scheduler {

    private val slots = mutableListOf<Slot>()

    override fun schedule(patientId: Long, reason: String): Slot {
        val slot = Slot(patientId, clock().plus(nextGap()), reason)
        slots += slot
        return slot
    }

    override fun cancel(patientId: Long): Boolean = slots.removeIf { it.patientId == patientId }

    override fun pending(): List<Slot> = slots.toList()

    private fun nextGap(): Duration = Duration.ofMinutes(15L * (slots.size + 1))

    companion object {
        fun empty(): InMemoryScheduler = InMemoryScheduler()
    }
}

fun Slot.describe(): String = "patient ${'$'}patientId at ${'$'}at (${'$'}reason)"
