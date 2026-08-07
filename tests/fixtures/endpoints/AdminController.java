package com.acme.api;

import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestMapping;

@RequestMapping("/admin/api")
public class AdminController {
    @GetMapping("/reports")
    public String reports() {
        return "";
    }

    @PostMapping("/jobs")
    public String startJob() {
        return "";
    }
}
