//! Minimal asynchronous HTML-to-PDF rendering example.

use quire::{Html, PdfOptions, RenderOptions};

#[tokio::main]
async fn main() -> quire::Result<()> {
    Html::from_string("<title>Basic</title><p>Hello, world</p>")
        .write_pdf(
            "output/quire-basic.pdf",
            &RenderOptions::default(),
            &PdfOptions::default(),
        )
        .await
}
