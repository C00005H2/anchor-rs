use std::sync::Arc;
use tokio::sync::Mutex;
use crate::messages::{CS_SYS_PING, SC_SYS_PING};
use crate::state::ConnectionContext;
use crate::packet::build_server_packet;

/// Handle CS_SYS_PING (10000) -> SC_SYS_PING (10001)
pub async fn handle_ping(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_SYS_PING,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    {
        let mut connection = ctx.lock().await;
        connection.update_heartbeat();
    }

    let response = SC_SYS_PING {
        time: chrono::Utc::now().timestamp() as i32,
    };

    Ok(vec![build_server_packet(10001, &response.encode())?])
}
