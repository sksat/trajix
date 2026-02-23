use serde::{Deserialize, Serialize};

/// Device and logger information parsed from the file header.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaderInfo {
    pub version: String,
    pub platform: u32,
    pub manufacturer: String,
    pub model: String,
    pub gnss_hardware: String,
}

impl HeaderInfo {
    /// Parse header info from comment lines.
    ///
    /// Expects lines starting with "# " as they appear in GNSS Logger output.
    /// Looks for the line containing "Version:" to extract device metadata.
    pub fn parse(lines: &[&str]) -> Option<Self> {
        for line in lines {
            let line = line.trim();
            if !line.starts_with('#') {
                continue;
            }
            let content = line.trim_start_matches('#').trim();

            if let Some(info) = Self::parse_version_line(content) {
                return Some(info);
            }
        }
        None
    }

    fn parse_version_line(line: &str) -> Option<Self> {
        if !line.starts_with("Version:") {
            return None;
        }

        let version = Self::extract_field(line, "Version:", "Platform:")?;
        let platform_str = Self::extract_field(line, "Platform:", "Manufacturer:")?;
        let platform = platform_str.parse::<u32>().ok()?;
        let manufacturer = Self::extract_field(line, "Manufacturer:", "Model:")?;
        let model = Self::extract_field(line, "Model:", "GNSS Hardware Model Name:")?;
        let gnss_hardware = Self::extract_field_to_end(line, "GNSS Hardware Model Name:")?;

        Some(HeaderInfo {
            version,
            platform,
            manufacturer,
            model,
            gnss_hardware,
        })
    }

    fn extract_field(line: &str, key: &str, next_key: &str) -> Option<String> {
        let start = line.find(key)? + key.len();
        let end = line.find(next_key)?;
        Some(line[start..end].trim().to_string())
    }

    fn extract_field_to_end(line: &str, key: &str) -> Option<String> {
        let start = line.find(key)? + key.len();
        let value = line[start..].trim().trim_end_matches(';').trim();
        Some(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_lines() -> String {
        std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/header.txt"
        ))
        .unwrap()
    }

    #[test]
    fn parse_header_from_fixture() {
        let content = fixture_lines();
        let lines: Vec<&str> = content.lines().collect();
        let info = HeaderInfo::parse(&lines).expect("should parse header");

        assert_eq!(info.version, "v3.1.1.2");
        assert_eq!(info.platform, 15);
        assert_eq!(info.manufacturer, "SHARP");
        assert_eq!(info.model, "SH-M26");
        assert!(info.gnss_hardware.starts_with("qcom;MPSS.DE.3.1.1"));
    }

    #[test]
    fn parse_header_no_version_line() {
        let lines = vec!["# ", "# Header Description:", "# "];
        assert!(HeaderInfo::parse(&lines).is_none());
    }

    #[test]
    fn parse_header_empty() {
        let lines: Vec<&str> = vec![];
        assert!(HeaderInfo::parse(&lines).is_none());
    }
}
