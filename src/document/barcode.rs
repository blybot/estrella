//! Emit logic for barcode components: QrCode, Pdf417, Barcode.

use super::types::{Barcode, Pdf417, QrCode};
use crate::ir::{BarcodeKind, Op};
use crate::protocol::barcode::qr::QrErrorLevel;
use crate::protocol::text::Alignment;

/// Printable width in dots.
const PRINT_WIDTH: usize = 576;

/// Shrink `cell_size` until the QR code fits the paper. Printers skip a QR
/// code wider than the print area. StarPRNT additionally caps cells at 8.
fn fit_qr_cell_size(data: &str, error_level: QrErrorLevel, cell_size: u8) -> u8 {
    use qrcode::EcLevel;
    let ec_level = match error_level {
        QrErrorLevel::L => EcLevel::L,
        QrErrorLevel::M => EcLevel::M,
        QrErrorLevel::Q => EcLevel::Q,
        QrErrorLevel::H => EcLevel::H,
    };
    match qrcode::QrCode::with_error_correction_level(data, ec_level) {
        Ok(code) => cell_size.min((PRINT_WIDTH / code.width()).max(1) as u8),
        Err(_) => cell_size,
    }
}

impl QrCode {
    /// Emit IR ops for this QR code component.
    pub fn emit(&self, ops: &mut Vec<Op>) {
        // Resolve alignment (default: center)
        let alignment = match self.align.as_deref() {
            Some("left") => Alignment::Left,
            Some("right") => Alignment::Right,
            _ => Alignment::Center, // default
        };

        // Resolve error level (default: M)
        let error_level = match self
            .error_level
            .as_deref()
            .map(|s| s.to_uppercase())
            .as_deref()
        {
            Some("L") => QrErrorLevel::L,
            Some("Q") => QrErrorLevel::Q,
            Some("H") => QrErrorLevel::H,
            _ => QrErrorLevel::M, // default
        };

        let cell_size = fit_qr_cell_size(
            &self.data,
            error_level,
            self.cell_size.unwrap_or(4).clamp(1, 16),
        );

        ops.push(Op::SetAlign(alignment));
        ops.push(Op::QrCode {
            data: self.data.clone(),
            cell_size,
            error_level,
        });
    }
}

impl Pdf417 {
    /// Emit IR ops for this PDF417 barcode component.
    pub fn emit(&self, ops: &mut Vec<Op>) {
        // Resolve alignment (default: center)
        let alignment = match self.align.as_deref() {
            Some("left") => Alignment::Left,
            Some("right") => Alignment::Right,
            _ => Alignment::Center, // default
        };

        let module_width = self.module_width.unwrap_or(3).clamp(1, 15);
        let ecc_level = self.ecc_level.unwrap_or(2).min(8);

        ops.push(Op::SetAlign(alignment));
        ops.push(Op::Pdf417 {
            data: self.data.clone(),
            module_width,
            ecc_level,
        });
    }
}

impl Barcode {
    /// Emit IR ops for this 1D barcode component.
    pub fn emit(&self, ops: &mut Vec<Op>) {
        let kind = match self.format.to_lowercase().as_str() {
            "code39" => BarcodeKind::Code39,
            "code128" => BarcodeKind::Code128,
            "ean13" => BarcodeKind::Ean13,
            "upca" => BarcodeKind::UpcA,
            "itf" => BarcodeKind::Itf,
            _ => return, // Unknown format — emit nothing
        };

        let height = self.height.unwrap_or(80).max(1);

        ops.push(Op::Barcode1D {
            kind,
            data: self.data.clone(),
            height,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qr_code_default() {
        let qr = QrCode::new("https://example.com");
        let mut ops = Vec::new();
        qr.emit(&mut ops);
        assert!(ops.iter().any(|op| matches!(
            op,
            Op::QrCode {
                cell_size: 4,
                error_level: QrErrorLevel::M,
                ..
            }
        )));
        assert!(
            ops.iter()
                .any(|op| matches!(op, Op::SetAlign(Alignment::Center)))
        );
    }

    #[test]
    fn test_qr_code_options() {
        let qr = QrCode {
            data: "test".into(),
            cell_size: Some(6),
            error_level: Some("H".into()),
            align: Some("left".into()),
        };
        let mut ops = Vec::new();
        qr.emit(&mut ops);
        assert!(ops.iter().any(|op| matches!(
            op,
            Op::QrCode {
                cell_size: 6,
                error_level: QrErrorLevel::H,
                ..
            }
        )));
        assert!(
            ops.iter()
                .any(|op| matches!(op, Op::SetAlign(Alignment::Left)))
        );
    }

    fn emitted_cell_size(qr: &QrCode) -> u8 {
        let mut ops = Vec::new();
        qr.emit(&mut ops);
        ops.iter()
            .find_map(|op| match op {
                Op::QrCode { cell_size, .. } => Some(*cell_size),
                _ => None,
            })
            .unwrap()
    }

    fn qr(data: &str, cell_size: u8) -> QrCode {
        QrCode {
            data: data.into(),
            cell_size: Some(cell_size),
            error_level: None,
            align: None,
        }
    }

    #[test]
    fn qr_cell_size_goes_up_to_16() {
        // 25 modules × 16 = 400 dots, fits on 576
        assert_eq!(emitted_cell_size(&qr("https://example.com", 16)), 16);
        assert_eq!(emitted_cell_size(&qr("https://example.com", 40)), 16);
    }

    #[test]
    fn large_qr_shrinks_to_fit_the_paper() {
        let url = format!("https://example.com/{}", "a".repeat(150));
        let modules = qrcode::QrCode::with_error_correction_level(&url, qrcode::EcLevel::M)
            .unwrap()
            .width();

        let cell = emitted_cell_size(&qr(&url, 16));

        assert!(cell < 16, "expected the cell size to shrink, got {cell}");
        assert!(modules * cell as usize <= 576);
        assert!(
            modules * (cell as usize + 1) > 576,
            "should use the largest fit"
        );
    }

    #[test]
    fn test_pdf417() {
        let pdf = Pdf417 {
            data: "TICKET".into(),
            module_width: Some(4),
            ecc_level: Some(3),
            ..Default::default()
        };
        let mut ops = Vec::new();
        pdf.emit(&mut ops);
        assert!(ops.iter().any(|op| matches!(
            op,
            Op::Pdf417 {
                module_width: 4,
                ecc_level: 3,
                ..
            }
        )));
    }

    #[test]
    fn test_barcode_code128() {
        let barcode = Barcode {
            format: "code128".into(),
            data: "ABC-123".into(),
            height: Some(100),
        };
        let mut ops = Vec::new();
        barcode.emit(&mut ops);
        assert!(ops.iter().any(|op| matches!(
            op,
            Op::Barcode1D {
                kind: BarcodeKind::Code128,
                height: 100,
                ..
            }
        )));
    }

    #[test]
    fn test_barcode_invalid_format() {
        let barcode = Barcode {
            format: "invalid".into(),
            data: "123".into(),
            height: None,
        };
        let mut ops = Vec::new();
        barcode.emit(&mut ops);
        assert!(ops.is_empty());
    }
}
