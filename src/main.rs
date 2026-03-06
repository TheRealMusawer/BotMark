use std::{net::SocketAddr, sync::Arc, time::Duration};
use clap::Parser;
use client::Client;
use tokio::{net::TcpStream, time::timeout};

mod client;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    #[arg(long)]
    pub ip: String,
    #[arg(short, long, default_value_t = 1)]
    pub count: u32,
    #[arg(short, long, default_value_t = 200)]
    pub delay: u64,
    #[arg(short, long, default_value_t = 5000)]
    pub timeout: u64,
    #[arg(long, default_value = "Bot Active")]
    pub spam_message: Option<String>,
    #[arg(long, default_value_t = 150)]
    pub spam_message_delay_min: u32,
    #[arg(long, default_value_t = 250)]
    pub spam_message_delay_max: u32,
    #[arg(long, default_value_t = true)]
    pub enable_rotation: bool,
    #[arg(long, default_value_t = true)]
    pub enable_swing: bool,
}

async fn start_web_dashboard(bot: Arc<Client>) {
    let port = std::env::var("PORT").unwrap_or_else(|_| "10000".to_string());
    let addr = format!("0.0.0.0:{}", port);
    let server = tiny_http::Server::http(&addr).unwrap();
    log::info!("Dashboard active at http://0.0.0.0:{}", port);
    
    tokio::task::spawn_blocking(move || {
        for request in server.incoming_requests() {
            let response_text = match request.url() {
                "/move" => {
                    bot.set_afk(true);
                    "Anti-AFK Movement: ENABLED"
                },
                "/stop" => {
                    bot.set_afk(false);
                    "Anti-AFK Movement: DISABLED"
                },
                "/status" => {
                    "Bot is online and connected."
                },
                _ => "Bot Dashboard. Use /move or /stop to control movement.",
            };
            let response = tiny_http::Response::from_string(response_text);
            let _ = request.respond(response);
        }
    });
}

#[tokio::main]
async fn main() {
    simple_logger::init_with_level(log::Level::Info).unwrap();
    let args = Arc::new(Args::parse());
    let address = args.ip.parse::<SocketAddr>().expect("Invalid IP:Port. Use format 127.0.0.1:25565");

    log::info!("Connecting to {}...", address);
    let stream = match timeout(Duration::from_millis(args.timeout), TcpStream::connect(address)).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => panic!("Connect Failed: {}", e),
        Err(_) => panic!("Connect Timeout"),
    };

    let client = Arc::new(Client::new(stream));
    
    // Start Web Dashboard for Render health checks and remote control
    start_web_dashboard(client.clone()).await;

    // Join Server sequence
    client.join_server(address, "BotMark".to_string()).await;

    let cloned_args = args.clone();
    let bot = client.clone();
    
    let bot_task = tokio::spawn(async move {
        let mut tick_interval = tokio::time::interval(Duration::from_millis(50));
        loop {
            tokio::select! {
                // Process incoming packets (KeepAlive, Teleport, etc)
                still_connected = bot.process_packets() => { 
                    if !still_connected { 
                        log::warn!("Disconnected from server.");
                        break; 
                    } 
                }
                // Handle Movement/Rotation/Swing ticks
                _ = tick_interval.tick() => { 
                    bot.tick(&cloned_args).await; 
                }
            }
        }
    });

    // Wait for shutdown signal
    tokio::signal::ctrl_c().await.unwrap();
    log::info!("Shutting down...");
    bot_task.abort();
}
