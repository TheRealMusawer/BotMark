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

// Render needs a web port open or it will shut down the bot
async fn start_web_server() {
    let port = std::env::var("PORT").unwrap_or_else(|_| "10000".to_string());
    let server = tiny_http::Server::http(format!("0.0.0.0:{}", port)).unwrap();
    log::info!("Web server (keep-alive) listening on port {}", port);
    tokio::task::spawn_blocking(move || {
        for request in server.incoming_requests() {
            let response = tiny_http::Response::from_string("Bot is Running");
            let _ = request.respond(response);
        }
    });
}

#[tokio::main]
async fn main() {
    simple_logger::init_with_level(log::Level::Info).unwrap();
    let args = Arc::new(Args::parse());
    let address = args.ip.parse::<SocketAddr>().expect("Invalid IP:Port format");

    // 1. Keep-alive for Render
    start_web_server().await;

    log::info!("Connecting single bot to {}...", address);
    
    let timeout_dur = Duration::from_millis(args.timeout);
    let stream_result = timeout(timeout_dur, TcpStream::connect(address)).await;

    let stream = match stream_result {
        Ok(Ok(s)) => s,
        _ => {
            log::error!("Failed to connect to {}", address);
            return;
        }
    };

    let client = Arc::new(Client::new(stream));
    
    // Connect as a single bot (Change name here)
    client.join_server(address, "EaglerBot".to_string()).await;

    let cloned_args = args.clone();
    let bot = client.clone();

    // 2. Main Logic Loop
    let bot_task = tokio::spawn(async move {
        let mut tick_interval = tokio::time::interval(Duration::from_millis(50));
        let mut afk_interval = tokio::time::interval(Duration::from_secs(30));
        
        loop {
            tokio::select! {
                // Handle Chat & Commands
                res = bot.process_packets() => {
                    if !res { 
                        log::warn!("Connection lost.");
                        break; 
                    }
                }
                // Anti-AFK (Move slightly every 30s)
                _ = afk_interval.tick() => {
                    log::info!("Anti-AFK Triggered: Moving...");
                    // This triggers the rotation/swing logic in client.rs
                    bot.tick(&cloned_args).await; 
                }
                // Standard Ticks
                _ = tick_interval.tick() => {
                    // Constant updates to keep connection alive
                    bot.tick(&cloned_args).await;
                }
            }
        }
    });

    // Handle shutdown
    tokio::signal::ctrl_c().await.unwrap();
    log::info!("Shutting down...");
    bot_task.abort();
}
