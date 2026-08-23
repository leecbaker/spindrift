//! Minimal reproduction for xml5ever's namespace-insensitive duplicate check.

use markup5ever_rcdom::RcDom;
use xml5ever::{driver::parse_document, tendril::TendrilSink};

fn main() {
    let document = r#"<x xmlns:n1="http://www.w3.org" xmlns="http://www.w3.org">
  <good n1:a="2" a="1" />
</x>"#;
    let dom = parse_document(RcDom::default(), Default::default()).one(document);
    let errors = dom.errors.borrow();

    assert!(
        errors.is_empty(),
        "valid XML was rejected with parse errors: {errors:?}"
    );
}
