package com.clinic;

public record Patient(long id, String name, int ageYears) {

    public boolean isMinor() {
        return ageYears < 18;
    }

    public static Patient of(long id, String name) {
        return new Patient(id, name, 30);
    }
}
