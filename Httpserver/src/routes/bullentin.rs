use actix_web::{post, web, HttpResponse, Responder};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
pub struct BulletinForm {
    time: String,
    data: String,
    sign: String,
}

#[post("/Api/Bulletin/getLoginBulletins/g/{gid}")]
async fn get_login_bulletins(
    form: web::Form<BulletinForm>,
    path: web::Path<(u32,)>,
) -> impl Responder {
    let gid = path.into_inner().0;
    println!("[Bulletin] group={gid}, data={}", form.data);

    HttpResponse::Ok().json(json!({
        "status": 1,
        "data": [
            {
                "tag": "Closed Beta Test Announcement",
                "title": "Game Notice",
                "titleImg": "",
                "content": [
                    "The game will be available on September 12, 2025. Specific release times are shown below:\n\
                     Eastern Standard Time: September 12th 07:00 (UTC-5)\n\
                     Greenwich Mean Time:  September 12th 12:00 (UTC 0)\n\
                     UTC+8 Time: September 12th 20:00 (UTC+8)\n\
                     Thank you for your patience. Please check the official announcement for details.\n\n\
                     【Game Introduction】\n\
                     Humanity rebuilt behind protective \"Skyborne Barriers\" and created enhanced soldiers called \
                     \"Operators\" using AIMBS technology. Though the war ended, global powers now fracture from within, \
                     and a new crisis threatens to unravel a fragile peace. The fate of the world once again hangs in balance. \
                     You, as a linker, collide with singular girls and uncover the truth of the world.\n"
                ],
                "open_srv_day_after": 0
            }
        ]
    }))
}

pub fn config(cfg: &mut actix_web::web::ServiceConfig) {
    cfg.service(get_login_bulletins);
}
