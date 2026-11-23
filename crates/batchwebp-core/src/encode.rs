//! libwebp encoding of an 8-bit RGB/RGBA buffer.

use std::borrow::Cow;
use std::ffi::{c_int, c_void};

use image::{ColorType, DynamicImage};
use libwebp_sys as webp;
use serde::{Deserialize, Serialize};

/// WebP's hard limit on either side.
const MAX_DIMENSION: u32 = 16383;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    /// Pixel-exact (cwebp -lossless). Fully transparent pixels may lose their hidden RGB.
    Lossless,
    /// Lossless bitstream after a small value-snapping pass (cwebp -near_lossless).
    NearLossless,
    /// VP8 lossy (cwebp -q).
    Lossy,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Encoding {
    pub mode: Mode,
    /// 0 (fastest) ..= 9 (smallest). Lossless: libwebp's `-z` preset. Lossy: mapped onto `-m 0..6`.
    pub effort: u8,
    /// Near-lossless strength, 0 (strongest) ..= 100 (off).
    pub near_lossless: u8,
    /// Lossy quality, 0 ..= 100.
    pub quality: u8,
}

impl Default for Encoding {
    fn default() -> Self {
        // Effort 0 is several times faster than 1 (~8 points less saving): the default for bulk runs.
        Self { mode: Mode::Lossless, effort: 0, near_lossless: 60, quality: 90 }
    }
}

pub enum Encoded {
    Webp(Vec<u8>),
    /// WebP cannot hold it as asked: over 16383 px, more than 8 bits per channel in lossless mode,
    /// or lossy partition 0 still over 512 KiB after the method 2 retry.
    Keep,
}

pub fn encode(img: &DynamicImage, enc: &Encoding) -> Result<Encoded, String> {
    let (width, height) = (img.width(), img.height());
    if width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Ok(Encoded::Keep);
    }
    let eight_bit = matches!(img.color(), ColorType::L8 | ColorType::La8 | ColorType::Rgb8 | ColorType::Rgba8);
    if !eight_bit && enc.mode == Mode::Lossless {
        return Ok(Encoded::Keep);
    }

    // Borrow the common layouts; convert the rest (gray, 16-bit, float) once.
    let (pixels, bpp): (Cow<[u8]>, c_int) = match img {
        DynamicImage::ImageRgb8(b) => (Cow::Borrowed(b.as_raw()), 3),
        DynamicImage::ImageRgba8(b) => (Cow::Borrowed(b.as_raw()), 4),
        _ if img.color().has_alpha() => (Cow::Owned(img.to_rgba8().into_raw()), 4),
        _ => (Cow::Owned(img.to_rgb8().into_raw()), 3),
    };

    let mut config = webp::WebPConfig::new().map_err(|_| "libwebp: config init failed")?;
    match enc.mode {
        Mode::Lossless | Mode::NearLossless => {
            // libwebp skips the near-lossless pass at method 0 (effort 0), so it starts at effort 1.
            let level = if enc.mode == Mode::NearLossless { enc.effort.clamp(1, 9) } else { enc.effort.min(9) };
            // SAFETY: config is initialised; the preset only writes plain fields.
            if unsafe { webp::WebPConfigLosslessPreset(&mut config, level as c_int) } == 0 {
                return Err("libwebp: bad lossless preset".into());
            }
            if enc.mode == Mode::NearLossless {
                config.near_lossless = enc.near_lossless.min(100) as c_int;
            }
        }
        Mode::Lossy => {
            config.quality = enc.quality.min(100) as f32;
            config.method = ((enc.effort.min(9) as c_int) * 2 + 1) / 3;
        }
    }
    // One image per worker thread: libwebp's internal threading would only oversubscribe the cores.
    config.thread_level = 0;
    // SAFETY: config is initialised.
    if unsafe { webp::WebPValidateConfig(&config) } == 0 {
        return Err("libwebp: invalid config".into());
    }

    let mut out: Vec<u8> = Vec::with_capacity(pixels.len() / 8);
    let mut picture = webp::WebPPicture::new().map_err(|_| "libwebp: picture init failed")?;
    picture.use_argb = (enc.mode != Mode::Lossy) as c_int;
    picture.width = width as c_int;
    picture.height = height as c_int;
    picture.writer = Some(write_to_vec);
    picture.custom_ptr = (&mut out as *mut Vec<u8>).cast::<c_void>();

    let stride = width as c_int * bpp;
    // SAFETY: `pixels` holds height * stride bytes and outlives the calls; `out` outlives every
    // WebPEncode, the only caller of `write_to_vec`. WebPPictureFree runs on every path after the import.
    unsafe {
        let imported = if bpp == 4 {
            webp::WebPPictureImportRGBA(&mut picture, pixels.as_ptr(), stride)
        } else {
            webp::WebPPictureImportRGB(&mut picture, pixels.as_ptr(), stride)
        };
        let mut ok = imported != 0 && webp::WebPEncode(&config, &mut picture) != 0;
        // Lossy only. VP8 caps partition 0 (per-macroblock modes) at 512 KiB. From method 2 up, libwebp
        // budgets mode bits to fit; methods 0-1 pick modes without that budget, so large detailed images
        // (85 MP in our test) overflow. Retry once at method 2, which fit even a 16380 px worst case.
        if !ok && picture.error_code == webp::WebPEncodingError::VP8_ENC_ERROR_PARTITION0_OVERFLOW && config.method < 2 {
            out.clear();
            config.method = 2;
            ok = webp::WebPEncode(&config, &mut picture) != 0;
        }
        let error = picture.error_code;
        webp::WebPPictureFree(&mut picture);
        match error {
            _ if ok => Ok(Encoded::Webp(out)),
            // Still too big even with libwebp's mode budget: lossy VP8 cannot hold this image.
            webp::WebPEncodingError::VP8_ENC_ERROR_PARTITION0_OVERFLOW => Ok(Encoded::Keep),
            _ => Err(format!("libwebp: {error:?}")),
        }
    }
}

unsafe extern "C" fn write_to_vec(data: *const u8, size: usize, picture: *const webp::WebPPicture) -> c_int {
    if size == 0 {
        return 1;
    }
    // SAFETY: custom_ptr is the `Vec<u8>` set up in `encode`, alive for the whole WebPEncode call;
    // libwebp passes `size` readable bytes at `data`.
    unsafe {
        let out = &mut *(*picture).custom_ptr.cast::<Vec<u8>>();
        out.extend_from_slice(std::slice::from_raw_parts(data, size));
    }
    1
}

#[cfg(test)]
mod tests {
    use image::{ImageBuffer, Rgb};

    use super::*;

    /// 85 MP where every 4x4 block is a gradient in a random direction: intra-4 modes everywhere, which
    /// overflows partition 0 at method 0-1 (VP8_ENC_ERROR_PARTITION0_OVERFLOW). Takes ~30 s.
    #[test]
    fn lossy_partition0_overflow_is_encoded() {
        let side = 9216u32;
        let blocks = side / 4;
        let mut s = 0x9E37_79B9u32;
        let dirs: Vec<(i32, i32)> = (0..blocks * blocks)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                [(1, 0), (0, 1), (1, 1), (1, -1), (-1, 0), (0, -1), (-1, -1), (-1, 1)][(s % 8) as usize]
            })
            .collect();
        let img = ImageBuffer::from_fn(side, side, |x, y| {
            let (dx, dy) = dirs[((y / 4) * blocks + x / 4) as usize];
            let v = (128 + 30 * (dx * (x % 4) as i32 + dy * (y % 4) as i32)) as u8;
            Rgb([v, v.wrapping_add(40), v.wrapping_sub(40)])
        });
        let enc = Encoding { mode: Mode::Lossy, effort: 0, quality: 100, ..Encoding::default() };
        assert!(matches!(encode(&DynamicImage::ImageRgb8(img), &enc), Ok(Encoded::Webp(_))));
    }
}
