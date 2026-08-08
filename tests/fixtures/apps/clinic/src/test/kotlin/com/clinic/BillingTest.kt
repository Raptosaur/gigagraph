package com.clinic

import kotlin.test.assertEquals
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.RepeatedTest

class BillingTest {

    @Test
    fun `total sums every charge`() {
        assertEquals(300, total(listOf(Charge.Visit(100), Charge.Visit(200))))
    }

    @Test
    fun insurerCopayIsOneFifth() {
        assertEquals(20, copay(Payer.INSURER, Charge.Visit(100)))
    }

    @RepeatedTest(3)
    fun employerCopayIsAlwaysZero() {
        assertEquals(0, copay(Payer.EMPLOYER, Charge.Visit(500)))
    }

    private fun visit(cents: Int): Charge = Charge.Visit(cents)
}

class SchedulerTest {

    @Test
    fun cancelReturnsFalseWhenUnknown() {
        assertEquals(false, InMemoryScheduler.empty().cancel(9L))
    }
}
