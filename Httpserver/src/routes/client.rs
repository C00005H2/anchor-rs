use actix_web::{post, web, HttpResponse, Responder, HttpRequest};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
pub struct StepLogForm {
    data: String,
    time: String,
    sign: String, // we keep it in the struct so deserialization works, but ignore it
}

#[post("/Api/ClientServer/loginStepLogs/g/{gid}")]
async fn login_step_logs(
    req: HttpRequest,
    form: web::Form<StepLogForm>,
    path: web::Path<(u32,)>
) -> impl Responder {
    let gid = path.into_inner().0;
    println!("[StepLogs] group={gid}, data={}", form.data);

    // check cookie header only
    let has_cookie = req
        .headers()
        .get("cookie")
        .map(|v| v.to_str().unwrap_or("").contains("PHPSESSID"))
        .unwrap_or(false);

    if has_cookie {
        HttpResponse::Ok().json(json!({
            "status": 1,
            "data": form.time
        }))
    } else {
        HttpResponse::Ok().json(json!({
            "status": 1,
            "errorcode": 0,
            "message": "ok"
        }))
    }
}

pub fn config(cfg: &mut actix_web::web::ServiceConfig) {
    cfg.service(login_step_logs);
}
