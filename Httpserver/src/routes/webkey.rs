use actix_web::{get, web, HttpResponse, Responder};
use serde_json::json;

#[get("/WebKeyConfig/getWebKeyConfig/g/{gid}")]
async fn get_webkey(path: web::Path<(u32,)>) -> impl Responder {
    let gid = path.into_inner().0;
    println!("[WebKeyConfig] request for group {gid}");

    HttpResponse::Ok().json(json!({
        "status": 1,
        "data": "yXwBqDoXY3wTzo1DFf"
    }))
}

pub fn config(cfg: &mut actix_web::web::ServiceConfig) {
    cfg.service(get_webkey);
}
