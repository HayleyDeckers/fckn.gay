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

/// Validates a single DNS label (the part between dots)
/// - Must be 1-63 characters
/// - ASCII alphanumeric or '-'
/// - Can start with '_' for service labels (_dmarc, _dkim, _atproto, etc.)
/// - Underscore NOT allowed in the middle of a label (my_site is invalid)
/// - Cannot start or end with '-'
fn validate_dns_label(label: &str) -> Vec<&'static str> {
    let mut errors = Vec::new();

    if label.is_empty() {
        errors.push("Label cannot be empty");
        return errors;
    }

    if label.len() > 63 {
        errors.push("Label must be 63 characters or less");
    }

    if label.starts_with('-') {
        errors.push("Label cannot start with a dash");
    }
    if label.ends_with('-') {
        errors.push("Label cannot end with a dash");
    }

    // Check characters - underscore only allowed at the very start (service labels like _dmarc)
    for (i, char) in label.chars().enumerate() {
        if char == '_' {
            if i != 0 {
                errors.push("Underscore only allowed at the start of a label (for service records like _dmarc)");
                break;
            }
            // underscore at position 0 is fine, continue checking rest
        } else if !(char.is_ascii_alphanumeric() || char == '-') {
            errors.push("Label can only contain letters, numbers, and dashes");
            break;
        }
    }

    errors
}

/// Validates a DNS record name (full domain name like "subdomain.example.com")
/// - Total length: max 253 characters
/// - Each label: 1-63 characters, alphanumeric + hyphens
/// - Labels cannot start or end with hyphens
/// - At least one label required
pub fn validate_dns_record_name(name: &str) -> ValidationResult {
    let result = ValidationResult::new();
    let mut errors = Vec::new();

    if name.is_empty() {
        errors.push("Record name cannot be empty");
        return result.with_errors(errors);
    }

    // Max 253 chars (RFC 1035 allows 255 with trailing dot, we don't require it)
    if name.len() > 253 {
        errors.push("Record name must be 253 characters or less");
    }

    // Check for consecutive dots (empty labels)
    if name.contains("..") {
        errors.push("Record name cannot contain consecutive dots");
    }

    // Validate each label
    let labels: Vec<&str> = name.split('.').collect();
    for label in &labels {
        let label_errors = validate_dns_label(label);
        errors.extend(label_errors);
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

    #[wasm_bindgen]
    pub fn validate_dns_record_name_wasm(name: JsString) -> JsArray {
        let array = JsArray::new();
        let Some(name) = name.as_string() else {
            array.push(&JsString::from("Invalid utf-8 string"));
            return array;
        };
        let result = validate_dns_record_name(&name);
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

    #[test]
    fn valid_dns_record_name() {
        assert!(validate_dns_record_name("example.com").is_valid());
        assert!(validate_dns_record_name("sub.example.com").is_valid());
        assert!(validate_dns_record_name("a.b.c.d.e.f.g").is_valid());
        assert!(validate_dns_record_name("alice.is.fckn.gay").is_valid());
        assert!(validate_dns_record_name("sub.alice.is.fckn.gay").is_valid());
        // single label is valid
        assert!(validate_dns_record_name("localhost").is_valid());
        // hyphens in middle are fine
        assert!(validate_dns_record_name("my-cool-site.example.com").is_valid());
        // numbers are fine
        assert!(validate_dns_record_name("123.456.789").is_valid());
        // mixed case is allowed in DNS (case-insensitive matching)
        assert!(validate_dns_record_name("Alice.Is.Fckn.Gay").is_valid());
    }

    #[test]
    fn dns_record_name_empty() {
        assert!(!validate_dns_record_name("").is_valid());
    }

    #[test]
    fn dns_record_name_too_long() {
        // 253 chars is max - use multiple 63-char labels
        // 63 + 1 + 63 + 1 + 63 + 1 + 58 + 1 + 2 = 253
        let long_name = format!(
            "{}.{}.{}.{}.aa",
            "a".repeat(63),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(58)
        );
        assert_eq!(long_name.len(), 253);
        assert!(validate_dns_record_name(&long_name).is_valid());

        // 254 chars is too long
        let too_long = format!(
            "{}.{}.{}.{}.aaa",
            "a".repeat(63),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(58)
        );
        assert_eq!(too_long.len(), 254);
        assert!(!validate_dns_record_name(&too_long).is_valid());
    }

    #[test]
    fn dns_record_name_label_too_long() {
        // 63 char label is fine
        let ok_label = format!("{}.com", "a".repeat(63));
        assert!(validate_dns_record_name(&ok_label).is_valid());

        // 64 char label is too long
        let bad_label = format!("{}.com", "a".repeat(64));
        assert!(!validate_dns_record_name(&bad_label).is_valid());
    }

    #[test]
    fn dns_record_name_invalid_characters() {
        // no spaces
        assert!(!validate_dns_record_name("my site.com").is_valid());
        // no emoji
        assert!(!validate_dns_record_name("🐛.com").is_valid());
        // no special chars
        assert!(!validate_dns_record_name("site$.com").is_valid());
        assert!(!validate_dns_record_name("site@example.com").is_valid());
    }

    #[test]
    fn dns_record_name_service_labels() {
        // underscore prefixes for service labels are valid (RFC 2782, etc.)
        assert!(validate_dns_record_name("_dmarc.example.com").is_valid());
        assert!(validate_dns_record_name("_dkim.example.com").is_valid());
        assert!(validate_dns_record_name("_atproto.alice.is.fckn.gay").is_valid());
        assert!(validate_dns_record_name("_acme-challenge.example.com").is_valid());
        // SRV record style (multiple underscore-prefixed labels)
        assert!(validate_dns_record_name("_sip._tcp.example.com").is_valid());
        // DKIM style with selector
        assert!(validate_dns_record_name("selector._domainkey.example.com").is_valid());
    }

    #[test]
    fn dns_record_name_underscore_in_middle_rejected() {
        // underscore in the MIDDLE of a label is NOT allowed (breaks DNS providers)
        assert!(!validate_dns_record_name("my_site.example.com").is_valid());
        assert!(!validate_dns_record_name("hello_world.example.com").is_valid());
        assert!(!validate_dns_record_name("test_.example.com").is_valid());
        // but underscore at start is fine
        assert!(validate_dns_record_name("_test.example.com").is_valid());
    }

    #[test]
    fn dns_record_name_hyphen_rules() {
        // can't start with hyphen
        assert!(!validate_dns_record_name("-example.com").is_valid());
        // can't end with hyphen
        assert!(!validate_dns_record_name("example-.com").is_valid());
        // label can't start with hyphen
        assert!(!validate_dns_record_name("sub.-example.com").is_valid());
        // label can't end with hyphen
        assert!(!validate_dns_record_name("sub.example-.com").is_valid());
    }

    #[test]
    fn dns_record_name_consecutive_dots() {
        // no empty labels (consecutive dots)
        assert!(!validate_dns_record_name("example..com").is_valid());
        assert!(!validate_dns_record_name("..example.com").is_valid());
        assert!(!validate_dns_record_name("example.com..").is_valid());
    }

    #[test]
    fn dns_record_name_trailing_dot() {
        // trailing dot creates empty label which is invalid
        // (we don't support FQDN notation with trailing dot)
        assert!(!validate_dns_record_name("example.com.").is_valid());
    }
}
