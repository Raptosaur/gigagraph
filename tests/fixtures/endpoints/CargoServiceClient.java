package com.acme.api;

import org.springframework.cloud.openfeign.FeignClient;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RequestMethod;

// Spring Cloud OpenFeign declarative client: interface-level @FeignClient
// makes the legacy-form @RequestMapping methods outbound calls, not routes.
@FeignClient(name = "cargo-service")
public interface CargoServiceClient {

    @RequestMapping(method = RequestMethod.PUT, value = "/cargo/{name}")
    void updateCargo(String name);

    @RequestMapping(method = RequestMethod.GET, value = "/cargo/{name}")
    String getCargo(String name);
}
