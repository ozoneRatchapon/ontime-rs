use crate::types::EventItem;
use calamine::{Reader, Xlsx, XlsxError};
use std::io::Cursor;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ParserError {
    #[error("Xlsx error: {0}")]
    Xlsx(#[from] XlsxError),
    #[error("CSV parse error: {0}")]
    Csv(String),
    #[error("No sheets found in workbook")]
    EmptyWorkbook,
}

pub struct RundownParser;

impl RundownParser {
    pub fn parse_csv(content: &str) -> Result<Vec<EventItem>, ParserError> {
        let mut events = Vec::new();
        for (idx, line) in content.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split(',').collect();
            if parts.len() >= 2 {
                let cue = parts[0].trim();
                let title = parts[1].trim();
                let duration_sec: u64 = parts
                    .get(2)
                    .and_then(|s| s.trim().parse().ok())
                    .unwrap_or(300);
                events.push(EventItem::new(cue, title, duration_sec));
            } else if parts.len() == 1 {
                let title = parts[0].trim();
                let cue = format!("{idx}");
                events.push(EventItem::new(cue, title, 300));
            }
        }
        Ok(events)
    }

    pub fn parse_excel(bytes: &[u8]) -> Result<Vec<EventItem>, ParserError> {
        let cursor = Cursor::new(bytes);
        let mut workbook: Xlsx<_> = Xlsx::new(cursor)?;
        let sheet_name = workbook
            .sheet_names()
            .first()
            .cloned()
            .ok_or(ParserError::EmptyWorkbook)?;

        let range = workbook.worksheet_range(&sheet_name)?;

        let mut events = Vec::new();
        for (i, row) in range.rows().enumerate() {
            if i == 0 {
                // Header row
                continue;
            }
            let cue = row
                .first()
                .map(|c| c.to_string())
                .unwrap_or_else(|| format!("{i}"));
            let title = row
                .get(1)
                .map(|c| c.to_string())
                .unwrap_or_else(|| "Untitled Event".to_string());
            let duration_sec: u64 = row
                .get(2)
                .and_then(|c| c.to_string().parse().ok())
                .unwrap_or(300);

            events.push(EventItem::new(cue, title, duration_sec));
        }

        Ok(events)
    }
}
