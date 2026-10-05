use actix_web::{post, web, HttpResponse, Responder};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
pub struct LogForm {
    brand: String,
    channel: String,
    data: String,
    eventType: String,
    game: String,
    game_version: String,
    model: String,
    package_name: String,
    platform: String,
    sdk_version: String,
    source_id: String,
    sub_chl: String,
    system: String,
    tmp_uid: String,
    ts: String,
    version_code: String,
    version_name: String,
    sign: String,
}

#[post("/v1/LogHandle/Common")]
async fn log_handle_common(form: web::Form<LogForm>) -> impl Responder {
    tracing::info!(event_type = %form.eventType, "Common log event received");

    HttpResponse::Ok().json(json!({
        "code": 1,
        "msg": format!("success:{}", form.eventType),
        "d": [],
        "ec": false
    }))
}

pub fn config(cfg: &mut actix_web::web::ServiceConfig) {
    cfg.service(log_handle_common);
}
