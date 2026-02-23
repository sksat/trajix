use serde::{Deserialize, Serialize};

/// GNSS constellation type as defined by Android GnssStatus.
///
/// Known values: GPS(1), Sbas(2), Glonass(3), Qzss(4), BeiDou(5), Galileo(6), Irnss(7).
/// Unknown values are preserved as `Unknown(u8)` to avoid parse failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConstellationType {
    Gps,
    Sbas,
    Glonass,
    Qzss,
    BeiDou,
    Galileo,
    Irnss,
    Unknown(u8),
}

impl ConstellationType {
    pub fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Gps,
            2 => Self::Sbas,
            3 => Self::Glonass,
            4 => Self::Qzss,
            5 => Self::BeiDou,
            6 => Self::Galileo,
            7 => Self::Irnss,
            other => Self::Unknown(other),
        }
    }

    pub fn as_u8(self) -> u8 {
        match self {
            Self::Gps => 1,
            Self::Sbas => 2,
            Self::Glonass => 3,
            Self::Qzss => 4,
            Self::BeiDou => 5,
            Self::Galileo => 6,
            Self::Irnss => 7,
            Self::Unknown(v) => v,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Gps => "GPS",
            Self::Sbas => "SBAS",
            Self::Glonass => "GLONASS",
            Self::Qzss => "QZSS",
            Self::BeiDou => "BeiDou",
            Self::Galileo => "Galileo",
            Self::Irnss => "IRNSS",
            Self::Unknown(_) => "Unknown",
        }
    }
}

/// Serialize `ConstellationType` as its u8 value (for compact JSON/binary output).
pub fn serialize_constellation_u8<S: serde::Serializer>(
    ct: &ConstellationType,
    s: S,
) -> Result<S::Ok, S::Error> {
    s.serialize_u8(ct.as_u8())
}

/// Deserialize `ConstellationType` from its u8 value.
pub fn deserialize_constellation_u8<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<ConstellationType, D::Error> {
    let v = u8::deserialize(d)?;
    Ok(ConstellationType::from_u8(v))
}

impl std::fmt::Display for ConstellationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown(v) => write!(f, "Unknown({v})"),
            _ => f.write_str(self.name()),
        }
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
    #[allow(clippy::should_implement_trait)]
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
    #[allow(clippy::should_implement_trait)]
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
    #[allow(clippy::should_implement_trait)]
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
            (2, ConstellationType::Sbas),
            (3, ConstellationType::Glonass),
            (4, ConstellationType::Qzss),
            (5, ConstellationType::BeiDou),
            (6, ConstellationType::Galileo),
            (7, ConstellationType::Irnss),
        ];
        for (val, expected) in cases {
            let ct = ConstellationType::from_u8(val);
            assert_eq!(ct, expected);
            assert_eq!(ct.as_u8(), val);
        }
    }

    #[test]
    fn constellation_type_unknown_preserved() {
        let ct = ConstellationType::from_u8(0);
        assert_eq!(ct, ConstellationType::Unknown(0));
        assert_eq!(ct.as_u8(), 0);

        let ct = ConstellationType::from_u8(99);
        assert_eq!(ct, ConstellationType::Unknown(99));
        assert_eq!(ct.as_u8(), 99);
    }

    #[test]
    fn constellation_type_display() {
        assert_eq!(ConstellationType::Gps.to_string(), "GPS");
        assert_eq!(ConstellationType::Sbas.to_string(), "SBAS");
        assert_eq!(ConstellationType::Glonass.to_string(), "GLONASS");
        assert_eq!(ConstellationType::Qzss.to_string(), "QZSS");
        assert_eq!(ConstellationType::BeiDou.to_string(), "BeiDou");
        assert_eq!(ConstellationType::Galileo.to_string(), "Galileo");
        assert_eq!(ConstellationType::Irnss.to_string(), "IRNSS");
        assert_eq!(ConstellationType::Unknown(99).to_string(), "Unknown(99)");
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
            "Raw",
            "Fix",
            "Status",
            "UncalAccel",
            "UncalGyro",
            "UncalMag",
            "OrientationDeg",
            "GameRotationVector",
            "Nav",
            "Agc",
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
