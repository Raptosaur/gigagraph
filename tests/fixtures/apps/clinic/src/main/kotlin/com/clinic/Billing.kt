package com.clinic

enum class Payer { SELF, INSURER, EMPLOYER }

sealed class Charge(val cents: Int) {
    class Visit(cents: Int) : Charge(cents)
    class Procedure(cents: Int, val code: String) : Charge(cents)
}

object Tariff {
    fun lookup(code: String): Int = when (code) {
        "A1" -> 12_000
        "B2" -> 45_000
        else -> 0
    }
}

fun total(charges: List<Charge>): Int = charges.sumOf { it.cents }

fun copay(payer: Payer, charge: Charge): Int = when (payer) {
    Payer.SELF -> charge.cents
    Payer.INSURER -> charge.cents / 5
    Payer.EMPLOYER -> 0
}

suspend fun settle(charges: List<Charge>, payer: Payer): Int {
    return charges.sumOf { copay(payer, it) }
}
