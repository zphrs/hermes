use sky::server::SkyServer;
use tracing_subscriber::fmt::time::tokio_uptime;

#[tokio::main]
async fn main() {
    // Create a `fmt` subscriber that uses our custom event format, and set it
    // as the default.
    tracing_subscriber::fmt()
        .pretty()
        .with_test_writer()
        .with_timer(tokio_uptime())
        .init();
    let ss = SkyServer::new().await.unwrap();
    ss.run().await.unwrap().unwrap();
}
