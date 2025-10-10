use actix_web::{post, web, HttpResponse, Responder};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
pub struct CollectForm {
    time: String,
    data: String,
    sign: String, // still included for deserialization compatibility
}

#[post("/Api/GameReport/checkCollect/g/{gid}")]
async fn check_collect(form: web::Form<CollectForm>, path: web::Path<(u32,)>) -> impl Responder {
    let gid = path.into_inner().0;
    println!("[GameReport] group={gid}, data={}", form.data);

    // Ignore signature completely, always return a fixed response
    HttpResponse::Ok().json(json!({
        "status": 0,
        "errorcode": 5,
        "message": "数据不存在"
    }))
}

pub fn config(cfg: &mut actix_web::web::ServiceConfig) {
    cfg.service(check_collect);
}
