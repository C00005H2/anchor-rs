use actix_web::{post, web, HttpResponse, Responder};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize, Debug)]
pub struct LoginForm {
    account: String,
    action: String,
    brand: String,
    channel: String,
    game: String,
    game_version: String,
    loginType: String,
    model: String,
    package_name: String,
    password: String,
    platform: String,
    sdk_version: String,
    source_id: String,
    sub_chl: String,
    system: String,
    tmp_uid: String,
    token: String,
    ts: String,
    version_code: String,
    version_name: String,
    sign: String,
}

#[post("/v1/User/Login")]
async fn user_login(form: web::Form<LoginForm>) -> impl Responder {
    println!(
        "[Login] account={} type={} game={} ts={}",
        form.account, form.loginType, form.game, form.ts
    );

    HttpResponse::Ok().json(json!({
        "code": 1,
        "msg": "success",
        "d": {
            "account_name": "117682939561437290736",
            "account_type": 3,
            "loginType": form.loginType,
            "game": form.game,
            "uid": "210696_15_1",
            "isNew": 0,
            "token": "hEoVD2lT7dJTeptPXLucn2d6d0VUMDBlNXEwL1oyeDVmaWxOVzhmYjc5RS9lN3RGb3pYV3lsdEZZQjNocUZDUy93cXM3RTlNS1c4REhMMXI4VmZHOXcrWVhiTHkvRGdxbkEzRmVSSXNPMFdaZWlKY1lyR3NnQk1TejAzNnFHM2ZQMURBNHhOQ09aUjhPZFhHR3FWb0dyQTFDS1d0eUdBb0c5ZEE3dz09",
            "account": form.account,
            "password": form.password,
            "account_state": 0
        },
        "ec": false
    }))
}

pub fn config(cfg: &mut actix_web::web::ServiceConfig) {
    cfg.service(user_login);
}
