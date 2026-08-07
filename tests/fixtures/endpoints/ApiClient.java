package com.acme.api;

import retrofit2.Call;
import retrofit2.http.GET;
import retrofit2.http.POST;
import retrofit2.http.Path;

public interface ApiClient {
    @GET("/accounts/{id}")
    Call<String> account(@Path("id") long id);

    @POST("/accounts")
    Call<String> create();
}
