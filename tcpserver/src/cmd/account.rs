use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

use crate::data_loader::GameDataLoader;
use crate::messages::CS_ACCOUNT_LOGIN;
use crate::state::ConnectionContext;

/// Handle CS_ACCOUNT_LOGIN (11000) -> prepare initialization packets.
pub async fn handle_account_login(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_ACCOUNT_LOGIN,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    // Avoid logging account names, device tokens, and login credentials.
    info!(
        device_platform = request.dev_platform_type,
        server_id = request.srv_id,
        channel_id = request.channel_id,
        "Processing login"
    );

    // For now, use a fixed account id until the local account store is implemented.
    let account_id = 7825473380164860269;
    let server_time = chrono::Utc::now().timestamp() as i32;

    {
        let mut connection = ctx.lock().await;
        connection.logged_in = true;
        connection.player_id = Some(account_id);
        connection.session_id = format!("{:032x}", rand::random::<u128>());
        connection.update_heartbeat();
    }

    info!(player_id = account_id, "Login accepted");
    GameDataLoader::load_login_sequence(account_id, server_time)
}
