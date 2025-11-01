use std::collections::HashMap;
use std::time::Duration;

// Time conversion constants
const SECONDS_PER_HOUR: u64 = 3600;
const SECONDS_PER_MINUTE: u64 = 60;

// Statistics display constants
const COLUMN_WIDTH_POKEMON: usize = 50;
const COLUMN_WIDTH_COUNT: usize = 5;
const COLUMN_WIDTH_RATE: usize = 6;
const TABLE_WIDTH: usize = 70;
const PERCENTAGE_MULTIPLIER: f64 = 100.0;

/// Format duration into human-readable string (e.g., "1h 23m 45s")
/// 
/// # Arguments
/// * `duration` - The duration to format
/// 
/// # Returns
/// * A human-readable string representation of the duration
pub fn format_duration(duration: Duration) -> String {
    let total_secs = duration.as_secs();
    let hours = total_secs / SECONDS_PER_HOUR;
    let minutes = (total_secs % SECONDS_PER_HOUR) / SECONDS_PER_MINUTE;
    let seconds = total_secs % SECONDS_PER_MINUTE;
    
    if hours > 0 {
        format!("{}h {}m {}s", hours, minutes, seconds)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, seconds)
    } else {
        format!("{}s", seconds)
    }
}

/// Print statistics table with encounter counts and rates
/// 
/// Displays a formatted table showing each pokemon, count, and percentage.
/// Also shows total encounters and hunt duration.
/// 
/// # Arguments
/// * `text_counts` - HashMap of pokemon names to encounter counts
/// * `hunt_duration` - Total active hunting time (excluding pauses)
pub fn print_statistics(text_counts: &HashMap<String, usize>, hunt_duration: Duration) {
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

    println!("{:<width_name$} | {:>width_count$} | {:>width_rate$}", 
             "Pokemon", "Count", "Rate",
             width_name = COLUMN_WIDTH_POKEMON,
             width_count = COLUMN_WIDTH_COUNT,
             width_rate = COLUMN_WIDTH_RATE);
    println!("{}", "-".repeat(TABLE_WIDTH));
    
    for (text, count) in sorted {
        let percentage = (*count as f64 / total as f64) * PERCENTAGE_MULTIPLIER;
        println!("{:<width_name$} | {:>width_count$} | {:>width_rate$.1}%", 
                 text, count, percentage,
                 width_name = COLUMN_WIDTH_POKEMON,
                 width_count = COLUMN_WIDTH_COUNT,
                 width_rate = COLUMN_WIDTH_RATE);
    }
    
    println!("{}", "-".repeat(TABLE_WIDTH));
    println!("{:<width_name$} | {:>width_count$}", 
             "TOTAL", total,
             width_name = COLUMN_WIDTH_POKEMON,
             width_count = COLUMN_WIDTH_COUNT);
    println!("{:<width_name$} | {}", 
             "Hunt Duration", format_duration(hunt_duration),
             width_name = COLUMN_WIDTH_POKEMON);
}
