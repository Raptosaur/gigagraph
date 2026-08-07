package com.acme.api;

import java.util.Set;

import javax.ws.rs.GET;
import javax.ws.rs.Path;

import org.eclipse.microprofile.rest.client.inject.RegisterRestClient;

// MicroProfile rest-client interface (javax-era imports): same annotation
// surface as a server resource, but @RegisterRestClient (argument-less
// marker form) marks every mapped method as an OUTBOUND call.
@Path("/depots")
@RegisterRestClient
public interface DepotService {

    @GET
    Set<String> all();

    @GET
    @Path("{id}")
    String byId(long id);
}
