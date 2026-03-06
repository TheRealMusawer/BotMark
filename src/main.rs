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
    let server = tiny_http::Server::http(format!("0.0.0.0:{}", port)).unwrap();
    log::info!("Dashboard active at http://your-app.onrender.com");
    
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
    let address = args.ip.parse::<SocketAddr>().expect("Invalid IP:Port");

    let stream = timeout(Duration::from_millis(args.timeout), TcpStream::connect(address))
        .await.expect("Connect Timeout").expect("Connect Failed");

    let client = Arc::new(Client::new(stream));
    
    // Start Web Dashboard for stealth control
    start_web_dashboard(client.clone()).await;

    client.join_server(address, "HelperBot".to_string()).await;

    let cloned_args = args.clone();
    let bot = client.clone();
    
    let bot_task = tokio::spawn(async move {
        let mut tick_interval = tokio::time::interval(Duration::from_millis(50));
        loop {
            tokio::select! {
                res = bot.process_packets() => { if !res { break; } }
                _ = tick_interval.tick() => { bot.tick(&cloned_args).await; }
            }
        }
    });

    tokio::signal::ctrl_c().await.unwrap();
    bot_task.abort();
}
