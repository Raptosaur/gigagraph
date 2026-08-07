package com.acme.api;

import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RequestMethod;

public class UserController {
    @GetMapping("/accounts/{id}")
    public String account(long id) {
        return "";
    }

    @RequestMapping(value = "/accounts", method = RequestMethod.POST)
    public String create() {
        return "";
    }
}
