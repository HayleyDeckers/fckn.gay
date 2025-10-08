/// Validation result with detailed error information
pub struct ValidationResult {
    errors: Vec<&'static str>,
}

impl ValidationResult {
    fn new() -> Self {
        Self { errors: Vec::new() }
    }

    fn with_errors(mut self, errors: Vec<&'static str>) -> Self {
        self.errors.extend(errors);
        self
    }
}

impl ValidationResult {
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn errors(&self) -> &[&'static str] {
        &self.errors
    }
}

/// Validates a username according to DNS label rules
/// - Must be 1-63 characters
/// - ASCII alphanumeric or '-'
/// - Cannot start or end with '-'
/// - Must be lowercase
pub fn validate_username(username: &str) -> ValidationResult {
    let result = ValidationResult::new();
    let mut errors = Vec::new();

    let len = username.len();
    if len == 0 {
        errors.push("Username cannot be empty");
    } else if len > 63 {
        errors.push("Username must be 63 characters or less");
    }

    if username.starts_with('-') {
        errors.push("Username cannot start with a dash");
    }
    if username.ends_with('-') {
        errors.push("Username cannot end with a dash");
    }

    for char in username.chars() {
        if !(char.is_ascii_lowercase() || char.is_ascii_digit() || (char == '-')) {
            errors.push("Username can only contain lowercase letters, numbers, and dashes");
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
pub fn validate_password(password: &str) -> ValidationResult {
    let result = ValidationResult::new();
    let mut errors = Vec::new();

    let len = password.len();
    if len < 12 {
        errors.push("Password must be at least 12 characters long");
    }
    if len > 128 {
        errors.push("Password must be 128 characters or less");
    }

    // Check all character requirements in a single pass for efficiency
    let mut has_upper = false;
    let mut has_lower = false;
    let mut has_digit = false;
    let mut has_punct = false;

    for char in password.chars() {
        if char.is_ascii_uppercase() {
            has_upper = true;
        } else if char.is_ascii_lowercase() {
            has_lower = true;
        } else if char.is_ascii_digit() {
            has_digit = true;
        } else if char.is_ascii_punctuation() {
            has_punct = true;
        }
    }

    if !has_upper {
        errors.push("Password must contain at least one uppercase letter");
    }
    if !has_lower {
        errors.push("Password must contain at least one lowercase letter");
    }
    if !has_digit {
        errors.push("Password must contain at least one number");
    }
    if !has_punct {
        errors.push("Password must contain at least one punctuation character");
    }

    result.with_errors(errors)
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    // WASM-specific functions that return JsArray for JavaScript consumption
    use js_sys::{Array as JsArray, JsString};
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen]
    pub fn validate_username_wasm(username: JsString) -> JsArray {
        let array = JsArray::new();
        let Some(username) = username.as_string() else {
            array.push(&JsString::from("Invalid utf-8 string"));
            return array;
        };
        let result = validate_username(&username);
        for error in result.errors {
            array.push(&JsString::from(error));
        }
        array
    }

    #[wasm_bindgen]
    pub fn validate_password_wasm(password: JsString) -> JsArray {
        let array = JsArray::new();
        let Some(password) = password.as_string() else {
            array.push(&JsString::from("Invalid utf-8 string"));
            return array;
        };
        let result = validate_password(&password);
        for error in result.errors {
            array.push(&JsString::from(error));
        }
        array
    }
}
#[cfg(target_arch = "wasm32")]
pub use wasm::*;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn valid_password() {
        assert!(validate_password("aB1.aB1.aB1.").is_valid());
    }

    #[test]
    fn password_min_max_length() {
        assert!(!validate_password("aB1.").is_valid());
        assert!(validate_password(&"aB1.".repeat(32)).is_valid());
        assert!(!validate_password(&("aB1.".repeat(32) + "a")).is_valid());
    }
    #[test]
    fn password_character_set() {
        // no punc
        assert!(!validate_password("aB1xaB1xaB1xa").is_valid());
        // no lowercase
        assert!(!validate_password("XB1.XB1.XB1.").is_valid());
        // no uppercase
        assert!(!validate_password("ab1.ab1.ab1.").is_valid());
        // no numbers
        assert!(!validate_password("aBi.aBi.aBi.").is_valid());
    }

    #[test]
    fn valid_username() {
        assert!(validate_username("username").is_valid());
        assert!(validate_username("i").is_valid());
    }

    #[test]
    fn username_min_max_length() {
        assert!(!validate_username("").is_valid());
        assert!(validate_username("x").is_valid());
        assert!(!validate_username(&"x".repeat(64)).is_valid());
        assert!(validate_username(&"x".repeat(63)).is_valid());
    }

    #[test]
    fn username_character_set() {
        // not start with -
        assert!(!validate_username("-username").is_valid());
        // not end with -
        assert!(!validate_username("username-").is_valid());
        // all lowercase
        assert!(!validate_username("uSeRnaMe").is_valid());
        // do allow all digits
        assert!(validate_username("5").is_valid());
        // don't allow emoji
        assert!(!validate_username("🐛").is_valid());
        // but punycode should work
        assert!(validate_username("xn--jo8h").is_valid());
        // no underscores
        assert!(!validate_username("user_name").is_valid());
        // also not at the start
        assert!(!validate_username("_username").is_valid());
        // no whitespace
        assert!(!validate_username("user name").is_valid());
        // no dots
        assert!(!validate_username("user.name").is_valid());
        // no ü
        assert!(!validate_username("üsername").is_valid());
    }
}
