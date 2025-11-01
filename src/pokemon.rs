use std::collections::HashMap;

/// The pattern to search for in OCR text
const VS_WILD_PATTERN: &str = "VS. WILD";

/// Extract pokemon name from text containing "VS. Wild [Pokemon Name]"
/// Uses case-insensitive matching without allocating uppercase string
/// 
/// # Arguments
/// * `text` - The OCR text to search for the pattern
/// 
/// # Returns
/// * `Some(String)` containing the pokemon name if pattern is found
/// * `None` if pattern is not found or no name follows the pattern
pub fn extract_pokemon_name(text: &str) -> Option<String> {
    // Find "VS. WILD" using case-insensitive byte-by-byte comparison
    let vs_pos = text
        .char_indices()
        .position(|(i, _)| {
            text[i..]
                .chars()
                .zip(VS_WILD_PATTERN.chars())
                .take(VS_WILD_PATTERN.len())
                .all(|(a, b)| a.eq_ignore_ascii_case(&b))
        })?;
    
    let after_wild = text[vs_pos..]
        .char_indices()
        .nth(VS_WILD_PATTERN.chars().count())
        .map(|(i, _)| vs_pos + i)?;
    
    let remaining = text[after_wild..].trim_start();
    
    remaining
        .split_whitespace()
        .next()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Normalize Pokemon names by merging superstrings into substrings
/// 
/// This is useful when OCR occasionally captures extra characters.
/// For example, "Pidgey!" would be merged into "Pidgey".
/// 
/// # Arguments
/// * `text_counts` - HashMap of pokemon names to encounter counts
/// 
/// # Returns
/// * A new HashMap with normalized names and merged counts
pub fn normalize_pokemon_names(text_counts: &HashMap<String, usize>) -> HashMap<String, usize> {
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
