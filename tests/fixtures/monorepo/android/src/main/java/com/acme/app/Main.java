package com.acme.app;

import java.util.List;

public class Main {
    public static void main(String[] args) {
        List<String> names = List.of("ada", "grace");
        for (String name : names) {
            System.out.println(Util.shout(name));
        }
    }
}
