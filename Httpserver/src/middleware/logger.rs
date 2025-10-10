use actix_web::Error;
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::middleware::Next;

#[allow(non_snake_case)]
pub async fn Logger<B>(req: ServiceRequest, next: Next<B>) -> Result<ServiceResponse<B>, Error> {
    let method = req.method().clone();
    let path = req.path().to_string();
    tracing::info!("Incoming: {} {}", method, path);
    let rsp = next.call(req).await?;
    let status = rsp.status();
    tracing::info!("{} - {} {}", status, method, path);
    Ok(rsp)
}
