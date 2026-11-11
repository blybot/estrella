//! # ESC/POS Command Builders
//!
//! Byte builders for the Epson ESC/POS command set, as implemented by most
//! generic 80mm thermal printers (Epson TM series, Xprinter, Rongta, ...).
//!
//! Only the commands needed to compile estrella's IR are implemented here;
//! the IR → ESC/POS compiler lives in `ir::codegen`.
//!
//! ## Reference
//!
//! Epson "ESC/POS Command Reference for TM Printers". All multi-byte integers
//! are little-endian, like StarPRNT.

use super::barcode::qr::QrErrorLevel;
use super::text::{Alignment, Font};
use crate::ir::BarcodeKind;

pub const ESC: u8 = 0x1B;
pub const GS: u8 = 0x1D;
pub const FS: u8 = 0x1C;
pub const LF: u8 = 0x0A;

/// ESC @ - initialize printer.
pub fn init() -> Vec<u8> {
    vec![ESC, b'@']
}

/// FS . - cancel Kanji/Chinese character mode.
///
/// Many Chinese-market printers (e.g. Xprinter) boot with double-byte Chinese
/// mode enabled, which makes bytes ≥ 0x80 print as Chinese glyphs instead of
/// code-page characters. Cancelling it is harmless on printers without it.
pub fn cancel_kanji_mode() -> Vec<u8> {
    vec![FS, b'.']
}

/// GS P x y - set the horizontal and vertical motion units to 1/x and 1/y
/// inch. Feeds (ESC J), positions (ESC $) and cut feeds (GS V) are counted in
/// these units, and their defaults vary by printer (Xprinter's 80mm series
/// defaults to 1/180 and 1/360 inch).
pub fn motion_units(x: u8, y: u8) -> Vec<u8> {
    vec![GS, b'P', x, y]
}

/// ESC t n - select character code table (0 = PC437).
pub fn code_table(n: u8) -> Vec<u8> {
    vec![ESC, b't', n]
}

/// GS V m n - feed paper to the cutting position plus `n` dots, then cut.
///
/// Function B: m = 65 (full cut) or 66 (partial cut).
pub fn cut_feed(partial: bool) -> Vec<u8> {
    vec![GS, b'V', if partial { 66 } else { 65 }, 0]
}

/// ESC B n t - sound the buzzer `n` times for `t` × 50 ms each.
///
/// Not part of Epson's command set: Xprinter and other Chinese-market printers
/// add it, documenting 1-9 for both parameters.
pub fn beep(times: u8, duration: u8) -> Vec<u8> {
    vec![ESC, b'B', times.clamp(1, 9), duration.clamp(1, 9)]
}

/// ESC J n - feed paper `n` vertical motion units (dots at 203 dpi).
pub fn feed_dots(n: u8) -> Vec<u8> {
    vec![ESC, b'J', n]
}

/// ESC a n - justification.
pub fn align(alignment: Alignment) -> Vec<u8> {
    vec![ESC, b'a', alignment as u8]
}

/// ESC M n - select character font (0 = A, 1 = B).
///
/// ESC/POS has no equivalent of Star's Font C (9×17), so it maps to Font B,
/// which has the same 64-column line width.
pub fn font(font: Font) -> Vec<u8> {
    let n = match font {
        Font::A => 0,
        Font::B | Font::C => 1,
    };
    vec![ESC, b'M', n]
}

/// ESC E n - emphasized (bold) mode.
pub fn bold(enabled: bool) -> Vec<u8> {
    vec![ESC, b'E', enabled as u8]
}

/// ESC - n - underline mode (1 = one dot thick).
pub fn underline(enabled: bool) -> Vec<u8> {
    vec![ESC, b'-', enabled as u8]
}

/// GS B n - white/black reverse printing.
pub fn invert(enabled: bool) -> Vec<u8> {
    vec![GS, b'B', enabled as u8]
}

/// GS b n - smoothing.
pub fn smoothing(enabled: bool) -> Vec<u8> {
    vec![GS, b'b', enabled as u8]
}

/// ESC { n - upside-down printing.
pub fn upside_down(enabled: bool) -> Vec<u8> {
    vec![ESC, b'{', enabled as u8]
}

/// GS ! n - character size.
///
/// `width` and `height` are 0-based multipliers (0 = 1×, 7 = 8×), matching
/// the IR's `SetSize`. Bits 4-6 hold the width, bits 0-2 the height.
pub fn char_size(width: u8, height: u8) -> Vec<u8> {
    vec![GS, b'!', (width.min(7) << 4) | height.min(7)]
}

/// ESC $ nL nH - absolute horizontal print position, in dots.
pub fn absolute_position(dots: u16) -> Vec<u8> {
    let [lo, hi] = dots.to_le_bytes();
    vec![ESC, b'$', lo, hi]
}

/// GS v 0 m xL xH yL yH d... - print raster bit image (normal density).
///
/// `width_bytes` is the row width in bytes, `height` the number of rows.
/// Data is row-major, MSB = leftmost dot, 1 = black.
pub fn raster(width_bytes: u16, height: u16, data: &[u8]) -> Vec<u8> {
    let [xl, xh] = width_bytes.to_le_bytes();
    let [yl, yh] = height.to_le_bytes();
    let mut cmd = Vec::with_capacity(8 + data.len());
    cmd.extend_from_slice(&[GS, b'v', b'0', 0, xl, xh, yl, yh]);
    cmd.extend_from_slice(data);
    cmd
}

/// GS ( k - one QR code function: `GS ( k pL pH cn fn params...` with cn = 49.
fn qr_function(fn_code: u8, params: &[u8]) -> Vec<u8> {
    let len = (params.len() + 2) as u16;
    let [pl, ph] = len.to_le_bytes();
    let mut cmd = vec![GS, b'(', b'k', pl, ph, 49, fn_code];
    cmd.extend_from_slice(params);
    cmd
}

/// Complete QR code sequence: model 2, module size, error correction, store
/// the data, then print it.
pub fn qr_code(data: &[u8], cell_size: u8, error_level: QrErrorLevel) -> Vec<u8> {
    let ec = match error_level {
        QrErrorLevel::L => 48,
        QrErrorLevel::M => 49,
        QrErrorLevel::Q => 50,
        QrErrorLevel::H => 51,
    };
    let mut cmd = Vec::new();
    cmd.extend(qr_function(65, &[50, 0])); // fn 165: model 2
    cmd.extend(qr_function(67, &[cell_size.clamp(1, 16)])); // fn 167: module size
    cmd.extend(qr_function(69, &[ec])); // fn 169: error correction
    let mut store = vec![48];
    store.extend_from_slice(data);
    cmd.extend(qr_function(80, &store)); // fn 180: store data
    cmd.extend(qr_function(81, &[48])); // fn 181: print
    cmd
}

/// 1D barcode: height (GS h), module width (GS w), HRI below (GS H 2), then
/// GS k m n d... (function B).
///
/// CODE128 data is sent in code set B with `{` escaped as `{{`, so arbitrary
/// printable ASCII prints as written.
pub fn barcode1d(kind: BarcodeKind, data: &[u8], height: u8) -> Vec<u8> {
    let (m, module_width) = match kind {
        BarcodeKind::UpcA => (65, 3),
        BarcodeKind::Ean13 => (67, 3),
        BarcodeKind::Code39 => (69, 2),
        BarcodeKind::Itf => (70, 3),
        BarcodeKind::Code128 => (73, 2),
    };

    let payload: Vec<u8> = if kind == BarcodeKind::Code128 {
        let mut p = b"{B".to_vec();
        for &b in data {
            if b == b'{' {
                p.push(b'{');
            }
            p.push(b);
        }
        p
    } else {
        data.to_vec()
    };
    let len = payload.len().min(255);

    let mut cmd = vec![
        GS,
        b'h',
        height.max(1),
        GS,
        b'w',
        module_width,
        GS,
        b'H',
        2,
        GS,
        b'k',
        m,
        len as u8,
    ];
    cmd.extend_from_slice(&payload[..len]);
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_is_esc_at() {
        assert_eq!(init(), vec![0x1B, 0x40]);
    }

    #[test]
    fn cancel_kanji_mode_is_fs_dot() {
        assert_eq!(cancel_kanji_mode(), vec![0x1C, 0x2E]);
    }

    #[test]
    fn motion_units_is_gs_p() {
        assert_eq!(motion_units(203, 203), vec![0x1D, 0x50, 0xCB, 0xCB]);
    }

    #[test]
    fn code_table_pc437() {
        assert_eq!(code_table(0), vec![0x1B, 0x74, 0x00]);
    }

    #[test]
    fn cut_uses_function_b_with_feed() {
        assert_eq!(cut_feed(true), vec![0x1D, 0x56, 0x42, 0x00]);
        assert_eq!(cut_feed(false), vec![0x1D, 0x56, 0x41, 0x00]);
    }

    #[test]
    fn feed_dots_is_esc_j() {
        assert_eq!(feed_dots(48), vec![0x1B, 0x4A, 0x30]);
    }

    #[test]
    fn align_values() {
        assert_eq!(align(Alignment::Left), vec![0x1B, 0x61, 0x00]);
        assert_eq!(align(Alignment::Center), vec![0x1B, 0x61, 0x01]);
        assert_eq!(align(Alignment::Right), vec![0x1B, 0x61, 0x02]);
    }

    #[test]
    fn font_c_falls_back_to_font_b() {
        assert_eq!(font(Font::A), vec![0x1B, 0x4D, 0x00]);
        assert_eq!(font(Font::B), vec![0x1B, 0x4D, 0x01]);
        assert_eq!(font(Font::C), vec![0x1B, 0x4D, 0x01]);
    }

    #[test]
    fn style_toggles() {
        assert_eq!(bold(true), vec![0x1B, 0x45, 0x01]);
        assert_eq!(bold(false), vec![0x1B, 0x45, 0x00]);
        assert_eq!(underline(true), vec![0x1B, 0x2D, 0x01]);
        assert_eq!(invert(true), vec![0x1D, 0x42, 0x01]);
        assert_eq!(smoothing(true), vec![0x1D, 0x62, 0x01]);
        assert_eq!(upside_down(true), vec![0x1B, 0x7B, 0x01]);
    }

    #[test]
    fn char_size_packs_width_high_height_low() {
        assert_eq!(char_size(0, 0), vec![0x1D, 0x21, 0x00]);
        // double width + double height = 0x11
        assert_eq!(char_size(1, 1), vec![0x1D, 0x21, 0x11]);
        // 3x wide, 1x high = 0x20
        assert_eq!(char_size(2, 0), vec![0x1D, 0x21, 0x20]);
        // clamped to 8x
        assert_eq!(char_size(9, 9), vec![0x1D, 0x21, 0x77]);
    }

    #[test]
    fn absolute_position_little_endian() {
        assert_eq!(absolute_position(300), vec![0x1B, 0x24, 0x2C, 0x01]);
    }

    #[test]
    fn beep_is_esc_b_times_and_duration() {
        assert_eq!(beep(2, 3), vec![0x1B, 0x42, 0x02, 0x03]);
        // Xprinter documents 1-9 for both
        assert_eq!(beep(0, 12), vec![0x1B, 0x42, 0x01, 0x09]);
    }

    #[test]
    fn raster_header() {
        let cmd = raster(72, 2, &[0xFF; 144]);
        assert_eq!(&cmd[..8], &[0x1D, 0x76, 0x30, 0x00, 72, 0, 2, 0]);
        assert_eq!(cmd.len(), 8 + 144);
    }

    #[test]
    fn qr_code_sequence() {
        let cmd = qr_code(b"hi", 6, QrErrorLevel::M);
        let expected: Vec<u8> = [
            // fn 165: model 2
            &[0x1D, 0x28, 0x6B, 0x04, 0x00, 0x31, 0x41, 0x32, 0x00][..],
            // fn 167: module size 6
            &[0x1D, 0x28, 0x6B, 0x03, 0x00, 0x31, 0x43, 0x06],
            // fn 169: error correction M
            &[0x1D, 0x28, 0x6B, 0x03, 0x00, 0x31, 0x45, 0x31],
            // fn 180: store "hi" (pL = 2 + 3)
            &[0x1D, 0x28, 0x6B, 0x05, 0x00, 0x31, 0x50, 0x30, b'h', b'i'],
            // fn 181: print
            &[0x1D, 0x28, 0x6B, 0x03, 0x00, 0x31, 0x51, 0x30],
        ]
        .concat();
        assert_eq!(cmd, expected);
    }

    #[test]
    fn barcode_ean13() {
        let cmd = barcode1d(BarcodeKind::Ean13, b"4006381333931", 80);
        let mut expected = vec![
            0x1D, 0x68, 80, // height
            0x1D, 0x77, 3, // module width
            0x1D, 0x48, 2, // HRI below
            0x1D, 0x6B, 67, 13,
        ];
        expected.extend_from_slice(b"4006381333931");
        assert_eq!(cmd, expected);
    }

    #[test]
    fn barcode_code128_uses_code_set_b_and_escapes_braces() {
        let cmd = barcode1d(BarcodeKind::Code128, b"a{b", 50);
        let tail = &cmd[9..];
        assert_eq!(
            tail,
            &[0x1D, 0x6B, 73, 6, b'{', b'B', b'a', b'{', b'{', b'b']
        );
    }
}
