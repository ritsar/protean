use anyhow::{Context, Result};
use image::{DynamicImage, GrayImage};
use ocrs::{ImageSource, OcrEngine};
use screenshots::Screen;

use crate::config::Region;

// Image preprocessing constants
const GRAYSCALE_LEVELS: usize = 256;
const MAX_PIXEL_VALUE: u8 = 255;
const MIN_PIXEL_VALUE: u8 = 0;

/// Trait for OCR operations to allow for testing and different implementations
pub trait OcrProvider {
    /// Extract text from an image
    fn extract_text(&self, image: &DynamicImage, preprocess: bool) -> Result<String>;
}

/// Standard OCR provider using the ocrs library
pub struct StandardOcrProvider<'a> {
    engine: &'a OcrEngine,
}

impl<'a> StandardOcrProvider<'a> {
    pub fn new(engine: &'a OcrEngine) -> Self {
        Self { engine }
    }
}

impl<'a> OcrProvider for StandardOcrProvider<'a> {
    fn extract_text(&self, image: &DynamicImage, preprocess: bool) -> Result<String> {
        extract_text(self.engine, image, preprocess)
    }
}

/// Capture a specific region of the screen
/// 
/// # Arguments
/// * `screen` - The screen to capture from
/// * `region` - The rectangular region to capture
/// 
/// # Returns
/// * `Ok(DynamicImage)` containing the captured region
/// * `Err` if capture fails
pub fn capture_region(screen: &Screen, region: &Region) -> Result<DynamicImage> {
    let image = screen
        .capture_area(region.x, region.y, region.width, region.height)
        .context("Failed to capture screen region")?;
    Ok(DynamicImage::ImageRgba8(image))
}

/// Preprocess image for better OCR accuracy
/// 
/// Applies three transformations:
/// 1. Grayscale conversion - simplifies processing
/// 2. Contrast enhancement - histogram stretching for better dynamic range
/// 3. Binary thresholding - Otsu's method for optimal black/white separation
/// 
/// # Arguments
/// * `image` - The input image to preprocess
/// 
/// # Returns
/// * A binary (black and white) grayscale image optimized for OCR
fn preprocess_image(image: &DynamicImage) -> GrayImage {
    // Convert to grayscale
    let mut grayscale = image.to_luma8();
    
    // Apply contrast enhancement using histogram stretching
    let (min_value, max_value) = grayscale.pixels().fold((MAX_PIXEL_VALUE, MIN_PIXEL_VALUE), |(min_val, max_val), pixel| {
        let pixel_value = pixel.0[0];
        (min_val.min(pixel_value), max_val.max(pixel_value))
    });
    
    // Stretch histogram only if there's meaningful contrast
    if max_value > min_value {
        let scale_factor = MAX_PIXEL_VALUE as f32 / (max_value - min_value) as f32;
        for pixel in grayscale.pixels_mut() {
            let original_value = pixel.0[0];
            pixel.0[0] = (original_value.saturating_sub(min_value) as f32 * scale_factor) as u8;
        }
    }
    
    // Apply simple binary thresholding using Otsu's method approximation
    let threshold = calculate_otsu_threshold(&grayscale);
    for pixel in grayscale.pixels_mut() {
        pixel.0[0] = if pixel.0[0] > threshold { MAX_PIXEL_VALUE } else { MIN_PIXEL_VALUE };
    }
    
    grayscale
}

/// Calculate optimal threshold using Otsu's method
/// 
/// Otsu's method automatically determines the best threshold value by
/// maximizing the between-class variance of pixel intensities.
/// 
/// # Arguments
/// * `grayscale` - A grayscale image to analyze
/// 
/// # Returns
/// * The optimal threshold value (0-255)
fn calculate_otsu_threshold(grayscale: &GrayImage) -> u8 {
    let mut histogram = [0u32; GRAYSCALE_LEVELS];
    
    // Build histogram
    for pixel in grayscale.pixels() {
        histogram[pixel.0[0] as usize] += 1;
    }
    
    let total_pixels = grayscale.width() * grayscale.height();
    let mut weighted_sum = 0.0;
    for (intensity, &count) in histogram.iter().enumerate() {
        weighted_sum += intensity as f32 * count as f32;
    }
    
    let mut background_weight = 0;
    let mut background_sum = 0.0;
    let mut max_variance = 0.0;
    let mut optimal_threshold = MIN_PIXEL_VALUE;
    
    for (threshold, &pixel_count) in histogram.iter().enumerate() {
        background_weight += pixel_count;
        if background_weight == 0 {
            continue;
        }
        
        let foreground_weight = total_pixels - background_weight;
        if foreground_weight == 0 {
            break;
        }
        
        background_sum += threshold as f32 * pixel_count as f32;
        
        let background_mean = background_sum / background_weight as f32;
        let foreground_mean = (weighted_sum - background_sum) / foreground_weight as f32;
        
        let between_class_variance = background_weight as f32 * foreground_weight as f32 
            * (background_mean - foreground_mean).powi(2);
        
        if between_class_variance > max_variance {
            max_variance = between_class_variance;
            optimal_threshold = threshold as u8;
        }
    }
    
    optimal_threshold
}

/// Extract text from an image using OCR with optional preprocessing
/// 
/// # Arguments
/// * `engine` - The OCR engine to use
/// * `image` - The image to extract text from
/// * `preprocess` - Whether to apply preprocessing (grayscale, contrast, threshold)
/// 
/// # Returns
/// * `Ok(String)` containing the extracted text
/// * `Err` if OCR processing fails
fn extract_text(engine: &OcrEngine, image: &DynamicImage, preprocess: bool) -> Result<String> {
    // Create the appropriate image format based on preprocessing flag
    let preprocessed_grayscale;
    let original_rgb;
    
    let img_source = if preprocess {
        // Preprocess the image for better OCR accuracy
        preprocessed_grayscale = preprocess_image(image);
        let (width, height) = preprocessed_grayscale.dimensions();
        ImageSource::from_bytes(preprocessed_grayscale.as_raw(), (width, height))?
    } else {
        // Use original image without preprocessing
        original_rgb = image.to_rgb8();
        let (width, height) = original_rgb.dimensions();
        ImageSource::from_bytes(original_rgb.as_raw(), (width, height))?
    };
    
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
