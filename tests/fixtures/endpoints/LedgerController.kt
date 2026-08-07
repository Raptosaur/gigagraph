package com.acme.ledger

import org.springframework.web.bind.annotation.GetMapping
import org.springframework.web.bind.annotation.RestController

@RestController
class LedgerController {
    @GetMapping("/ledgers/{id}")
    fun ledger(id: Long): String = ""
}
