use rmcp::{ServiceExt, transport::stdio};

use opencut_mcp::OpenCutServer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    OpenCutServer::new().serve(stdio()).await?.waiting().await?;
    Ok(())
}
