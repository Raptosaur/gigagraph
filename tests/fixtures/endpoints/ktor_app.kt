import io.ktor.server.auth.authenticate
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
        // Slashless segments (Ktor treats "wish"/"make" as /wish/make) and
        // pathless verbs (`post { }` binds to the enclosing route's path) —
        // both only trusted inside a route(...) span.
        route("crates") {
            get("list") {
            }
            post {
            }
            route("{id}") {
                get {
                }
            }
        }
        // Wrapper lambdas (authenticate/etc.) between route levels are
        // transparent to byte containment.
        authenticate("admin") {
            route("/vault") {
                get("/keys") {
                }
            }
        }
        // A slashless verb with NO enclosing route(...) span stays ignored:
        // more likely a map lookup than a route.
        get("orphan") {
        }
    }
}
