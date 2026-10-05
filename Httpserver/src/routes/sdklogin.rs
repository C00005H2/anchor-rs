use actix_web::{post, web, HttpResponse, Responder};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize, Debug)]
pub struct SdkLoginForm {
    time: String,
    data: String,
    sign: String,
}

#[post("/User/Login/g/{gid}")]
async fn sdk_user_login(form: web::Form<SdkLoginForm>, path: web::Path<(u32,)>) -> impl Responder {
    let gid = path.into_inner().0;
    tracing::info!(group = gid, request_time = %form.time, "SDK login request received");

    // In a real server you'd verify `sign`, parse `data` (URL-encoded key/value pairs)
    // But since you are emulating, we just skip checks and return a fixed response.

    HttpResponse::Ok().json(json!({
        "status": 1,
        "data": {
            "account_id": "210696_15_1",
            "adult": 2,
            "time": 1758055352,
            "token": "801dbafb9996e3c1035048e32e33a7b16d7437cb8fbc36a2f35af4c69706f426",
            "is_white": 0
        }
    }))
}

pub fn config(cfg: &mut actix_web::web::ServiceConfig) {
    cfg.service(sdk_user_login);
}
