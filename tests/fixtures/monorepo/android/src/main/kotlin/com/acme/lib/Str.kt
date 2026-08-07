package com.acme.lib

fun titleCase(s: String): String {
    if (s.isEmpty()) {
        return s
    }
    return s.replaceFirstChar { it.uppercase() }
}
