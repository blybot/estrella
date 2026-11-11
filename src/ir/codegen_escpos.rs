//! # ESC/POS Code Generation
//!
//! Compiles IR programs to ESC/POS bytes. The IR was designed around
//! StarPRNT, so a few ops need translation:
//!
//! - Star's separate size / expanded-width / expanded-height multipliers are
//!   combined into a single `GS !` character size.
//! - Feeds are in 1/4 mm; ESC/POS feeds in motion units, which Init sets to
//!   printer dots with GS P (2 dots per unit at 203 dpi). Without it the
//!   printer's defaults apply (1/360 inch vertical on Xprinter 80mm models).
//! - Rasters narrower than the print width are padded to full width here, so
//!   placement doesn't depend on how the firmware applies `ESC a` / `ESC $` to
//!   `GS v 0`. `SetAbsolutePosition` is only sent to the printer before text.
//! - `Beep` uses the vendor `ESC B` buzzer command (Xprinter and similar).
//! - Ops with no ESC/POS equivalent (PDF417, built-in NV logos) are rendered
//!   through the preview engine and sent as raster images; band graphics are
//!   sent as raster images. Upperline and reduced printing are dropped.

use super::ops::{Op, Program};
use crate::error::EstrellaError;
use crate::printer::PrinterConfig;
use crate::protocol::text::{Alignment, Font};
use crate::protocol::{cp437, escpos};

/// Rows per `GS v 0` command. Keeps each command well inside the receive
/// buffer of cheap printers.
const RASTER_CHUNK_ROWS: usize = 256;

/// Dots at 203 dpi per IR feed unit (1/4 mm). Init sets the motion units to
/// dots with GS P, so ESC J and ESC $ take dot counts.
const DOTS_PER_FEED_UNIT: u32 = 2;

/// Rows per Star band (`Op::Band`).
const BAND_ROWS: usize = 24;

#[derive(Default)]
struct EncoderState {
    alignment: Alignment,
    size_width: u8,
    size_height: u8,
    expanded_width: u8,
    expanded_height: u8,
    pending_position: Option<u16>,
}

impl EncoderState {
    fn char_size(&self) -> Vec<u8> {
        let width = (self.size_width + 1) * (self.expanded_width + 1);
        let height = (self.size_height + 1) * (self.expanded_height + 1);
        escpos::char_size(width.min(8) - 1, height.min(8) - 1)
    }
}

impl Program {
    /// Compile the IR program to ESC/POS bytes.
    pub fn to_escpos_bytes(&self, config: &PrinterConfig) -> Result<Vec<u8>, EstrellaError> {
        let print_width = config.width_dots as usize;
        let dpi = config.dpi.min(255) as u8;
        let mut state = EncoderState::default();
        let mut out = Vec::new();
        // Feeds are held back so one right before a cut can go into the cut
        // command itself. Xprinter's cut feeds to the cutter counting from the
        // last printed line, so a separate ESC J there would be swallowed.
        let mut pending_feed_dots: u32 = 0;

        for op in &self.ops {
            if !matches!(op, Op::Feed { .. } | Op::Cut { .. }) {
                feed(&mut out, std::mem::take(&mut pending_feed_dots));
            }
            match op {
                Op::Init => {
                    out.extend(escpos::init());
                    out.extend(escpos::motion_units(dpi, dpi));
                    out.extend(escpos::cancel_kanji_mode());
                    out.extend(escpos::code_table(0));
                    state = EncoderState::default();
                }
                Op::Cut { partial } => {
                    let extra = std::mem::take(&mut pending_feed_dots);
                    let in_cut = extra.min(255);
                    feed(&mut out, extra - in_cut);
                    out.extend(escpos::cut_feed(*partial, in_cut as u8));
                }
                // Two 150 ms beeps
                Op::Beep => out.extend(escpos::beep(2, 3)),
                Op::Feed { units } => {
                    pending_feed_dots += *units as u32 * DOTS_PER_FEED_UNIT;
                }

                Op::SetAlign(alignment) => {
                    state.alignment = *alignment;
                    out.extend(escpos::align(*alignment));
                }
                Op::SetFont(font) => out.extend(escpos::font(*font)),
                Op::SetBold(enabled) => out.extend(escpos::bold(*enabled)),
                Op::SetUnderline(enabled) => out.extend(escpos::underline(*enabled)),
                Op::SetInvert(enabled) => out.extend(escpos::invert(*enabled)),
                Op::SetSize { height, width } => {
                    state.size_height = (*height).min(7);
                    state.size_width = (*width).min(7);
                    out.extend(state.char_size());
                }
                Op::SetExpandedWidth(mult) => {
                    state.expanded_width = (*mult).min(7);
                    out.extend(state.char_size());
                }
                Op::SetExpandedHeight(mult) => {
                    state.expanded_height = (*mult).min(7);
                    out.extend(state.char_size());
                }
                Op::SetSmoothing(enabled) => out.extend(escpos::smoothing(*enabled)),
                Op::SetUpsideDown(enabled) => out.extend(escpos::upside_down(*enabled)),
                Op::SetUpperline(_) | Op::SetReduced(_) => {}
                // Star code page 1 is CP437, which is ESC/POS table 0. Init already
                // selects it, and no other code page is ever emitted.
                Op::SetCodepage(_) => out.extend(escpos::code_table(0)),
                Op::ResetStyle => {
                    let alignment = Alignment::Left;
                    state = EncoderState {
                        alignment,
                        ..EncoderState::default()
                    };
                    out.extend(escpos::align(alignment));
                    out.extend(escpos::font(Font::A));
                    out.extend(escpos::bold(false));
                    out.extend(escpos::underline(false));
                    out.extend(escpos::invert(false));
                    out.extend(state.char_size());
                    out.extend(escpos::upside_down(false));
                    out.extend(escpos::smoothing(true));
                }

                Op::Text(s) => {
                    if let Some(dots) = state.pending_position.take() {
                        out.extend(escpos::absolute_position(dots));
                    }
                    out.extend(cp437::encode(s));
                }
                Op::Newline => {
                    state.pending_position = None;
                    out.push(escpos::LF);
                }
                Op::Raw(bytes) => out.extend(bytes),
                Op::SetAbsolutePosition(dots) => state.pending_position = Some(*dots),

                Op::Raster {
                    width,
                    height,
                    data,
                } => {
                    let left = state.pending_position.take();
                    raster(
                        &mut out,
                        print_width,
                        *width as usize,
                        *height as usize,
                        data,
                        left,
                    );
                }
                Op::Band { width_bytes, data } => {
                    let width_bytes = *width_bytes as usize;
                    if width_bytes > 0 {
                        let band_bytes = width_bytes * BAND_ROWS;
                        let mut padded = data.clone();
                        padded.resize(data.len().div_ceil(band_bytes) * band_bytes, 0x00);
                        let left = state.pending_position.take();
                        raster(
                            &mut out,
                            print_width,
                            width_bytes * 8,
                            padded.len() / width_bytes,
                            &padded,
                            left,
                        );
                    }
                }

                Op::QrCode {
                    data,
                    cell_size,
                    error_level,
                } => out.extend(escpos::qr_code(data.as_bytes(), *cell_size, *error_level)),
                Op::Barcode1D { kind, data, height } => {
                    out.extend(escpos::barcode1d(*kind, data.as_bytes(), *height))
                }
                Op::Pdf417 { .. } => {
                    let mut ops = vec![Op::SetAlign(state.alignment)];
                    ops.push(op.clone());
                    rasterize(&mut out, print_width, ops)?;
                }
                Op::NvPrint { key, .. } => {
                    if crate::logos::by_key(key).is_none() {
                        return Err(EstrellaError::InvalidCommand(format!(
                            "ESC/POS printers have no stored logos; '{key}' is not a built-in logo"
                        )));
                    }
                    let mut ops = Vec::new();
                    if let Some(dots) = state.pending_position.take() {
                        ops.push(Op::SetAbsolutePosition(dots));
                    }
                    ops.push(op.clone());
                    rasterize(&mut out, print_width, ops)?;
                }
                Op::NvStore { .. } | Op::NvDelete { .. } => {
                    return Err(EstrellaError::InvalidCommand(
                        "NV logo storage is only supported on StarPRNT printers".into(),
                    ));
                }
            }
        }

        feed(&mut out, pending_feed_dots);
        Ok(out)
    }
}

/// Feed `dots` with ESC J, which takes at most 255 per command.
fn feed(out: &mut Vec<u8>, mut dots: u32) {
    while dots > 0 {
        let step = dots.min(255);
        out.extend(escpos::feed_dots(step as u8));
        dots -= step;
    }
}

/// Render ops through the preview engine and emit the result as a raster.
fn rasterize(out: &mut Vec<u8>, print_width: usize, ops: Vec<Op>) -> Result<(), EstrellaError> {
    let raw = crate::preview::render_raw(&Program { ops })
        .map_err(|e| EstrellaError::Image(format!("Failed to rasterize: {e}")))?;
    raster(out, print_width, raw.width, raw.height, &raw.data, Some(0));
    Ok(())
}

/// Emit a raster image padded to the full print width, in row chunks.
///
/// `left` is the horizontal offset in dots; `None` centres the image, which is
/// how the preview places rasters.
fn raster(
    out: &mut Vec<u8>,
    print_width: usize,
    width: usize,
    height: usize,
    data: &[u8],
    left: Option<u16>,
) {
    let src_bytes = width.div_ceil(8);
    if src_bytes == 0 || height == 0 {
        return;
    }
    let height = height.min(data.len() / src_bytes);

    let out_width = print_width.max(width);
    let out_bytes = out_width.div_ceil(8);
    let left = match left {
        Some(dots) => (dots as usize).min(out_width - width),
        None => (out_width - width) / 2,
    };

    let mut padded = vec![0u8; out_bytes * height];
    for row in 0..height {
        let src = &data[row * src_bytes..(row + 1) * src_bytes];
        let dst = &mut padded[row * out_bytes..(row + 1) * out_bytes];
        if left % 8 == 0 {
            let start = left / 8;
            dst[start..start + src_bytes].copy_from_slice(src);
        } else {
            for x in 0..width {
                if src[x / 8] & (0x80 >> (x % 8)) != 0 {
                    let dx = left + x;
                    dst[dx / 8] |= 0x80 >> (dx % 8);
                }
            }
        }
    }

    for chunk in padded.chunks(out_bytes * RASTER_CHUNK_ROWS) {
        out.extend(escpos::raster(
            out_bytes as u16,
            (chunk.len() / out_bytes) as u16,
            chunk,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::BarcodeKind;

    const PREAMBLE: [u8; 11] = [
        0x1B, 0x40, 0x1D, 0x50, 203, 203, 0x1C, 0x2E, 0x1B, 0x74, 0x00,
    ];

    fn encode(ops: Vec<Op>) -> Vec<u8> {
        Program { ops }
            .to_escpos_bytes(&PrinterConfig::ESCPOS_80MM)
            .unwrap()
    }

    #[test]
    fn init_sets_dot_motion_units_cancels_chinese_mode_and_selects_cp437() {
        assert_eq!(encode(vec![Op::Init]), PREAMBLE);
    }

    #[test]
    fn spacer_feeds_in_printer_dots() {
        // The 80XX manual's default vertical motion unit is 1/360 inch, so
        // ESC J's dot count only holds once GS P sets the unit to 1/203 inch.
        let doc = crate::document::Document {
            document: vec![
                crate::document::Component::Text(crate::document::Text {
                    content: "above".into(),
                    ..Default::default()
                }),
                crate::document::Component::Spacer(crate::document::Spacer::mm(2.0)),
            ],
            cut: false,
            ..Default::default()
        };
        let bytes = doc.build_for(crate::printer::Protocol::EscPos).unwrap();

        let set_units = [0x1D, 0x50, 203, 203];
        let feed_2mm = [0x1B, 0x4A, 16];
        let units_at = bytes.windows(4).position(|w| w == set_units);
        let feed_at = bytes.windows(3).position(|w| w == feed_2mm);
        assert!(units_at.is_some(), "motion units never set: {bytes:02X?}");
        assert!(units_at < feed_at, "{bytes:02X?}");
    }

    #[test]
    fn text_is_cp437_encoded() {
        let bytes = encode(vec![Op::Text("café".into()), Op::Newline]);
        assert_eq!(bytes, vec![b'c', b'a', b'f', 0x82, 0x0A]);
    }

    #[test]
    fn feed_converts_quarter_mm_to_dots_and_splits_large_feeds() {
        assert_eq!(encode(vec![Op::Feed { units: 24 }]), vec![0x1B, 0x4A, 48]);
        // 200 units = 50mm = 400 dots → 255 + 145
        assert_eq!(
            encode(vec![Op::Feed { units: 200 }]),
            vec![0x1B, 0x4A, 255, 0x1B, 0x4A, 145]
        );
    }

    #[test]
    fn feed_before_a_cut_becomes_the_cuts_extra_feed() {
        // Xprinter's cut feeds to the cutter counting from the last printed
        // line, which swallows a separate ESC J feed before it.
        let bytes = encode(vec![
            Op::Text("x".into()),
            Op::Newline,
            Op::Feed { units: 80 },
            Op::Cut { partial: true },
        ]);
        assert_eq!(bytes, vec![b'x', 0x0A, 0x1D, 0x56, 0x42, 160]);
    }

    #[test]
    fn feed_before_a_cut_beyond_255_dots_feeds_the_rest_first() {
        // 200 units = 400 dots → ESC J 145, then cut with 255
        let bytes = encode(vec![Op::Feed { units: 200 }, Op::Cut { partial: false }]);
        assert_eq!(bytes, vec![0x1B, 0x4A, 145, 0x1D, 0x56, 0x41, 255]);
    }

    #[test]
    fn consecutive_feeds_before_a_cut_add_up() {
        let bytes = encode(vec![
            Op::Feed { units: 20 },
            Op::Feed { units: 4 },
            Op::Cut { partial: true },
        ]);
        assert_eq!(bytes, vec![0x1D, 0x56, 0x42, 48]);
    }

    #[test]
    fn feed_followed_by_content_is_sent_before_it() {
        let bytes = encode(vec![Op::Feed { units: 24 }, Op::Text("x".into())]);
        assert_eq!(bytes, vec![0x1B, 0x4A, 48, b'x']);
    }

    #[test]
    fn size_and_expansion_combine_into_one_char_size() {
        let bytes = encode(vec![
            Op::SetSize {
                height: 1,
                width: 0,
            },
            Op::SetExpandedWidth(1),
        ]);
        assert_eq!(bytes, vec![0x1D, 0x21, 0x01, 0x1D, 0x21, 0x11]);
    }

    #[test]
    fn beep_sounds_the_buzzer_twice() {
        assert_eq!(encode(vec![Op::Beep]), vec![0x1B, 0x42, 0x02, 0x03]);
    }

    #[test]
    fn upperline_and_reduced_are_dropped() {
        assert!(encode(vec![Op::SetUpperline(true), Op::SetReduced(true)]).is_empty());
    }

    #[test]
    fn absolute_position_is_sent_before_text() {
        let bytes = encode(vec![Op::SetAbsolutePosition(300), Op::Text("x".into())]);
        assert_eq!(bytes, vec![0x1B, 0x24, 0x2C, 0x01, b'x']);
    }

    #[test]
    fn absolute_position_offsets_a_raster_instead_of_being_sent() {
        let bytes = encode(vec![
            Op::SetAbsolutePosition(16),
            Op::Raster {
                width: 8,
                height: 1,
                data: vec![0xFF],
            },
        ]);
        let mut expected = vec![0x1D, 0x76, 0x30, 0x00, 72, 0, 1, 0];
        let mut row = vec![0u8; 72];
        row[2] = 0xFF;
        expected.extend(row);
        assert_eq!(bytes, expected);
    }

    #[test]
    fn narrow_raster_is_centred_without_a_position() {
        let bytes = encode(vec![Op::Raster {
            width: 8,
            height: 1,
            data: vec![0x81],
        }]);
        // (576 - 8) / 2 = 284 dots = byte 35, bit offset 4
        let row = &bytes[8..];
        assert_eq!(row.len(), 72);
        assert_eq!(row[35], 0x08);
        assert_eq!(row[36], 0x10);
        assert_eq!(row.iter().filter(|b| **b != 0).count(), 2);
    }

    #[test]
    fn tall_raster_is_chunked() {
        let bytes = encode(vec![Op::Raster {
            width: 576,
            height: 300,
            data: vec![0xAA; 72 * 300],
        }]);
        assert_eq!(&bytes[..8], &[0x1D, 0x76, 0x30, 0x00, 72, 0, 0, 1]);
        let second = 8 + 72 * 256;
        assert_eq!(
            &bytes[second..second + 8],
            &[0x1D, 0x76, 0x30, 0x00, 72, 0, 44, 0]
        );
        assert_eq!(bytes.len(), 16 + 72 * 300);
    }

    #[test]
    fn band_is_sent_as_raster_padded_to_whole_bands() {
        let bytes = encode(vec![Op::Band {
            width_bytes: 72,
            data: vec![0xFF; 72 * 10],
        }]);
        assert_eq!(&bytes[..8], &[0x1D, 0x76, 0x30, 0x00, 72, 0, 24, 0]);
        assert_eq!(bytes.len(), 8 + 72 * 24);
    }

    #[test]
    fn pdf417_is_rasterized() {
        let bytes = encode(vec![Op::Pdf417 {
            data: "HELLO".into(),
            module_width: 3,
            ecc_level: 2,
        }]);
        assert_eq!(&bytes[..6], &[0x1D, 0x76, 0x30, 0x00, 72, 0]);
        assert!(bytes[8..].iter().any(|b| *b != 0));
    }

    #[test]
    fn barcode_and_qr_use_native_commands() {
        let bytes = encode(vec![Op::Barcode1D {
            kind: BarcodeKind::Code39,
            data: "AB".into(),
            height: 40,
        }]);
        assert_eq!(&bytes[9..], &[0x1D, 0x6B, 69, 2, b'A', b'B']);
    }

    #[test]
    fn built_in_logo_is_rasterized() {
        let key = crate::logos::list_keys()[0];
        let bytes = encode(vec![Op::NvPrint {
            key: key.into(),
            scale_x: 1,
            scale_y: 1,
        }]);
        assert_eq!(&bytes[..6], &[0x1D, 0x76, 0x30, 0x00, 72, 0]);
    }

    #[test]
    fn unknown_logo_and_nv_storage_are_errors() {
        let program = Program {
            ops: vec![Op::NvPrint {
                key: "nope".into(),
                scale_x: 1,
                scale_y: 1,
            }],
        };
        assert!(
            program
                .to_escpos_bytes(&PrinterConfig::ESCPOS_80MM)
                .is_err()
        );

        let program = Program {
            ops: vec![Op::NvDelete { key: "A0".into() }],
        };
        assert!(
            program
                .to_escpos_bytes(&PrinterConfig::ESCPOS_80MM)
                .is_err()
        );
    }
}
