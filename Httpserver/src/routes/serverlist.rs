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
    let params = parse_urlencoded(&form.data);
    let requested_srv_id = params.get("srv_id");
    tracing::info!(group = gid, requested_srv_id = ?requested_srv_id, "Server list requested");
    let game_host = std::env::var("GAME_SERVER_HOST")
        .unwrap_or_else(|_| ::common::GAMESERVER.to_owned());
    let game_port = std::env::var("GAME_SERVER_PORT")
        .ok()
        .and_then(|port| port.parse::<u16>().ok())
        .unwrap_or(::common::GAMESERVER_PORT)
        .to_string();

    // Stubbed server list response (copied from your capture)
    let servers = json!({
        "1822010005": {
            "srv_id": "1822010005",
            "logsrv_id": "10005",
            "domain": game_host.as_str(),
            "gateway": "",
            "client_port": game_port.as_str(),
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
            "domain": game_host.as_str(),
            "gateway": "",
            "client_port": game_port.as_str(),
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
            "domain": game_host.as_str(),
            "gateway": "",
            "client_port": game_port.as_str(),
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
            "domain": game_host.as_str(),
            "gateway": "",
            "client_port": game_port.as_str(),
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
            "domain": game_host.as_str(),
            "gateway": "",
            "client_port": game_port.as_str(),
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
            "domain": game_host.as_str(),
            "gateway": "",
            "client_port": game_port.as_str(),
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
        // Preserve the server id as the dynamic map key. `json!({ srv_id: ... })`
        // stringifies an identifier and incorrectly returns a `"srv_id"` key.
        match servers.get(srv_id.as_str()).cloned() {
            Some(server) => {
                let mut selected = serde_json::Map::new();
                selected.insert(srv_id.to_owned(), server);
                serde_json::Value::Object(selected)
            }
            None => json!({}),
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{http::header, test, App};

    #[actix_web::test]
    async fn requested_server_is_keyed_by_its_actual_id() {
        let app = test::init_service(App::new().service(pf_recommend_srv_list)).await;
        let request = test::TestRequest::post()
            .uri("/ApiServer/pfRecommendSrvList/g/1")
            .insert_header((header::CONTENT_TYPE, "application/x-www-form-urlencoded"))
            .set_payload("time=1&data=srv_id%3D1822010005&sign=x")
            .to_request();
        let response: serde_json::Value = test::call_and_read_body_json(&app, request).await;
        let data = response.get("data").and_then(serde_json::Value::as_object).unwrap();
        assert!(data.contains_key("1822010005"));
        assert!(!data.contains_key("srv_id"));
    }
}
