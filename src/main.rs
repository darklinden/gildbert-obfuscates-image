use clap::Parser;
use image::{GenericImageView, ImageBuffer, Rgba};
use std::{path::PathBuf, time};

/// Image obfuscation tool based on Gilbert curve pixel rearrangement.
/// Losslessly shuffles pixels along a space-filling curve for encoding,
/// and reverses the process for decoding.
#[derive(Parser)]
#[command(name = "gildbert-obfuscates-image", version, about)]
struct Cli {
    /// Encode (obfuscate) the image
    #[arg(short = 'e', long = "encode", conflicts_with = "decode")]
    encode: bool,

    /// Decode (restore) the image
    #[arg(short = 'd', long = "decode", conflicts_with = "encode")]
    decode: bool,

    /// Input image path
    input: PathBuf,

    /// Output image path (defaults to <input>_encoded.png or <input>_decoded.png)
    #[arg(short, long)]
    output: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Gilbert 2D curve generation (direct port from the HTML/JS version)
// ---------------------------------------------------------------------------

fn gilbert2d(width: i64, height: i64) -> Vec<(i64, i64)> {
    let mut coordinates = Vec::with_capacity((width * height) as usize);
    if width >= height {
        generate2d(0, 0, width, 0, 0, height, &mut coordinates);
    } else {
        generate2d(0, 0, 0, height, width, 0, &mut coordinates);
    }
    coordinates
}

fn generate2d(x: i64, y: i64, ax: i64, ay: i64, bx: i64, by: i64, coords: &mut Vec<(i64, i64)>) {
    let w = (ax + ay).unsigned_abs() as i64;
    let h = (bx + by).unsigned_abs() as i64;
    let dax = ax.signum();
    let day = ay.signum();
    let dbx = bx.signum();
    let dby = by.signum();

    if h == 1 {
        let (mut cx, mut cy) = (x, y);
        for _ in 0..w {
            coords.push((cx, cy));
            cx += dax;
            cy += day;
        }
        return;
    }
    if w == 1 {
        let (mut cx, mut cy) = (x, y);
        for _ in 0..h {
            coords.push((cx, cy));
            cx += dbx;
            cy += dby;
        }
        return;
    }

    let mut ax2 = ax.div_euclid(2);
    let mut ay2 = ay.div_euclid(2);
    let mut bx2 = bx.div_euclid(2);
    let mut by2 = by.div_euclid(2);

    if 2 * w > 3 * h {
        if ((ax2 + ay2).abs() % 2 != 0) && (w > 2) {
            ax2 += dax;
            ay2 += day;
        }
        generate2d(x, y, ax2, ay2, bx, by, coords);
        generate2d(x + ax2, y + ay2, ax - ax2, ay - ay2, bx, by, coords);
    } else {
        if ((bx2 + by2).abs() % 2 != 0) && (h > 2) {
            bx2 += dbx;
            by2 += dby;
        }
        generate2d(x, y, bx2, by2, ax2, ay2, coords);
        generate2d(x + bx2, y + by2, ax, ay, bx - bx2, by - by2, coords);
        generate2d(
            x + (ax - dax) + (bx2 - dbx),
            y + (ay - day) + (by2 - dby),
            -bx2,
            -by2,
            -(ax - ax2),
            -(ay - ay2),
            coords,
        );
    }
}

// ---------------------------------------------------------------------------
// Core encode / decode
// ---------------------------------------------------------------------------

fn process_image(img: &image::DynamicImage, encrypt: bool) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
    let (width, height) = img.dimensions();
    let total_pixels = (width as usize) * (height as usize);

    // Generate curve
    let curve = gilbert2d(width as i64, height as i64);
    assert_eq!(
        curve.len(),
        total_pixels,
        "Gilbert curve length ({}) does not match image pixel count ({})",
        curve.len(),
        total_pixels,
    );

    // Golden-ratio based offset (same as the JS version)
    let phi_frac = (5.0_f64.sqrt() - 1.0) / 2.0;
    let offset = ((phi_frac * total_pixels as f64).floor() as usize) % total_pixels;

    let src = img.to_rgba8();
    let mut dest = ImageBuffer::<Rgba<u8>, Vec<u8>>::new(width, height);

    for i in 0..total_pixels {
        let (src_pos, dest_pos) = if encrypt {
            // Source[curve[i]] -> Dest[curve[(i+offset) % N]]
            let p1 = curve[i];
            let p2 = curve[(i + offset) % total_pixels];
            (p1, p2)
        } else {
            // Source[curve[(i+offset) % N]] -> Dest[curve[i]]
            let p1 = curve[(i + offset) % total_pixels];
            let p2 = curve[i];
            (p1, p2)
        };

        let pixel = *src.get_pixel(src_pos.0 as u32, src_pos.1 as u32);
        dest.put_pixel(dest_pos.0 as u32, dest_pos.1 as u32, pixel);
    }

    dest
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let cli = Cli::parse();

    if !cli.encode && !cli.decode {
        eprintln!("Error: specify either -e/--encode or -d/--decode");
        std::process::exit(1);
    }

    let img = image::open(&cli.input).unwrap_or_else(|e| {
        eprintln!("Error: failed to open '{}': {e}", cli.input.display());
        std::process::exit(1);
    });

    let encrypt = cli.encode;

    println!(
        "{} '{}' ({}x{}) …",
        if encrypt { "Encoding" } else { "Decoding" },
        cli.input.display(),
        img.width(),
        img.height(),
    );

    let result = process_image(&img, encrypt);
    let mut output_path = if let Some(output_path) = cli.output {
        output_path
    } else {
        cli.input
    };

    if output_path
        .extension()
        .unwrap_or_else(|| "".as_ref())
        .to_str()
        .unwrap_or_default()
        .to_lowercase()
        != "png"
    {
        output_path = output_path.with_extension("png");
    }

    let tmp_path = output_path.with_extension(format!(
        "tmp_{}.png",
        time::SystemTime::now()
            .duration_since(time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));

    result.save(&tmp_path).unwrap_or_else(|e| {
        eprintln!("Error: failed to save '{}': {e}", tmp_path.display());
        std::process::exit(1);
    });

    std::fs::rename(&tmp_path, &output_path).unwrap_or_else(|e| {
        eprintln!(
            "Error: failed to rename '{}' to '{}': {e}",
            tmp_path.display(),
            output_path.display()
        );
        std::process::exit(1);
    });

    println!("Saved to '{}'", output_path.display());
}
