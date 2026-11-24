//! Cutting a script text into teleprompter lines. Ported from upstream's
//! `teleprompter.wrap.ts`; the fixture `teleprompter_wrap.json` is generated
//! from that code and pins every case.
//!
//! Two things keep the output byte-for-byte compatible with the browser:
//! - widths count user-perceived characters (extended grapheme clusters, as
//!   `Intl.Segmenter` does), not bytes or code points
//! - every `start` is a position in UTF-16 code units, which is what a
//!   JavaScript string index is

use serde::{Deserialize, Serialize};
use unicode_general_category::{get_general_category, GeneralCategory};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WrappedLine {
    pub text: String,
    /// Position of the first character in the normalised text, in UTF-16 units.
    pub start: usize,
    pub indent: usize,
}

/// JavaScript's `\s`: Unicode space separators, the ASCII controls it counts,
/// the line and paragraph separators and the byte order mark. Unlike Rust's
/// `char::is_whitespace`, it leaves out U+0085 and includes U+FEFF.
fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | '\u{2028}' | '\u{2029}' | '\u{feff}'
    ) || get_general_category(c) == GeneralCategory::SpaceSeparator
}

/// A space a line may break at, which leaves out the no-break spaces.
fn is_breaking_space(c: char) -> bool {
    is_js_space(c) && !matches!(c, '\u{a0}' | '\u{2007}' | '\u{202f}')
}

fn trim_end_js(text: &str) -> &str {
    text.trim_end_matches(is_js_space)
}

fn trim_start_js(text: &str) -> &str {
    text.trim_start_matches(is_js_space)
}

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// The script text as it is read: tabs become 4 spaces, Windows line endings
/// become `\n` and trailing whitespace is ignored. Positions are positions here.
pub fn normalise_script_text(text: &str) -> String {
    trim_end_js(&text.replace("\r\n", "\n").replace('\t', "    ")).to_string()
}

/// Cuts a script text into lines of at most `chars_per_line` user-perceived
/// characters. Typed line breaks, blank lines, indentation and inner spaces
/// are kept. A text holding only whitespace has no lines.
pub fn wrap_text(text: &str, chars_per_line: f64) -> Vec<WrappedLine> {
    let normalised = normalise_script_text(text);
    if normalised.is_empty() {
        return Vec::new();
    }
    let limit = (chars_per_line.floor() as i64).max(1) as usize;
    let mut lines = Vec::new();
    let mut line_start = 0;
    for typed_line in normalised.split('\n') {
        wrap_typed_line(typed_line, line_start, limit, &mut lines);
        line_start += utf16_len(typed_line) + 1;
    }
    lines
}

/// Byte positions within the content being wrapped.
#[derive(Debug, Clone, Copy)]
struct Piece {
    start: usize,
    end: usize,
    width: usize,
    is_space: bool,
}

/// Pieces which must stay on one line, with the space where a line may break
/// before them.
#[derive(Debug, Clone)]
struct Chunk {
    pieces: Vec<Piece>,
    space_before: Option<Piece>,
    width: usize,
}

#[derive(Debug, Clone, Copy)]
struct Filling {
    start: usize,
    end: usize,
    width: usize,
}

fn wrap_typed_line(
    typed_line: &str,
    line_start: usize,
    chars_per_line: usize,
    lines: &mut Vec<WrappedLine>,
) {
    let trimmed = trim_end_js(typed_line);
    if trimmed.is_empty() {
        lines.push(WrappedLine {
            text: String::new(),
            start: line_start,
            indent: 0,
        });
        return;
    }

    let content = trim_start_js(trimmed);
    let typed_indent = utf16_len(trimmed) - utf16_len(content);
    // keep room for the text, should the indentation take most of the line
    let indent = typed_indent.min(chars_per_line / 2);
    let width = chars_per_line - indent;
    let content_start = line_start + typed_indent;

    let push_line = |lines: &mut Vec<WrappedLine>, start: usize, end: usize| {
        lines.push(WrappedLine {
            text: content[start..end].to_string(),
            start: content_start + utf16_len(&content[..start]),
            indent,
        });
    };
    let fits = |current: Option<Filling>, chunk: &Chunk, space_width: usize| {
        current.map_or(0, |filling| filling.width) + space_width + chunk.width <= width
    };

    let mut current: Option<Filling> = None;
    let mut queue = make_chunks(content, &to_pieces(content));
    let mut i = 0;
    while i < queue.len() {
        let chunk = queue[i].clone();
        i += 1;
        let chunk_start = chunk.pieces[0].start;
        let chunk_end = chunk.pieces[chunk.pieces.len() - 1].end;
        let space_width = chunk.space_before.map_or(0, |space| space.width);

        let joins_line = fits(current, &chunk, space_width);
        if let (Some(filling), true) = (current.as_mut(), joins_line) {
            filling.end = chunk_end;
            filling.width += space_width + chunk.width;
            continue;
        }

        // the space where the line breaks is used up
        if let Some(filling) = current.take() {
            push_line(lines, filling.start, filling.end);
        }

        if fits(None, &chunk, 0) {
            current = Some(Filling {
                start: chunk_start,
                end: chunk_end,
                width: chunk.width,
            });
            continue;
        }

        // too long for a whole line: give up keeping its words together before cutting a word
        if chunk.pieces.len() > 1 {
            let split = split_chunk(&chunk.pieces);
            queue.splice(i..i, split);
            continue;
        }

        // a word longer than a line breaks at the limit, never inside a character
        let graphemes = graphemes_of(&content[chunk_start..chunk_end]);
        let mut from = 0;
        let mut offset = chunk_start;
        while graphemes.len() - from > width {
            let end = offset
                + graphemes[from..from + width]
                    .iter()
                    .map(|g| g.len())
                    .sum::<usize>();
            push_line(lines, offset, end);
            from += width;
            offset = end;
        }
        current = (from < graphemes.len()).then_some(Filling {
            start: offset,
            end: chunk_end,
            width: graphemes.len() - from,
        });
    }
    if let Some(filling) = current {
        push_line(lines, filling.start, filling.end);
    }
}

fn to_pieces(content: &str) -> Vec<Piece> {
    let mut pieces = Vec::new();
    let mut word_start = 0;
    let mut space_start: Option<usize> = None;
    for (at, c) in content.char_indices() {
        match (is_breaking_space(c), space_start) {
            (true, None) => {
                if at > word_start {
                    add_word(content, word_start, at, &mut pieces);
                }
                space_start = Some(at);
            }
            (false, Some(start)) => {
                pieces.push(space_piece(content, start, at));
                space_start = None;
                word_start = at;
            }
            _ => {}
        }
    }
    match space_start {
        Some(start) => pieces.push(space_piece(content, start, content.len())),
        None if word_start < content.len() => {
            add_word(content, word_start, content.len(), &mut pieces)
        }
        None => {}
    }
    pieces
}

/// A run of spaces is as wide as its length in UTF-16 units, as upstream measures it.
fn space_piece(content: &str, start: usize, end: usize) -> Piece {
    Piece {
        start,
        end,
        width: utf16_len(&content[start..end]),
        is_space: true,
    }
}

fn is_break_after(grapheme: &str) -> bool {
    matches!(grapheme, "-" | "–" | "—" | "/")
}

fn add_word(content: &str, start: usize, end: usize, pieces: &mut Vec<Piece>) {
    let word = &content[start..end];
    if !word.contains(['-', '–', '—', '/']) {
        pieces.push(Piece {
            start,
            end,
            width: grapheme_count(word),
            is_space: false,
        });
        return;
    }

    let graphemes = graphemes_of(word);
    let mut from = 0;
    let mut from_offset = start;
    let mut offset = start;
    for (i, grapheme) in graphemes.iter().enumerate() {
        offset += grapheme.len();
        if i == 0 || i + 1 >= graphemes.len() {
            continue;
        }
        let next = graphemes[i + 1];
        if is_break_after(grapheme) && !is_break_after(next) && !is_closing(next) {
            pieces.push(Piece {
                start: from_offset,
                end: offset,
                width: i + 1 - from,
                is_space: false,
            });
            from = i + 1;
            from_offset = offset;
        }
    }
    pieces.push(Piece {
        start: from_offset,
        end,
        width: graphemes.len() - from,
        is_space: false,
    });
}

fn make_chunks(content: &str, pieces: &[Piece]) -> Vec<Chunk> {
    let mut chunks: Vec<Chunk> = Vec::new();
    let mut space_before: Option<Piece> = None;
    for piece in pieces {
        if piece.is_space {
            space_before = Some(*piece);
            continue;
        }
        let joins = match (chunks.last(), space_before) {
            (Some(current), Some(_)) => {
                !can_break_between(content, &current.pieces[current.pieces.len() - 1], piece)
            }
            _ => false,
        };
        match (joins, chunks.last_mut(), space_before) {
            (true, Some(current), Some(space)) => {
                current.pieces.push(space);
                current.pieces.push(*piece);
                current.width += space.width + piece.width;
            }
            _ => chunks.push(Chunk {
                pieces: vec![*piece],
                space_before,
                width: piece.width,
            }),
        }
        space_before = None;
    }
    chunks
}

fn split_chunk(pieces: &[Piece]) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut space_before = None;
    for piece in pieces {
        match piece.is_space {
            true => space_before = Some(*piece),
            false => {
                chunks.push(Chunk {
                    pieces: vec![*piece],
                    space_before,
                    width: piece.width,
                });
                space_before = None;
            }
        }
    }
    chunks
}

fn can_break_between(content: &str, before: &Piece, after: &Piece) -> bool {
    let before_text = &content[before.start..before.end];
    if is_opening(before_text) || is_number_word(before_text) {
        return false;
    }
    !is_closing(&content[after.start..after.end])
}

/// Punctuation which closes what comes before it, so it never starts a line.
fn is_closing(text: &str) -> bool {
    !text.is_empty()
        && text.chars().all(|c| {
            matches!(c, '.' | ',' | ';' | ':' | '!' | '?' | '…' | '%')
                || matches!(
                    get_general_category(c),
                    GeneralCategory::ClosePunctuation | GeneralCategory::FinalPunctuation
                )
        })
}

/// Punctuation which opens what comes after it, so it never ends a line.
fn is_opening(text: &str) -> bool {
    !text.is_empty()
        && text.chars().all(|c| {
            matches!(c, '¿' | '¡')
                || matches!(
                    get_general_category(c),
                    GeneralCategory::OpenPunctuation | GeneralCategory::InitialPunctuation
                )
        })
}

fn is_number(c: char) -> bool {
    matches!(
        get_general_category(c),
        GeneralCategory::DecimalNumber
            | GeneralCategory::LetterNumber
            | GeneralCategory::OtherNumber
    )
}

/// A word made of a number (`10`, `3.5`, `10:30`), which stays with the word after it.
fn is_number_word(text: &str) -> bool {
    // digits, then any number of (separator, digits)
    let mut after_digit = false;
    for c in text.chars() {
        match (is_number(c), matches!(c, '.' | ',' | ':')) {
            (true, _) => after_digit = true,
            (false, true) if after_digit => after_digit = false,
            _ => return false,
        }
    }
    after_digit
}

/// Text below the combining marks, where every character is one grapheme.
fn is_single_unit(text: &str) -> bool {
    text.chars().all(|c| (c as u32) < 0x300)
}

fn grapheme_count(text: &str) -> usize {
    match is_single_unit(text) {
        true => text.chars().count(),
        false => text.graphemes(true).count(),
    }
}

fn graphemes_of(text: &str) -> Vec<&str> {
    match is_single_unit(text) {
        true => text
            .char_indices()
            .map(|(at, c)| &text[at..at + c.len_utf8()])
            .collect(),
        false => text.graphemes(true).collect(),
    }
}
