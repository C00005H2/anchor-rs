use actix_web::{post, web, HttpResponse, Responder};
use serde::Deserialize;
use serde_json::json;

use crate::utils::sign::check_sign;

#[derive(Deserialize)]
pub struct LoginForm {
    pub(crate) account: String,
    pub(crate) password: String,
    token: String,
    #[serde(rename = "loginType")]
    pub(crate) login_type: String,
    pub(crate) game: String,
    pub(crate) platform: String,
    pub(crate) ts: String,
    pub(crate) sign: String,
}

#[post("/v1/User/Login")]
async fn login(form: web::Form<LoginForm>) -> impl Responder {
    // Avoid logging account credentials, tokens, or signatures.
    tracing::info!(game = %form.game, "Received login request");

    if !check_sign(&form) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_expected_form_signature_and_rejects_a_tampered_one() {
        let signing_text = "account=test-account&action=quick_login&game=demo&loginType=1&password=secret&platform=android&ts=12345yXwBqDoXY3wTzo1DFf";
        let sign = format!("{:x}", md5::compute(signing_text.as_bytes()));
        let form = LoginForm {
            account: "test-account".into(),
            password: "secret".into(),
            token: String::new(),
            login_type: "1".into(),
            game: "demo".into(),
            platform: "android".into(),
            ts: "12345".into(),
            sign,
        };
        assert!(check_sign(&form));

        let mut tampered = form;
        tampered.password.push('!');
        assert!(!check_sign(&tampered));
    }
}
