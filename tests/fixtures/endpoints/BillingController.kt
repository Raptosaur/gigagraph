package com.acme.billing

import org.springframework.web.bind.annotation.GetMapping
import org.springframework.web.bind.annotation.RequestMapping

@RequestMapping("/billing")
class BillingController {
    @GetMapping("/invoices/{id}")
    fun invoice(id: Long): String {
        return ""
    }
}
