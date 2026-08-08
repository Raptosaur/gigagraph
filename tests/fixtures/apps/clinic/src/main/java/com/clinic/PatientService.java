package com.clinic;

import java.util.ArrayList;
import java.util.List;
import java.util.Optional;
import java.util.stream.Collectors;

public class PatientService {

    private final List<Patient> patients = new ArrayList<>();
    private final Scheduler scheduler;

    public PatientService(Scheduler scheduler) {
        this.scheduler = scheduler;
    }

    public List<Patient> page(int page) {
        return patients.stream().skip(page * 20L).limit(20).collect(Collectors.toList());
    }

    public Optional<Patient> find(long id) {
        return patients.stream().filter(p -> p.id() == id).findFirst();
    }

    public Patient admit(Patient patient) {
        patients.add(patient);
        scheduler.schedule(patient.id(), "intake");
        return patient;
    }

    public void discharge(long id) {
        patients.removeIf(p -> p.id() == id);
    }

    static String describe(Patient patient) {
        return patient.name() + " #" + patient.id();
    }
}
