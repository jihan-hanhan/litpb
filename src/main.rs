use clap::Parser;
use litbp::{
    img_to_snbt, scaled_dims, validate_grid, Options, Resampling, WARN_CELLS,
};
use std::io::Write;
use std::process::exit;

#[derive(Parser)]
#[command(name = "litbp", version, about = "Convert images to LittleTiles blueprints (.snbt)")]
struct Args {
    /// Image input
    #[arg(short = 'i', long)]
    input: String,

    /// Export result as .snbt file
    #[arg(short = 'o', long)]
    output: Option<String>,

    /// Scale
    #[arg(short = 's', long, default_value = "1.0")]
    scale: f64,

    /// Grid: 1 or a multiple of 4, within 1..=32
    #[arg(short = 'g', long, default_value = "16", value_parser = parse_grid)]
    grid: i32,

    /// Block type
    #[arg(short = 'b', long, default_value = "littletiles:ltcoloredblock")]
    block: String,

    /// Resampling filter: nearest, bilinear, bicubic, lanczos
    #[arg(short = 'r', long, default_value = "nearest", value_parser = parse_resampling)]
    resampling: Resampling,

    /// Merge adjacent same-color tiles into larger bBoxes
    #[arg(short = 'm', long)]
    merge: bool,

    /// Posterize RGB to multiples of this step (0 = off), keeps alpha
    #[arg(short = 'q', long, default_value = "0")]
    quantize: u8,
}

fn parse_grid(s: &str) -> Result<i32, String> {
    let grid: i32 = s.parse().map_err(|_| "Fail : Invalid grid".to_string())?;
    if !validate_grid(grid) {
        return Err("Fail : Invalid grid (must be 1 or a multiple of 4, max=32)".to_string());
    }
    Ok(grid)
}

fn parse_resampling(s: &str) -> Result<Resampling, String> {
    Resampling::from_str(s)
        .ok_or_else(|| format!("Fail : Invalid resampling type '{s}' (nearest|bilinear|bicubic|lanczos)"))
}

fn main() {
    let args = Args::parse();

    println!("\x1b[32mStart chores\x1b[0m");
    println!("With Image \x1b[33m{}\x1b[0m", args.input);
    println!("With Scale \x1b[33m{}\x1b[0m", args.scale);
    println!("With grid \x1b[33m{}\x1b[0m", args.grid);
    println!("With block \x1b[33m{}\x1b[0m", args.block);
    println!("With resampling \x1b[33m{}\x1b[0m", args.resampling.name());
    println!("With merge \x1b[33m{}\x1b[0m", args.merge);
    println!(
        "With quantize \x1b[33m{}\x1b[0m",
        if args.quantize > 1 {
            args.quantize.to_string()
        } else {
            "off".to_string()
        }
    );

    if let Ok((w, h)) = image::image_dimensions(&args.input) {
        let (nw, nh) = scaled_dims(w, h, args.scale);
        if nw as u64 * nh as u64 >= WARN_CELLS {
            println!("\x1b[31mWarning : The Result was TOO BIG to load with LittleTiles . Are you SURE to make that?\x1b[0m");
        }
    }

    let opts = Options {
        scale: args.scale,
        grid: args.grid,
        block: args.block,
        resampling: args.resampling,
        merge: args.merge,
        quantize: args.quantize,
    };

    let result = match img_to_snbt(&args.input, &opts, |done, cells| {
        process_bar(done, cells);
    }) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            exit(1);
        }
    };
    println!();

    if let Some(output) = &args.output {
        if let Err(e) = std::fs::write(output, result.snbt.serialize()) {
            eprintln!("Fail : Cannot write output file {output}: {e}");
            exit(1);
        }
        println!("Saved file at \x1b[33m{}\x1b[0m", output);
    }

    println!("\x1b[32mAll chores done!\x1b[0m");
}

fn process_bar(var: u64, end: u64) {
    let filled = (20 * var / end) as usize;
    let bar = "━".repeat(filled) + &"─".repeat(20 - filled);
    print!("\rProcess[\x1b[36m{bar}\x1b[0m] {var}/{end}");
    let _ = std::io::stdout().flush();
}
