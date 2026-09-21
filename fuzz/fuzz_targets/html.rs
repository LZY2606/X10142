#![no_main]
use libfuzzer_sys::fuzz_target;

use libfuzzer_sys::arbitrary::{self, Arbitrary};
use pulldown_cmark_fuzz::ArbitraryOptions;

#[derive(Debug, Arbitrary)]
struct FuzzingInput<'a> {
    options: ArbitraryOptions,
    markdown: &'a str,
}

fuzz_target!(|data: FuzzingInput<'_>| {
    let parser = pulldown_cmark::Parser::new_ext(data.markdown, data.options.0);
    let mut output = String::new();

    pulldown_cmark::html::push_html(&mut output, parser);

    assert!(!output.contains('\0'));
    // assert!(!output.contains('\r'));
});
