use actix_web::{post, web, HttpResponse, Responder};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
pub struct IpForm {
    time: String,
    data: String,
    sign: String, // still kept for compatibility, but ignored
}

#[post("/Api/IpInfo/getClientIP/g/{gid}")]
async fn get_client_ip(form: web::Form<IpForm>, path: web::Path<(u32,)>) -> impl Responder {
    let gid = path.into_inner().0;

    tracing::info!(group = gid, request_time = %form.time, "Client IP requested");

    // Always return success — skip signature validation
    HttpResponse::Ok().json(json!({
        "status": 1,
        "data": {
            "ip": "10.11.79.244",
            "address": "局域网"
        }
    }))
}

pub fn config(cfg: &mut actix_web::web::ServiceConfig) {
    cfg.service(get_client_ip);
}
