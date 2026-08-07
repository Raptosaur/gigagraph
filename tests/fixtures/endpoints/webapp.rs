use axum::{
    Router,
    routing::{delete, get, post},
};

async fn show_widget() -> String {
    String::new()
}

async fn add_sprocketeer() -> String {
    String::new()
}

async fn list_sprocketeers() -> String {
    String::new()
}

async fn panel_health() -> String {
    String::new()
}

async fn drop_gauge() -> String {
    String::new()
}

pub fn app() -> Router {
    Router::new()
        .route("/widgets/{id}", get(show_widget))
        // Chained routes + multi-verb method router (mongodb/realworld-axum
        // shape): each verb must bind to ITS route call, not the chain's
        // first.
        .route("/sprocketeers", post(add_sprocketeer).get(list_sprocketeers))
        // Cross-function nest prefix (key-value-store shape).
        .nest("/panel", panel_routes())
        // merge() keeps paths as-is but must still resolve handlers.
        .merge(gauges_router())
}

fn panel_routes() -> Router {
    Router::new().route("/health", get(panel_health))
}

fn gauges_router() -> Router {
    Router::new().route("/gauges/{id}", delete(drop_gauge))
}
