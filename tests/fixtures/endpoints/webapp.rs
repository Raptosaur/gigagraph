use axum::{Router, routing::get};

async fn show_widget() -> String {
    String::new()
}

pub fn app() -> Router {
    Router::new().route("/widgets/{id}", get(show_widget))
}
