package com.acme.api

import jakarta.transaction.Transactional
import jakarta.ws.rs.GET
import jakarta.ws.rs.POST
import jakarta.ws.rs.Path

// JAX-RS on Kotlin (Quarkus-style): class-level @Path prefix rides along,
// verb annotations are bare MARKERS (no value_arguments — distinct capture
// shape from @GetMapping("/x"), see src/lang/kotlin.rs).
@Path("barrels")
class BarrelResource {
    @GET
    fun list(): List<String> {
        return listOf()
    }

    @GET
    @Path("{id}")
    fun single(id: Long): String {
        return ""
    }

    @POST
    @Transactional
    fun create(name: String): String {
        return ""
    }
}
