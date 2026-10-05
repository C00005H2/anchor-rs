mod routes;
mod utils;
mod middleware;

use actix_web::{middleware::from_fn, middleware::Logger, App, HttpServer};
use rustls::{pki_types::{CertificateDer, PrivateKeyDer}, ServerConfig};
use rustls_pemfile::{certs, pkcs8_private_keys};
use std::error::Error;
use std::fs::File;
use std::io::BufReader;
use tracing::{info, warn};

use crate::middleware::{cors::cors_handler, logger::Logger as RequestLogger};
use common::{init_tracing, CERT_FILE_PATH, HOST, HTTPS_PORT, HTTP_PORT, KEY_FILE_PATH};

#[actix_web::main]
async fn main() -> Result<(), Box<dyn Error>> {
    init_tracing();

    let bind_host = std::env::var("HTTP_BIND_HOST").unwrap_or_else(|_| HOST.to_owned());
    let http_addr = format!("{}:{}", bind_host, HTTP_PORT);
    let https_addr = format!("{}:{}", bind_host, HTTPS_PORT);

    let tls_config = load_tls_config()?;
    let factory = || {
        App::new()
            .wrap(from_fn(RequestLogger))
            .wrap(from_fn(cors_handler))
            .wrap(Logger::default())
            .configure(routes::health::config)
            .configure(routes::webkey::config)
            .configure(routes::user::config)
            .configure(routes::client::config)
            .configure(routes::gamereport::config)
            .configure(routes::ipinfo::config)
            .configure(routes::sdk::config)
            .configure(routes::common::config)
            .configure(routes::generic::config)
            .configure(routes::serverlist::config)
            .configure(routes::bullentin::config)
            .configure(routes::sdklogin::config)
    };

    info!("Running HTTP on {}", http_addr);
    let http_server = HttpServer::new(factory).bind(&http_addr)?.run();

    if let Some(tls_config) = tls_config {
        info!("Running HTTPS on {}", https_addr);
        let https_server = HttpServer::new(factory)
            .bind_rustls_0_22(&https_addr, tls_config)?
            .run();
        tokio::try_join!(http_server, https_server)?;
    } else {
        warn!(
            "TLS certificate or key not found; serving HTTP only. Configure cert/localhost.crt and cert/localhost.key to enable HTTPS."
        );
        http_server.await?;
    }

    Ok(())
}

/// TLS is optional because the development certificate directory is excluded
/// from version control. A missing certificate must not prevent HTTP routes
/// from starting; a present but invalid certificate still fails explicitly.
fn load_tls_config() -> Result<Option<ServerConfig>, Box<dyn Error>> {
    if !CERT_FILE_PATH.is_file() || !KEY_FILE_PATH.is_file() {
        return Ok(None);
    }

    info!("Loading TLS certificate from: {}", CERT_FILE_PATH.display());
    info!("Loading TLS private key from: {}", KEY_FILE_PATH.display());

    let cert_file = File::open(&*CERT_FILE_PATH)?;
    let mut cert_reader = BufReader::new(cert_file);
    let cert_chain: Vec<CertificateDer> = certs(&mut cert_reader).collect::<Result<_, _>>()?;
    if cert_chain.is_empty() {
        return Err("certificate file contains no certificates".into());
    }

    // Try PKCS#8 first, then RSA PEM keys for compatibility with older certs.
    let key_file = File::open(&*KEY_FILE_PATH)?;
    let mut key_reader = BufReader::new(key_file);
    let mut keys: Vec<PrivateKeyDer> = pkcs8_private_keys(&mut key_reader)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(PrivateKeyDer::Pkcs8)
        .collect();

    if keys.is_empty() {
        let key_file = File::open(&*KEY_FILE_PATH)?;
        let mut key_reader = BufReader::new(key_file);
        keys = rustls_pemfile::rsa_private_keys(&mut key_reader)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(PrivateKeyDer::from)
            .collect();
    }
    let private_key = keys
        .into_iter()
        .next()
        .ok_or("no usable PKCS#8 or RSA private key found")?;

    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, private_key)?;
    Ok(Some(config))
}
