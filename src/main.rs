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
    let server = tiny_http::Server::http(&addr).expect("Failed to bind to port");
    log::info!("Dashboard active on port {}", port);
    
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
                "/status" => "Bot is online.",
                _ => "Bot Dashboard. Use /move or /stop",
            };
            let _ = request.respond(tiny_http::Response::from_string(response_text));
        }
    });
}

#[tokio::main]
async fn main() {
    simple_logger::init_with_level(log::Level::Info).unwrap();
    let args = Arc::new(Args::parse());
    
    let address = args.ip.parse::<SocketAddr>().expect("Invalid IP:Port");

    log::info!("Connecting to {}...", address);
    let stream = match timeout(Duration::from_millis(args.timeout), TcpStream::connect(address)).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            log::error!("Connection error: {}", e);
            return;
        }
        Err(_) => {
            log::error!("Connection timed out");
            return;
        }
    };

    let client = Arc::new(Client::new(stream));
    
    // Start Web Dashboard immediately for Render's health check
    start_web_dashboard(client.clone()).await;

    // Start the Login sequence
    client.join_server(address, "BotMark".to_string()).await;

    let bot_reader = client.clone();
    let reader_task = tokio::spawn(async move {
        loop {
            if !bot_reader.process_packets().await {
                log::warn!("Reader loop exited (Disconnected).");
                break;
            }
        }
    });

    let bot_ticker = client.clone();
    let ticker_args = args.clone();
    let ticker_task = tokio::spawn(async move {
        let mut tick_interval = tokio::time::interval(Duration::from_millis(50));
        loop {
            tick_interval.tick().await;
            bot_ticker.tick(&ticker_args).await;
        }
    });

    // Keep main alive until Ctrl+C
    tokio::select! {
        _ = tokio::signal::ctrl_c() => log::info!("Shutdown signal received."),
        _ = reader_task => log::error!("Reader task failed."),
    }
}
