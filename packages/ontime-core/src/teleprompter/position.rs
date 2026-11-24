//! Where the reader is: a place in an event's text, never a line number, so
//! edits and rewrapping elsewhere do not move them. Rows are only ever
//! calculated from it. Ported from upstream's `teleprompterPosition.ts`.
//!
//! Rows count every line of every event (headings, text and the gap after
//! each event) and may be fractional between lines.

use serde::{Deserialize, Serialize};

use super::script::{Line, ScriptEvent};

/// A place in the script.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Anchor {
    pub event_id: String,
    /// Position in the event's text, in UTF-16 units, which edits move.
    pub char_offset: usize,
    /// Distance in lines from the line holding `char_offset`: a fraction
    /// between lines, negative in the headings, past the last line in the gap.
    pub lines: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextRow {
    pub row: usize,
    pub start: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutEvent {
    pub id: String,
    pub cue: String,
    pub title: String,
    /// The event's first row, its heading when it has one.
    pub first_row: usize,
    /// Where reading starts: the first line of text, or where it would be.
    pub start_row: usize,
    /// Reached once every line of text has been read.
    pub end_row: usize,
    /// The row after the event's last.
    pub next_row: usize,
    pub text: Vec<TextRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    pub row_count: usize,
    pub events: Vec<LayoutEvent>,
}

impl Layout {
    pub fn new(events: &[ScriptEvent]) -> Self {
        let mut row = 0;
        let mut layout_events = Vec::with_capacity(events.len());
        for event in events {
            let first_row = row;
            let mut text = Vec::new();
            let mut heading_rows = 0;
            for (index, line) in event.lines.iter().enumerate() {
                match line {
                    Line::Text { start, .. } => text.push(TextRow { row: first_row + index, start: *start }),
                    Line::Heading { .. } if text.is_empty() => heading_rows += 1,
                    _ => {}
                }
            }
            let start_row = text.first().map_or(first_row + heading_rows, |line| line.row);
            let end_row = text.last().map_or(start_row, |line| line.row + 1);
            row += event.lines.len();
            layout_events.push(LayoutEvent {
                id: event.id.clone(),
                cue: event.cue.clone(),
                title: event.title.clone(),
                first_row,
                start_row,
                end_row,
                next_row: row,
                text,
            });
        }
        Self { row_count: row, events: layout_events }
    }

    pub fn event(&self, id: &str) -> Option<&LayoutEvent> {
        self.events.iter().find(|event| event.id == id)
    }

    /// The row of an anchor, or `None` when its event is not in the script.
    pub fn row_of(&self, anchor: &Anchor) -> Option<f64> {
        let event = self.event(&anchor.event_id)?;
        let Some(first) = event.text.first() else {
            return Some(event.start_row as f64 + anchor.lines);
        };
        let line = event.text.iter().take_while(|line| line.start <= anchor.char_offset).last().unwrap_or(first);
        // an anchor stays in its event, even when its headings shrink under a reader resting on them
        Some((line.row as f64 + anchor.lines).max(event.first_row as f64))
    }

    /// The event a row belongs to, counting its headings and the gap after it.
    pub fn event_at(&self, row: f64) -> Option<&LayoutEvent> {
        let mut owner = self.events.first()?;
        for event in &self.events {
            if event.first_row as f64 > row {
                break;
            }
            owner = event;
        }
        Some(owner)
    }

    /// The anchor at a row, or `None` for an empty script.
    pub fn anchor_at(&self, row: f64) -> Option<Anchor> {
        let event = self.event_at(row)?;
        let (Some(first), Some(last)) = (event.text.first(), event.text.last()) else {
            return Some(Anchor { event_id: event.id.clone(), char_offset: 0, lines: row - event.start_row as f64 });
        };
        let whole = row.floor();
        // above its text, or in the gap after it
        let nearest = event.text.iter().find(|line| line.row as f64 == whole).unwrap_or(match row < first.row as f64 {
            true => first,
            false => last,
        });
        Some(Anchor { event_id: event.id.clone(), char_offset: nearest.start, lines: row - nearest.row as f64 })
    }
}

/// The anchor at the start of an event: its first line of text on the reading line.
pub fn start_of_event(event_id: &str) -> Anchor {
    Anchor { event_id: event_id.to_string(), char_offset: 0, lines: 0.0 }
}

/// Maps a position through an edit, found by comparing the old and new text
/// from both ends. Texts and positions are in UTF-16 units.
/// - a change after the position leaves it in place
/// - a change before it shifts it by the difference in length
/// - a change around it moves it to the start of the change
pub fn map_offset_through_edit(previous: &[u16], next: &[u16], offset: usize) -> usize {
    let shortest = previous.len().min(next.len());
    let prefix = previous.iter().zip(next).take_while(|(a, b)| a == b).count().min(shortest);
    let suffix = previous
        .iter()
        .rev()
        .zip(next.iter().rev())
        .take(shortest - prefix)
        .take_while(|(a, b)| a == b)
        .count();

    if offset <= prefix {
        return offset;
    }
    if offset >= previous.len() - suffix {
        return offset + next.len() - previous.len();
    }
    prefix
}

/// Moves an anchor from one version of the script to the next.
/// - an event which moved takes the reader with it
/// - an edit to the reader's event is mapped through its text
/// - a deleted event leaves the reader where its text used to begin
/// - with nothing of the previous script left the reader goes to the top
///
/// `None` for an empty script.
pub fn reanchor(previous: &[ScriptEvent], next: &[ScriptEvent], anchor: Option<&Anchor>) -> Option<Anchor> {
    let top = start_of_event(&next.first()?.id);
    let Some(anchor) = anchor else {
        return Some(top);
    };

    let next_event = next.iter().find(|event| event.id == anchor.event_id);
    let previous_index = previous.iter().position(|event| event.id == anchor.event_id);

    match (next_event, previous_index) {
        (Some(_), None) => return Some(anchor.clone()),
        (Some(next_event), Some(index)) => {
            let char_offset =
                map_offset_through_edit(&text_of_lines(&previous[index]), &text_of_lines(next_event), anchor.char_offset);
            return Some(Anchor { char_offset, ..anchor.clone() });
        }
        (None, None) => return Some(top),
        (None, Some(_)) => {}
    }
    let index = previous_index?;

    let survives = |event: &&ScriptEvent| next.iter().any(|candidate| candidate.id == event.id);
    if let Some(following) = previous[index + 1..].iter().find(survives) {
        return Some(start_of_event(&following.id));
    }
    let Some(preceding) = previous[..index].iter().rev().find(survives) else {
        return Some(top);
    };
    let layout = Layout::new(next);
    let end_row = layout.event(&preceding.id)?.end_row;
    layout.anchor_at(end_row as f64)
}

/// The event's text as its lines show it, every character at its position.
/// What the lines leave out (spaces used up at a break, line breaks,
/// indentation) becomes spaces, which is enough to compare two versions of a
/// text from both ends. UTF-16 units, as positions are.
fn text_of_lines(event: &ScriptEvent) -> Vec<u16> {
    let mut text: Vec<u16> = Vec::new();
    for line in &event.lines {
        if let Line::Text { text: line_text, start, .. } = line {
            let padding = start.saturating_sub(text.len());
            text.extend(std::iter::repeat_n(u16::from(b' '), padding));
            text.extend(line_text.encode_utf16());
        }
    }
    text
}
