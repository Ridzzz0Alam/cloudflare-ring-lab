//! Section "Trick 1: Six Bytes Instead of Eight": why a u16 index alone saves nothing.
//!
//! cargo run --release --example 05_struct_layout
use std::mem::{align_of, size_of};

use clap::Parser;
use ring_lab::{CommonArgs, Run};
use serde::Serialize;

#[derive(Parser)]
struct Args {
    #[command(flatten)]
    common: CommonArgs,
}

// 1. The original point: 4-byte hash + 4-byte server index.
#[allow(dead_code)]
struct PointU32 { hash: u32, node: u32 }

// 2. The obvious fix: shrink the index to u16. Alignment pads it back to 8.
#[allow(dead_code)]
struct PointU16 { hash: u32, node: u16 }

// 3. Packed struct: 6 bytes, but references to its fields are unaligned (unsafe to borrow).
#[allow(dead_code)]
#[repr(C, packed)]
struct PointPacked { hash: u32, node: u16 }

// 4. What pingora-ketama v2 does: raw bytes, read back with from_ne_bytes.
#[allow(dead_code)]
#[repr(transparent)]
struct PointBytes([u8; 6]);

#[derive(Serialize)]
struct Row {
    layout: &'static str,
    size: usize,
    align: usize,
    padding: usize,
    mb_per_100m_points: f64,
}

fn main() {
    let args = Args::parse();
    let mut run = Run::start(&args.common, "e4_layout", serde_json::json!({})).unwrap();
    let rows = [
        ("struct { u32, u32 }", size_of::<PointU32>(), align_of::<PointU32>(), 8),
        ("struct { u32, u16 }", size_of::<PointU16>(), align_of::<PointU16>(), 6),
        ("packed { u32, u16 }", size_of::<PointPacked>(), align_of::<PointPacked>(), 6),
        ("[u8; 6]", size_of::<PointBytes>(), align_of::<PointBytes>(), 6),
    ];
    println!("{:<20} | {:>4} | {:>5} | {:>7} | {:>16}", "layout", "size", "align", "padding", "100M points (MB)");
    for (layout, size, align, payload) in rows {
        let mb = size as f64 * 1e8 / 1e6;
        println!("{layout:<20} | {size:>3}B | {align:>5} | {:>6}B | {mb:>16.0}", size - payload);
        run.row(&Row { layout, size, align, padding: size - payload, mb_per_100m_points: mb });
    }
    // 5. Arrays of points are laid out back to back, so element size is all that matters.
    println!(
        "\n[PointU16; 1000] = {} bytes, [PointBytes; 1000] = {} bytes ({:.0}% smaller)",
        size_of::<[PointU16; 1000]>(),
        size_of::<[PointBytes; 1000]>(),
        (1.0 - size_of::<[PointBytes; 1000]>() as f64 / size_of::<[PointU16; 1000]>() as f64) * 100.0
    );
    run.finish();
}
