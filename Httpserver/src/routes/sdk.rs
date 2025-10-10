use actix_web::{post, web, HttpResponse, Responder};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
pub struct InitForm {
    brand: String,
    channel: String,
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

#[post("/v1/AppSdk/init")]
async fn app_sdk_init(_form: web::Form<InitForm>) -> impl Responder {
    // Ignore validation, always return a success response
    HttpResponse::Ok().json(json!({
        "code": 1,
        "msg": "success",
        "d": {
            "af_log_stat": 1,
            "token": "JzqCQVRoN98nY9WK0pQUU0gvT08rOUh1MmtYdlk4YmZrUTJSS0ZxU09yc3JoNlZNS2tvVVJqajRWcVRHM1pENEJrTUpQSEk1SHRaNmJUdkxQK3ZhWlJaRDhVa3ZONmVkenU1d0ZUbVdZaEN6NWhramMvUnhrbG5rMXdLOTdqd0NnN3M5MlplbWF6d0RIZzFSS2NyeUdRMHBiV3ZGUW5yQnZxZVBiS0xwYmd0anZBS0ZwVFFYcy9TNnFTcU5nQW1OcXdXeWZpR2lXY045R0pvRw%3D%3D",
            "login_page": "https://api-us.51haodong.com/v1/AppGame/index",
            "recharge_level": 99,
            "quick_stat": "1",
            "isOpenDelete": "1",
            "is_open_PioneerTesting": "0",
            "google_login_open": "1",
            "open_facebook_login": "1",
            "unsubscribe_link": "https://static-dev.51haodong.com/pravicy/en/index.html?platform=android"
        },
        "ec": false
    }))
}

pub fn config(cfg: &mut actix_web::web::ServiceConfig) {
    cfg.service(app_sdk_init);
}
