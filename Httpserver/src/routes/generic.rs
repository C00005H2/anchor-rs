use actix_web::{post, web, HttpResponse, Responder};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
pub struct GenericArgsForm {
    time: String,
    data: String,
    sign: String,
}

#[post("/ClientServer/genericArgs/g/{gid}")]
async fn generic_args(form: web::Form<GenericArgsForm>, path: web::Path<(u32,)>) -> impl Responder {
    let gid = path.into_inner().0;
    tracing::info!(group = gid, "Generic arguments requested");

    // Always reply with a stubbed "parameter error" (to mimic real server)
    HttpResponse::Ok().json(json!({
        "status": 0,
        "errorcode": 3,
        "message": "请求参数错误"
    }))
}

pub fn config(cfg: &mut actix_web::web::ServiceConfig) {
    cfg.service(generic_args);
}
