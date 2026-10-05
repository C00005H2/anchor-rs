use actix_web::{get, web, HttpResponse, Responder};
use serde_json::json;

#[get("/health")]
async fn health() -> impl Responder {
    HttpResponse::Ok().json(json!({ "status": "ok" }))
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(health);
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{test, App};

    #[actix_web::test]
    async fn returns_a_simple_readiness_response() {
        let app = test::init_service(App::new().configure(config)).await;
        let response: serde_json::Value =
            test::call_and_read_body_json(&app, test::TestRequest::get().uri("/health").to_request())
                .await;
        assert_eq!(response["status"], "ok");
    }
}
