use actix_web::{post, web, HttpResponse, Responder};
use serde::Deserialize;
use serde_json::json;
use crate::utils::sign::check_sign;

#[derive(Deserialize)]
pub struct LoginForm {
    pub(crate) account: String,
    pub(crate) password: String,
    token: String,
    pub(crate) loginType: String,
    pub(crate) game: String,
    pub(crate) platform: String,
    pub(crate) ts: String,
    pub(crate) sign: String,
    // add more fields if needed
}

#[post("/v1/User/Login")]
async fn login(form: web::Form<LoginForm>) -> impl Responder {
    println!("[UserLogin] got request for account {}", form.account);

    let valid = check_sign(&form);
    if !valid {
        return HttpResponse::BadRequest().json(json!({
            "code": 1,
            "msg": "Invalid sign"
        }));
    }

    HttpResponse::Ok().json(json!({
        "code": 0,
        "msg": "Login success (emulated)",
        "d": {
            "uid": "210696_15_1",
            "account": form.account,
            "token": form.token,
            "game": form.game,
            "account_type": "3",
            "account_state": 0,
            "isNew": 0,
        }
    }))
}

pub fn config(cfg: &mut actix_web::web::ServiceConfig) {
    cfg.service(login);
}
