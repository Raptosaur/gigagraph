package com.example.soap;

import javax.jws.WebMethod;
import javax.jws.WebService;

@WebService
public class CalculatorService {
    @WebMethod
    public int add(int a, int b) {
        return a + b;
    }

    @WebMethod(operationName = "SubtractNumbers")
    public int subtract(int a, int b) {
        return a - b;
    }

    public int helper(int a) {
        return a;
    }
}
