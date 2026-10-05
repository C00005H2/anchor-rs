use actix_web::body::{BoxBody, EitherBody, MessageBody};
use actix_web::{
    dev::{ServiceRequest, ServiceResponse},
    http::{header, header::HeaderValue, HeaderMap, Method},
    middleware::Next,
    Error, HttpResponse,
};

fn add_cors_headers(headers: &mut HeaderMap) {
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, POST, PUT, DELETE, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("content-type, authorization, x-requested-with"),
    );
}

pub async fn cors_handler<B>(
    req: ServiceRequest,
    next: Next<B>,
) -> Result<ServiceResponse<EitherBody<B, BoxBody>>, Error>
where
    B: MessageBody + 'static,
{
    if req.method() == Method::OPTIONS {
        let mut response = HttpResponse::NoContent().finish();
        add_cors_headers(response.headers_mut());
        response.headers_mut().insert(
            header::ACCESS_CONTROL_MAX_AGE,
            HeaderValue::from_static("86400"),
        );
        return Ok(req.into_response(response.map_into_right_body()));
    }

    let mut response = next.call(req).await?.map_into_left_body();
    // Add CORS headers to normal responses too; otherwise browsers can issue
    // preflight successfully but still block the actual API response.
    add_cors_headers(response.headers_mut());
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{middleware::from_fn, test, web, App};

    #[actix_web::test]
    async fn cors_headers_are_present_on_normal_and_preflight_responses() {
        let app = test::init_service(
            App::new()
                .wrap(from_fn(cors_handler))
                .route("/test", web::get().to(|| async { HttpResponse::Ok().finish() })),
        )
        .await;

        let normal = test::call_service(&app, test::TestRequest::get().uri("/test").to_request()).await;
        assert_eq!(
            normal
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .and_then(|value| value.to_str().ok()),
            Some("*")
        );

        let preflight = test::call_service(
            &app,
            test::TestRequest::default()
                .method(Method::OPTIONS)
                .uri("/test")
                .to_request(),
        )
        .await;
        assert_eq!(preflight.status(), actix_web::http::StatusCode::NO_CONTENT);
        assert_eq!(
            preflight
                .headers()
                .get(header::ACCESS_CONTROL_MAX_AGE)
                .and_then(|value| value.to_str().ok()),
            Some("86400")
        );
    }
}
