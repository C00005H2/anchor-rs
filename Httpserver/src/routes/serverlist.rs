use std::collections::HashMap;
use actix_web::{post, web, HttpResponse, Responder};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
pub struct PfForm {
    time: String,
    data: String,
    sign: String,
}

fn parse_urlencoded(data: &str) -> HashMap<String, String> {
    url::form_urlencoded::parse(data.as_bytes())
        .into_owned()
        .collect::<HashMap<_, _>>()
}

#[post("/ApiServer/pfRecommendSrvList/g/{gid}")]
async fn pf_recommend_srv_list(
    form: web::Form<PfForm>,
    path: web::Path<(u32,)>,
) -> impl Responder {
    let gid = path.into_inner().0;
    println!("[PfRecommendSrvList] group={gid}, data={}", form.data);

    let params = parse_urlencoded(&form.data);
    let requested_srv_id = params.get("srv_id");

    // Stubbed server list response (copied from your capture)
    let servers = json!({
        "1822010005": {
            "srv_id": "1822010005",
            "logsrv_id": "10005",
            "domain": "127.0.0.1",
            "gateway": "",
            "client_port": "8702",
            "status": "1",
            "open_time": "1757666400",
            "srv_type": "18220001",
            "maintain_notice": "维护",
            "maintain_time": "1757678400",
            "is_merge": "0",
            "srv_name": "Global 10005 server",
            "is_recommend": "0",
            "is_rec_list": "1",
            "is_hot_update": "0",
            "style_status": "0",
            "cdn": "http://mdjl-cdn.51haodong.com/windows/laoqb_release_lw_en_1",
            "is_grayscale": "0",
            "sort": 6
        },
        "1822010004": {
            "srv_id": "1822010004",
            "logsrv_id": "10004",
            "domain": "127.0.0.1",
            "gateway": "",
            "client_port": "8702",
            "status": "1",
            "open_time": "1757666400",
            "srv_type": "18220001",
            "maintain_notice": "维护.",
            "maintain_time": "1757678400",
            "is_merge": "0",
            "srv_name": "Global 10004 server",
            "is_recommend": "0",
            "is_rec_list": "1",
            "is_hot_update": "0",
            "style_status": "0",
            "cdn": "http://mdjl-cdn.51haodong.com/windows/laoqb_release_lw_en_1",
            "is_grayscale": "0",
            "sort": 5
        },
        "1822010003": {
            "srv_id": "1822010003",
            "logsrv_id": "10003",
            "domain": "127.0.0.1",
            "gateway": "",
            "client_port": "8702",
            "status": "1",
            "open_time": "1757666400",
            "srv_type": "18220001",
            "maintain_notice": "维护",
            "maintain_time": "1757678400",
            "is_merge": "0",
            "srv_name": "Global 10003 servers",
            "is_recommend": "0",
            "is_rec_list": "1",
            "is_hot_update": "0",
            "style_status": "0",
            "cdn": "http://mdjl-cdn.51haodong.com/windows/laoqb_release_lw_en_1",
            "is_grayscale": "0",
            "sort": 4
        },
        "1822010002": {
            "srv_id": "1822010002",
            "logsrv_id": "10002",
            "domain": "127.0.0.1",
            "gateway": "",
            "client_port": "8702",
            "status": "1",
            "open_time": "1757666400",
            "srv_type": "18220001",
            "maintain_notice": "维护",
            "maintain_time": "1757678400",
            "is_merge": "0",
            "srv_name": "Global 10002 servers",
            "is_recommend": "0",
            "is_rec_list": "1",
            "is_hot_update": "0",
            "style_status": "0",
            "cdn": "http://mdjl-cdn.51haodong.com/windows/laoqb_release_lw_en_1",
            "is_grayscale": "0",
            "sort": 3
        },
        "1822010001": {
            "srv_id": "1822010001",
            "logsrv_id": "10001",
            "domain": "127.0.0.1",
            "gateway": "",
            "client_port": "8702",
            "status": "1",
            "open_time": "1757666400",
            "srv_type": "18220001",
            "maintain_notice": "维护",
            "maintain_time": "1757678400",
            "is_merge": "0",
            "srv_name": "Global 10001 servers",
            "is_recommend": "0",
            "is_rec_list": "1",
            "is_hot_update": "0",
            "style_status": "0",
            "cdn": "http://mdjl-cdn.51haodong.com/windows/laoqb_release_lw_en_1",
            "is_grayscale": "0",
            "sort": 2
        },
        "1822010000": {
            "srv_id": "1822010000",
            "logsrv_id": "10000",
            "domain": "127.0.0.1",
            "gateway": "",
            "client_port": "8702",
            "status": "1",
            "open_time": "1757666400",
            "srv_type": "18220001",
            "maintain_notice": "维护",
            "maintain_time": "1757678400",
            "is_merge": "0",
            "srv_name": "Global 10000 servers",
            "is_recommend": "0",
            "is_rec_list": "1",
            "is_hot_update": "0",
            "style_status": "0",
            "cdn": "http://mdjl-cdn.51haodong.com/windows/laoqb_release_lw_en_1",
            "is_grayscale": "0",
            "sort": 1
        }
    });

    let response_data = if let Some(srv_id) = requested_srv_id {
        // Only return the requested server if found
        servers.get(srv_id).cloned().map(|srv| {
            json!({ srv_id: srv })
        }).unwrap_or_else(|| json!({}))
    } else {
        servers
    };

    HttpResponse::Ok().json(json!({
        "status": 1,
        "data": response_data
    }))
}


pub fn config(cfg: &mut actix_web::web::ServiceConfig) {
    cfg.service(pf_recommend_srv_list);
}
