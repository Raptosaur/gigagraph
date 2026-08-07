use actix_web::{get, post, HttpResponse};

#[get("/invoices/{id}")]
async fn show_invoice() -> HttpResponse {
    HttpResponse::Ok().finish()
}

#[post("/invoices")]
async fn create_invoice() -> HttpResponse {
    HttpResponse::Ok().finish()
}

#[actix_web::get("/probes/live")]
async fn live_probe() -> HttpResponse {
    HttpResponse::Ok().finish()
}
