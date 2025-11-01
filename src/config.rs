use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Duration;

/// Configuration presets for the default PROClient window
pub const PRESET_X: i32 = 2575;
pub const PRESET_Y: i32 = 70;
pub const PRESET_WIDTH: u32 = 870;
pub const PRESET_HEIGHT: u32 = 55;
pub const PRESET_REFRESH_MS: u64 = 500;
pub const PRESET_EMPTY_THRESHOLD: u32 = 2;
pub const PRESET_WINDOW_DETECTION: bool = true;
pub const PRESET_PREPROCESS_IMAGES: bool = false;
/// The window class to monitor when window detection is enabled
pub const TARGET_WINDOW_CLASS: &str = "PROClient.x86_64";
/// Default minimum OCR confidence threshold (currently unused)
pub const MIN_OCR_CONFIDENCE: f32 = 0.5;

const CONFIG_DIR_NAME: &str = "protean";
const CONFIG_FILE_NAME: &str = "settings.toml";

/// Structure to hold the selected region coordinates
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Region {
    /// Create a region with preset coordinates for PROClient
    pub fn preset() -> Self {
        Self {
            x: PRESET_X,
            y: PRESET_Y,
            width: PRESET_WIDTH,
            height: PRESET_HEIGHT,
        }
    }
}

/// Application configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Screen region to capture for OCR
    pub region: Region,
    /// How frequently to capture and process OCR
    #[serde(with = "duration_ms")]
    pub refresh_rate: Duration,
    /// Number of empty frames required to confirm battle end
    pub empty_threshold: u32,
    /// Whether to auto-pause when target window loses focus
    pub window_detection: bool,
    /// Minimum OCR confidence threshold (reserved for future use)
    #[serde(default = "default_min_confidence")]
    pub min_ocr_confidence: f32,
    /// Whether to apply image preprocessing before OCR
    #[serde(default = "default_preprocess_images")]
    pub preprocess_images: bool,
}

fn default_min_confidence() -> f32 {
    MIN_OCR_CONFIDENCE
}

fn default_preprocess_images() -> bool {
    PRESET_PREPROCESS_IMAGES
}

// Custom serde serialization for Duration
mod duration_ms {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(duration.as_millis() as u64)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let ms = u64::deserialize(deserializer)?;
        Ok(Duration::from_millis(ms))
    }
}

impl Config {
    /// Create a config with preset values optimized for PROClient
    pub fn preset() -> Self {
        Self {
            region: Region::preset(),
            refresh_rate: Duration::from_millis(PRESET_REFRESH_MS),
            empty_threshold: PRESET_EMPTY_THRESHOLD,
            window_detection: PRESET_WINDOW_DETECTION,
            min_ocr_confidence: MIN_OCR_CONFIDENCE,
            preprocess_images: PRESET_PREPROCESS_IMAGES,
        }
    }

    /// Get the default config file path
    pub fn default_config_path() -> Result<PathBuf> {
        let config_dir = dirs::config_dir()
            .context("Could not determine config directory")?;
        Ok(config_dir.join(CONFIG_DIR_NAME).join(CONFIG_FILE_NAME))
    }

    /// Load config from file, or create via user input if it doesn't exist
    /// This is the preferred way to initialize config in the application
    pub fn load_or_create() -> Result<Self> {
        let config_path = Self::default_config_path()?;
        
        if config_path.exists() {
            println!("Loading configuration from: {}", config_path.display());
            let contents = fs::read_to_string(&config_path)
                .context("Failed to read config file")?;
            let config: Config = toml::from_str(&contents)
                .context("Failed to parse config file")?;
            
            println!("✓ Configuration loaded successfully!");
            Self::display_config(&config);
            Ok(config)
        } else {
            println!("No config file found at: {}", config_path.display());
            Self::from_user_input()
        }
    }

    /// Save current config to the default config file location
    pub fn save(&self) -> Result<()> {
        let config_path = Self::default_config_path()?;
        
        // Create parent directory if it doesn't exist
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let toml_string = toml::to_string_pretty(self)
            .context("Failed to serialize config")?;
        
        fs::write(&config_path, toml_string)
            .context("Failed to write config file")?;
        
        println!("✓ Configuration saved to: {}", config_path.display());
        Ok(())
    }

    /// Display the current configuration in a human-readable format
    fn display_config(config: &Config) {
        println!("\nCurrent configuration:");
        println!("  X: {}, Y: {}", config.region.x, config.region.y);
        println!("  Width: {}, Height: {}", config.region.width, config.region.height);
        println!("  Refresh rate: {}ms", config.refresh_rate.as_millis());
        println!("  Empty threshold: {}", config.empty_threshold);
        println!("  Window detection: {}", config.window_detection);
        println!("  Min OCR confidence: {}", config.min_ocr_confidence);
        println!("  Preprocess images: {}", config.preprocess_images);
    }

    /// Create config by prompting user for input
    /// Offers preset or custom configuration options
    pub fn from_user_input() -> Result<Self> {
        println!("=== Pokemon Battle Text Monitor ===\n");
        
        print!("Use preset coordinates? (y/n): ");
        io::stdout().flush()?;
        let mut choice = String::new();
        io::stdin().read_line(&mut choice)?;
        
        let config = if choice.trim().to_lowercase() == "y" {
            let config = Self::preset();
            println!("\nUsing preset configuration:");
            Self::display_config(&config);
            config
        } else {
            Self::from_custom_input()?
        };

        // Ask if user wants to save this config
        print!("\nSave this configuration for future use? (y/n): ");
        io::stdout().flush()?;
        let mut save_choice = String::new();
        io::stdin().read_line(&mut save_choice)?;
        
        if save_choice.trim().to_lowercase() == "y" {
            config.save()?;
        }

        Ok(config)
    }

    /// Create config from custom user-provided values
    fn from_custom_input() -> Result<Self> {
        println!("\nEnter custom coordinates:");
        
        let x = Self::read_input::<i32>("X coordinate (left): ", "Invalid X")?;
        let y = Self::read_input::<i32>("Y coordinate (top): ", "Invalid Y")?;
        let width = Self::read_input::<u32>("Width: ", "Invalid width")?;
        let height = Self::read_input::<u32>("Height: ", "Invalid height")?;
        let refresh_ms = Self::read_input::<u64>("Refresh rate (ms): ", "Invalid refresh rate")?;
        let empty_threshold = Self::read_input::<u32>("Empty threshold: ", "Invalid threshold")?;
        
        print!("Enable window detection? (y/n): ");
        io::stdout().flush()?;
        let mut window_input = String::new();
        io::stdin().read_line(&mut window_input)?;
        let window_detection = window_input.trim().to_lowercase() == "y";

        print!("Minimum OCR confidence (0.0-1.0, default 0.5): ");
        io::stdout().flush()?;
        let mut confidence_input = String::new();
        io::stdin().read_line(&mut confidence_input)?;
        let min_ocr_confidence = confidence_input.trim().parse()
            .unwrap_or(MIN_OCR_CONFIDENCE);

        print!("Enable image preprocessing? (y/n, default n): ");
        io::stdout().flush()?;
        let mut preprocess_input = String::new();
        io::stdin().read_line(&mut preprocess_input)?;
        let preprocess_images = preprocess_input.trim().to_lowercase() == "y";

        Ok(Self {
            region: Region { x, y, width, height },
            refresh_rate: Duration::from_millis(refresh_ms),
            empty_threshold,
            window_detection,
            min_ocr_confidence,
            preprocess_images,
        })
    }

    /// Helper function to read and parse user input
    fn read_input<T: std::str::FromStr>(prompt: &str, error_msg: &str) -> Result<T>
    where
        T::Err: std::fmt::Display,
    {
        print!("{}", prompt);
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        input.trim().parse()
            .map_err(|e| anyhow::anyhow!("{}: {}", error_msg, e))
    }
}
