//! Source decoding. The `image` crate is the single decode path: Project-Curator benchmarked it as the
//! fastest accurate decoder for every format in release builds (zune-jpeg, png/fdeflate, gif, tiff).

use std::io::Cursor;

use image::codecs::gif::GifDecoder;
use image::codecs::png::PngDecoder;
use image::{AnimationDecoder, DynamicImage, ImageDecoder, ImageFormat, ImageReader, ImageResult, Limits};

pub enum Decoded {
    Still(DynamicImage),
    /// Animated (APNG, GIF) or empty: a still WebP would lose content, so the original stays.
    Keep,
}

pub fn decode(data: &[u8]) -> ImageResult<Decoded> {
    let format = image::guess_format(data)?;
    match format {
        ImageFormat::Png => {
            let dec = PngDecoder::with_limits(Cursor::new(data), Limits::no_limits())?;
            if dec.is_apng()? {
                return Ok(Decoded::Keep);
            }
            still(dec)
        }
        ImageFormat::Gif => {
            let mut frames = GifDecoder::new(Cursor::new(data))?.into_frames();
            let first = match frames.next() {
                Some(frame) => frame?,
                None => return Ok(Decoded::Keep),
            };
            if frames.next().is_some() {
                return Ok(Decoded::Keep);
            }
            Ok(Decoded::Still(DynamicImage::ImageRgba8(first.into_buffer())))
        }
        _ => {
            let mut reader = ImageReader::with_format(Cursor::new(data), format);
            reader.no_limits();
            still(reader.into_decoder()?)
        }
    }
}

/// Decodes one frame and bakes in the EXIF orientation: the WebP carries no EXIF, so a rotated JPEG
/// would otherwise display sideways. Rotation and flips move pixels; they never change values.
fn still(mut dec: impl ImageDecoder) -> ImageResult<Decoded> {
    let orientation = dec.orientation()?;
    let mut img = DynamicImage::from_decoder(dec)?;
    img.apply_orientation(orientation);
    Ok(Decoded::Still(img))
}
