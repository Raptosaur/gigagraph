package com.acme.api;

import jakarta.ws.rs.DELETE;
import jakarta.ws.rs.GET;
import jakarta.ws.rs.POST;
import jakarta.ws.rs.Path;

// JAX-RS (Quarkus-style) resource: class-level @Path prefix (no leading
// slash, per convention), bare verb markers, method-level @Path composition.
@Path("fleets")
public class FleetResource {
    @GET
    public String list() {
        return "";
    }

    @GET
    @Path("{id}")
    public String get(long id) {
        return "";
    }

    @POST
    public String create() {
        return "";
    }

    @DELETE
    @Path("{id}")
    public String remove(long id) {
        return "";
    }
}
