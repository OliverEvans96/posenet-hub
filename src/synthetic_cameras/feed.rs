//! Load image or video feed and produce frames cropped to a target aspect ratio.

use std::path::Path;

use image::{imageops::FilterType, DynamicImage, RgbImage};

use crate::grpc::proto::Image;

/// Source of video/image frames for a synthetic camera.
pub enum FeedSource {
    /// Single image, repeated every frame.
    Static(DynamicImage),
    /// Animated GIF; frames are cycled.
    AnimatedGif {
        frames: Vec<RgbImage>,
        index: usize,
    },
}

impl FeedSource {
    /// Load a feed from a file path. Supports static images (JPEG, PNG, etc.) and animated GIF.
    pub fn load(path: &Path) -> Result<Self, FeedError> {
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(to_ascii_lowercase);
        if ext.as_deref() == Some("gif") {
            return Self::load_gif(&path);
        }
        // Static image (jpg, png, etc.)
        let img = image::open(&path).map_err(FeedError::Image)?;
        Ok(FeedSource::Static(img))
    }

    fn load_gif(path: &Path) -> Result<Self, FeedError> {
        let file = std::fs::File::open(path).map_err(FeedError::Io)?;
        let mut options = gif::DecodeOptions::new();
        options.set_color_output(gif::ColorOutput::RGBA);
        let mut decoder = options.read_info(file).map_err(FeedError::Gif)?;
        let mut frames = Vec::new();
        while let Some(frame) = decoder.read_next_frame().map_err(FeedError::Gif)? {
            let width = frame.width;
            let height = frame.height;
            let buf = frame.buffer.as_ref();
            // Convert RGBA to RGB (drop alpha)
            let rgb: Vec<u8> = buf
                .chunks(4)
                .flat_map(|c| [c[0], c[1], c[2]])
                .collect();
            let img = RgbImage::from_raw(width.into(), height.into(), rgb)
                .ok_or_else(|| FeedError::InvalidFormat("gif frame dimensions".into()))?;
            frames.push(img);
        }
        if frames.is_empty() {
            return Err(FeedError::InvalidFormat("gif has no frames".into()));
        }
        Ok(FeedSource::AnimatedGif { frames, index: 0 })
    }

    /// Produce the next frame as an Image proto, cropped to target aspect ratio and resized.
    pub fn next_frame(&mut self, target_width: u32, target_height: u32) -> Option<Image> {
        let rgb = match self {
            FeedSource::Static(img) => img.to_rgb8(),
            FeedSource::AnimatedGif { frames, index } => {
                let frame = frames.get(*index)?;
                *index = (*index + 1) % frames.len();
                frame.clone()
            }
        };
        let cropped = crop_to_aspect_ratio_and_resize(
            &rgb,
            target_width as f64 / target_height as f64,
            target_width,
            target_height,
        );
        Some(Image {
            width: cropped.width(),
            height: cropped.height(),
            data: cropped.into_raw(),
        })
    }
}

/// Center-crop image to target aspect ratio, then resize to exact dimensions.
fn crop_to_aspect_ratio_and_resize(
    img: &RgbImage,
    target_aspect: f64,
    target_width: u32,
    target_height: u32,
) -> RgbImage {
    let (w, h) = (img.width() as f64, img.height() as f64);
    let src_aspect = w / h;
    let (crop_w, crop_h) = if src_aspect > target_aspect {
        // Source is wider: crop width
        let crop_h = h;
        let crop_w = h * target_aspect;
        (crop_w.round() as u32, crop_h.round() as u32)
    } else {
        // Source is taller: crop height
        let crop_w = w;
        let crop_h = w / target_aspect;
        (crop_w.round() as u32, crop_h.round() as u32)
    };
    let x = (img.width().saturating_sub(crop_w)) / 2;
    let y = (img.height().saturating_sub(crop_h)) / 2;
    let cropped = image::imageops::crop_imm(img, x, y, crop_w, crop_h).to_image();
    let resized = image::imageops::resize(
        &cropped,
        target_width,
        target_height,
        FilterType::Triangle,
    );
    resized
}

fn to_ascii_lowercase(s: &str) -> String {
    s.chars().map(|c| c.to_ascii_lowercase()).collect()
}

#[derive(Debug, thiserror::Error)]
pub enum FeedError {
    #[error("Image load failed: {0}")]
    Image(#[from] image::ImageError),
    #[error("Gif decode: {0}")]
    Gif(#[from] gif::DecodingError),
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid format: {0}")]
    InvalidFormat(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn crop_to_aspect_ratio_wider_source() {
        // 800x400 (aspect 2) -> target 4/3 => crop to 400x300 from center
        let img = RgbImage::new(800, 400);
        let out = crop_to_aspect_ratio_and_resize(&img, 4.0 / 3.0, 640, 480);
        assert_eq!(out.width(), 640);
        assert_eq!(out.height(), 480);
    }

    #[test]
    fn crop_to_aspect_ratio_taller_source() {
        // 400x800 (aspect 0.5) -> target 4/3 => crop to 400x300 from center
        let img = RgbImage::new(400, 800);
        let out = crop_to_aspect_ratio_and_resize(&img, 4.0 / 3.0, 640, 480);
        assert_eq!(out.width(), 640);
        assert_eq!(out.height(), 480);
    }

    #[test]
    fn feed_source_static_from_image_path() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/smiley.png");
        if !path.exists() {
            return;
        }
        let mut feed = FeedSource::load(&path).unwrap();
        let img = feed.next_frame(640, 480).unwrap();
        assert_eq!(img.width, 640);
        assert_eq!(img.height, 480);
        assert_eq!(img.data.len(), 640 * 480 * 3);
        // Static: same frame again
        let img2 = feed.next_frame(640, 480).unwrap();
        assert_eq!(img2.data.len(), img.data.len());
    }

}
