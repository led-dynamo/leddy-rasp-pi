#![forbid(unsafe_code)]

use futures_util::{SinkExt, StreamExt};
use leddy_interfaces::{
    DeviceCapabilities, DeviceCommand, DeviceEvent, DisplayConfig, PixelOrigin,
};
use leddy_lib::{FrameBuffer, content_width, render_text_5x7, scroll_offset};
use std::{env, time::Instant};
use tokio::time::{Duration, sleep};
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let device_id = env::var("LEDDY_DEVICE_ID").unwrap_or_else(|_| "pi-development".into());
    let url = env::var("LEDDY_DEVICE_WS_URL")
        .unwrap_or_else(|_| "ws://localhost:8080/v1/ws/devices".into());
    let width = env_u16("LEDDY_MATRIX_WIDTH", 100);
    let height = env_u16("LEDDY_MATRIX_HEIGHT", 10);
    let config = DisplayConfig {
        width,
        height,
        brightness: 96,
        serpentine: true,
        origin: PixelOrigin::TopLeft,
    };
    config.validate()?;

    loop {
        match run_session(&url, &device_id, config.clone()).await {
            Ok(()) => tracing::warn!("device socket closed"),
            Err(error) => tracing::error!(%error, "device session failed"),
        }
        sleep(Duration::from_secs(2)).await;
    }
}

async fn run_session(
    url: &str,
    device_id: &str,
    config: DisplayConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let (socket, _) = connect_async(url).await?;
    let (mut writer, mut reader) = socket.split();
    let hello = DeviceEvent::Hello {
        device_id: device_id.to_owned(),
        firmware_version: env!("CARGO_PKG_VERSION").into(),
        capabilities: DeviceCapabilities {
            max_width: config.width,
            max_height: config.height,
            color_depth_bits: 1,
            supports_brightness: true,
        },
    };
    writer
        .send(Message::Text(serde_json::to_string(&hello)?.into()))
        .await?;

    while let Some(message) = reader.next().await {
        let Message::Text(text) = message? else {
            continue;
        };
        let command: DeviceCommand = serde_json::from_str(&text)?;
        match command {
            DeviceCommand::Show(message) => preview_message(&config, &message).await,
            DeviceCommand::Clear => println!("[{}x{}] clear", config.width, config.height),
            DeviceCommand::Configure(next) => next.validate()?,
            DeviceCommand::Ping { nonce } => {
                let pong = DeviceEvent::Pong { nonce };
                writer
                    .send(Message::Text(serde_json::to_string(&pong)?.into()))
                    .await?;
            }
        }
    }
    Ok(())
}

async fn preview_message(config: &DisplayConfig, message: &leddy_interfaces::MessageEnvelope) {
    let mut frame = FrameBuffer::new(config);
    let started = Instant::now();
    let message_width = content_width(&message.text);
    for _ in 0..20 {
        let offset = scroll_offset(
            started.elapsed().as_millis() as u64,
            message.speed_pixels_per_second,
            message_width,
            frame.width(),
            message.direction,
        );
        render_text_5x7(&mut frame, &message.text, offset);
        let lit = frame
            .row_major()
            .iter()
            .filter(|pixel| **pixel != 0)
            .count();
        println!(
            "message={} offset={} lit_pixels={}",
            message.id, offset, lit
        );
        sleep(Duration::from_millis(50)).await;
    }
}

fn env_u16(name: &str, fallback: u16) -> u16 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(fallback)
}
