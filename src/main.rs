use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent};
use image::DynamicImage;
use ocrs::{ImageSource, OcrEngine, OcrEngineParams};
use rten::Model;
use screenshots::Screen;
use serde::Deserialize;
use std::collections::HashMap;
use std::io::{self, Write};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

/// Structure to hold the selected region coordinates
#[derive(Debug, Clone, Copy)]
struct Region {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

/// Configuration presets
const PRESET_X: i32 = 2575;
const PRESET_Y: i32 = 70;
const PRESET_WIDTH: u32 = 870;
const PRESET_HEIGHT: u32 = 55;
const PRESET_REFRESH_MS: u64 = 500;
const PRESET_EMPTY_THRESHOLD: u32 = 2;
const PRESET_WINDOW_DETECTION: bool = true;
const TARGET_WINDOW_CLASS: &str = "PROClient.x86_64";

#[derive(Debug, Clone)]
struct Config {
    region: Region,
    refresh_rate: Duration,
    empty_threshold: u32,
    window_detection: bool,
}

#[derive(Deserialize)]
struct HyprlandWindow {
    class: String,
}

fn get_config_from_user() -> Result<Config> {
    println!("=== Pokemon Battle Text Monitor ===\n");
    
    print!("Use preset coordinates? (y/n): ");
    io::stdout().flush()?;
    let mut choice = String::new();
    io::stdin().read_line(&mut choice)?;
    
    let (region, refresh_rate, empty_threshold, window_detection) = if choice.trim().to_lowercase() == "y" {
        println!("\nUsing preset configuration:");
        println!("  X: {}, Y: {}", PRESET_X, PRESET_Y);
        println!("  Width: {}, Height: {}", PRESET_WIDTH, PRESET_HEIGHT);
        println!("  Refresh rate: {}ms", PRESET_REFRESH_MS);
        println!("  Empty threshold: {}", PRESET_EMPTY_THRESHOLD);
        println!("  Window detection: {}", PRESET_WINDOW_DETECTION);
        
        (
            Region {
                x: PRESET_X,
                y: PRESET_Y,
                width: PRESET_WIDTH,
                height: PRESET_HEIGHT,
            },
            Duration::from_millis(PRESET_REFRESH_MS),
            PRESET_EMPTY_THRESHOLD,
            PRESET_WINDOW_DETECTION,
        )
    } else {
        println!("\nEnter custom coordinates:");
        
        print!("X coordinate (left): ");
        io::stdout().flush()?;
        let mut x_input = String::new();
        io::stdin().read_line(&mut x_input)?;
        let x: i32 = x_input.trim().parse().context("Invalid X")?;

        print!("Y coordinate (top): ");
        io::stdout().flush()?;
        let mut y_input = String::new();
        io::stdin().read_line(&mut y_input)?;
        let y: i32 = y_input.trim().parse().context("Invalid Y")?;

        print!("Width: ");
        io::stdout().flush()?;
        let mut width_input = String::new();
        io::stdin().read_line(&mut width_input)?;
        let width: u32 = width_input.trim().parse().context("Invalid width")?;

        print!("Height: ");
        io::stdout().flush()?;
        let mut height_input = String::new();
        io::stdin().read_line(&mut height_input)?;
        let height: u32 = height_input.trim().parse().context("Invalid height")?;

        print!("Refresh rate (ms): ");
        io::stdout().flush()?;
        let mut refresh_input = String::new();
        io::stdin().read_line(&mut refresh_input)?;
        let refresh_ms: u64 = refresh_input.trim().parse().context("Invalid refresh rate")?;

        print!("Empty threshold: ");
        io::stdout().flush()?;
        let mut threshold_input = String::new();
        io::stdin().read_line(&mut threshold_input)?;
        let empty_threshold: u32 = threshold_input.trim().parse().context("Invalid threshold")?;

        print!("Enable window detection? (y/n): ");
        io::stdout().flush()?;
        let mut window_input = String::new();
        io::stdin().read_line(&mut window_input)?;
        let window_detection = window_input.trim().to_lowercase() == "y";

        (
            Region { x, y, width, height },
            Duration::from_millis(refresh_ms),
            empty_threshold,
            window_detection,
        )
    };
    
    Ok(Config { region, refresh_rate, empty_threshold, window_detection })
}

fn check_active_window() -> Result<bool> {
    let output = Command::new("hyprctl")
        .args(&["activewindow", "-j"])
        .output()
        .context("Failed to run hyprctl")?;

    if !output.status.success() {
        return Ok(false);
    }

    let json_str = String::from_utf8(output.stdout)?;
    let window: HyprlandWindow = serde_json::from_str(&json_str)?;
    
    Ok(window.class == TARGET_WINDOW_CLASS)
}

fn capture_region(screen: &Screen, region: &Region) -> Result<DynamicImage> {
    let image = screen
        .capture_area(region.x, region.y, region.width, region.height)
        .context("Failed to capture screen region")?;
    Ok(DynamicImage::ImageRgba8(image))
}

fn extract_text(engine: &OcrEngine, image: &DynamicImage) -> Result<String> {
    let rgb_image = image.to_rgb8();
    let (width, height) = rgb_image.dimensions();
    
    let img_source = ImageSource::from_bytes(rgb_image.as_raw(), (width, height))?;
    let ocr_input = engine.prepare_input(img_source)?;
    
    let word_rects = engine.detect_words(&ocr_input)?;
    let line_rects = engine.find_text_lines(&ocr_input, &word_rects);
    let line_texts = engine.recognize_text(&ocr_input, &line_rects)?;
    
    let text = line_texts
        .iter()
        .filter_map(|opt_line| opt_line.as_ref())
        .map(|line| {
            line.words()
                .map(|word| word.to_string())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join(" ");
    
    Ok(text.trim().to_string())
}

/// Normalize Pokemon names by merging superstrings into substrings
fn normalize_pokemon_names(text_counts: &HashMap<String, usize>) -> HashMap<String, usize> {
    let mut normalized: HashMap<String, usize> = HashMap::new();
    let mut keys: Vec<_> = text_counts.keys().collect();
    keys.sort_by_key(|k| k.len()); // Process shorter strings first
    
    for key in keys {
        let count = text_counts[key];
        let mut merged = false;
        
        // Check if this key is a superstring of any existing normalized key
        for (norm_key, norm_count) in normalized.iter_mut() {
            if key.contains(norm_key.as_str()) && key != norm_key {
                *norm_count += count;
                merged = true;
                println!("  Merged \"{}\" ({}) into \"{}\"", key, count, norm_key);
                break;
            }
        }
        
        if !merged {
            normalized.insert(key.clone(), count);
        }
    }
    
    normalized
}

/// Format duration into human-readable string
fn format_duration(duration: Duration) -> String {
    let total_secs = duration.as_secs();
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    
    if hours > 0 {
        format!("{}h {}m {}s", hours, minutes, seconds)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, seconds)
    } else {
        format!("{}s", seconds)
    }
}

fn print_statistics(text_counts: &HashMap<String, usize>, hunt_duration: Duration) {
    println!("\n╔════════════════════════════════════════════════════════╗");
    println!("║                    FINAL STATISTICS                    ║");
    println!("╚════════════════════════════════════════════════════════╝\n");
    
    if text_counts.is_empty() {
        println!("No encounters recorded.");
        println!("Hunt Duration: {}", format_duration(hunt_duration));
        return;
    }

    let total: usize = text_counts.values().sum();
    let mut sorted: Vec<_> = text_counts.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1));

    println!("{:<50} | {:>5} | {:>6}", "Pokemon", "Count", "Rate");
    println!("{}", "-".repeat(70));
    
    for (text, count) in sorted {
        let percentage = (*count as f64 / total as f64) * 100.0;
        println!("{:<50} | {:>5} | {:>5.1}%", text, count, percentage);
    }
    
    println!("{}", "-".repeat(70));
    println!("{:<50} | {:>5}", "TOTAL", total);
    println!("{:<50} | {}", "Hunt Duration", format_duration(hunt_duration));
}

fn show_help() {
    println!("\n╔════════════════════════════════════════════════════════╗");
    println!("║                   KEYBOARD CONTROLS                    ║");
    println!("╚════════════════════════════════════════════════════════╝");
    println!("  [P] - Pause/Resume monitoring");
    println!("  [R] - Restart (clear all statistics)");
    println!("  [S] - Show current statistics");
    println!("  [N] - Normalize Pokemon names (merge superstrings)");
    println!("  [?] - Show this help menu");
    println!("  [Q] - Quit and show final statistics\n");
}

/// Extract pokemon name from text containing "VS. Wild [Pokemon Name]"
fn extract_pokemon_name(text: &str) -> Option<String> {
    let text_upper = text.to_uppercase();
    
    if let Some(vs_pos) = text_upper.find("VS. WILD") {
        let after_wild = vs_pos + "VS. WILD".len();
        let remaining = text[after_wild..].trim_start();
        
        if let Some(pokemon) = remaining.split_whitespace().next() {
            if !pokemon.is_empty() {
                return Some(pokemon.to_string());
            }
        }
    }
    
    None
}

fn monitor_text(engine: &OcrEngine, screen: &Screen, config: &Config) -> Result<()> {
    let mut text_counts: HashMap<String, usize> = HashMap::new();
    let mut last_text = String::new();
    let mut pending_pokemon: Option<String> = None;
    let mut no_pattern_count = 0;
    let mut paused = false;
    let mut window_paused = false;
    
    // Time tracking
    let start_time = Instant::now();
    let mut total_paused_duration = Duration::ZERO;
    let mut pause_start: Option<Instant> = None;

    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║                  MONITORING STARTED                  ║");
    println!("╚══════════════════════════════════════════════════════╝");
    if config.window_detection {
        println!("Window detection enabled: {} ", TARGET_WINDOW_CLASS);
    }
    show_help();
    println!("Tracking encounters with 'VS. Wild [Pokemon]' pattern");
    println!("Counts registered AFTER battle ends\n");

    loop {
        // Window detection check
        if config.window_detection {
            match check_active_window() {
                Ok(is_target) => {
                    if !is_target && !window_paused {
                        window_paused = true;
                        pause_start = Some(Instant::now());
                        println!("\n⏸  Auto-paused (window not focused)");
                    } else if is_target && window_paused {
                        if let Some(pause_time) = pause_start {
                            total_paused_duration += pause_time.elapsed();
                        }
                        pause_start = None;
                        window_paused = false;
                        println!("\n▶  Auto-resumed (window focused)");
                    }
                }
                Err(_) => {
                    // Silently continue if hyprctl fails
                }
            }
        

        // Check for keyboard input (non-blocking)
        if event::poll(Duration::from_millis(0))? {
            if let Event::Key(KeyEvent { code, .. }) = event::read()? {
                match code {
                    KeyCode::Char('p') | KeyCode::Char('P') => {
                        paused = !paused;
                        if paused {
                            pause_start = Some(Instant::now());
                            println!("\n⏸  PAUSED - Press 'P' to resume");
                        } else {
                            if let Some(pause_time) = pause_start {
                                total_paused_duration += pause_time.elapsed();
                            }
                            pause_start = None;
                            println!("\n▶  RESUMED");
                        }
                    }
                    KeyCode::Char('r') | KeyCode::Char('R') => {
                        text_counts.clear();
                        last_text.clear();
                        pending_pokemon = None;
                        no_pattern_count = 0;
                        println!("\n=> RESTARTED - All statistics cleared");
                    }
                    KeyCode::Char('s') | KeyCode::Char('S') => {
                        let active_duration = start_time.elapsed() - total_paused_duration;
                        println!("\n");
                        print_statistics(&text_counts, active_duration);
                        println!();
                    }
                    KeyCode::Char('n') | KeyCode::Char('N') => {
                        println!("\n=> Normalizing Pokemon names...");
                        text_counts = normalize_pokemon_names(&text_counts);
                        println!("✓ Normalization complete\n");
                    }
                    KeyCode::Char('?') => {
                        show_help();
                    }
                    KeyCode::Char('q') | KeyCode::Char('Q') => {
                        let active_duration = start_time.elapsed() - total_paused_duration;
                        println!("\n\n=> Monitoring stopped by user.");
                        print_statistics(&text_counts, active_duration);
                        return Ok(());
                    }
                    _ => {}
                }
            }
        }

        if paused || window_paused {
            thread::sleep(Duration::from_millis(100));
            continue;
        }

        let image = match capture_region(screen, &config.region) {
            Ok(img) => img,
            Err(e) => {
                eprintln!("Capture error: {}", e);
                thread::sleep(config.refresh_rate);
                continue;
            }
        };

        match extract_text(engine, &image) {
            Ok(text) => {
                if let Some(pokemon_name) = extract_pokemon_name(&text) {
                    no_pattern_count = 0;
                    
                    let should_update = pending_pokemon.as_ref()
                        .map(|p| p != &pokemon_name)
                        .unwrap_or(true);
                    
                    if should_update {
                        pending_pokemon = Some(pokemon_name.clone());
                        println!("⏳ Detected: \"{}\" from \"{}\"", pokemon_name, text);
                    }
                    last_text = text;
                } else {
                    no_pattern_count += 1;
                    
                    if no_pattern_count >= config.empty_threshold {
                        if let Some(pokemon) = pending_pokemon.take() {
                            *text_counts.entry(pokemon.clone()).or_insert(0) += 1;
                            let count = text_counts[&pokemon];
                            println!("✓ Counted: \"{}\" (Total: {})", pokemon, count);
                        }
                        
                        if !last_text.is_empty() {
                            println!("[Battle ended - ready for next encounter]");
                            last_text.clear();
                        }
                        
                        no_pattern_count = 0;
                    } else if text != last_text && text.len() >= 10 {
                        println!("✗ Ignored (no 'VS. Wild' pattern): \"{}\"", text);
                        last_text = text;
                    }
                }
            }
            Err(e) => {
                eprintln!("OCR Error: {}", e);
            }
        }

        thread::sleep(config.refresh_rate);
    }
}

fn main() -> Result<()> {
    println!("Loading OCR models...");
    
    let home = std::env::var("HOME").context("HOME not set")?;
    let cache_dir = format!("{}/.cache/ocrs", home);
    
    let detection_path = format!("{}/text-detection.rten", cache_dir);
    let recognition_path = format!("{}/text-recognition.rten", cache_dir);
    
    let detection_model = Model::load_file(&detection_path)
        .context("Failed to load detection model. Download to ~/.cache/ocrs/")?;
    let recognition_model = Model::load_file(&recognition_path)
        .context("Failed to load recognition model. Download to ~/.cache/ocrs/")?;
    
    let engine = OcrEngine::new(OcrEngineParams {
        detection_model: Some(detection_model),
        recognition_model: Some(recognition_model),
        ..Default::default()
    })?;

    println!("✓ Models loaded successfully!\n");

    let screens = Screen::all()?;
    let screen = screens.first().context("No screens found")?;
    let config = get_config_from_user()?;

    println!("\nStarting in 3 seconds...");
    thread::sleep(Duration::from_secs(3));

    monitor_text(&engine, screen, &config)?;
    Ok(())
}
