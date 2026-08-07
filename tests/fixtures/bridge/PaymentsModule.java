package com.acme.bridge;

import com.facebook.react.bridge.ReactMethod;

public class PaymentsModule {
    @ReactMethod
    public void charge(double amount) {
    }

    @ReactMethod
    public void refund(String id) {
    }

    public void internalHelper() {
    }
}
