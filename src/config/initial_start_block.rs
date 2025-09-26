use std::fmt;

use serde::{
    Deserialize, Deserializer,
    de::{self, Visitor},
};

/// Initial block configuration
#[derive(Debug, Clone, PartialEq)]
pub enum InitialStartBlock {
    Absolute(u64), // Positive block number
    Offset(i64),   // Negative offset from the latest block
    Latest,        // The latest block
}

impl Default for InitialStartBlock {
    fn default() -> Self {
        InitialStartBlock::Offset(-100)
    }
}

impl<'de> Deserialize<'de> for InitialStartBlock {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct StartBlockVisitor;

        impl<'de> Visitor<'de> for StartBlockVisitor {
            type Value = InitialStartBlock;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter
                    .write_str("a positive block number, a negative offset, or the string 'latest'")
            }

            // Handles integer values (e.g., 18000000 or -100)
            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                if value >= 0 {
                    Ok(InitialStartBlock::Absolute(value as u64))
                } else {
                    Ok(InitialStartBlock::Offset(value))
                }
            }

            // Handles string values (e.g., "latest")
            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                if value.eq_ignore_ascii_case("latest") {
                    Ok(InitialStartBlock::Latest)
                } else {
                    Err(de::Error::invalid_value(de::Unexpected::Str(value), &self))
                }
            }
        }

        deserializer.deserialize_any(StartBlockVisitor)
    }
}

#[cfg(test)]
mod tests {
    use config::Config;

    use super::*;

    #[test]
    fn test_deserialize_initial_start_block_absolute() {
        let yaml = "initial_start_block: 18000000";
        let config: InitialStartBlock = Config::builder()
            .add_source(config::File::from_str(yaml, config::FileFormat::Yaml))
            .build()
            .unwrap()
            .get::<InitialStartBlock>("initial_start_block")
            .unwrap();
        assert_eq!(config, InitialStartBlock::Absolute(18000000));
    }

    #[test]
    fn test_deserialize_initial_start_block_offset() {
        let yaml = "initial_start_block: -100";
        let config: InitialStartBlock = Config::builder()
            .add_source(config::File::from_str(yaml, config::FileFormat::Yaml))
            .build()
            .unwrap()
            .get::<InitialStartBlock>("initial_start_block")
            .unwrap();
        assert_eq!(config, InitialStartBlock::Offset(-100));
    }

    #[test]
    fn test_deserialize_initial_start_block_latest() {
        let yaml = "initial_start_block: latest";
        let config: InitialStartBlock = Config::builder()
            .add_source(config::File::from_str(yaml, config::FileFormat::Yaml))
            .build()
            .unwrap()
            .get::<InitialStartBlock>("initial_start_block")
            .unwrap();
        assert_eq!(config, InitialStartBlock::Latest);
    }

    #[test]
    fn test_deserialize_initial_start_block_invalid() {
        let yaml = "initial_start_block: unknown";
        let result: Result<InitialStartBlock, _> = Config::builder()
            .add_source(config::File::from_str(yaml, config::FileFormat::Yaml))
            .build()
            .unwrap()
            .get::<InitialStartBlock>("initial_start_block");
        assert!(result.is_err());
    }
}
