import io.ktor.server.routing.delete
import io.ktor.server.routing.get
import io.ktor.server.routing.post
import io.ktor.server.routing.route
import io.ktor.server.routing.routing

fun module() {
    routing {
        get("/telemetry/live") {
        }
        post("/telemetry") {
        }
        route("/api") {
            get("/users") {
            }
            route("/v2") {
                delete("/sessions") {
                }
            }
        }
    }
}
