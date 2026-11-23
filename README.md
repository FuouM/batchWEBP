# batchWEBP

Convert a folder of images to WebP: lossless, near-lossless or lossy, using every core. Tauri app with a SoulWindow UI, plus a CLI (`batchwebp`) on the same engine.

## What it does

- Walks the source folder (subfolders optional) for `png jpg jpeg jfif jpe bmp dib gif tif tiff tga qoi`. Other files and existing `.webp` files are left alone.
- **Output folder** (default `<source>_webp` next to the source): mirrors the tree. **In place**: writes `name.webp` next to the original, then deletes the original.
- Encodes with libwebp, one image per worker thread, one worker per logical core.
- **Keep original if WebP isn't smaller** (on by default): the original stays, or is copied into the output folder so the mirror is complete. Lossless WebP is often bigger than a JPEG.
- Never turned into a still WebP: animated GIF/APNG; images over 16383 px (the WebP limit); 16-bit images in lossless mode. These are kept the same way.
- Lossy mode: VP8 limits the first partition (per-block modes) to 512 KiB. At effort 0–2 (libwebp `-m 0/1`), modes are chosen without that budget, so large, detailed images can exceed the limit (`VP8_ENC_ERROR_PARTITION0_OVERFLOW`, seen at 85 MP). Those images are re-encoded once at `-m 2`, where libwebp budgets the mode bits; that handled a worst-case 16380 px test image. If an image still does not fit, the original is kept.
- Applies the EXIF orientation to the pixels, because the WebP has no EXIF. Metadata (EXIF, ICC) is not copied.
- Writes to a temp file and renames it, and keeps the source mtime. Existing outputs are skipped, so you can stop a run and resume it. In place never overwrites: `a.png` and `a.jpg` both map to `a.webp`.
- Lossless means the visible pixels are identical. Fully transparent pixels can lose their hidden RGB, as with cwebp's default (`-exact` off).

## Effort

Effort is libwebp's lossless preset (`cwebp -z 0..9`); in lossy mode it maps to `-m 0..6`. Measured on 524 PNGs of 2–4 MP with 12 threads (Ryzen 5 3600):

| Effort | Lossless saving | Throughput |
| --- | --- | --- |
| 0 (default) | 18.9% | 45 img/s |
| 1 | 27.0% | 8 img/s |

Above effort 2, files get only slightly smaller and encoding gets much slower. Near-lossless needs effort 1 or more; at effort 0 it runs at effort 1, because libwebp skips the near-lossless pass at method 0.

## CLI

```
batchwebp <folder> [-o <out> | --in-place] [-m lossless|near|lossy] [-e 0-9] [--near 60] [-q 90]
          [--no-recursive] [--always-webp] [--overwrite] [-j threads]
```

The exit code is 1 when any file failed. Each failure is printed at the end.

## Setup

The Rust toolchain is shared with a sibling repo through a junction (gitignored):

```
cmd /c mklink /J .rust ..\..\Sabitori\.rust
```

Then:

```
.\dev.ps1            # Tauri dev app (add -Release to judge speed)
. .\env.ps1; cargo build --release -p batchwebp-cli
. .\env.ps1; cargo test --release -p batchwebp-core
```

Layout: `crates/batchwebp-core` (walk, decode, encode, write), `crates/batchwebp-cli`, `src-tauri` (commands `start`, `cancel`, `status`), `src` (Solid + SoulWindow UI).

## License

MIT
