use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

/// Validation result with detailed error information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub errors: Vec<String>,
}

impl ValidationResult {
    pub fn new() -> Self {
        Self {
            is_valid: true,
            errors: Vec::new(),
        }
    }

    pub fn with_error(mut self, error: String) -> Self {
        self.is_valid = false;
        self.errors.push(error);
        self
    }

    pub fn with_errors(mut self, errors: Vec<String>) -> Self {
        if !errors.is_empty() {
            self.is_valid = false;
            self.errors.extend(errors);
        }
        self
    }
}

/// Validates a username according to DNS label rules
/// - Must be 1-63 characters
/// - ASCII alphanumeric or '-'
/// - Cannot start or end with '-'
/// - Must be lowercase
pub fn is_valid_username(username: &str) -> ValidationResult {
    let result = ValidationResult::new();
    let mut errors = Vec::new();

    let len = username.len();
    if len == 0 {
        errors.push("Username cannot be empty".to_string());
    } else if len > 63 {
        errors.push("Username must be 63 characters or less".to_string());
    }

    if username.starts_with('-') {
        errors.push("Username cannot start with a dash".to_string());
    }
    if username.ends_with('-') {
        errors.push("Username cannot end with a dash".to_string());
    }

    for char in username.chars() {
        if !(char.is_ascii_lowercase() || char.is_ascii_digit() || (char == '-')) {
            errors.push("Username can only contain lowercase letters, numbers, and dashes".to_string());
            break; // Only show this error once
        }
    }

    result.with_errors(errors)
}

/// Validates a password according to security requirements
/// - Must be 12-128 characters
/// - Must contain at least one uppercase letter
/// - Must contain at least one lowercase letter  
/// - Must contain at least one digit
/// - Must contain at least one punctuation character
pub fn is_valid_password(password: &str) -> ValidationResult {
    let result = ValidationResult::new();
    let mut errors = Vec::new();

    if password.len() < 12 {
        errors.push("Password must be at least 12 characters long".to_string());
    }
    if password.len() > 128 {
        errors.push("Password must be 128 characters or less".to_string());
    }

    if !password.chars().any(|c| c.is_ascii_uppercase()) {
        errors.push("Password must contain at least one uppercase letter".to_string());
    }
    if !password.chars().any(|c| c.is_ascii_lowercase()) {
        errors.push("Password must contain at least one lowercase letter".to_string());
    }
    if !password.chars().any(|c| c.is_ascii_digit()) {
        errors.push("Password must contain at least one number".to_string());
    }
    if !password.chars().any(|c| c.is_ascii_punctuation()) {
        errors.push("Password must contain at least one punctuation character".to_string());
    }

    result.with_errors(errors)
}

/// Validates both username and password
pub fn validate_credentials(username: &str, password: &str) -> ValidationResult {
    let username_result = is_valid_username(username);
    let password_result = is_valid_password(password);
    
    let mut all_errors = Vec::new();
    all_errors.extend(username_result.errors);
    all_errors.extend(password_result.errors);
    
    ValidationResult {
        is_valid: all_errors.is_empty(),
        errors: all_errors,
    }
}

// WASM bindings for frontend use
#[wasm_bindgen]
pub fn validate_username_wasm(username: &str) -> String {
    let result = is_valid_username(username);
    serde_json::to_string(&result).unwrap_or_else(|_| r#"{"is_valid":false,"errors":["Serialization error"]}"#.to_string())
}

#[wasm_bindgen]
pub fn validate_password_wasm(password: &str) -> String {
    let result = is_valid_password(password);
    serde_json::to_string(&result).unwrap_or_else(|_| r#"{"is_valid":false,"errors":["Serialization error"]}"#.to_string())
}

#[wasm_bindgen]
pub fn validate_credentials_wasm(username: &str, password: &str) -> String {
    let result = validate_credentials(username, password);
    serde_json::to_string(&result).unwrap_or_else(|_| r#"{"is_valid":false,"errors":["Serialization error"]}"#.to_string())
}

// Console logging for debugging
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

#[wasm_bindgen]
pub fn console_log(s: &str) {
    log(s);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_username() {
        assert!(is_valid_username("username").is_valid);
        assert!(is_valid_username("i").is_valid);
        assert!(is_valid_username("5").is_valid);
        assert!(is_valid_username("xn--jo8h").is_valid);
    }

    #[test]
    fn test_invalid_username() {
        assert!(!is_valid_username("").is_valid);
        assert!(!is_valid_username("-username").is_valid);
        assert!(!is_valid_username("username-").is_valid);
        assert!(!is_valid_username("uSeRnaMe").is_valid);
        assert!(!is_valid_username("🐛").is_valid);
        assert!(!is_valid_username("user_name").is_valid);
        assert!(!is_valid_username("_username").is_valid);
        assert!(!is_valid_username("user name").is_valid);
        assert!(!is_valid_username("user.name").is_valid);
        assert!(!is_valid_username("üsername").is_valid);
    }

    #[test]
    fn test_valid_password() {
        assert!(is_valid_password("aB1.aB1.aB1.").is_valid);
    }

    #[test]
    fn test_invalid_password() {
        assert!(!is_valid_password("aB1.").is_valid); // too short
        assert!(!is_valid_password("aB1xaB1xaB1xa").is_valid); // no punctuation
        assert!(!is_valid_password("XB1.XB1.XB1.").is_valid); // no lowercase
        assert!(!is_valid_password("ab1.ab1.ab1.").is_valid); // no uppercase
        assert!(!is_valid_password("aBi.aBi.aBi.").is_valid); // no numbers
    }

    #[test]
    fn test_validate_credentials() {
        let result = validate_credentials("validuser", "ValidPass123!");
        assert!(result.is_valid);
        
        let result = validate_credentials("", "ValidPass123!");
        assert!(!result.is_valid);
        assert!(result.errors.iter().any(|e| e.contains("empty")));
        
        let result = validate_credentials("validuser", "weak");
        assert!(!result.is_valid);
        assert!(result.errors.iter().any(|e| e.contains("12 characters")));
    }
}
