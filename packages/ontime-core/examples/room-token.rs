//! Prints the bearer token for a password.
//!
//! The API never takes a password directly — a token is `hash_password(password)`,
//! and the master one (the hash of the Worker's `SESSION_PASSWORD`) is what a
//! request needs to bring a new room into existence. There is no endpoint that
//! exchanges one for the other, so an operator scripting against a gated
//! deployment has to compute it:
//!
//!     cargo run -q --manifest-path packages/ontime-core/Cargo.toml \
//!         --example room-token -- 'the password'
//!
//! Reads stdin when given no argument, which keeps the password out of the shell
//! history and the process list:
//!
//!     read -rs password && printf '%s' "$password" | \
//!         cargo run -q --manifest-path packages/ontime-core/Cargo.toml --example room-token

use std::io::Read;

fn main() {
    let password = match std::env::args().nth(1) {
        Some(argument) => argument,
        None => {
            let mut piped = String::new();
            std::io::stdin()
                .read_to_string(&mut piped)
                .expect("could not read the password from stdin");
            piped.trim_end_matches('\n').to_string()
        }
    };

    println!("{}", ontime_core::project::hash_password(&password));
}
