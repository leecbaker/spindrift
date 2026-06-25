use quire::{Html, RenderOptions};

#[tokio::main]
async fn main() -> quire::Result<()> {
    Html::from_string("<title>Basic</title><p>Hello, world</p>")
        .write_pdf_async("output/reasyprint-basic.pdf", &RenderOptions::default())
        .await
}
