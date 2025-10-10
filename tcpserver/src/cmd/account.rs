use std::sync::Arc;
use tokio::sync::Mutex;

use crate::messages::CS_ACCOUNT_LOGIN;
use crate::state::ConnectionContext;
use crate::data_loader::GameDataLoader;

/// Handle CS_ACCOUNT_LOGIN (11000) -> prepare multiple initialization packets
pub async fn handle_account_login(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_ACCOUNT_LOGIN,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    println!("Processing login for account: {}", request.acc_name);
    println!(
        "Device: {} ({})",
        request.dev_model, request.dev_platform_type
    );
    println!("Server ID: {}, Channel: {}", request.srv_id, request.channel_id);

    // For now, use a fixed account id
    let account_id = 7825473380164860269;
    let server_time = chrono::Utc::now().timestamp() as i32;

    {
        let mut connection = ctx.lock().await;
        connection.logged_in = true;
        connection.player_id = Some(account_id);
        connection.session_id = format!("{:032x}", rand::random::<u128>());
        connection.update_heartbeat();
    }

    println!("Login successful - Player ID: {}", account_id);

    // Load the initialization sequence
    let packets = GameDataLoader::load_login_sequence(account_id, server_time)?;

    Ok(packets)
}
