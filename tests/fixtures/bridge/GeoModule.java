package com.acme.bridge;

import com.facebook.react.bridge.ReactMethod;

public class GeoModule {
    public String getName() {
        return "Geo2";
    }

    @ReactMethod
    public void ping(double lat) {
    }
}
