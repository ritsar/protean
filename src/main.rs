use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent};
use ocrs::{OcrEngine, OcrEngineParams};
use rten::Model;
use screenshots::Screen;
use std::collections::HashMap;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

mod config;
mod ocr;
mod pokemon;
mod statistics;
mod ui;
mod window;

use config::Config;
use ocr::{capture_region, OcrProvider, StandardOcrProvider};
use pokemon::{extract_pokemon_name, normalize_pokemon_names};
use statistics::print_statistics;
use ui::show_help;
use window::check_active_window;

// Constants for timing and thresholds
const PAUSE_POLL_INTERVAL_MS: u64 = 100;
const MIN_TEXT_LENGTH_TO_LOG: usize = 10;
const STARTUP_DELAY_SECONDS: u64 = 3;

/// Battle detection states
#[derive(Debug, Clone, PartialEq)]
enum BattlePhase {
    /// Not in battle, waiting for pokemon detection
    Idle,
    /// Pokemon detected via "VS. Wild \[name\]" pattern
    PokemonDetected { name: String },
    /// Battle is active, monitoring for end
    BattleActive { name: String },
    /// Battle ending, waiting to count
    BattleEnding { name: String, empty_count: u32 },
}

/// Manages pause state and duration tracking
struct PauseManager {
    manual_pause: bool,
    window_pause: bool,
    total_paused_duration: Duration,
    pause_start: Option<Instant>,
}

impl PauseManager {
    fn new() -> Self {
        Self {
            manual_pause: false,
            window_pause: false,
            total_paused_duration: Duration::ZERO,
            pause_start: None,
        }
    }

    fn is_paused(&self) -> bool {
        self.manual_pause || self.window_pause
    }

    fn toggle_manual_pause(&mut self) {
        self.manual_pause = !self.manual_pause;
        if self.manual_pause {
            self.start_pause();
            println!("\n⏸  PAUSED - Press 'P' to resume");
        } else {
            self.end_pause();
            println!("\n▶  RESUMED");
        }
    }

    fn set_window_pause(&mut self, paused: bool) {
        if paused && !self.window_pause {
            self.window_pause = true;
            self.start_pause();
            println!("\n⏸  Auto-paused (window not focused)");
        } else if !paused && self.window_pause {
            self.window_pause = false;
            self.end_pause();
            println!("\n▶  Auto-resumed (window focused)");
        }
    }

    fn start_pause(&mut self) {
        if self.pause_start.is_none() {
            self.pause_start = Some(Instant::now());
        }
    }

    fn end_pause(&mut self) {
        if let Some(pause_time) = self.pause_start.take() {
            self.total_paused_duration += pause_time.elapsed();
        }
    }

    fn active_duration(&self, start_time: Instant) -> Duration {
        let mut duration = start_time.elapsed() - self.total_paused_duration;
        // Account for currently active pause
        if let Some(pause_time) = self.pause_start {
            duration -= pause_time.elapsed();
        }
        duration
    }
}

/// Tracks the state of battle detection using explicit state machine
struct BattleState {
    phase: BattlePhase,
    last_text: String,
}

impl BattleState {
    fn new() -> Self {
        Self {
            phase: BattlePhase::Idle,
            last_text: String::new(),
        }
    }

    fn reset(&mut self) {
        self.phase = BattlePhase::Idle;
        self.last_text.clear();
    }
    
    /// Update state based on OCR text and return whether to count the pokemon
    fn update(&mut self, text: &str, config: &Config) -> Option<String> {
        let pokemon_in_text = extract_pokemon_name(text);
        
        match &self.phase {
            BattlePhase::Idle => {
                if let Some(pokemon_name) = pokemon_in_text {
                    println!("⏳ Detected: \"{}\" from \"{}\"", pokemon_name, text);
                    self.phase = BattlePhase::PokemonDetected { name: pokemon_name };
                    self.last_text = text.to_string();
                } else if text != self.last_text && text.len() >= MIN_TEXT_LENGTH_TO_LOG {
                    println!("✗ Ignored (no 'VS. Wild' pattern): \"{}\"", text);
                    self.last_text = text.to_string();
                }
                None
            }
            
            BattlePhase::PokemonDetected { name } => {
                if let Some(new_name) = pokemon_in_text {
                    if &new_name != name {
                        // Different pokemon detected, transition to new detection
                        println!("⏳ Detected: \"{}\" from \"{}\"", new_name, text);
                        self.phase = BattlePhase::PokemonDetected { name: new_name };
                    } else {
                        // Same pokemon, transition to active battle
                        self.phase = BattlePhase::BattleActive { name: name.clone() };
                    }
                    self.last_text = text.to_string();
                } else {
                    // No pokemon detected, start counting empties
                    self.phase = BattlePhase::BattleEnding { name: name.clone(), empty_count: 1 };
                }
                None
            }
            
            BattlePhase::BattleActive { name } => {
                if pokemon_in_text.is_none() {
                    // Battle ending, start counting
                    self.phase = BattlePhase::BattleEnding { name: name.clone(), empty_count: 1 };
                } else {
                    self.last_text = text.to_string();
                }
                None
            }
            
            BattlePhase::BattleEnding { name, empty_count } => {
                if let Some(new_name) = pokemon_in_text {
                    // New pokemon detected during ending phase
                    println!("⏳ Detected: \"{}\" from \"{}\"", new_name, text);
                    self.phase = BattlePhase::PokemonDetected { name: new_name };
                    self.last_text = text.to_string();
                    None
                } else {
                    let new_count = empty_count + 1;
                    if new_count >= config.empty_threshold {
                        // Battle confirmed ended, count the pokemon
                        let counted_name = name.clone();
                        println!("[Battle ended - ready for next encounter]");
                        self.phase = BattlePhase::Idle;
                        self.last_text.clear();
                        Some(counted_name)
                    } else {
                        // Keep counting
                        self.phase = BattlePhase::BattleEnding { name: name.clone(), empty_count: new_count };
                        None
                    }
                }
            }
        }
    }
}

enum KeyAction {
    Continue,
    Quit,
}

/// Handle keyboard input and return action
fn handle_keyboard_input(
    pause_manager: &mut PauseManager,
    battle_state: &mut BattleState,
    text_counts: &mut HashMap<String, usize>,
    start_time: Instant,
) -> Result<KeyAction> {
    if !event::poll(Duration::from_millis(0))? {
        return Ok(KeyAction::Continue);
    }

    if let Event::Key(KeyEvent { code, .. }) = event::read()? {
        match code {
            KeyCode::Char('p') | KeyCode::Char('P') => {
                pause_manager.toggle_manual_pause();
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                text_counts.clear();
                battle_state.reset();
                println!("\n=> RESTARTED - All statistics cleared");
            }
            KeyCode::Char('s') | KeyCode::Char('S') => {
                let active_duration = pause_manager.active_duration(start_time);
                println!("\n");
                print_statistics(text_counts, active_duration);
                println!();
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                println!("\n=> Normalizing Pokemon names...");
                *text_counts = normalize_pokemon_names(text_counts);
                println!("✓ Normalization complete\n");
            }
            KeyCode::Char('?') => {
                show_help();
            }
            KeyCode::Char('q') | KeyCode::Char('Q') => {
                return Ok(KeyAction::Quit);
            }
            _ => {}
        }
    }

    Ok(KeyAction::Continue)
}

/// Process OCR text and update battle state using state machine
fn process_ocr_text(
    text: &str,
    battle_state: &mut BattleState,
    text_counts: &mut HashMap<String, usize>,
    config: &Config,
) {
    if let Some(pokemon_name) = battle_state.update(text, config) {
        let count = text_counts.entry(pokemon_name.clone()).and_modify(|c| *c += 1).or_insert(1);
        println!("✓ Counted: \"{}\" (Total: {})", pokemon_name, count);
    }
}

fn monitor_text(ocr_provider: &dyn OcrProvider, screen: &Screen, config: &Config) -> Result<()> {
    let mut text_counts: HashMap<String, usize> = HashMap::new();
    let mut pause_manager = PauseManager::new();
    let mut battle_state = BattleState::new();
    let start_time = Instant::now();

    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║                  MONITORING STARTED                  ║");
    println!("╚══════════════════════════════════════════════════════╝");
    if config.window_detection {
        println!("Window detection enabled: {} ", config::TARGET_WINDOW_CLASS);
    }
    show_help();
    println!("Tracking encounters with 'VS. Wild [Pokemon]' pattern");
    println!("Counts registered AFTER battle ends\n");

    loop {
        // Window detection check
        if config.window_detection && let Ok(is_target) = check_active_window() {
            pause_manager.set_window_pause(!is_target);
        }

        // Check for keyboard input
        match handle_keyboard_input(&mut pause_manager, &mut battle_state, &mut text_counts, start_time)? {
            KeyAction::Quit => {
                let active_duration = pause_manager.active_duration(start_time);
                println!("\n\n=> Monitoring stopped by user.");
                print_statistics(&text_counts, active_duration);
                return Ok(());
            }
            KeyAction::Continue => {}
        }

        if pause_manager.is_paused() {
            thread::sleep(Duration::from_millis(PAUSE_POLL_INTERVAL_MS));
            continue;
        }

        let image =         match capture_region(screen, &config.region) {
            Ok(img) => img,
            Err(e) => {
                eprintln!("Capture error: {}", e);
                thread::sleep(config.refresh_rate);
                continue;
            }
        };

        match ocr_provider.extract_text(&image, config.preprocess_images) {
            Ok(text) => process_ocr_text(&text, &mut battle_state, &mut text_counts, config),
            Err(e) => eprintln!("OCR Error: {}", e),
        }

        thread::sleep(config.refresh_rate);
    }
}

fn main() -> Result<()> {
    println!("Loading OCR models...");
    
    let home = std::env::var("HOME").context("HOME not set")?;
    let cache_dir = PathBuf::from(home).join(".cache/ocrs");
    
    let detection_path = cache_dir.join("text-detection.rten");
    let recognition_path = cache_dir.join("text-recognition.rten");
    
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
    let config = Config::load_or_create()?;

    let ocr_provider = StandardOcrProvider::new(&engine);

    println!("\nStarting in {} seconds...", STARTUP_DELAY_SECONDS);
    thread::sleep(Duration::from_secs(STARTUP_DELAY_SECONDS));

    monitor_text(&ocr_provider, screen, &config)?;
    Ok(())
}
