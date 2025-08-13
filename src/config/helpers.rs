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
