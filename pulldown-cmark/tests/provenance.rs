//! Tests for the opt-in provenance event stream (`Parser::into_provenance_iter`).

use pulldown_cmark::{Event, Options, Parser, Provenance, ProvenanceKind, Tag};
use std::ops::Range;

/// Runs the plain parser, the offset iterator and the provenance iterator over
/// the same input and asserts that
///  - all three produce the exact same event sequence,
///  - every primary provenance range equals the range `OffsetIter` reports,
///  - every provenance range (primary and definition) lies within the input.
///
/// Returns the collected `(Event, Provenance)` pairs for further inspection.
fn assert_consistent(text: &str, options: Options) -> Vec<(Event<'static>, Provenance)> {
    let plain: Vec<Event<'_>> = Parser::new_ext(text, options).collect();
    let offsets: Vec<(Event<'_>, Range<usize>)> =
        Parser::new_ext(text, options).into_offset_iter().collect();
    let provenance: Vec<(Event<'_>, Provenance)> = Parser::new_ext(text, options)
        .into_provenance_iter()
        .collect();

    assert_eq!(plain.len(), provenance.len(), "event count mismatch");
    assert_eq!(offsets.len(), provenance.len(), "event count mismatch");

    for ((plain_event, (offset_event, range)), (prov_event, prov)) in
        plain.iter().zip(offsets.iter()).zip(provenance.iter())
    {
        assert_eq!(plain_event, offset_event);
        assert_eq!(plain_event, prov_event);
        assert_eq!(*range, prov.range, "primary range must match OffsetIter");
        assert!(prov.range.start <= prov.range.end);
        assert!(prov.range.end <= text.len(), "range escapes input");
        if let ProvenanceKind::Reference { definition } = &prov.kind {
            assert!(definition.start <= definition.end);
            assert!(
                definition.end <= text.len(),
                "definition range escapes input"
            );
        }
    }

    // Detach the events from the input lifetime for easy reuse in assertions.
    provenance
        .into_iter()
        .zip(plain)
        .map(|((_, prov), event)| (event.into_static(), prov))
        .collect()
}

/// Finds the provenance of the first event matching `pred`.
fn provenance_of_first<'a>(
    events: &'a [(Event<'static>, Provenance)],
    pred: impl Fn(&Event<'static>) -> bool,
) -> &'a Provenance {
    &events
        .iter()
        .find(|(event, _)| pred(event))
        .expect("event not found")
        .1
}

fn is_link_start(event: &Event<'static>) -> bool {
    matches!(event, Event::Start(Tag::Link { .. }))
}

fn is_image_start(event: &Event<'static>) -> bool {
    matches!(event, Event::Start(Tag::Image { .. }))
}

#[test]
fn inline_link_is_direct() {
    let text = "before [text](https://example.com \"title\") after";
    let events = assert_consistent(text, Options::empty());

    let prov = provenance_of_first(&events, is_link_start);
    assert_eq!(prov.kind, ProvenanceKind::Direct);
    assert_eq!(
        &text[prov.range.clone()],
        "[text](https://example.com \"title\")"
    );

    let text_prov = provenance_of_first(&events, |e| matches!(e, Event::Text(_)));
    assert_eq!(text_prov.kind, ProvenanceKind::Direct);
}

#[test]
fn full_reference_link_carries_use_and_definition() {
    let text = "[text][ref]\n\n[ref]: /url \"title\"\n";
    let def_span = Parser::new_ext(text, Options::empty())
        .reference_definitions()
        .get("ref")
        .expect("definition")
        .span
        .clone();

    let events = assert_consistent(text, Options::empty());
    let prov = provenance_of_first(&events, is_link_start);

    assert_eq!(
        prov.kind,
        ProvenanceKind::Reference {
            definition: def_span.clone()
        }
    );
    // Primary range is the use site.
    assert_eq!(&text[prov.range.clone()], "[text][ref]");
    // Definition evidence points at the definition in the source.
    assert!(text[def_span].starts_with("[ref]:"));
}

#[test]
fn collapsed_reference_link() {
    let text = "[ref][]\n\n[ref]: /url\n";
    let def_span = Parser::new_ext(text, Options::empty())
        .reference_definitions()
        .get("ref")
        .unwrap()
        .span
        .clone();

    let events = assert_consistent(text, Options::empty());
    let prov = provenance_of_first(&events, is_link_start);
    assert_eq!(
        prov.kind,
        ProvenanceKind::Reference {
            definition: def_span
        }
    );
    assert_eq!(&text[prov.range.clone()], "[ref][]");
}

#[test]
fn shortcut_reference_link() {
    let text = "[ref]\n\n[ref]: /url\n";
    let def_span = Parser::new_ext(text, Options::empty())
        .reference_definitions()
        .get("ref")
        .unwrap()
        .span
        .clone();

    let events = assert_consistent(text, Options::empty());
    let prov = provenance_of_first(&events, is_link_start);
    assert_eq!(
        prov.kind,
        ProvenanceKind::Reference {
            definition: def_span
        }
    );
    assert_eq!(&text[prov.range.clone()], "[ref]");
}

#[test]
fn shared_definition_keeps_individual_use_sites() {
    let text = "[a][r] and [b][r]\n\n[r]: /url\n";
    let events = assert_consistent(text, Options::empty());

    let uses: Vec<&Provenance> = events
        .iter()
        .filter(|(event, _)| is_link_start(event))
        .map(|(_, prov)| prov)
        .collect();
    assert_eq!(uses.len(), 2);

    let definitions: Vec<Range<usize>> = uses
        .iter()
        .map(|prov| match &prov.kind {
            ProvenanceKind::Reference { definition } => definition.clone(),
            other => panic!("expected reference provenance, got {other:?}"),
        })
        .collect();
    // Both uses share the same definition evidence...
    assert_eq!(definitions[0], definitions[1]);
    assert!(text[definitions[0].clone()].starts_with("[r]:"));
    // ...but keep their own use sites.
    assert_ne!(uses[0].range, uses[1].range);
    assert_eq!(&text[uses[0].range.clone()], "[a][r]");
    assert_eq!(&text[uses[1].range.clone()], "[b][r]");
}

#[test]
fn duplicate_definitions_follow_winning_rule() {
    let text = "[x][r]\n\n[r]: /first\n[r]: /second\n";
    let events = assert_consistent(text, Options::empty());
    let prov = provenance_of_first(&events, is_link_start);

    match &prov.kind {
        ProvenanceKind::Reference { definition } => {
            // The first definition wins, as with `RefDefs`.
            assert!(text[definition.clone()].contains("/first"));
        }
        other => panic!("expected reference provenance, got {other:?}"),
    }
}

#[test]
fn broken_link_callback_is_callback_provenance() {
    let text = "see [broken] and [collapsed][]";
    let mut options = Options::empty();
    options.insert(Options::ENABLE_FOOTNOTES);

    let parser = Parser::new_with_broken_link_callback(
        text,
        options,
        Some(|broken: pulldown_cmark::BrokenLink<'_>| {
            Some(("https://callback".into(), broken.reference.into_static()))
        }),
    );
    let events: Vec<(Event<'_>, Provenance)> = parser.into_provenance_iter().collect();

    let links: Vec<&Provenance> = events
        .iter()
        .filter(|(event, _)| is_link_start(event))
        .map(|(_, prov)| prov)
        .collect();
    assert_eq!(links.len(), 2);
    assert_eq!(links[0].kind, ProvenanceKind::Callback);
    assert_eq!(&text[links[0].range.clone()], "[broken]");
    assert_eq!(links[1].kind, ProvenanceKind::Callback);
    // The collapsed reference's span covers the whole `[collapsed][]`.
    assert_eq!(&text[links[1].range.clone()], "[collapsed][]");

    for (_, prov) in &events {
        assert!(prov.range.end <= text.len());
    }
}

#[test]
fn footnote_reference_points_at_definition() {
    let text = "note [^a] here\n\n[^a]: definition text\n";
    let mut options = Options::empty();
    options.insert(Options::ENABLE_FOOTNOTES);
    let events = assert_consistent(text, options);

    let prov = provenance_of_first(&events, |e| matches!(e, Event::FootnoteReference(_)));
    match &prov.kind {
        ProvenanceKind::Reference { definition } => {
            assert!(text[definition.clone()].starts_with("[^a]:"));
            assert!(text[definition.clone()].contains("definition text"));
        }
        other => panic!("expected reference provenance, got {other:?}"),
    }
    assert_eq!(&text[prov.range.clone()], "[^a]");

    // The definition block itself is a direct container event.
    let def_prov = provenance_of_first(&events, |e| {
        matches!(e, Event::Start(Tag::FootnoteDefinition(_)))
    });
    assert_eq!(def_prov.kind, ProvenanceKind::Direct);
}

#[test]
fn task_list_marker_is_synthesized() {
    let text = "- [x] done\n- [ ] todo\n";
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TASKLISTS);
    let events = assert_consistent(text, options);

    let markers: Vec<&Provenance> = events
        .iter()
        .filter(|(event, _)| matches!(event, Event::TaskListMarker(_)))
        .map(|(_, prov)| prov)
        .collect();
    assert_eq!(markers.len(), 2);
    for marker in &markers {
        assert_eq!(marker.kind, ProvenanceKind::Synthesized);
        assert!(text[marker.range.clone()].starts_with('['));
    }
    assert_eq!(
        &text[markers[0].range.start..markers[0].range.start + 3],
        "[x]"
    );
    assert_eq!(
        &text[markers[1].range.start..markers[1].range.start + 3],
        "[ ]"
    );
}

#[test]
fn smart_punctuation_is_synthesized() {
    let text = "\"quoted\" -- --- ...\n";
    let mut options = Options::empty();
    options.insert(Options::ENABLE_SMART_PUNCTUATION);
    let events = assert_consistent(text, options);

    let synthesized: Vec<(&Event<'static>, &Provenance)> = events
        .iter()
        .filter(|(_, prov)| prov.kind == ProvenanceKind::Synthesized)
        .map(|(event, prov)| (event, prov))
        .collect();
    // “ ” – — …
    assert_eq!(synthesized.len(), 5);

    let quote = provenance_of_first(
        &events,
        |e| matches!(e, Event::Text(t) if t.as_ref() == "\u{201c}"),
    );
    assert_eq!(quote.kind, ProvenanceKind::Synthesized);
    // The trigger range covers exactly the ASCII quote in the source.
    assert_eq!(&text[quote.range.clone()], "\"");

    let ellipsis = provenance_of_first(
        &events,
        |e| matches!(e, Event::Text(t) if t.as_ref() == "\u{2026}"),
    );
    assert_eq!(&text[ellipsis.range.clone()], "...");
}

#[test]
fn nested_image_in_reference_link() {
    let text = "[![alt](img.png)][ref]\n\n[ref]: /url\n";
    let events = assert_consistent(text, Options::empty());

    let image = provenance_of_first(&events, is_image_start);
    assert_eq!(image.kind, ProvenanceKind::Direct);
    assert_eq!(&text[image.range.clone()], "![alt](img.png)");

    let link = provenance_of_first(&events, is_link_start);
    assert!(matches!(link.kind, ProvenanceKind::Reference { .. }));
    assert_eq!(&text[link.range.clone()], "[![alt](img.png)][ref]");
}

#[test]
fn nested_reference_image_in_link() {
    let text = "[![alt][img]][ref]\n\n[img]: /img.png\n[ref]: /url\n";
    let events = assert_consistent(text, Options::empty());

    let image = provenance_of_first(&events, is_image_start);
    match &image.kind {
        ProvenanceKind::Reference { definition } => {
            assert!(text[definition.clone()].starts_with("[img]:"));
        }
        other => panic!("expected reference provenance, got {other:?}"),
    }

    let link = provenance_of_first(&events, is_link_start);
    match &link.kind {
        ProvenanceKind::Reference { definition } => {
            assert!(text[definition.clone()].starts_with("[ref]:"));
        }
        other => panic!("expected reference provenance, got {other:?}"),
    }
}

#[test]
fn nul_replacement_is_synthesized() {
    let text = "a\u{0}b";
    let events = assert_consistent(text, Options::empty());

    let replacement = provenance_of_first(
        &events,
        |e| matches!(e, Event::Text(t) if t.as_ref() == "\u{fffd}"),
    );
    assert_eq!(replacement.kind, ProvenanceKind::Synthesized);
    // The trigger range covers exactly the NUL byte in the source.
    assert_eq!(replacement.range, 1..2);
    assert_eq!(&text[replacement.range.clone()], "\u{0}");

    // Surrounding text stays direct.
    let a = provenance_of_first(
        &events,
        |e| matches!(e, Event::Text(t) if t.as_ref() == "a"),
    );
    assert_eq!(a.kind, ProvenanceKind::Direct);
}

#[test]
fn plain_parser_and_offset_iter_are_unaffected() {
    // A kitchen-sink document exercising many constructs at once.
    let text = "# Heading\n\npara *em* `code` [l](u) [r][x] ![i](u)\n\n[x]: /def\n\n> quote\n\n- [ ] t\n\n\"smart\" ...\n\n[^f]: note\n\nuse [^f] and a\u{0}nul\n";
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_SMART_PUNCTUATION);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);

    let events = assert_consistent(text, options);
    assert!(events.len() > 20);

    // HTML rendering still works on the plain parser.
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, Parser::new_ext(text, options));
    assert!(html.contains("<h1>Heading</h1>"));
}
