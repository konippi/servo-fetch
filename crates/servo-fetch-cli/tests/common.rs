//! Shared helpers for integration tests.

#![allow(dead_code, unreachable_pub)]

use std::time::Duration;

use tokio::sync::mpsc;
use wiremock::{Request, ResponseTemplate};

pub fn mock_page(html: impl Into<String>) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(html.into().into_bytes(), "text/html; charset=utf-8")
}

/// A slow page whose responder reports each arriving request, so tests can act mid-fetch.
pub fn slow_page(
    html: &'static str,
    delay: Duration,
) -> (mpsc::UnboundedReceiver<()>, impl Fn(&Request) -> ResponseTemplate) {
    let (arrived, arrivals) = mpsc::unbounded_channel();
    let responder = move |_: &Request| {
        let _ = arrived.send(());
        mock_page(html).set_delay(delay)
    };
    (arrivals, responder)
}

/// A minimal single-page PDF whose only content is `text`, for exercising PDF extraction end to end.
pub fn pdf_with_text(text: &str) -> Vec<u8> {
    use std::io::Write as _;

    let escaped = text.replace('\\', r"\\").replace('(', r"\(").replace(')', r"\)");
    let stream = format!("BT\n/F1 18 Tf\n72 720 Td\n({escaped}) Tj\nET\n");
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>"
            .to_string(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
    ];
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        write!(&mut pdf, "{} 0 obj\n{object}\nendobj\n", index + 1).expect("write PDF object");
    }
    let xref = pdf.len();
    write!(&mut pdf, "xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).expect("write PDF xref header");
    for offset in offsets {
        writeln!(&mut pdf, "{offset:010} 00000 n ").expect("write PDF xref entry");
    }
    write!(
        &mut pdf,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    )
    .expect("write PDF trailer");
    pdf
}
