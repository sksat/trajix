use serde::{Deserialize, Serialize};

/// GNSS constellation type as defined by Android GnssStatus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum ConstellationType {
    Gps = 1,
    Glonass = 3,
    Qzss = 4,
    BeiDou = 5,
    Galileo = 6,
}

impl ConstellationType {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Gps),
            3 => Some(Self::Glonass),
            4 => Some(Self::Qzss),
            5 => Some(Self::BeiDou),
            6 => Some(Self::Galileo),
            _ => None,
        }
    }

    pub fn as_u8(self) -> u8 {
        self as u8
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Gps => "GPS",
            Self::Glonass => "GLONASS",
            Self::Qzss => "QZSS",
            Self::BeiDou => "BeiDou",
            Self::Galileo => "Galileo",
        }
    }
}

impl std::fmt::Display for ConstellationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// Fix provider type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FixProvider {
    /// GPS hardware provider
    Gps,
    /// Fused Location Provider
    Flp,
    /// Network Location Provider
    Nlp,
}

impl FixProvider {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "GPS" => Some(Self::Gps),
            "FLP" => Some(Self::Flp),
            "NLP" => Some(Self::Nlp),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gps => "GPS",
            Self::Flp => "FLP",
            Self::Nlp => "NLP",
        }
    }
}

impl std::fmt::Display for FixProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// GNSS signal code type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CodeType {
    /// Coarse/Acquisition
    C,
    /// In-phase
    I,
    /// Precision
    P,
    /// Other/unknown code type
    Other(char),
}

impl CodeType {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "C" => Some(Self::C),
            "I" => Some(Self::I),
            "P" => Some(Self::P),
            s if s.len() == 1 => s.chars().next().map(Self::Other),
            _ => None,
        }
    }
}

/// Record type identifier (the prefix before the first comma).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecordType {
    Raw,
    Fix,
    Status,
    UncalAccel,
    UncalGyro,
    UncalMag,
    OrientationDeg,
    GameRotationVector,
    Nav,
    Agc,
}

impl RecordType {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "Raw" => Some(Self::Raw),
            "Fix" => Some(Self::Fix),
            "Status" => Some(Self::Status),
            "UncalAccel" => Some(Self::UncalAccel),
            "UncalGyro" => Some(Self::UncalGyro),
            "UncalMag" => Some(Self::UncalMag),
            "OrientationDeg" => Some(Self::OrientationDeg),
            "GameRotationVector" => Some(Self::GameRotationVector),
            "Nav" => Some(Self::Nav),
            "Agc" => Some(Self::Agc),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Raw => "Raw",
            Self::Fix => "Fix",
            Self::Status => "Status",
            Self::UncalAccel => "UncalAccel",
            Self::UncalGyro => "UncalGyro",
            Self::UncalMag => "UncalMag",
            Self::OrientationDeg => "OrientationDeg",
            Self::GameRotationVector => "GameRotationVector",
            Self::Nav => "Nav",
            Self::Agc => "Agc",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constellation_type_roundtrip() {
        let cases = [
            (1u8, ConstellationType::Gps),
            (3, ConstellationType::Glonass),
            (4, ConstellationType::Qzss),
            (5, ConstellationType::BeiDou),
            (6, ConstellationType::Galileo),
        ];
        for (val, expected) in cases {
            let ct = ConstellationType::from_u8(val).unwrap();
            assert_eq!(ct, expected);
            assert_eq!(ct.as_u8(), val);
        }
    }

    #[test]
    fn constellation_type_unknown() {
        assert!(ConstellationType::from_u8(0).is_none());
        assert!(ConstellationType::from_u8(2).is_none());
        assert!(ConstellationType::from_u8(7).is_none());
    }

    #[test]
    fn constellation_type_display() {
        assert_eq!(ConstellationType::Gps.to_string(), "GPS");
        assert_eq!(ConstellationType::Glonass.to_string(), "GLONASS");
        assert_eq!(ConstellationType::Qzss.to_string(), "QZSS");
        assert_eq!(ConstellationType::BeiDou.to_string(), "BeiDou");
        assert_eq!(ConstellationType::Galileo.to_string(), "Galileo");
    }

    #[test]
    fn fix_provider_roundtrip() {
        let cases = [
            ("GPS", FixProvider::Gps),
            ("FLP", FixProvider::Flp),
            ("NLP", FixProvider::Nlp),
        ];
        for (s, expected) in cases {
            let fp = FixProvider::from_str(s).unwrap();
            assert_eq!(fp, expected);
            assert_eq!(fp.as_str(), s);
        }
    }

    #[test]
    fn fix_provider_unknown() {
        assert!(FixProvider::from_str("UNKNOWN").is_none());
        assert!(FixProvider::from_str("").is_none());
    }

    #[test]
    fn code_type_parse() {
        assert_eq!(CodeType::from_str("C"), Some(CodeType::C));
        assert_eq!(CodeType::from_str("I"), Some(CodeType::I));
        assert_eq!(CodeType::from_str("P"), Some(CodeType::P));
        assert_eq!(CodeType::from_str("X"), Some(CodeType::Other('X')));
        assert!(CodeType::from_str("").is_none());
        assert!(CodeType::from_str("CC").is_none());
    }

    #[test]
    fn record_type_roundtrip() {
        let cases = [
            "Raw", "Fix", "Status", "UncalAccel", "UncalGyro", "UncalMag",
            "OrientationDeg", "GameRotationVector", "Nav", "Agc",
        ];
        for s in cases {
            let rt = RecordType::from_str(s).unwrap();
            assert_eq!(rt.as_str(), s);
        }
    }

    #[test]
    fn record_type_unknown() {
        assert!(RecordType::from_str("Unknown").is_none());
        assert!(RecordType::from_str("").is_none());
    }
}
