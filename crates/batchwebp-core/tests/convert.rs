//! End to end: fixtures -> run() in output-folder and in-place modes -> decode the WebPs with
//! libwebp and compare pixels; check resume, kept originals and in-place deletion.
use std::path::Path;
use std::sync::atomic::AtomicBool;

use batchwebp_core::{Encoding, Mode, Options, Progress, run};
use image::codecs::gif::GifEncoder;
use image::{Delay, DynamicImage, Frame, ImageBuffer, Luma, Rgb, Rgba, RgbaImage};

fn noise(w: u32, h: u32, alpha: bool) -> RgbaImage {
    let mut s = 0x1234_5678u32;
    ImageBuffer::from_fn(w, h, |x, y| {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        let a = if alpha { [0, 128, 255][(s % 3) as usize] } else { 255 };
        Rgba([((x * 7) ^ y) as u8, (s >> 8) as u8 & 0xF0, (y * 3) as u8, a])
    })
}

fn webp_rgba(path: &Path) -> RgbaImage {
    let data = std::fs::read(path).unwrap();
    let (mut w, mut h) = (0, 0);
    unsafe {
        let p = libwebp_sys::WebPDecodeRGBA(data.as_ptr(), data.len(), &mut w, &mut h);
        assert!(!p.is_null(), "decode {}", path.display());
        let v = std::slice::from_raw_parts(p, (w * h * 4) as usize).to_vec();
        libwebp_sys::WebPFree(p.cast());
        RgbaImage::from_raw(w as u32, h as u32, v).unwrap()
    }
}

/// Visible pixels must match exactly; fully transparent ones only need alpha 0 (exact = 0).
fn assert_lossless(src: &RgbaImage, webp: &RgbaImage, name: &str) {
    assert_eq!(src.dimensions(), webp.dimensions(), "{name} dims");
    for (a, b) in src.pixels().zip(webp.pixels()) {
        if a[3] == 0 {
            assert_eq!(b[3], 0, "{name} alpha");
        } else {
            assert_eq!(a, b, "{name} pixel");
        }
    }
}

#[test]
fn output_folder_resume_and_in_place() {
    let root = std::env::temp_dir().join(format!("batchwebp-test-{}", std::process::id()));
    let src = root.join("in");
    std::fs::create_dir_all(src.join("sub/deeper")).unwrap();

    let opaque = noise(64, 48, false);
    let alpha = noise(33, 17, true);
    DynamicImage::ImageRgba8(opaque.clone()).to_rgb8().save(src.join("opaque.png")).unwrap();
    alpha.save(src.join("sub/alpha.PNG")).unwrap();
    DynamicImage::ImageRgba8(opaque.clone()).to_rgb8().save(src.join("sub/deeper/b.bmp")).unwrap();
    ImageBuffer::from_fn(40, 30, |x, y| Luma([(x * y) as u8])).save(src.join("gray.png")).unwrap();
    ImageBuffer::from_fn(20, 20, |x, y| Rgb([(x * 3000) as u16, (y * 3000) as u16, 7u16])).save(src.join("deep16.png")).unwrap();
    DynamicImage::ImageRgba8(opaque.clone()).to_rgb8().save(src.join("photo.jpg")).unwrap();
    {
        let f = std::fs::File::create(src.join("anim.gif")).unwrap();
        let mut enc = GifEncoder::new(f);
        let d = Delay::from_numer_denom_ms(100, 1);
        enc.encode_frames([Frame::from_parts(noise(16, 16, false), 0, 0, d), Frame::from_parts(noise(16, 16, true), 0, 0, d)]).unwrap();
    }
    std::fs::write(src.join("broken.png"), b"not a png").unwrap();
    std::fs::write(src.join("notes.txt"), b"ignored").unwrap();

    // Output-folder mode, lossless, always write the WebP so every convertible file shows up.
    let out = root.join("out");
    let mut opts = Options::new(src.clone());
    opts.output = Some(out.clone());
    opts.keep_smaller_original = false;
    let progress = Progress::default();
    run(&opts, &AtomicBool::new(false), &progress).unwrap();
    let s = progress.snapshot();
    assert_eq!((s.found, s.converted, s.kept, s.failed), (8, 5, 2, 1), "counts");
    assert_lossless(&opaque, &webp_rgba(&out.join("opaque.webp")), "opaque");
    assert_lossless(&alpha, &webp_rgba(&out.join("sub/alpha.webp")), "alpha");
    assert_lossless(&opaque, &webp_rgba(&out.join("sub/deeper/b.webp")), "bmp");
    assert!(out.join("anim.gif").exists() && out.join("deep16.png").exists(), "kept originals copied");
    assert!(!out.join("notes.txt").exists());
    let mt = |p: &Path| std::fs::metadata(p).unwrap().modified().unwrap();
    assert_eq!(mt(&out.join("opaque.webp")), mt(&src.join("opaque.png")), "mtime kept");

    // Resume: everything already exists.
    let progress = Progress::default();
    run(&opts, &AtomicBool::new(false), &progress).unwrap();
    let s = progress.snapshot();
    assert_eq!((s.skipped, s.converted), (7, 0), "rerun skips");

    // In place, near-lossless, keep originals that do not shrink.
    let mut opts = Options::new(src.clone());
    opts.output = None;
    opts.encoding = Encoding { mode: Mode::NearLossless, ..Encoding::default() };
    let progress = Progress::default();
    run(&opts, &AtomicBool::new(false), &progress).unwrap();
    let s = progress.snapshot();
    // Near-lossless may drop 16-bit to 8-bit; the noisy JPEG grows as WebP and stays.
    assert!(src.join("anim.gif").exists() && src.join("broken.png").exists() && src.join("photo.jpg").exists());
    assert!(src.join("deep16.webp").exists() && !src.join("deep16.png").exists() && !src.join("opaque.png").exists());
    assert_eq!((s.converted, s.kept, s.failed), (5, 2, 1), "in-place counts");

    std::fs::remove_dir_all(&root).unwrap();
}
