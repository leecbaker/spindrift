//! Minimal asynchronous HTML-to-PDF rendering example.

use std::fs::File;

use spindrift::{Html, PdfOptions, RenderOptions};

#[tokio::main]
async fn main() -> spindrift::Result<()> {
    let mut output = File::create("output/spindrift-basic.pdf")?;
    Html::from_string("<title>Basic</title><p>Hello, world</p>")
        .write_pdf(
            &mut output,
            &RenderOptions::default(),
            &PdfOptions::default(),
        )
        .await
}
