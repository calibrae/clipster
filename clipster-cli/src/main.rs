mod client;

use clap::Parser;
use clipster_common::config::ClientConfig;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "clipster", about = "Clipster CLI — manage your clipboard history")]
struct Cli {
    /// Path to config file
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    /// List recent clips
    List {
        /// Max number of clips
        #[arg(short = 'n', long, default_value = "20")]
        limit: u32,
        /// Filter by content type (text, image)
        #[arg(short = 't', long)]
        r#type: Option<String>,
        /// Filter by device
        #[arg(short, long)]
        device: Option<String>,
    },
    /// Get a clip by ID
    Get {
        /// Clip ID
        id: Uuid,
    },
    /// Copy a clip's content to your local clipboard
    Copy {
        /// Clip ID
        id: Uuid,
    },
    /// Search clips by text content
    Search {
        /// Search query
        query: String,
        /// Max results
        #[arg(short = 'n', long, default_value = "20")]
        limit: u32,
    },
    /// Auth utilities
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
}

#[derive(clap::Subcommand)]
enum AuthCommand {
    /// Generate a new API key
    GenerateKey,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = load_config(cli.config.as_deref())?;
    let client = client::ApiClient::new(&config);

    match cli.command {
        Command::List {
            limit,
            r#type,
            device,
        } => {
            let resp = client.list(limit, r#type.as_deref(), device.as_deref()).await?;
            println!(
                "{} clips (showing {}/{})",
                resp.total_count,
                resp.clips.len(),
                resp.total_count
            );
            println!("{:-<80}", "");
            for clip in &resp.clips {
                print_clip_summary(clip);
            }
        }
        Command::Get { id } => {
            let clip = client.get(&id).await?;
            print_clip_detail(&clip);
        }
        Command::Copy { id } => {
            let clip = client.get(&id).await?;
            let mut clipboard = arboard::Clipboard::new()?;
            match clip.content_type {
                clipster_common::models::ClipContentType::Text => {
                    let text = clip.text_content.unwrap_or_default();
                    clipboard.set_text(&text)?;
                    println!("Copied text to clipboard ({} chars)", text.len());
                }
                clipster_common::models::ClipContentType::Image => {
                    let data = client.get_content(&id).await?;
                    // Decode PNG to RGBA for arboard
                    let decoder = png::Decoder::new(std::io::Cursor::new(&data));
                    let mut reader = decoder.read_info()?;
                    let mut buf = vec![0u8; reader.output_buffer_size()];
                    let info = reader.next_frame(&mut buf)?;
                    buf.truncate(info.buffer_size());
                    let img = arboard::ImageData {
                        width: info.width as usize,
                        height: info.height as usize,
                        bytes: buf.into(),
                    };
                    clipboard.set_image(img)?;
                    println!("Copied image to clipboard");
                }
                _ => {
                    anyhow::bail!("unsupported content type for copy");
                }
            }
        }
        Command::Search { query, limit } => {
            let resp = client.search(&query, limit).await?;
            println!("{} results", resp.total_count);
            println!("{:-<80}", "");
            for clip in &resp.clips {
                print_clip_summary(clip);
            }
        }
        Command::Auth {
            command: AuthCommand::GenerateKey,
        } => {
            let key = generate_api_key();
            println!("Generated API key:\n\n  {key}\n");
            println!("Add this to your server config (server.toml):");
            println!("  api_key = \"{key}\"");
            println!("\nAnd to your client config (client.toml):");
            println!("  api_key = \"{key}\"");
        }
    }

    Ok(())
}

fn print_clip_summary(clip: &clipster_common::models::Clip) {
    let type_badge = match clip.content_type {
        clipster_common::models::ClipContentType::Text => "TXT",
        clipster_common::models::ClipContentType::Image => "IMG",
        clipster_common::models::ClipContentType::FileRef => "FILE",
    };
    let preview = match &clip.text_content {
        Some(text) => {
            let truncated: String = text.chars().take(60).collect();
            if text.len() > 60 {
                format!("{truncated}...")
            } else {
                truncated
            }
        }
        None => format!("[{} {}B]", type_badge.to_lowercase(), clip.byte_size),
    };
    let fav = if clip.is_favorite { " *" } else { "" };
    let time = clip.created_at.format("%Y-%m-%d %H:%M");
    let short_id = &clip.id.to_string()[..8];
    println!("[{type_badge}] {short_id}  {time}  {}{fav}  ({})", preview, clip.source_device);
}

fn print_clip_detail(clip: &clipster_common::models::Clip) {
    println!("ID:       {}", clip.id);
    println!("Type:     {}", clip.content_type);
    println!("Device:   {}", clip.source_device);
    println!("Created:  {}", clip.created_at);
    println!("Size:     {} bytes", clip.byte_size);
    println!("Favorite: {}", clip.is_favorite);
    if let Some(ref text) = clip.text_content {
        println!("---");
        println!("{text}");
    }
}

fn generate_api_key() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    // Encode two random u128s to get a long enough key
    let a: u128 = rng.random();
    let b: u128 = rng.random();
    let encoded = format!("{}{}", base62::encode(a), base62::encode(b));
    format!("clp_{encoded}")
}

fn load_config(path: Option<&std::path::Path>) -> anyhow::Result<ClientConfig> {
    if let Some(p) = path {
        let content = std::fs::read_to_string(p)?;
        Ok(toml::from_str(&content)?)
    } else {
        let default_path = config_dir().join("client.toml");
        if default_path.exists() {
            let content = std::fs::read_to_string(&default_path)?;
            Ok(toml::from_str(&content)?)
        } else {
            Ok(ClientConfig::default())
        }
    }
}

fn config_dir() -> PathBuf {
    directories::ProjectDirs::from("com", "clipster", "clipster")
        .map(|d| d.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}
