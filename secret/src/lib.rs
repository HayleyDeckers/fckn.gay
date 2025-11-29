//! A type for safely handling secrets in configuration files.
//!
//! Secrets can be specified as:
//! - Direct string values (backward compatible)
//! - File paths to read the secret from
//! - Environment variable names
//!
//! # Examples
//!
//! ```toml
//! # Direct value
//! api_key = "my-secret-key"
//!
//! # Read from file
//! api_key = { file = "/path/to/secret" }
//!
//! # Read from environment variable
//! api_key = { env = "API_KEY" }
//! ```

use std::fmt;

use serde::{
    de::{self, MapAccess, Visitor},
    Deserialize,
};

/// A secret value that can be deserialized from a string, file path, or environment variable.
///
/// The actual secret is resolved at deserialization time. Debug output always shows `[redacted]`
/// to prevent accidental secret leakage in logs.
pub struct Secret(String);

/// Error type for Secret construction
#[derive(Debug)]
pub enum SecretError {
    /// Failed to read from file
    FileRead {
        path: String,
        source: std::io::Error,
    },
    /// Failed to read from environment variable
    EnvVar {
        name: String,
        source: std::env::VarError,
    },
}

impl std::fmt::Display for SecretError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SecretError::FileRead { path, source: _ } => {
                write!(f, "failed to read secret from file '{path}'")
            }
            SecretError::EnvVar { name, source: _ } => {
                write!(f, "failed to read secret from env var '{name}'")
            }
        }
    }
}

impl std::error::Error for SecretError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SecretError::FileRead { source, .. } => Some(source),
            SecretError::EnvVar { source, .. } => Some(source),
        }
    }
}

impl Secret {
    /// Creates a Secret from a direct string value.
    pub fn from_string(value: String) -> Self {
        Secret(value)
    }

    /// Creates a Secret by reading from a file. Whitespace is trimmed.
    pub fn from_file(path: &str) -> Result<Self, SecretError> {
        let content = std::fs::read_to_string(path).map_err(|e| SecretError::FileRead {
            path: path.to_owned(),
            source: e,
        })?;
        Ok(Secret(content.trim().to_owned()))
    }

    /// Creates a Secret by reading from an environment variable.
    pub fn from_env(var_name: &str) -> Result<Self, SecretError> {
        let value = std::env::var(var_name).map_err(|e| SecretError::EnvVar {
            name: var_name.to_owned(),
            source: e,
        })?;
        Ok(Secret(value))
    }

    /// Exposes the secret value as a reference. Use with care - don't log this!
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Consumes the Secret and returns the inner value. Use with care - don't log this!
    pub fn into_exposed(self) -> String {
        self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[redacted]")
    }
}

impl<'de> Deserialize<'de> for Secret {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(SecretVisitor)
    }
}

struct SecretVisitor;

impl<'de> Visitor<'de> for SecretVisitor {
    type Value = Secret;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a string, or a map with 'file' or 'env' key")
    }

    // Direct string value
    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(Secret::from_string(value.to_owned()))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(Secret::from_string(value))
    }

    // Map with "file" or "env" key
    fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
    where
        M: MapAccess<'de>,
    {
        let Some(key) = map.next_key::<String>()? else {
            return Err(de::Error::custom(
                "expected 'file' or 'env' key in secret config",
            ));
        };

        match key.as_str() {
            "file" => {
                let path: String = map.next_value()?;
                Secret::from_file(&path).map_err(de::Error::custom)
            }
            "env" => {
                let var_name: String = map.next_value()?;
                Secret::from_env(&var_name).map_err(de::Error::custom)
            }
            other => Err(de::Error::custom(format!(
                "unknown secret source '{other}', expected 'file' or 'env'"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_direct_string() {
        let secret: Secret = toml::from_str(r#"secret = "hunter2""#)
            .map(|v: toml::Value| Secret::deserialize(v.get("secret").unwrap().clone()).unwrap())
            .unwrap();
        assert_eq!(secret.expose(), "hunter2");
    }

    #[test]
    fn test_debug_is_redacted() {
        let secret = Secret::from_string("super-secret".to_owned());
        assert_eq!(format!("{:?}", secret), "[redacted]");
    }

    #[test]
    fn test_into_exposed() {
        let secret = Secret::from_string("my-secret".to_owned());
        let exposed: String = secret.into_exposed();
        assert_eq!(exposed, "my-secret");
    }

    #[test]
    fn test_from_file_trims_whitespace() {
        use std::io::Write;
        let dir = std::env::temp_dir();
        let path = dir.join("test_secret_whitespace.txt");
        let mut file = std::fs::File::create(&path).unwrap();
        write!(file, "  secret-with-spaces  \n\n").unwrap();
        drop(file);

        let secret = Secret::from_file(path.to_str().unwrap()).unwrap();
        assert_eq!(secret.expose(), "secret-with-spaces");

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn test_from_env() {
        std::env::set_var("TEST_SECRET_VAR", "env-secret-value");
        let toml_str = r#"secret = { env = "TEST_SECRET_VAR" }"#;
        let value: toml::Value = toml::from_str(toml_str).unwrap();
        let secret = Secret::deserialize(value.get("secret").unwrap().clone()).unwrap();
        assert_eq!(secret.expose(), "env-secret-value");
        std::env::remove_var("TEST_SECRET_VAR");
    }

    #[test]
    fn test_from_file() {
        use std::io::Write;
        let dir = std::env::temp_dir();
        let path = dir.join("test_secret_file.txt");
        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(file, "file-secret-value").unwrap();
        drop(file);

        // Use forward slashes for TOML (backslashes are escape sequences)
        let path_str = path.display().to_string().replace('\\', "/");
        let toml_str = format!(r#"secret = {{ file = "{}" }}"#, path_str);
        let value: toml::Value = toml::from_str(&toml_str).unwrap();
        let secret = Secret::deserialize(value.get("secret").unwrap().clone()).unwrap();
        assert_eq!(secret.expose(), "file-secret-value");

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn test_missing_env_var_error() {
        let toml_str = r#"secret = { env = "DEFINITELY_NOT_SET_12345" }"#;
        let value: toml::Value = toml::from_str(toml_str).unwrap();
        let result = Secret::deserialize(value.get("secret").unwrap().clone());
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_file_error() {
        let toml_str = r#"secret = { file = "/nonexistent/path/to/secret" }"#;
        let value: toml::Value = toml::from_str(toml_str).unwrap();
        let result = Secret::deserialize(value.get("secret").unwrap().clone());
        assert!(result.is_err());
    }
}
