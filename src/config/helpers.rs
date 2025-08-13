use serde::{Deserialize, Deserializer, Serializer, de};
use std::time::Duration;
use url::Url;

/// Custom deserializer for Duration from milliseconds
pub fn deserialize_duration_from_ms<'de, D>(deserializer: D) -> Result<Duration, D::Error>
where
    D: Deserializer<'de>,
{
    let ms = u64::deserialize(deserializer)?;
    Ok(Duration::from_millis(ms))
}

/// Custom deserializer for Duration from seconds
pub fn deserialize_duration_from_seconds<'de, D>(deserializer: D) -> Result<Duration, D::Error>
where
    D: Deserializer<'de>,
{
    let secs = u64::deserialize(deserializer)?;
    Ok(Duration::from_secs(secs))
}

/// Custom serializer for Duration to milliseconds
pub fn serialize_duration_to_ms<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_u64(duration.as_millis() as u64)
}

/// Custom serializer for Duration to seconds
pub fn serialize_duration_to_seconds<S>(
    duration: &Duration,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_u64(duration.as_secs())
}

/// Custom deserializer for a vector of URLs.
pub fn deserialize_urls<'de, D>(deserializer: D) -> Result<Vec<Url>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = Vec::<String>::deserialize(deserializer)?;
    s.into_iter()
        .map(|url_str| Url::parse(&url_str).map_err(de::Error::custom))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;
    use serde_json;

    #[derive(Debug, Deserialize, Serialize, PartialEq)]
    struct TestDurationMs {
        #[serde(
            deserialize_with = "deserialize_duration_from_ms",
            serialize_with = "serialize_duration_to_ms"
        )]
        duration: Duration,
    }

    #[derive(Debug, Deserialize, Serialize, PartialEq)]
    struct TestDurationSecs {
        #[serde(
            deserialize_with = "deserialize_duration_from_seconds",
            serialize_with = "serialize_duration_to_seconds"
        )]
        duration: Duration,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct TestUrls {
        #[serde(deserialize_with = "deserialize_urls")]
        urls: Vec<Url>,
    }

    #[test]
    fn test_deserialize_duration_from_ms() {
        let json = r#"{"duration": 5000}"#;
        let expected = TestDurationMs {
            duration: Duration::from_millis(5000),
        };
        let actual: TestDurationMs = serde_json::from_str(json).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_serialize_duration_to_ms() {
        let data = TestDurationMs {
            duration: Duration::from_millis(5000),
        };
        let expected = r#"{"duration":5000}"#;
        let actual = serde_json::to_string(&data).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_deserialize_duration_from_seconds() {
        let json = r#"{"duration": 5}"#;
        let expected = TestDurationSecs {
            duration: Duration::from_secs(5),
        };
        let actual: TestDurationSecs = serde_json::from_str(json).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_serialize_duration_to_seconds() {
        let data = TestDurationSecs {
            duration: Duration::from_secs(5),
        };
        let expected = r#"{"duration":5}"#;
        let actual = serde_json::to_string(&data).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_deserialize_urls() {
        let json = r#"{"urls": ["http://example.com/1", "https://example.com/2"]}"#;
        let expected = TestUrls {
            urls: vec![
                Url::parse("http://example.com/1").unwrap(),
                Url::parse("https://example.com/2").unwrap(),
            ],
        };
        let actual: TestUrls = serde_json::from_str(json).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_deserialize_invalid_url() {
        let json = r#"{"urls": ["not a valid url"]}"#;
        let result: Result<TestUrls, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }
}
