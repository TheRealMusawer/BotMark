use crossbeam::atomic::AtomicCell;
use pumpkin_data::packet::CURRENT_MC_PROTOCOL;
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::java::client::config::{CConfigDisconnect, CFinishConfig};
use pumpkin_protocol::java::client::login::{
    CEncryptionRequest, CLoginDisconnect, CLoginSuccess, CSetCompression,
};
use pumpkin_protocol::java::client::play::{CKeepAlive, CLogin, CPlayDisconnect, CPlayerPosition};
use pumpkin_protocol::java::packet_decoder::TCPNetworkDecoder;
use pumpkin_protocol::java::packet_encoder::TCPNetworkEncoder;
use pumpkin_protocol::java::server::config::{SAcknowledgeFinishConfig, SKnownPacks};
use pumpkin_protocol::java::server::handshake::SHandShake;
use pumpkin_protocol::java::server::login::{SLoginAcknowledged, SLoginStart};
use pumpkin_protocol::java::server::play::{
    SChatMessage, SConfirmTeleport, SKeepAlive, SPlayerLoaded, SPlayerPosition, SPlayerRotation,
    SSwingArm,
};
use pumpkin_protocol::ser::NetworkWriteExt;
use pumpkin_protocol::ser::{ReadingError, WritingError};
use pumpkin_protocol::{
    ClientPacket, CompressionLevel, CompressionThreshold, ConnectionState, PacketDecodeError,
    RawPacket, ServerPacket,
};
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::version::MinecraftVersion;
use std::io::Write;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};
use std::sync::{Arc, atomic::AtomicBool};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{BufReader, BufWriter};
use tokio::sync::Notify;
use tokio::{
    net::{
        TcpStream,
        tcp::{OwnedReadHalf, OwnedWriteHalf},
    },
    sync::Mutex,
};
use uuid::Uuid;

use crate::Args;

pub struct Client {
    pub connection_state: AtomicCell<ConnectionState>,
    pub closed: AtomicBool,
    pub network_writer: Arc<Mutex<TCPNetworkEncoder<BufWriter<OwnedWriteHalf>>>>,
    pub network_reader: Arc<Mutex<TCPNetworkDecoder<BufReader<OwnedReadHalf>>>>,
    close_interrupt: Arc<Notify>,
    entity_id: AtomicI32,
    message_spam_cooldown: AtomicU32,
    message_count: AtomicU32,
    is_loaded: AtomicBool,
    swing_cooldown: AtomicU32,
    afk_active: AtomicBool,
    // Position
    current_x: AtomicCell<f64>, current_y: AtomicCell<f64>, current_z: AtomicCell<f64>,
    velocity_x: AtomicCell<f64>, velocity_y: AtomicCell<f64>, velocity_z: AtomicCell<f64>,
    start_x: AtomicCell<f64>, start_z: AtomicCell<f64>,
    target_x: AtomicCell<f64>, target_z: AtomicCell<f64>,
    move_progress: AtomicCell<f32>, move_cooldown: AtomicU32,
    // Rotation
    current_yaw: AtomicCell<f32>, current_pitch: AtomicCell<f32>,
    start_yaw: AtomicCell<f32>, start_pitch: AtomicCell<f32>,
    target_yaw: AtomicCell<f32>, target_pitch: AtomicCell<f32>,
    rotation_progress: AtomicCell<f32>, rotation_cooldown: AtomicU32,
}

impl Client {
    pub fn new(stream: TcpStream) -> Self {
        let (connection_reader, connection_writer) = stream.into_split();
        Self {
            connection_state: AtomicCell::new(ConnectionState::HandShake),
            network_writer: Arc::new(Mutex::new(TCPNetworkEncoder::new(BufWriter::new(connection_writer)))),
            network_reader: Arc::new(Mutex::new(TCPNetworkDecoder::new(BufReader::new(connection_reader)))),
            entity_id: AtomicI32::new(0),
            velocity_x: AtomicCell::new(0.0), velocity_y: AtomicCell::new(0.0), velocity_z: AtomicCell::new(0.0),
            closed: AtomicBool::new(false),
            swing_cooldown: AtomicU32::new(0),
            close_interrupt: Arc::new(Notify::new()),
            message_spam_cooldown: AtomicU32::new(1),
            message_count: AtomicU32::new(0),
            is_loaded: AtomicBool::new(false),
            afk_active: AtomicBool::new(true), // Default to ON for stealth AFK
            rotation_cooldown: AtomicU32::new(0),
            rotation_progress: AtomicCell::new(1.0),
            current_yaw: AtomicCell::new(0.0), current_pitch: AtomicCell::new(0.0),
            start_yaw: AtomicCell::new(0.0), start_pitch: AtomicCell::new(0.0),
            target_yaw: AtomicCell::new(0.0), target_pitch: AtomicCell::new(0.0),
            current_x: AtomicCell::new(0.0), current_y: AtomicCell::new(0.0), current_z: AtomicCell::new(0.0),
            start_x: AtomicCell::new(0.0), start_z: AtomicCell::new(0.0),
            target_x: AtomicCell::new(0.0), target_z: AtomicCell::new(0.0),
            move_progress: AtomicCell::new(1.0),
            move_cooldown: AtomicU32::new(0),
        }
    }

    pub fn set_afk(&self, active: bool) {
        self.afk_active.store(active, Ordering::SeqCst);
        if active { self.move_cooldown.store(0, Ordering::Relaxed); }
    }

    pub async fn set_compression(&self, compression: Option<(usize, i32)>) {
        if let Some((threshold, _level)) = compression {
            self.network_reader.lock().await.set_compression(CompressionThreshold(threshold as u32));
            self.network_writer.lock().await.set_compression((CompressionThreshold(threshold as u32), CompressionLevel::Default));
        }
    }

    pub async fn send_packet<P: ClientPacket>(&self, packet: &P) {
        let mut buf = Vec::new();
        let mut writer = &mut buf;
        writer.write_var_int(&VarInt(P::PACKET_ID.latest_id)).unwrap();
        packet.write_packet_data(writer, &MinecraftVersion::V_1_21_1).unwrap();
        let _ = self.network_writer.lock().await.write_packet(buf.into()).await;
    }

    pub async fn tick(&self, args: &Args) {
        if self.connection_state.load() != ConnectionState::Play || !self.is_loaded.load(Ordering::Relaxed) { return; }
        if args.enable_rotation { self.tick_rotation().await; }
        if args.enable_swing { self.tick_swing().await; }
        if self.afk_active.load(Ordering::Relaxed) { self.tick_movement().await; }
    }

    async fn tick_movement(&self) {
        let progress = self.move_progress.load();
        if progress >= 1.0 {
            if self.move_cooldown.fetch_sub(1, Ordering::Relaxed) == 0 {
                let cz = self.current_z.load();
                self.start_z.store(cz);
                // "Up and Back" Toggle
                let target = if (self.target_z.load() - cz).abs() < 0.1 { cz + 0.5 } else { cz - 0.5 };
                self.target_z.store(target);
                self.move_progress.store(0.0);
                self.move_cooldown.store(rand::random_range(80..200), Ordering::Relaxed);
            }
        } else {
            let np = (progress + 0.08).min(1.0);
            self.move_progress.store(np);
            let t = 3.0 * np.powi(2) - 2.0 * np.powi(3);
            let nz = self.start_z.load() + (self.target_z.load() - self.start_z.load()) * t as f64;
            self.current_z.store(nz);
            self.send_packet(&SPlayerPosition {
                position: Vector3::new(self.current_x.load(), self.current_y.load(), nz),
                collision: 1, 
            }).await;
        }
    }

    async fn tick_rotation(&self) {
        let progress = self.rotation_progress.load();
        if progress >= 1.0 {
            if self.rotation_cooldown.fetch_sub(1, Ordering::Relaxed) == 0 {
                self.start_yaw.store(self.current_yaw.load());
                self.target_yaw.store(rand::random_range(-180.0..180.0));
                self.rotation_progress.store(0.0);
                self.rotation_cooldown.store(rand::random_range(40..100), Ordering::Relaxed);
            }
        } else {
            let np = (progress + 0.02).min(1.0);
            self.rotation_progress.store(np);
            let t = 3.0 * np.powi(2) - 2.0 * np.powi(3);
            let yaw = self.start_yaw.load() + (self.target_yaw.load() - self.start_yaw.load()) * t;
            self.current_yaw.store(yaw);
            self.send_packet(&SPlayerRotation { yaw, pitch: self.current_pitch.load(), ground: true }).await;
        }
    }

    async fn tick_swing(&self) {
        if self.swing_cooldown.fetch_sub(1, Ordering::Relaxed) == 0 {
            if rand::random_bool(0.02) {
                self.send_packet(&SSwingArm { hand: VarInt(0) }).await;
                self.swing_cooldown.store(rand::random_range(40..120), Ordering::Relaxed);
            }
        }
    }

    pub async fn handle_packet(&self, packet: &mut RawPacket) -> Result<(), ReadingError> {
        match self.connection_state.load() {
            ConnectionState::Login => self.handle_login_packet(packet).await?,
            ConnectionState::Config => self.handle_config_packet(packet).await?,
            ConnectionState::Play => self.handle_play_packet(packet).await?,
            _ => {}
        }
        Ok(())
    }

    async fn handle_login_packet(&self, packet: &mut RawPacket) -> Result<(), ReadingError> {
        let bytebuf = &packet.payload[..];
        match packet.id {
            id if id == CSetCompression::PACKET_ID => {
                let p = CSetCompression::read(bytebuf)?;
                self.set_compression(Some((p.threshold.0 as usize, 6))).await;
            }
            id if id == CLoginSuccess::PACKET_ID => {
                self.send_packet(&SLoginAcknowledged).await;
                self.connection_state.store(ConnectionState::Config);
                self.send_packet(&SKnownPacks { known_pack_count: VarInt(0) }).await;
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_config_packet(&self, packet: &mut RawPacket) -> Result<(), ReadingError> {
        if packet.id == CFinishConfig::PACKET_ID {
            self.send_packet(&SAcknowledgeFinishConfig).await;
            self.connection_state.store(ConnectionState::Play);
        }
        Ok(())
    }

    async fn handle_play_packet(&self, packet: &mut RawPacket) -> Result<(), ReadingError> {
        let bytebuf = &packet.payload[..];
        match packet.id {
            id if id == CKeepAlive::PACKET_ID => {
                let p = CKeepAlive::read(bytebuf)?;
                self.send_packet(&SKeepAlive { keep_alive_id: p.keep_alive_id }).await;
                self.move_cooldown.store(0, Ordering::Relaxed); // Autonomous Trigger
            }
            id if id == CPlayerPosition::PACKET_ID => {
                let p = CPlayerPosition::read(bytebuf)?;
                self.current_x.store(p.position.x); self.current_y.store(p.position.y); self.current_z.store(p.position.z);
                self.send_packet(&SConfirmTeleport { teleport_id: p.teleport_id }).await;
                self.is_loaded.store(true, Ordering::Relaxed);
            }
            id if id == CLogin::PACKET_ID => { self.send_packet(&SPlayerLoaded).await; }
            id if id == CPlayDisconnect::PACKET_ID => { self.close().await; }
            _ => {}
        }
        Ok(())
    }

    pub async fn join_server(&self, address: SocketAddr, name: String) {
        self.send_packet(&SHandShake { protocol_version: VarInt(CURRENT_MC_PROTOCOL as i32), server_address: address.ip().to_string(), server_port: address.port(), next_state: ConnectionState::Login }).await;
        self.connection_state.store(ConnectionState::Login);
        self.send_packet(&SLoginStart { name, uuid: Uuid::new_v4() }).await;
    }

    pub async fn close(&self) {
        self.close_interrupt.notify_waiters();
        self.closed.store(true, Ordering::Relaxed);
    }
}
