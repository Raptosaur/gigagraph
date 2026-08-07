import retrofit2.http.GET

interface RetrofitService {
    @GET("/widgets/{id}")
    fun widget(id: String): String
}
