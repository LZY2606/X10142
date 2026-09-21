//! Tests for the opt-in provenance event stream (`Parser::into_provenance_iter`).

use pulldown_cmark::{
    BrokenLink, Event, LinkType, Options, Parser, Provenance, Tag, TagEnd,
};

fn all_options() -> Options {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_SMART_PUNCTUATION);
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts
}

/// Collects only the link/image start tags with their range and provenance.
fn links(src: &str) -> Vec<(LinkType, std::ops::Range<usize>, Provenance)> {
    Parser::new(src)
        .into_provenance_iter()
        .filter_map(|(event, range, provenance)| match event {
            Event::Start(Tag::Link { link_type, .. } | Tag::Image { link_type, .. }) => {
                Some((link_type, range, provenance))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn inline_link_is_source() {
    let src = "before [text](/url \"title\") after";
    let found = links(src);
    assert_eq!(found.len(), 1);
    let (link_type, range, provenance) = &found[0];
    assert_eq!(*link_type, LinkType::Inline);
    assert_eq!(&src[range.clone()], "[text](/url \"title\")");
    assert_eq!(*provenance, Provenance::Source);
}

#[test]
fn full_reference_link() {
    let src = "[text][label]\n\n[label]: /url \"title\"\n";
    let found = links(src);
    assert_eq!(found.len(), 1);
    let (link_type, range, provenance) = &found[0];
    assert_eq!(*link_type, LinkType::Reference);
    assert_eq!(&src[range.clone()], "[text][label]");
    let definition = match provenance {
        Provenance::Reference { definition } => definition.clone(),
        other => panic!("expected reference provenance, got {other:?}"),
    };
    assert_eq!(&src[definition], "[label]: /url \"title\"");
}

#[test]
fn collapsed_reference_link() {
    let src = "[foo][]\n\n[foo]: /url\n";
    let found = links(src);
    assert_eq!(found.len(), 1);
    let (link_type, range, provenance) = &found[0];
    assert_eq!(*link_type, LinkType::Collapsed);
    assert_eq!(&src[range.clone()], "[foo][]");
    match provenance {
        Provenance::Reference { definition } => {
            assert_eq!(&src[definition.clone()], "[foo]: /url")
        }
        other => panic!("expected reference provenance, got {other:?}"),
    }
}

#[test]
fn shortcut_reference_link() {
    let src = "[foo]\n\n[foo]: /url\n";
    let found = links(src);
    assert_eq!(found.len(), 1);
    let (link_type, range, provenance) = &found[0];
    assert_eq!(*link_type, LinkType::Shortcut);
    assert_eq!(&src[range.clone()], "[foo]");
    match provenance {
        Provenance::Reference { definition } => {
            assert_eq!(&src[definition.clone()], "[foo]: /url")
        }
        other => panic!("expected reference provenance, got {other:?}"),
    }
}

#[test]
fn shared_definition_keeps_use_sites() {
    let src = "[a] and [a]\n\n[a]: /url\n";
    let found = links(src);
    assert_eq!(found.len(), 2);
    let (_, first_range, first_prov) = &found[0];
    let (_, second_range, second_prov) = &found[1];
    assert_eq!(&src[first_range.clone()], "[a]");
    assert_eq!(&src[second_range.clone()], "[a]");
    assert_ne!(first_range, second_range);
    let definition = match (first_prov, second_prov) {
        (
            Provenance::Reference { definition: first },
            Provenance::Reference { definition: second },
        ) => {
            assert_eq!(first, second, "uses of one reference share definition evidence");
            first.clone()
        }
        other => panic!("expected reference provenance, got {other:?}"),
    };
    assert_eq!(&src[definition], "[a]: /url");
}

#[test]
fn duplicate_definitions_follow_winner_rules() {
    // The first definition wins, as in the rest of the parser.
    let src = "[a]: /first\n[a]: /second\n\n[a]\n";
    let found = links(src);
    assert_eq!(found.len(), 1);
    match &found[0].2 {
        Provenance::Reference { definition } => {
            assert_eq!(&src[definition.clone()], "[a]: /first");
        }
        other => panic!("expected reference provenance, got {other:?}"),
    }
    // Sanity check that the winning definition is also the one used for the URL.
    let dest = Parser::new(src)
        .filter_map(|event| match event {
            Event::Start(Tag::Link { dest_url, .. }) => Some(dest_url.into_string()),
            _ => None,
        })
        .next()
        .unwrap();
    assert_eq!(dest, "/first");
}

#[test]
fn broken_link_callback_provenance() {
    let src = "a [missing] link";
    let mut callback = |broken: BrokenLink| {
        assert_eq!(&src[broken.span], "[missing]");
        Some(("/resolved".into(), "".into()))
    };
    let parser = Parser::new_with_broken_link_callback(src, Options::empty(), Some(&mut callback));
    let found: Vec<_> = parser
        .into_provenance_iter()
        .filter_map(|(event, range, provenance)| match event {
            Event::Start(Tag::Link { link_type, .. }) => Some((link_type, range, provenance)),
            _ => None,
        })
        .collect();
    assert_eq!(found.len(), 1);
    let (link_type, range, provenance) = &found[0];
    assert_eq!(*link_type, LinkType::ShortcutUnknown);
    assert_eq!(&src[range.clone()], "[missing]");
    assert_eq!(*provenance, Provenance::Callback);
}

#[test]
fn footnote_events_use_real_ranges() {
    let src = "text[^1] more\n\n[^1]: the note\n";
    let events: Vec<_> = Parser::new_ext(src, Options::ENABLE_FOOTNOTES)
        .into_provenance_iter()
        .collect();
    let reference = events
        .iter()
        .find(|(event, _, _)| matches!(event, Event::FootnoteReference(_)))
        .expect("footnote reference event");
    assert_eq!(&src[reference.1.clone()], "[^1]");
    assert_eq!(reference.2, Provenance::Source);
    let definition = events
        .iter()
        .find(|(event, _, _)| matches!(event, Event::Start(Tag::FootnoteDefinition(_))))
        .expect("footnote definition event");
    assert_eq!(&src[definition.1.clone()], "[^1]: the note\n");
    assert_eq!(definition.2, Provenance::Source);
}

#[test]
fn task_list_marker_is_synthesized() {
    let src = "- [ ] todo\n- [x] done\n";
    let markers: Vec<_> = Parser::new_ext(src, Options::ENABLE_TASKLISTS)
        .into_provenance_iter()
        .filter_map(|(event, range, provenance)| match event {
            Event::TaskListMarker(checked) => Some((checked, range, provenance)),
            _ => None,
        })
        .collect();
    assert_eq!(markers.len(), 2);
    assert_eq!(markers[0].0, false);
    assert_eq!(&src[markers[0].1.clone()], "[ ]");
    assert_eq!(markers[0].2, Provenance::Synthesized);
    assert_eq!(markers[1].0, true);
    assert_eq!(&src[markers[1].1.clone()], "[x]");
    assert_eq!(markers[1].2, Provenance::Synthesized);
}

#[test]
fn smart_punctuation_is_synthesized() {
    let src = "\"quoted\" and...\n";
    let texts: Vec<_> = Parser::new_ext(src, Options::ENABLE_SMART_PUNCTUATION)
        .into_provenance_iter()
        .filter_map(|(event, range, provenance)| match event {
            Event::Text(text) => Some((text.into_string(), range, provenance)),
            _ => None,
        })
        .collect();
    let curly_open = texts.iter().find(|(text, _, _)| text == "\u{201c}").unwrap();
    assert_eq!(&src[curly_open.1.clone()], "\"");
    assert_eq!(curly_open.2, Provenance::Synthesized);
    let curly_close = texts.iter().find(|(text, _, _)| text == "\u{201d}").unwrap();
    assert_eq!(&src[curly_close.1.clone()], "\"");
    assert_eq!(curly_close.2, Provenance::Synthesized);
    let ellipsis = texts.iter().find(|(text, _, _)| text == "\u{2026}").unwrap();
    assert_eq!(&src[ellipsis.1.clone()], "...");
    assert_eq!(ellipsis.2, Provenance::Synthesized);
}

#[test]
fn nested_image_in_link() {
    // An inline image nested in an inline link: both come straight from the source.
    let src = "[![alt](img.png)](page.html)";
    let mut saw_image = false;
    let mut saw_link = false;
    for (event, _range, provenance) in Parser::new(src).into_provenance_iter() {
        match event {
            Event::Start(Tag::Image { .. }) => {
                saw_image = true;
                assert_eq!(provenance, Provenance::Source);
            }
            Event::Start(Tag::Link { .. }) => {
                saw_link = true;
                assert_eq!(provenance, Provenance::Source);
            }
            _ => {}
        }
    }
    assert!(saw_image && saw_link);

    // A reference image nested in a link carries the definition evidence.
    let src = "[![alt][img]](page.html)\n\n[img]: /img.png\n";
    let found = links(src);
    assert_eq!(found.len(), 2);
    let image = found
        .iter()
        .find(|(link_type, _, _)| *link_type == LinkType::Reference)
        .expect("reference image");
    assert_eq!(&src[image.1.clone()], "![alt][img]");
    match &image.2 {
        Provenance::Reference { definition } => {
            assert_eq!(&src[definition.clone()], "[img]: /img.png")
        }
        other => panic!("expected reference provenance, got {other:?}"),
    }
}

#[test]
fn nul_replacement_is_synthesized() {
    let src = "nul\u{0}byte\n";
    let texts: Vec<_> = Parser::new(src)
        .into_provenance_iter()
        .filter_map(|(event, range, provenance)| match event {
            Event::Text(text) => Some((text.into_string(), range, provenance)),
            _ => None,
        })
        .collect();
    let replacement = texts
        .iter()
        .find(|(text, _, _)| text == "\u{fffd}")
        .expect("NUL replaced by U+FFFD");
    // The trigger range covers the NUL byte, but the replacement character
    // does not appear in the source.
    assert_eq!(&src[replacement.1.clone()], "\u{0}");
    assert_eq!(replacement.2, Provenance::Synthesized);
    // Neighbouring text is untouched direct text.
    let plain = texts.iter().find(|(text, _, _)| text == "nul").unwrap();
    assert_eq!(plain.2, Provenance::Source);
}

#[test]
fn end_events_inherit_start_provenance() {
    let src = "[foo]\n\n[foo]: /url\n";
    let events: Vec<_> = Parser::new(src).into_provenance_iter().collect();
    let start = events
        .iter()
        .find(|(event, _, _)| matches!(event, Event::Start(Tag::Link { .. })))
        .unwrap();
    let end = events
        .iter()
        .find(|(event, _, _)| matches!(event, Event::End(TagEnd::Link)))
        .unwrap();
    assert_eq!(start.2, end.2);
    assert!(matches!(end.2, Provenance::Reference { .. }));
}

#[test]
fn ranges_stay_within_input_bounds() {
    let src = "# Title\n\npara *em* [link](/url) [ref][r] `code`\n\n\
               - [ ] task\n\nfoot[^n]\n\n[^n]: note\n\n[r]: /r\n\n\
               \"smart\" -- punct\u{0}\n";
    for (_event, range, provenance) in Parser::new_ext(src, all_options()).into_provenance_iter() {
        assert!(range.start <= range.end, "range not empty-ordered: {range:?}");
        assert!(range.end <= src.len(), "range out of bounds: {range:?}");
        assert!(src.is_char_boundary(range.start));
        assert!(src.is_char_boundary(range.end));
        if let Provenance::Reference { definition } = provenance {
            assert!(definition.start <= definition.end);
            assert!(definition.end <= src.len(), "definition out of bounds: {definition:?}");
            assert!(src.is_char_boundary(definition.start));
            assert!(src.is_char_boundary(definition.end));
        }
    }
}

#[test]
fn events_match_offset_iter() {
    let src = "# Title\n\npara *em* [link](/url) [ref][r] ![img](i.png) `code`\n\n\
               - [x] task\n\nfoot[^n]\n\n[^n]: note\n\n[r]: /r\n\n\
               \"smart\" -- punct\u{0}\n\n> quote\n\n```rust\ncode\n```\n";
    let options = all_options();
    let from_provenance: Vec<_> = Parser::new_ext(src, options.clone())
        .into_provenance_iter()
        .map(|(event, range, _)| (event, range))
        .collect();
    let from_offsets: Vec<_> = Parser::new_ext(src, options).into_offset_iter().collect();
    assert_eq!(from_provenance, from_offsets);
}
