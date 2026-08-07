use actix_web::{get, post, web, HttpResponse};

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

#[get("/ledger-lines")]
async fn ledger_lines() -> HttpResponse {
    HttpResponse::Ok().finish()
}

async fn list_crates2() -> HttpResponse {
    HttpResponse::Ok().finish()
}

async fn add_crate2() -> HttpResponse {
    HttpResponse::Ok().finish()
}

async fn direct_ping() -> HttpResponse {
    HttpResponse::Ok().finish()
}

// Builder-style registration (actix/examples shapes): a scope prefix joins
// the attribute route of a `.service(handler)`d fn, `web::resource(...)`
// chains route through `web::VERB().to(handler)`, and a bare
// `.route("/x", web::get().to(h))` stays unprefixed.
fn boot() {
    let _app = actix_web::App::new()
        .service(web::scope("/portal").service(ledger_lines))
        .service(
            web::scope("/api2").service(
                web::resource("/crates2")
                    .route(web::get().to(list_crates2))
                    .route(web::post().to(add_crate2)),
            ),
        )
        .route("/direct-ping", web::get().to(direct_ping));
}
