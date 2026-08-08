package com.clinic;

import static org.junit.jupiter.api.Assertions.*;

import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.params.ParameterizedTest;
import org.junit.jupiter.params.provider.ValueSource;

class PatientServiceTest {

    private PatientService service;

    @BeforeEach
    void setUp() {
        service = new PatientService(InMemorySchedulerKt());
    }

    @Test
    void admitStoresThePatient() {
        service.admit(Patient.of(1L, "Ada"));
        assertEquals(1, service.page(0).size());
    }

    @Test
    @DisplayName("discharge removes a patient by id")
    void dischargeRemovesById() {
        service.admit(Patient.of(2L, "Grace"));
        service.discharge(2L);
        assertTrue(service.find(2L).isEmpty());
    }

    @ParameterizedTest
    @ValueSource(ints = {1, 17})
    void minorsAreUnderEighteen(int age) {
        assertTrue(new Patient(3L, "Kid", age).isMinor());
    }

    private static Scheduler InMemorySchedulerKt() {
        return new InMemoryScheduler();
    }
}
