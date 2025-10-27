use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent};
use image::DynamicImage;
use ocrs::{ImageSource, OcrEngine, OcrEngineParams};
use rten::Model;
use screenshots::Screen;
use std::collections::HashMap;
use std::io::{self, Write};
use std::thread;
use std::time::Duration;

/// Structure to hold the selected region coordinates
#[derive(Debug, Clone, Copy)]
struct Region {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

/// Configuration presets
const PRESET_X: i32 = 2280;
const PRESET_Y: i32 = 65;
const PRESET_WIDTH: u32 = 450;
const PRESET_HEIGHT: u32 = 45;
const PRESET_REFRESH_MS: u64 = 500;
const PRESET_EMPTY_THRESHOLD: u32 = 2;

#[derive(Debug, Clone, Copy)]
struct Config {
    region: Region,
    refresh_rate: Duration,
    empty_threshold: u32,
}

fn get_config_from_user() -> Result<Config> {
    println!("=== Pokemon Battle Text Monitor ===\n");
    
    print!("Use preset coordinates? (y/n): ");
    io::stdout().flush()?;
    let mut choice = String::new();
    io::stdin().read_line(&mut choice)?;
    
    let (region, refresh_rate, empty_threshold) = if choice.trim().to_lowercase() == "y" {
        println!("\nUsing preset configuration:");
        println!("  X: {}, Y: {}", PRESET_X, PRESET_Y);
        println!("  Width: {}, Height: {}", PRESET_WIDTH, PRESET_HEIGHT);
        println!("  Refresh rate: {}ms", PRESET_REFRESH_MS);
        println!("  Empty threshold: {}", PRESET_EMPTY_THRESHOLD);
        
        (
            Region {
                x: PRESET_X,
                y: PRESET_Y,
                width: PRESET_WIDTH,
                height: PRESET_HEIGHT,
            },
            Duration::from_millis(PRESET_REFRESH_MS),
            PRESET_EMPTY_THRESHOLD,
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

        (
            Region { x, y, width, height },
            Duration::from_millis(refresh_ms),
            empty_threshold,
        )
    };
    
    Ok(Config { region, refresh_rate, empty_threshold })
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

fn print_statistics(text_counts: &HashMap<String, usize>) {
    println!("\n╔════════════════════════════════════════════════════════╗");
    println!("║                    FINAL STATISTICS                    ║");
    println!("╚════════════════════════════════════════════════════════╝\n");
    
    if text_counts.is_empty() {
        println!("No encounters recorded.");
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
    println!("{:<50} | {:>5} | {:>5.1}%", "TOTAL", total, 100.0);
}

fn monitor_text(engine: &OcrEngine, screen: &Screen, config: &Config) -> Result<()> {
    let mut text_counts: HashMap<String, usize> = HashMap::new();
    let mut last_text = String::new();
    let mut pending_pokemon: Option<String> = None; // Store pokemon until battle ends
    let mut empty_count = 0;
    let mut paused = false;

    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║                  MONITORING STARTED                  ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!("\nControls:");
    println!("  [P] - Pause/Resume monitoring");
    println!("  [R] - Restart (clear all statistics)");
    println!("  [Q] - Quit and show final statistics");
    println!("\nOnly tracking encounters starting with 'Wild'");
    println!("Counts registered AFTER battle ends\n");

    loop {
        // Check for keyboard input (non-blocking)
        if event::poll(Duration::from_millis(0))? {
            if let Event::Key(KeyEvent { code, .. }) = event::read()? {
                match code {
                    KeyCode::Char('p') | KeyCode::Char('P') => {
                        paused = !paused;
                        if paused {
                            println!("\n⏸  PAUSED - Press 'P' to resume");
                        } else {
                            println!("\n▶  RESUMED");
                        }
                    }
                    KeyCode::Char('r') | KeyCode::Char('R') => {
                        text_counts.clear();
                        last_text.clear();
                        pending_pokemon = None;
                        empty_count = 0;
                        println!("\n=> RESTARTED - All statistics cleared");
                    }
                    KeyCode::Char('q') | KeyCode::Char('Q') => {
                        println!("\n\n=> Monitoring stopped by user.");
                        print_statistics(&text_counts);
                        return Ok(());
                    }
                    _ => {}
                }
            }
        }

        if paused {
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
                if text.is_empty() {
                    empty_count += 1;
                    
                    // Battle ended - count the pending pokemon
                    if empty_count >= config.empty_threshold {
                        if let Some(pokemon) = pending_pokemon.take() {
                            *text_counts.entry(pokemon.clone()).or_insert(0) += 1;
                            let count = text_counts[&pokemon];
                            println!("✓ Counted: \"{}\" (Total: {})", pokemon, count);
                        }
                        
                        if !last_text.is_empty() {
                            println!("[Battle ended - ready for next encounter]");
                            last_text.clear();
                        }
                    }
                } else {
                    empty_count = 0;
                    
                    if text.starts_with("Wild") {
                        if text != last_text {
                            // Store as pending, don't count yet
                            pending_pokemon = Some(text.clone());
                            println!("⏳ Detected: \"{}\" (pending confirmation...)", text);
                            last_text = text;
                        }
                    } else if text != last_text {
                        println!("✗ Ignored (doesn't start with 'Wild'): \"{}\"", text);
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
