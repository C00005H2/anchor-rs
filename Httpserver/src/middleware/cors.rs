use actix_web::body::{BoxBody, EitherBody, MessageBody};
use actix_web::{
    Error, HttpResponse,
    dev::{ServiceRequest, ServiceResponse},
    http::{Method, header},
    middleware::Next,
};

pub async fn cors_handler<B>(
    req: ServiceRequest,
    next: Next<B>,
) -> Result<ServiceResponse<EitherBody<B, BoxBody>>, Error>
where
    B: MessageBody + 'static,
{
    if req.method() == Method::OPTIONS {
        let response = HttpResponse::Ok()
            .insert_header((header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"))
            .insert_header((
                header::ACCESS_CONTROL_ALLOW_METHODS,
                "GET, POST, PUT, OPTIONS",
            ))
            .insert_header((header::ACCESS_CONTROL_ALLOW_HEADERS, "*"))
            .finish();

        return Ok(req.into_response(response.map_into_right_body()));
    }

    let res = next.call(req).await?;
    Ok(res.map_into_left_body())
}
