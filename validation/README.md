# fckn-gay-validation

A Rust validation crate for username and password validation, with conditional WASM support for frontend integration.

## Features

- **Username validation**: DNS label format (1-63 chars, lowercase alphanumeric + dashes)
- **Password validation**: Security requirements (12-128 chars, mixed case, digits, punctuation)
- **Conditional WASM support**: WASM functions only compiled for `wasm32` target

## Building for WASM

This crate requires `wasm-pack` to build WASM modules for frontend use.

### Installing wasm-pack

```bash
cargo install wasm-pack
```

### Building WASM module

```bash
wasm-pack build --target web --out-dir pkg
```

The generated files will be in the `pkg/` directory and should be copied to your static assets directory.

## Usage

### Rust (Native)

```rust
use fckn_gay_validation::{validate_username, validate_password};

// Validate individual fields
let username_result = validate_username("myuser");
if !username_result.is_valid() {
    println!("Username errors: {:?}", username_result.errors());
}

let password_result = validate_password("MyPassword123!");
if !password_result.is_valid() {
    println!("Password errors: {:?}", password_result.errors());
}
```

### JavaScript/WASM

```javascript
import init, { validate_username_wasm, validate_password_wasm } from './fckn_gay_validation.js';

// Initialize WASM module
const wasmModule = await init('./fckn_gay_validation_bg.wasm');

// Validate fields - returns JavaScript Array directly
const usernameErrors = wasmModule.validate_username_wasm("myuser");
const passwordErrors = wasmModule.validate_password_wasm("MyPassword123!");

// Use the arrays directly
if (usernameErrors.length > 0) {
    console.log("Username errors:", usernameErrors);
}
```
