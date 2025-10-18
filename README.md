# fckn.gay 🏳️‍🌈

> **⚠️ Early Development** - This is a work-in-progress project! Things might break, APIs might change, development may suddenly slow-down, and I might suddenly become a woodworking hermit. Use at your own risk! 💀

A modular, pluggable subdomain-registrar written in Rust. It gives you a webserver where users can sign-up, claim a subdomain, and manage the DNS of it through a web-interface or API. 

## What is this even? 🤔

This project lets you:
- **Give subdomains to all your friends** or internet strangers
- **Have them manage DNS records** for their subdomains (like `user.is.fckn.gay`)
- **Through a web-interface or API** with multiple backend options
- **Switch DNS, email, or database providers** without changing code (just config!)

It's built with the intent of being able to pick whatever service you like using for the hard parts of running a service - DNS providers, email services, user databases, you name it. Want to switch from SQLite to PostgreSQL? Just change a config file. Want to use Porkbun instead of Hickory DNS? Same deal! 

## Architecture 🏗️

The project follows a repository pattern with trait-based interfaces:

```
server/        # Main web server (Axum), here we define the user-facing stuff
dns/           # DNS management interface + implementors
email/         # Email sending interface + implementors  
user-database/ # User storage interface + implementors
validation/    # Validation functions shared between the backend (native) and frontend (WASM)
```

Each interface has multiple implementors:
- **DNS**: Hickory (self-hosted), Porkbun (cloud), Dummy (for testing)
- **Email**: Lettre (SMTP), dummy (print to stdout), more coming soon!
- **User DB**: Diesel (Sqlite), CSV, Dummy (for testing)

Each interface folder has the following substructure, where multiple implementors live in a sub-crate alongside a trait-defining interface crate.
```
dns/ 
├─interface/ # the interface crate defines the trait and any common structs.
├─implementors/ # the implementors folder holds all the implementations of the the interface
  ├─dummy/ # dummy is always a testing interface
  ├─porkbun/ # for DNS we support the porkbun http api
  ├─hickory/ # and an embedded hickory DNS server
├─src/# the top level is a crate, implementing the interface for an enum of all the implementors
```

## Quick Start 🚀

Simply, 
```bash
cargo run -- --config example_config.toml
```

## Configuration 🎛️

The config system is made to be flexible - you can mix and match providers

```toml
# Use Hickory for DNS, SQLite for users, dummy for email
dns.provider = "hickory"
user_database.provider = "diesel"
email.provider = "dummy"
```

Check out `example_config.toml` for all the options and documentation!

## Development Status 📊

### ✅ What Works
- Basic DNS record management
- User authentication (signup/login)
- Multiple provider backends
- Web interface with WASM validation
- Configurable everything

### 🚧 In Progress  
- Deleting and editing DNS records
- Rate-limiting, bot detection
- telemetry
see the issues page for more, I use it quite extensively

### 💭 Future Dreams
- Simple, production-ready `cargo run` deploys 
- Scalable deployment with PostgreSQL, HTTP-based email services, external DNS servers
- A not vibe-coded monstrosity of a frontend 💀

## Contributing 🦀🤝🦀

This is a personal project but PRs are welcome! If you're new to Rust and just want something to hack on as a learning experience I am willing to help mentor as best as I can.

Adding new providers for the interfaces should be something that is very independent of the rest of the repo, it'd recommend starting there or taking a look at the issues page if you don't have an idea of your own 🎉

If you would like to add a new implementor for one of the interfaces, the process is pretty simple. 
You make a new crate in the `implementors` subfolder, define your struct and it's config and implement the trait for it. And finally you extend the top-level crate with an enum variant for your implementor. 

## AI policy 🤖

I'm fine with AI assisted contributions, I made some myself, and in fact welcome experimentation or `.cursor/rules` and such _but_ **please** do manually review your code and disclose your usage of AI. Don't just drive-by dump an AI generated MR without giving it a bit of human polish. 

## License 📄

It's mine 😠 License t.b.d. but go ahead and use it non-commercially or ask me for one 🤷‍♀️

