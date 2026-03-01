use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Severity {
    Critical,
    Warning,
    Info,
}

impl Severity {
    pub fn label(&self) -> &str {
        match self {
            Severity::Critical => "CRITICAL",
            Severity::Warning => "WARNING",
            Severity::Info => "INFO",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsightSection {
    pub severity: Severity,
    pub title: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiInsights {
    pub session_id: String,
    pub timestamp: DateTime<Utc>,
    pub provider: String,
    pub model: String,
    pub raw_markdown: String,
    pub sections: Vec<InsightSection>,
}

impl AiInsights {
    /// Parse a raw markdown response into structured sections.
    pub fn from_response(
        session_id: String,
        provider: String,
        model: String,
        raw_markdown: String,
    ) -> Self {
        let sections = parse_sections(&raw_markdown);
        AiInsights {
            session_id,
            timestamp: Utc::now(),
            provider,
            model,
            raw_markdown,
            sections,
        }
    }

    /// Total line count for scroll calculations.
    pub fn line_count(&self) -> usize {
        // Header lines + blank line + all section lines
        let mut count = 3; // header, provider info, blank
        for section in &self.sections {
            count += 2; // section header + blank
            count += section.body.lines().count();
            count += 1; // trailing blank
        }
        if self.sections.is_empty() {
            count += self.raw_markdown.lines().count();
        }
        count
    }
}

/// Parse markdown response into severity-tagged sections.
fn parse_sections(markdown: &str) -> Vec<InsightSection> {
    let mut sections = Vec::new();
    let mut current_severity: Option<Severity> = None;
    let mut current_title = String::new();
    let mut current_body = String::new();

    for line in markdown.lines() {
        if let Some(header) = line.strip_prefix("## ") {
            // Flush previous section
            if current_severity.is_some() {
                sections.push(InsightSection {
                    severity: current_severity.take().unwrap(),
                    title: current_title.clone(),
                    body: current_body.trim().to_string(),
                });
                current_body.clear();
            }

            // Parse new header
            let (severity, title) = parse_header(header);
            current_severity = Some(severity);
            current_title = title;
        } else if current_severity.is_some() {
            current_body.push_str(line);
            current_body.push('\n');
        }
        // Lines before any ## header are ignored (preamble)
    }

    // Flush last section
    if let Some(severity) = current_severity {
        sections.push(InsightSection {
            severity,
            title: current_title,
            body: current_body.trim().to_string(),
        });
    }

    sections
}

/// Parse a header like `[CRITICAL] Engine Overheating` into (Severity, title).
fn parse_header(header: &str) -> (Severity, String) {
    let trimmed = header.trim();
    if let Some(rest) = trimmed.strip_prefix("[CRITICAL]") {
        (Severity::Critical, rest.trim().to_string())
    } else if let Some(rest) = trimmed.strip_prefix("[WARNING]") {
        (Severity::Warning, rest.trim().to_string())
    } else if let Some(rest) = trimmed.strip_prefix("[INFO]") {
        (Severity::Info, rest.trim().to_string())
    } else {
        // Default to INFO for untagged headers
        (Severity::Info, trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sections() {
        let md = r#"Some preamble text.

## [CRITICAL] Engine Overheating
Coolant temp at 110°C is dangerously high.

## [WARNING] Fuel Trims High
Long fuel trim B1 at +18% suggests a vacuum leak.

## [INFO] Normal Operation
All other systems are operating within normal parameters.
"#;
        let sections = parse_sections(md);
        assert_eq!(sections.len(), 3);
        assert_eq!(sections[0].severity, Severity::Critical);
        assert_eq!(sections[0].title, "Engine Overheating");
        assert!(sections[0].body.contains("110°C"));
        assert_eq!(sections[1].severity, Severity::Warning);
        assert_eq!(sections[2].severity, Severity::Info);
    }

    #[test]
    fn test_parse_untagged_header() {
        let md = "## General Notes\nSome content here.\n";
        let sections = parse_sections(md);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].severity, Severity::Info);
        assert_eq!(sections[0].title, "General Notes");
    }
}
