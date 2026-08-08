package com.clinic;

import java.util.List;
import java.util.Optional;
import org.springframework.http.ResponseEntity;
import org.springframework.web.bind.annotation.*;

@RestController
@RequestMapping("/api/patients")
public class PatientController {

    private final PatientService service;

    public PatientController(PatientService service) {
        this.service = service;
    }

    @GetMapping
    public List<Patient> list(@RequestParam(defaultValue = "0") int page) {
        return service.page(page);
    }

    @GetMapping("/{id}")
    public ResponseEntity<Patient> byId(@PathVariable long id) {
        Optional<Patient> found = service.find(id);
        return found.map(ResponseEntity::ok).orElseGet(() -> ResponseEntity.notFound().build());
    }

    @PostMapping
    public Patient create(@RequestBody Patient patient) {
        return service.admit(patient);
    }

    @DeleteMapping("/{id}")
    public void discharge(@PathVariable long id) {
        service.discharge(id);
    }
}
