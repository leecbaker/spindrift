//! HTML-to-PDF rendering example with an explicit author stylesheet.

use std::fs::File;

use quire::{Css, Html, PdfOptions, RenderOptions};

#[tokio::main]
async fn main() -> quire::Result<()> {
    let css = Css::from_string(
        r#"
        @page { size: 300px 240px; margin: 24px }
        h1 { color: blue; text-align: center }
        .box { margin: 0; padding: 6px; border: 1px solid red; background: #eeeeee; width: 180px }
        "#,
    );

    let mut output = File::create("output/quire-styled-example.pdf")?;
    Html::from_string(
        r#"
        <title>Styled</title>
        <h1>Styled PDF</h1>
        <div class="box">A styled block from quire.</div>
        <ol><li>Parse HTML</li><li>Apply CSS</li><li>Write PDF</li></ol>
        "#,
    )
    .with_stylesheet(css)
    .write_pdf(
        &mut output,
        &RenderOptions::default(),
        &PdfOptions::default(),
    )
    .await
}
