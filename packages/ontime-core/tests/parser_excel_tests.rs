//! Excel import, against upstream's own rundown template. Nothing else reads a
//! real .xlsx, so this is what notices when calamine (and the zip crate under
//! it) changes how a workbook is read.

#![cfg(feature = "spreadsheet")]

use ontime_core::RundownParser;

const TEMPLATE: &[u8] =
    include_bytes!("../../../e2e/tests/fixtures/Ontime rundown template v4.xlsx");

#[test]
fn upstreams_template_parses() {
    let events = RundownParser::parse_excel(TEMPLATE).expect("the template parses");
    // calamine 0.26 and 0.38 read the same 25 events, field for field
    // (checked 2026-11-25 when 0.38 came in for the zip crate's fixes)
    assert_eq!(events.len(), 25, "the template holds 25 events");
}

#[test]
fn something_that_is_not_a_workbook_is_refused() {
    assert!(RundownParser::parse_excel(b"not a workbook").is_err());
}
