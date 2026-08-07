package models

import java.time.Instant

data class User(val name: String, val email: String)

class AuditLog {
    private val entries = mutableListOf<String>()

    fun record(event: String) {
        entries.add(format(event))
    }

    private fun format(event: String): String {
        return when (event) {
            "" -> "empty"
            else -> Instant.now().toString() + " " + event
        }
    }
}
