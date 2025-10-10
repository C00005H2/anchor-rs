mod routes;
mod utils;
mod middleware;


use rustls::{pki_types::{CertificateDer, PrivateKeyDer}, ServerConfig};
use rustls_pemfile::{certs, pkcs8_private_keys};
use std::fs::File;
use std::io::BufReader;

use crate::middleware::{cors::cors_handler, logger::Logger as Log};
use actix_web::{middleware::from_fn, App, HttpServer};
use tracing::info;

use common::{init_tracing, CERT_FILE_PATH, HOST, HTTPS_PORT, HTTP_PORT, KEY_FILE_PATH};
use std::error::Error;
use actix_web::middleware::Logger;

#[actix_web::main]
async fn main() -> Result<(), Box<dyn Error>> {
    init_tracing();

    let http_addr = format!("{}:{}", HOST, HTTP_PORT);
    let https_addr = format!("{}:{}", HOST, HTTPS_PORT);

    // Load RustTLS config directly in main
    info!("Loading RustTLS certificate from: {}", CERT_FILE_PATH.display());
    info!("Loading RustTLS private key from: {}", KEY_FILE_PATH.display());

    // Load certificate chain
    let cert_file = File::open(&*CERT_FILE_PATH)?;
    let mut cert_reader = BufReader::new(cert_file);
    let cert_chain: Vec<CertificateDer> = certs(&mut cert_reader)
        .collect::<Result<Vec<_>, _>>()?;

    // Load private key (try PKCS#8 first, then RSA fallback)
    let key_file = File::open(&*KEY_FILE_PATH)?;
    let mut key_reader = BufReader::new(key_file);

    let mut keys: Vec<PrivateKeyDer> = pkcs8_private_keys(&mut key_reader)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(PrivateKeyDer::Pkcs8)
        .collect();

    if keys.is_empty() {
        // rewind file and try RSA keys
        let key_file = File::open(&*KEY_FILE_PATH)?;
        let mut key_reader = BufReader::new(key_file);
        keys = rustls_pemfile::rsa_private_keys(&mut key_reader)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(PrivateKeyDer::from)
            .collect();
    }

    if keys.is_empty() {
        return Err("No usable private keys found in key file".into());
    }

    let private_key = keys.remove(0);


    // Build RustTLS config
    let rustls_config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, private_key)?;

    info!("RustTLS configuration loaded successfully");

    let factory = move || {
        App::new()
            .wrap(from_fn(Log))
            .wrap(from_fn(cors_handler))
            .wrap(Logger::default())
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
            .configure(routes::login::config)
            .configure(routes::sdklogin::config)
    };

    info!("Running HTTP on {}", http_addr);
    info!("Running HTTPS on {} (RustTLS)", https_addr);

    let http_server = HttpServer::new(factory).bind(&http_addr)?.run();

    let https_server = HttpServer::new(factory)
        .bind_rustls_0_22(&https_addr, rustls_config)?
        .run();

    tokio::try_join!(http_server, https_server)?;

    Ok(())
}