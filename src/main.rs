use std::{net::SocketAddr, sync::Arc, time::Duration};
use clap::Parser;
use client::Client;
use tokio::{net::TcpStream, time::timeout};
use tiny_http::{Server, Response};

mod client;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    #[arg(long)]
    pub ip: String,
    #[arg(long, default_value = "BotMark")]
    pub username: String,
    #[arg(short, long, default_value_t = 5000)]
    pub timeout: u64,
    #[arg(long, default_value_t = true)]
    pub enable_rotation: bool,
    #[arg(long, default_value_t = true)]
    pub enable_swing: bool,
}

async fn start_web_dashboard(bot: Arc<Client>) {
    // Render looks for the PORT env var. Default to 10000.
    let port = std::env::var("PORT").unwrap_or_else(|_| "10000".to_string());
    let addr = format!("0.0.0.0:{}", port);
    let server = Server::http(&addr).expect("Failed to bind to port");
    log::info!("Dashboard active on http://{}", addr);
    
    // Use spawn_blocking because tiny_http is synchronous
    tokio::task::spawn_blocking(move || {
        for request in server.incoming_requests() {
            let url = request.url();
            
            let response_text = if url.starts_with("/cmd?") {
                // Example: /cmd?msg=/home%20afk
                let command = url.split("msg=").last().unwrap_or("");
                let decoded = urlencoding::decode(command).unwrap_or_default().into_owned();
                
                if !decoded.is_empty() {
                    let b = bot.clone();
                    tokio::spawn(async move {
                        b.send_chat_or_cmd(&decoded).await;
                    });
                    format!("Executed: {}", decoded)
                } else {
                    "No command provided.".to_string()
                }
            } else {
                match url {
                    "/move" => { bot.set_afk(true); "Anti-AFK: ENABLED".to_string() },
                    "/stop" => { bot.set_afk(false); "Anti-AFK: DISABLED".to_string() },
                    "/status" => "Bot is online and connected.".to_string(),
                    _ => "BotMark Dashboard. Usage: /move, /stop, or /cmd?msg=YOUR_COMMAND".to_string(),
                }
            };

            let _ = request.respond(Response::from_string(response_text));
        }
    });
}

#[tokio::main]
async fn main() {
    simple_logger::init_with_level(log::Level::Info).unwrap();
    let args = Arc::new(Args::parse());
    
    let address = args.ip.parse::<SocketAddr>().expect("Invalid IP:Port (e.g. 127.0.0.1:25565)");

    log::info!("Connecting to {} as {}...", address, args.username);
    
    let stream = match timeout(Duration::from_millis(args.timeout), TcpStream::connect(address)).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => { log::error!("Connection error: {}", e); return; }
        Err(_) => { log::error!("Connection timed out"); return; }
    };

    let client = Arc::new(Client::new(stream));
    
    // 1. Start Dashboard (Satisfies Render's health check)
    start_web_dashboard(client.clone()).await;

    // 2. Initial Handshake & Login (Cracked support)
    client.join_server(address, args.username.clone()).await;

    // 3. Network Packet Loop
    let bot_reader = client.clone();
    let reader_task = tokio::spawn(async move {
        loop {
            if !bot_reader.process_packets().await {
                log::warn!("Disconnected from server.");
                break;
            }
        }
    });

    // 4. Bot Tick Loop (Movement, Rotation, Swing)
    let bot_ticker = client.clone();
    let ticker_args = args.clone();
    let ticker_task = tokio::spawn(async move {
        let mut tick_interval = tokio::time::interval(Duration::from_millis(50));
        loop {
            tick_interval.tick().await;
            bot_ticker.tick(&ticker_args).await;
        }
    });

    tokio::select! {
        _ = tokio::signal::ctrl_c() => log::info!("Shutting down..."),
        _ = reader_task => log::error!("Network task ended."),
        _ = ticker_task => log::error!("Ticker task ended."),
    }
}
